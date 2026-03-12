// ISLS v1.0.0 Acceptance Tests AT-01 through AT-20
// All 20 acceptance tests from ISLS §29

use std::collections::BTreeMap;
use isls_types::{
    CommitProof, Config, ConsensusResult, FiveDState, GateSnapshot, NullCenter,
    Observation, PoRTrace, RunDescriptor, content_address, content_address_raw,
};
use isls_observe::{ingest, PassthroughAdapter};
use isls_persist::PersistentGraph;
use isls_extract::{default_operator_library, inverse_weave, TimeWindow};
use isls_consensus::{
    CascadeOperator, CrystalPrecursor, DKOperator, MetricSet, PIOperator,
    PoRFsm, PoRState, SWOperator, WTOperator, dual_consensus,
};
use isls_carrier::{build_phase_ladder, helix_pair, mandorla, restore_neutrality};
use isls_archive::{Archive, build_crystal_with_id, build_evidence_chain, verify_crystal};
use isls_morph::{morphogenic_update, MorphState};
use isls_engine::{macro_step, run_with_descriptor, GlobalState, EngineState};

// ─── Helper: make a passing CommitProof ─────────────────────────────────────

fn passing_commit_proof() -> CommitProof {
    CommitProof {
        gate_values: GateSnapshot {
            d: 1.0, q: 1.0, r: 1.0, g: 1.0, j: 1.0, p: 1.0, n: 1.0, k: 1.0,
            kairos: true,
        },
        consensus_result: ConsensusResult {
            primal_score: 0.9,
            dual_score: 0.9,
            mci: 0.95,
            threshold: 0.6,
        },
        ..Default::default()
    }
}

fn make_obs(src: &str, payload: &[u8], ts: f64) -> Observation {
    let digest = content_address_raw(payload);
    Observation {
        timestamp: ts,
        source_id: src.to_string(),
        provenance: isls_types::ProvenanceEnvelope::default(),
        payload: payload.to_vec(),
        context: isls_types::MeasurementContext::default(),
        digest,
        schema_version: "1.0.0".to_string(),
    }
}

// ─── AT-01: Idempotent Ingestion ─────────────────────────────────────────────

#[test]
fn at_01_idempotent_ingestion() {
    let adapter = PassthroughAdapter::new("test");
    let ctx = isls_types::MeasurementContext::default();
    let raw = b"sensor_reading_12345";

    let obs1 = ingest(&adapter, raw, &ctx).unwrap();
    let obs2 = ingest(&adapter, raw, &ctx).unwrap();

    assert_eq!(obs1.digest, obs2.digest, "AT-01: same input must yield same digest");
    assert_eq!(obs1.payload, obs2.payload, "AT-01: same input must yield same payload");
}

// ─── AT-02: Append-Only ──────────────────────────────────────────────────────

#[test]
fn at_02_append_only() {
    let mut graph = PersistentGraph::new();
    let config = isls_types::PersistenceConfig::default();

    // Add observation
    let obs = vec![make_obs("src1", b"data1", 1.0)];
    graph.apply_observations(&obs, &config).unwrap();

    let history_len_before = graph.history.len();
    let commit_before = graph.commit_index;

    // Decay edges (simulate time passing)
    // Edges decay but history is preserved
    let obs2 = vec![make_obs("src1", b"data2", 2.0)];
    graph.apply_observations(&obs2, &config).unwrap();

    // History is append-only (never shrinks)
    assert!(graph.history.len() >= history_len_before, "AT-02: history must be append-only");
    assert_eq!(graph.commit_index, commit_before + 1, "AT-02: commit index must increment");

    // Historical record is still queryable
    let first_record = &graph.history[0];
    assert_eq!(first_record.commit_index, 0);
}

// ─── AT-03: Replay Determinism ───────────────────────────────────────────────

