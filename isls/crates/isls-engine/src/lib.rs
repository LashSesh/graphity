// isls-engine: State machine, orchestrator (C9)
// depends on all other crates

use std::collections::BTreeMap;
use isls_types::{
    CommitIndex, CommitProof, Config, ConstraintProgram, FiveDState, GateSnapshot,
    MandorlaState, MeasurementContext, NullCenter, Observation, PhaseLadder, PoRTrace,
    RunDescriptor, SemanticCrystal, VertexId, content_address,
};
use isls_observe::{ingest, ObservationAdapter, PassthroughAdapter};
use isls_persist::PersistentGraph;
use isls_extract::{inverse_weave, TimeWindow, default_operator_library};
use isls_consensus::{
    CascadeOperator, CrystalPrecursor, DKOperator, dual_consensus, MetricSet, PIOperator,
    PoRFsm, PoRState, SWOperator, WTOperator,
};
use isls_carrier::{build_phase_ladder, helix_pair, mandorla, restore_neutrality};
use isls_archive::{Archive, build_crystal_with_id, build_evidence_chain};
use isls_morph::{intrinsic_step, morphogenic_update, MorphState};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("observation error: {0}")]
    Observe(#[from] isls_observe::ObserveError),
    #[error("persistence error: {0}")]
    Persist(#[from] isls_persist::PersistError),
    #[error("engine rejected: {0}")]
    Rejected(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

// ─── Engine State Machine ────────────────────────────────────────────────────

/// ISLS Engine states (ISLS Def 21.1)
#[derive(Clone, Debug, PartialEq)]
pub enum EngineState {
    Idle,
    Observing,
    Relating,
    Embedding,
    MandorlaForming,
    Resonating,
    KairosPrimed,
    Migrating,
    Capturing,
    Projecting,
    Stitching,
    Crystallizing,
    Monolithizing,
    Breaking,
    Restoring,
    Committed,
    Rejected(String),
}

// ─── Consensus State ─────────────────────────────────────────────────────────

#[derive(Default, Debug)]
pub struct ConsensusState {
    pub last_result: Option<isls_types::ConsensusResult>,
}

// ─── Global State (ISLS Def 17.1) ────────────────────────────────────────────

pub struct GlobalState {
    pub graph: PersistentGraph,
    pub candidates: Vec<CrystalPrecursor>,
    pub consensus: ConsensusState,
    pub morph: MorphState,
    pub active_carrier: usize,     // index into phase_ladder
    pub phase_ladder: PhaseLadder,
    pub h5_state: FiveDState,
    pub commit_index: CommitIndex,
    pub engine_state: EngineState,
    pub por_fsm: PoRFsm,
    pub archive: Archive,
    pub null_center: NullCenter, // Inv I13: always present, always empty
    pub t2: f64,                 // intrinsic time
}

impl GlobalState {
    pub fn new(config: &Config) -> Self {
        let phase_ladder = build_phase_ladder(config.carrier.num_carriers, 0.0, 1.0);
        Self {
            graph: PersistentGraph::new(),
            candidates: Vec::new(),
            consensus: ConsensusState::default(),
            morph: MorphState::new(),
            active_carrier: 0,
            phase_ladder,
            h5_state: FiveDState::default(),
            commit_index: 0,
            engine_state: EngineState::Idle,
            por_fsm: PoRFsm::new(),
            archive: Archive::new(),
            null_center: NullCenter,
            t2: 0.0,
        }
    }
}

// ─── Metric Computation ───────────────────────────────────────────────────────

/// Compute all metrics from the current graph, mandorla, and H5 state
pub fn compute_all_metrics(
    graph: &PersistentGraph,
    mandorla: &MandorlaState,
    h5: &FiveDState,
    config: &Config,
) -> MetricSet {
    let norm = &config.normalization;

    // D: deformation metric (proxy: embedding divergence from default)
    let avg_norm: f64 = if graph.embedding.is_empty() {
        0.0
    } else {
        graph.embedding.values().map(|s| s.norm_sq().sqrt()).sum::<f64>()
            / graph.embedding.len() as f64
    };
    let d_raw = avg_norm;
    let d = isls_consensus::norm_saturate(d_raw, norm.mu_d);

    // Q: coherence (from mandorla kappa)
    let q = mandorla.kappa;

    // R: resonance (exp(-d_R(H, ref)))
    let r = isls_consensus::norm_exp(h5.norm_sq().sqrt(), norm.lambda_r);

    // G: readiness = gamma_D*D + gamma_Q*Q + gamma_R*R
    let g = norm.gamma_d * d + norm.gamma_q * q + norm.gamma_r * r;

    // J: double-kick (proxy: edge count / vertex count)
    let j_raw = if graph.graph.node_count() > 0 {
        graph.graph.edge_count() as f64 / graph.graph.node_count() as f64
    } else {
        0.0
    };
    let j = isls_consensus::norm_saturate(j_raw, norm.mu_j);

    // P: projection (proxy: stability of h5 state from origin)
    let p = isls_consensus::norm_exp(h5.norm_sq().sqrt(), norm.lambda_p);

    // N: seam (proxy: mandorla delta_phi coherence)
    let n = isls_consensus::norm_exp(mandorla.delta_phi, norm.lambda_seam);

    // K: crystal score (lambda_C * coherence + lambda_E * entropy_factor)
    let k = norm.lambda_c * q + norm.lambda_e * (1.0 - mandorla.delta_phi / std::f64::consts::PI).max(0.0);

    // F: friction (proxy: rate of change in graph structure)
    let f_raw = graph.commit_index as f64 * 0.01;
    let f = isls_consensus::norm_saturate(f_raw, norm.mu_f);

    // S: shock (proxy: abrupt change in H5)
    let s_raw = h5.norm_sq().sqrt();
    let s = isls_consensus::norm_saturate(s_raw, norm.mu_s);

    // L: migration readiness
    let l = if !config.carrier.num_carriers == 0 {
        let carrier = &config.carrier;
        carrier.lambda_q * q + carrier.lambda_r * r + carrier.lambda_m * mandorla.kappa
    } else {
        0.0
    };

    MetricSet {
        d_deformation: d,
        q_coherence: q,
        r_resonance: r,
        g_readiness: g,
        j_doublekick: j,
        p_projection: p,
        n_seam: n,
        k_crystal: k,
        f_friction: f,
        s_shock: s,
        l_migration: l,
    }
}

// ─── Carrier Migration ────────────────────────────────────────────────────────

fn attempt_carrier_migration(state: &mut GlobalState, metrics: &MetricSet, config: &Config) {
    // Find best migration target (highest resonance in phase ladder)
    let current = state.active_carrier;
    let best = state
        .phase_ladder
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != current)
        .max_by(|(_, a), (_, b)| {
            a.mandorla.kappa
                .partial_cmp(&b.mandorla.kappa)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some((best_idx, best_carrier)) = best {
        if isls_carrier::migration_admissible(
            metrics,
            best_carrier,
            &config.thresholds,
            &config.carrier,
        ) {
            state.active_carrier = best_idx;
        }
    }
}

// ─── Macro Step (ISLS Algo 2) ─────────────────────────────────────────────────

/// T_ISLS(S_k, X_k; theta) = A_morph . C_commit . E_extract . T_persist . Gamma_obs
pub fn macro_step(
    state: &mut GlobalState,
    obs_payloads: &[Vec<u8>],
    config: &Config,
    adapter: &dyn ObservationAdapter,
) -> Result<Option<SemanticCrystal>> {
    // Unconditionally advance the macro-step counter so diagnostics and
    // downstream metrics always see a monotonically increasing tick index,
    // regardless of whether the step ends in a crystal commit or a gate
    // rejection.  Previously commit_index only incremented on a successful
    // crystal, causing all ticks to be reported as "tick 0".
    state.commit_index += 1;

    let ctx = MeasurementContext::default();

    // L0: Canonicalize observations
    state.engine_state = EngineState::Observing;
    let mut canonical_obs: Vec<Observation> = Vec::new();
    for raw in obs_payloads {
        let obs = ingest(adapter, raw, &ctx)?;
        canonical_obs.push(obs);
    }

    // L1: Update persistent graph (MCCE assimilated)
    state.engine_state = EngineState::Relating;
    state.graph.apply_observations(&canonical_obs, &config.persistence)?;

    // Embedding + Mandorla
    state.engine_state = EngineState::Embedding;
    // Advance the active carrier's phase by one dt2 step and recompute its
    // mandorla.  Previously (ha, hb, mand) were computed locally but never
    // written back, so carrier.helix_a.tau was always the initial value and
    // the carrier state never accumulated across ticks.
    let (ha, hb) = {
        let carrier = &state.phase_ladder[state.active_carrier];
        helix_pair(
            carrier.helix_a.tau + config.temporal.dt2,
            carrier.helix_a.phi,
            carrier.helix_a.r,
        )
    };
    let mand = mandorla(&ha, &hb, config.carrier.lambda, config.carrier.mu_r);

    // Write the new helix positions and mandorla back into the phase ladder so
    // the next tick picks up from where this one left off.
    {
        let carrier_mut = &mut state.phase_ladder[state.active_carrier];
        carrier_mut.helix_a = ha;
        carrier_mut.helix_b = hb;
        carrier_mut.mandorla = mand.clone();
    }

    state.engine_state = EngineState::MandorlaForming;

    // Resonance evaluation
    state.engine_state = EngineState::Resonating;
    let metrics = compute_all_metrics(&state.graph, &mand, &state.h5_state, config);

    // Check friction/shock -> migration
    if metrics.f_friction >= config.thresholds.f_friction
        || metrics.s_shock >= config.thresholds.s_shock
    {
        state.engine_state = EngineState::Migrating;
        attempt_carrier_migration(state, &metrics, config);
    }

    // Kairos gate check (Inv I9, Inv I18)
    let gate = metrics.gate_snapshot(&config.thresholds);
    if !gate.kairos {
        // Report which individual gates are failing so we can tune thresholds.
        eprintln!("tick {}: kairos FAILED — d={:.4}(need>={:.4}) q={:.4}(need>={:.4}) r={:.4}(need>={:.4}) g={:.4}(need>={:.4}) j={:.4}(need>={:.4}) p={:.4}(need>={:.4}) n={:.4}(need>={:.4}) k={:.4}(need>={:.4})",
                  state.commit_index,
                  gate.d, config.thresholds.d,
                  gate.q, config.thresholds.q,
                  gate.r, config.thresholds.r,
                  gate.g, config.thresholds.g,
                  gate.j, config.thresholds.j,
                  gate.p, config.thresholds.p,
                  gate.n, config.thresholds.n,
                  gate.k, config.thresholds.k);
        state.engine_state = EngineState::Rejected("kairos failed".into());
        return Ok(None);
    }
    eprintln!("tick {}: kairos PASSED", state.commit_index);
    state.engine_state = EngineState::KairosPrimed;

    // L2: Constraint extraction (ECLS assimilated)
    state.engine_state = EngineState::Capturing;
    let library = default_operator_library();
    let window = TimeWindow::all();
    let (program, region) = inverse_weave(&state.graph, &window, &library, &config.extraction);
    eprintln!("tick {}: extracted {} constraints, {} region vertices, graph has {} vertices {} edges",
              state.commit_index, program.len(), region.len(),
              state.graph.graph.node_count(), state.graph.graph.edge_count());

    // Operators: projection
    state.engine_state = EngineState::Projecting;
    let stability_score = metrics.g_readiness;
    let precursor = CrystalPrecursor {
        program: program.clone(),
        region: region.clone(),
        seam_score: metrics.n_seam,
        metrics: metrics.clone(),
        stability_score,
    };

    // Stitching (seam check)
    state.engine_state = EngineState::Stitching;
    if metrics.n_seam < config.thresholds.n {
        state.engine_state = EngineState::Rejected("seam failed".into());
        return Ok(None);
    }

    // L3: Crystallize
    state.engine_state = EngineState::Crystallizing;

    // Dual consensus
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
    let consensus = dual_consensus(&precursor, &primal, &dual, &config.consensus);

    if consensus.primal_score < consensus.threshold
        || consensus.dual_score < consensus.threshold
        || consensus.mci < config.consensus.mirror_consistency_eta
    {
        state.engine_state = EngineState::Rejected("consensus failed".into());
        return Ok(None);
    }

    state.consensus.last_result = Some(consensus.clone());

    // Build commit proof
    let por_trace = state.por_fsm.get_trace().clone();
    let operator_stack: Vec<(String, String)> = config
        .extraction
        .window_hours
        .to_string()
        .chars()
        .take(0)
        .collect::<String>()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| (s.to_string(), "1.0.0".to_string()))
        .collect(); // empty for now

    let commit_proof = CommitProof {
        evidence_digests: canonical_obs.iter().map(|o| o.digest).collect(),
        operator_stack,
        gate_values: gate.clone(),
        structural_result: true,
        consensus_result: consensus.clone(),
        por_trace,
        carrier_id: state.active_carrier,
        carrier_offset: state.phase_ladder[state.active_carrier].offset,
    };

    // Build crystal (Inv I17: crystal required before commit)
    let crystal = build_crystal_with_id(
        region,
        stability_score,
        state.commit_index,
        0.0, // free_energy computed separately
        state.active_carrier,
        program,
        commit_proof,
    );

    state.engine_state = EngineState::Monolithizing;

    // Commit (append to immutable archive, Inv I10)
    state.archive.append(crystal.clone());
    state.engine_state = EngineState::Committed;
    // commit_index is already incremented unconditionally at the top of
    // macro_step; do not increment again here.

    // L4: Morphogenic update (Inv I11: non-retroactive)
    morphogenic_update(&mut state.graph, &mut state.morph, &[crystal.clone()], &config.adaptation);

    // Intrinsic dynamics update (OI-08)
    state.t2 += config.temporal.dt2;
    intrinsic_step(
        &mut state.h5_state,
        &state.morph.attractor.clone(),
        &crystal.constraint_program,
        config.temporal.dt2,
        config.temporal.gamma,
    );

    // Restore neutrality (AT-20: symmetry restoration)
    state.engine_state = EngineState::Restoring;
    if let Some(carrier) = state.phase_ladder.get_mut(state.active_carrier) {
        restore_neutrality(carrier);
    }
    state.engine_state = EngineState::Idle;

    Ok(Some(crystal))
}

// ─── Engine Runner ────────────────────────────────────────────────────────────

/// Run ISLS engine with a RunDescriptor (for deterministic replay, Inv I4)
pub fn run_with_descriptor(
    descriptor: &RunDescriptor,
    obs_batches: &[Vec<Vec<u8>>],
) -> Result<Vec<Option<SemanticCrystal>>> {
    let mut state = GlobalState::new(&descriptor.config);
    let adapter = PassthroughAdapter::new("replay");
    let mut results = Vec::new();

    for batch in obs_batches {
        let result = macro_step(&mut state, batch, &descriptor.config, &adapter)?;
        results.push(result);
    }
    Ok(results)
}

// ─── Temperature Calibration (OI-06) ─────────────────────────────────────────

/// Compute temperature from realized standard deviation of resonance field (OI-06)
pub fn compute_temperature(
    resonance_window: &[f64],
    c_t: f64,
    t_default: f64,
) -> f64 {
    if resonance_window.len() < 2 {
        return t_default;
    }
    let n = resonance_window.len() as f64;
    let mean = resonance_window.iter().sum::<f64>() / n;
    let variance = resonance_window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let sigma = variance.sqrt();
    c_t * sigma
}

/// Temperature regime classification (OI-06)
pub fn temperature_regime(t: f64) -> &'static str {
    if t < 0.5 { "calm" } else if t < 2.0 { "normal" } else { "volatile" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_state_initializes() {
        let config = Config::default();
        let state = GlobalState::new(&config);
        assert_eq!(state.engine_state, EngineState::Idle);
        assert_eq!(state.commit_index, 0);
        assert_eq!(state.active_carrier, 0);
        assert!(!state.phase_ladder.is_empty());
    }

    #[test]
    fn temperature_calibration_basic() {
        let window = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let t = compute_temperature(&window, 5.0, 1.0);
        assert!(t > 0.0);
    }

    #[test]
    fn temperature_regime_classification() {
        assert_eq!(temperature_regime(0.3), "calm");
        assert_eq!(temperature_regime(1.0), "normal");
        assert_eq!(temperature_regime(3.0), "volatile");
    }
}