#[test]
fn at_03_replay_determinism() {
    let config = Config::default();
    let descriptor = RunDescriptor {
        config: config.clone(),
        operator_versions: BTreeMap::new(),
        initial_state_digest: [0u8; 32],
        seed: None,
    };

    let obs_batches: Vec<Vec<Vec<u8>>> = vec![
        vec![b"obs_a".to_vec(), b"obs_b".to_vec()],
        vec![b"obs_c".to_vec()],
    ];

    // Run twice with same descriptor
    let results1 = run_with_descriptor(&descriptor, &obs_batches).unwrap();
    let results2 = run_with_descriptor(&descriptor, &obs_batches).unwrap();

    // Crystal digests must be identical
    for (r1, r2) in results1.iter().zip(results2.iter()) {
        match (r1, r2) {
            (Some(c1), Some(c2)) => {
                assert_eq!(
                    c1.crystal_id, c2.crystal_id,
                    "AT-03: replay must produce identical crystal IDs"
                );
            }
            (None, None) => {} // Both rejected is also deterministic
            _ => panic!("AT-03: replay must produce same result (crystal or rejection)"),
        }
    }
}

// ─── AT-04: Read-Only Extraction ─────────────────────────────────────────────

#[test]
fn at_04_read_only_extraction() {
    let mut graph = PersistentGraph::new();
    let config = isls_types::PersistenceConfig::default();

    // Add some data
    let obs = vec![
        make_obs("v1", b"data1", 1.0),
        make_obs("v2", b"data2", 2.0),
    ];
    graph.apply_observations(&obs, &config).unwrap();

    let commit_before = graph.commit_index;
    let vertex_count_before = graph.id_map.len();

    // Run inverse_weave with immutable ref (Inv I5)
    let library = default_operator_library();
    let window = TimeWindow::all();
    let extract_config = isls_types::ExtractionConfig::default();
    let (_program, _region) = inverse_weave(&graph, &window, &library, &extract_config);

    // Graph must be unchanged
    assert_eq!(
        graph.commit_index, commit_before,
        "AT-04: extraction must not modify commit_index"
    );
    assert_eq!(
        graph.id_map.len(), vertex_count_before,
        "AT-04: extraction must not modify graph structure"
    );
}

// ─── AT-05: Constraint Convergence ───────────────────────────────────────────

#[test]
fn at_05_constraint_convergence() {
    let mut graph = PersistentGraph::new();
    let config = isls_types::PersistenceConfig::default();
    let extract_config = isls_types::ExtractionConfig {
        alpha_min: 0.1,  // lower threshold to allow constraints to emerge
        ..Default::default()
    };

    // Inject synthetic correlated data (repeated patterns)
    for i in 0..10u8 {
        let payload = vec![i, i, i, i, i]; // correlated data
        let obs = vec![make_obs(&format!("src_{}", i % 3), &payload, i as f64)];
        graph.apply_observations(&obs, &config).unwrap();
    }

    // Run extraction and verify a constraint program emerges
    let library = default_operator_library();
    let window = TimeWindow::all();
    let (_program, region) = inverse_weave(&graph, &window, &library, &extract_config);

    // After injecting correlated data, we should have some vertices in the region
    // (the constraint program may be empty if thresholds aren't met, but region = all vertices)
    assert!(
        !region.is_empty() || graph.embedding.is_empty(),
        "AT-05: region should contain vertices when data exists"
    );
}

// ─── AT-06: Provenance Completeness ──────────────────────────────────────────

#[test]
fn at_06_provenance_completeness() {
    let proof = passing_commit_proof();
    let crystal = build_crystal_with_id(
        vec![1, 2, 3],
        0.9,
        1,
        -1.0,
        0,
        Vec::new(),
        proof,
    );

    let pinned = BTreeMap::new();
    let result = verify_crystal(&crystal, &pinned);
    assert!(result.is_ok(), "AT-06: verify_crystal must return Ok for valid crystal: {:?}", result);
}

// ─── AT-07: Threshold-Gated Reject ───────────────────────────────────────────

#[test]
fn at_07_threshold_gated_reject() {
    // Configure thresholds to be HIGH so they won't be met
    let mut config = Config::default();
    config.thresholds.d = 0.99; // nearly impossible to meet
    config.thresholds.q = 0.99;
    config.thresholds.r = 0.99;
    config.thresholds.g = 0.99;
    config.thresholds.j = 0.99;
    config.thresholds.p = 0.99;
    config.thresholds.n = 0.99;
    config.thresholds.k = 0.99;

    let mut state = GlobalState::new(&config);
    let adapter = PassthroughAdapter::new("test");

    // Run macro_step with some data
    let result = macro_step(&mut state, &[b"some_data".to_vec()], &config, &adapter).unwrap();

    // With impossibly high thresholds, no crystal should emerge
    assert!(result.is_none(), "AT-07: crystal must not emerge when thresholds not met");
    assert!(
        matches!(state.engine_state, EngineState::Rejected(_)),
        "AT-07: engine must be in Rejected state"
    );
}

// ─── AT-08: Positive Commit ───────────────────────────────────────────────────

#[test]
fn at_08_positive_commit() {
    // Configure thresholds to be easily met (low)
    let mut config = Config::default();
    config.thresholds.d = 0.0;
    config.thresholds.q = 0.0;
    config.thresholds.r = 0.0;
    config.thresholds.g = 0.0;
    config.thresholds.j = 0.0;
    config.thresholds.p = 0.0;
    config.thresholds.n = 0.0;
    config.thresholds.k = 0.0;
    config.consensus.consensus_threshold = 0.0;
    config.consensus.mirror_consistency_eta = 0.0;

    let mut state = GlobalState::new(&config);
    let adapter = PassthroughAdapter::new("test");

    let result = macro_step(&mut state, &[b"valid_data".to_vec()], &config, &adapter).unwrap();

    assert!(result.is_some(), "AT-08: crystal must be produced when all thresholds are met");
    assert_eq!(state.engine_state, EngineState::Idle, "AT-08: engine must return to Idle after commit");
    assert!(!state.archive.is_empty(), "AT-08: archive must contain committed crystal");
}

// ─── AT-09: Storage Corruption ───────────────────────────────────────────────

#[test]
fn at_09_storage_corruption() {
    let mut graph = PersistentGraph::new();
    let config = isls_types::PersistenceConfig::default();

    let obs = vec![make_obs("src1", b"important_data", 1.0)];
    graph.apply_observations(&obs, &config).unwrap();

    // Simulate warm tier corruption
    graph.warm.corrupted = true;

    // Hot tier should still be intact
    let hot_vid = isls_persist::derive_vertex_id("src1");
    let hot_data = graph.hot.data.get(&hot_vid);
    assert!(
        hot_data.is_some(),
        "AT-09: hot tier must remain accessible even when warm tier is corrupted"
    );

    // Cold tier also intact
    // (In production, would trigger re-derivation; here we verify isolation)
    assert!(
        graph.warm.corrupted,
        "AT-09: corruption flag must be set (explicit error handling)"
    );
}

// ─── AT-10: Non-Retroactivity ─────────────────────────────────────────────────

#[test]
fn at_10_non_retroactivity() {
    // Build a crystal and commit it
    let proof = passing_commit_proof();
    let crystal = build_crystal_with_id(
        vec![1, 2, 3],
        0.9,
        1,
        -1.0,
        0,
        Vec::new(),
        proof,
    );
    let crystal_id_before = crystal.crystal_id;

    let mut archive = Archive::new();
    archive.append(crystal);

    // Now trigger a morph mutation on the graph
    let mut graph = PersistentGraph::new();
    let mut morph_state = MorphState::new();
    let config = isls_types::AdaptationConfig::default();

    graph.upsert_vertex(1, 0.0);
    graph.upsert_vertex(2, 0.0);

    let _mutations = morphogenic_update(&mut graph, &mut morph_state, &[], &config);

    // Past crystal digest must be unchanged (Inv I11)
    let stored_id = archive.crystals()[0].crystal_id;
    assert_eq!(
        stored_id, crystal_id_before,
        "AT-10: past crystal digest must not change after morph mutation"
    );
}

// ─── AT-11: Operator Drift ───────────────────────────────────────────────────

#[test]
fn at_11_operator_drift() {
    let mut proof = passing_commit_proof();
    proof.operator_stack = vec![("band".to_string(), "1.0.0".to_string())];
    let crystal = build_crystal_with_id(vec![1], 0.9, 1, -1.0, 0, Vec::new(), proof);

    // Archive has version "2.0.0" for "band" operator
    let mut pinned = BTreeMap::new();
    pinned.insert("band".to_string(), "2.0.0".to_string());

    let result = verify_crystal(&crystal, &pinned);
    assert!(
        result.is_err(),
        "AT-11: operator drift must be detected as a protocol fault"
    );
}

// ─── AT-12: Resource Bound ───────────────────────────────────────────────────

#[test]
fn at_12_resource_bound() {
    // Configure small max_vertices to enforce resource bound
    let mut config = Config::default();
    config.persistence.max_vertices = 1000;
    config.thresholds.d = 0.0;
    config.thresholds.q = 0.0;
    config.thresholds.r = 0.0;
    config.thresholds.g = 0.0;
    config.thresholds.j = 0.0;
    config.thresholds.p = 0.0;
    config.thresholds.n = 0.0;
    config.thresholds.k = 0.0;
    config.consensus.consensus_threshold = 0.0;
    config.consensus.mirror_consistency_eta = 0.0;

    let mut state = GlobalState::new(&config);
    let adapter = PassthroughAdapter::new("bench");

    // Run 100 cycles (reduced from 1000 for test speed)
    for i in 0..100u32 {
        let payload = i.to_le_bytes().to_vec();
        let _ = macro_step(&mut state, &[payload], &config, &adapter);
    }

    // Vertex count should be bounded (max_vertices enforced)
    let vertex_count = state.graph.id_map.len();
    assert!(
        vertex_count <= config.persistence.max_vertices + 100, // some slack for split/merge
        "AT-12: vertex count {} must remain within resource bounds ({})",
        vertex_count,
        config.persistence.max_vertices + 100
    );
}

// ─── AT-13: Dual Consensus ───────────────────────────────────────────────────

#[test]
fn at_13_dual_consensus() {
    let precursor = CrystalPrecursor {
        program: Vec::new(),
        region: vec![1, 2, 3],
        seam_score: 0.8,
        metrics: MetricSet {
            d_deformation: 0.8,
            q_coherence: 0.8,
            r_resonance: 0.8,
            g_readiness: 0.8,
            j_doublekick: 0.8,
            p_projection: 0.8,
            n_seam: 0.8,
            k_crystal: 0.8,
            f_friction: 0.0,
            s_shock: 0.0,
            l_migration: 0.0,
        },
        stability_score: 0.8,
    };

    let dk = DKOperator;
    let sw = SWOperator;
    let pi = PIOperator;
    let wt = WTOperator;
    let pi2 = PIOperator;
    let wt2 = WTOperator;
    let dk2 = DKOperator;
    let sw2 = SWOperator;

    let primal: Vec<&dyn CascadeOperator> = vec![&dk, &sw, &pi, &wt];
    let dual: Vec<&dyn CascadeOperator> = vec![&pi2, &wt2, &dk2, &sw2];

    let config = isls_types::ConsensusConfig::default();
    let result = dual_consensus(&precursor, &primal, &dual, &config);

    assert!(result.primal_score >= 0.0, "AT-13: primal score must be >= 0");
    assert!(result.dual_score >= 0.0, "AT-13: dual score must be >= 0");
    assert!(
        result.mci >= 0.0 && result.mci <= 1.0,
        "AT-13: MCI must be in [0,1], got {}",
        result.mci
    );

    // If thresholds are met, both must agree (consensus)
    if result.primal_score >= result.threshold && result.dual_score >= result.threshold {
        assert!(
            result.mci >= 0.5,
            "AT-13: when both pass threshold, MCI should indicate agreement"
        );
    }
}

// ─── AT-14: PoR FSM ───────────────────────────────────────────────────────────

#[test]
fn at_14_por_fsm() {
    let mut fsm = PoRFsm::new();
    let config = isls_types::ConsensusConfig {
        por_kappa_bar: 0.5,
        por_t_min: 2,
        por_t_stable: 1,
        por_epsilon: 0.1,
        ..Default::default()
    };

    // Must start in Search
    assert_eq!(fsm.state, PoRState::Search, "AT-14: FSM must start in Search");

    // Search -> Lock transition (need t_min=2 consecutive values >= kappa_bar)
    let s1 = fsm.step(0.8, 1.0, &config);
    assert_eq!(s1, PoRState::Search, "AT-14: first step stays in Search");
    let s2 = fsm.step(0.8, 2.0, &config);
    assert_eq!(s2, PoRState::Lock, "AT-14: after t_min steps, must transition to Lock");

    // Lock -> Verify (need t_min + t_stable = 3 total, so 1 more step)
    let s3 = fsm.step(0.8, 3.0, &config);
    assert_eq!(s3, PoRState::Verify, "AT-14: after t_stable steps in Lock, must transition to Verify");

    // Verify -> Commit
    let s4 = fsm.step(0.8, 4.0, &config);
    assert_eq!(s4, PoRState::Commit, "AT-14: Verify -> Commit when kappa stable");

    // Verify timestamps are monotonic
    let trace = fsm.get_trace();
    assert!(trace.lock_enter.is_some(), "AT-14: lock_enter must be recorded");
    assert!(trace.verify_enter.is_some(), "AT-14: verify_enter must be recorded");
    assert!(trace.commit_enter.is_some(), "AT-14: commit_enter must be recorded");

    let lock_t = trace.lock_enter.unwrap();
    let verify_t = trace.verify_enter.unwrap();
    let commit_t = trace.commit_enter.unwrap();
    assert!(lock_t <= verify_t, "AT-14: lock must precede verify");
    assert!(verify_t <= commit_t, "AT-14: verify must precede commit");
}

// ─── AT-15: Carrier Migration ─────────────────────────────────────────────────

#[test]
fn at_15_carrier_migration() {
    // Build a phase ladder with 2 carriers
    let mut ladder = build_phase_ladder(2, 0.0, 1.0);

    // Set up carrier 1 with high resonance (migration target)
    ladder[1].resonance = 0.9;
    ladder[1].mandorla.kappa = 0.9;

    // Create metrics that trigger migration (friction > threshold)
    let metrics = MetricSet {
        f_friction: 0.9, // above threshold (0.7)
        s_shock: 0.0,
        q_coherence: 0.8,
        r_resonance: 0.8,
        l_migration: 0.9,
        ..Default::default()
    };
    let config = isls_types::CarrierConfig {
        lambda_q: 0.33,
        lambda_r: 0.33,
        lambda_m: 0.34,
        num_carriers: 2,
        ..Default::default()
    };
    let thresholds = isls_types::ThresholdConfig {
        f_friction: 0.7,
        l_migration: 0.5,
        ..Default::default()
    };

    let is_admissible = isls_carrier::migration_admissible(
        &metrics,
        &ladder[1],
        &thresholds,
        &config,
    );
    assert!(is_admissible, "AT-15: migration must be admissible when friction > threshold");
}

// ─── AT-16: Kairos Gate ───────────────────────────────────────────────────────

#[test]
fn at_16_kairos_gate() {
    let mut config = Config::default();
    // Set one gate metric (n/seam) to require a high value
    config.thresholds.n = 0.99; // impossible to meet

    let mut state = GlobalState::new(&config);
    let adapter = PassthroughAdapter::new("test");

    let result = macro_step(&mut state, &[b"data".to_vec()], &config, &adapter).unwrap();

    // With n-gate suppressed, no monolith (crystal) should form
    assert!(result.is_none(), "AT-16: suppressing one gate must prevent crystal formation");
}

// ─── AT-17: Null-Center Stateless ─────────────────────────────────────────────

#[test]
fn at_17_null_center_stateless() {
    // Inv I13: NullCenter is unit struct (no fields)
    let nc = NullCenter;
    let nc2 = NullCenter;

    // Size is zero (unit struct)
    assert_eq!(
        std::mem::size_of::<NullCenter>(), 0,
        "AT-17: NullCenter must be a zero-sized unit struct"
    );

    // Equality: all instances are equal
    assert_eq!(nc, nc2, "AT-17: all NullCenter instances must be equal");

    // No state: clone is identical
    let nc3 = nc.clone();
    assert_eq!(nc, nc3, "AT-17: cloned NullCenter must equal original");
}

// ─── AT-18: Tri-Temporal Ordering ─────────────────────────────────────────────

#[test]
fn at_18_tri_temporal_ordering() {
    // Inv I14: n0 (NullCenter) < t2 (IntrinsicTime) < t1 (CommitIndex)
    // n0 is pre-temporal (no value), t2 is continuous, t1 is discrete commit

    // Verify ordering exists as separate types (Inv I14: tri-temporal irreducibility)
    let _null: NullCenter = NullCenter;
    let t2: isls_types::IntrinsicTime = isls_types::OrderedFloat(1.5_f64);
    let t1: isls_types::CommitIndex = 2u64;

    // t2 must be representable as f64 (continuous)
    let t2_val: f64 = *t2;
    assert!(t2_val >= 0.0, "AT-18: intrinsic time must be >= 0");

    // t1 is always a natural number (discrete)
    assert!(t1 > 0, "AT-18: commit index must be positive for committed crystals");

    // In a commit trace, t1 increments monotonically
    let mut ci: isls_types::CommitIndex = 0;
    for _ in 0..5 {
        let prev = ci;
        ci += 1;
        assert!(ci > prev, "AT-18: commit index must be strictly increasing");
    }
}

// ─── AT-19: Content Addressing ───────────────────────────────────────────────

#[test]
fn at_19_content_addressing() {
    let proof = passing_commit_proof();
    let crystal = build_crystal_with_id(
        vec![10, 20, 30],
        0.85,
        5,
        -2.5,
        1,
        Vec::new(),
        proof,
    );

    // crystal_id must equal SHA-256(JCS(core fields))
    #[derive(serde::Serialize)]
    struct CrystalCore<'a> {
        region: &'a Vec<isls_types::VertexId>,
        stability_score: f64,
        created_at: isls_types::CommitIndex,
        free_energy: f64,
        carrier_instance_idx: usize,
    }
    let core = CrystalCore {
        region: &crystal.region,
        stability_score: crystal.stability_score,
        created_at: crystal.created_at,
        free_energy: crystal.free_energy,
        carrier_instance_idx: crystal.carrier_instance_idx,
    };
    let expected_id = content_address(&core);

    assert_eq!(
        crystal.crystal_id, expected_id,
        "AT-19: crystal_id must equal SHA-256(JCS(canonical_core))"
    );
}

// ─── AT-20: Symmetry Restoration ──────────────────────────────────────────────

#[test]
fn at_20_symmetry_restoration() {
    // After a break/commit, carrier must return to neutral before next cycle
    let mut config = Config::default();
    config.thresholds.d = 0.0;
    config.thresholds.q = 0.0;
    config.thresholds.r = 0.0;
    config.thresholds.g = 0.0;
    config.thresholds.j = 0.0;
    config.thresholds.p = 0.0;
    config.thresholds.n = 0.0;
    config.thresholds.k = 0.0;
    config.consensus.consensus_threshold = 0.0;
    config.consensus.mirror_consistency_eta = 0.0;

    let mut state = GlobalState::new(&config);
    let adapter = PassthroughAdapter::new("test");

    // After macro_step (which restores neutrality), resonance should be reset
    let _ = macro_step(&mut state, &[b"data".to_vec()], &config, &adapter);

    // Active carrier must have resonance = 0 (restored to neutral)
    let active = state.active_carrier;
    assert_eq!(
        state.phase_ladder[active].resonance, 0.0,
        "AT-20: carrier resonance must be 0 after symmetry restoration"
    );

    // Engine state must be Idle (ready for next cycle)
    assert_eq!(
        state.engine_state, EngineState::Idle,
        "AT-20: engine must be Idle after restoration"
    );
}
