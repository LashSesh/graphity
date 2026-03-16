// isls-harness: Extension tests for C12–C15
// AT-R1..5, AT-M1..5, AT-C1..6, AT-S1..5

use std::collections::BTreeMap;
use isls_types::{Config, SchedulerConfig};
use isls_registry::{Registry, RegistryEntry, RegistryKind, RegistrySet};
use isls_manifest::{build_manifest, verify_manifest, TraceEntry};
use isls_capsule::{seal, open, CapsulePolicy};
use isls_scheduler::compute_substeps;
use isls_archive::Archive;

fn make_rd() -> isls_types::RunDescriptor {
    isls_types::RunDescriptor {
        config: Config::default(),
        operator_versions: BTreeMap::new(),
        initial_state_digest: [0u8; 32],
        seed: None,
        registry_digests: BTreeMap::new(),
        scheduler: SchedulerConfig::default(),
    }
}

fn make_entry(name: &str) -> RegistryEntry {
    RegistryEntry::new(
        name.to_string(),
        "1.0.0".to_string(),
        [0u8; 32],
        RegistryKind::Operator,
        BTreeMap::new(),
    )
}

// ─── Registry Tests ───────────────────────────────────────────────────────────

#[test]
fn harness_at_r1_content_address() {
    let entry = make_entry("TestOperator");
    // id is deterministic
    let entry2 = make_entry("TestOperator");
    assert_eq!(entry.id, entry2.id);
}

#[test]
fn harness_at_r5_deterministic_digest() {
    let mut reg1 = Registry::new(RegistryKind::Operator);
    reg1.register(make_entry("Alpha")).unwrap();
    reg1.register(make_entry("Beta")).unwrap();
    let mut reg2 = Registry::new(RegistryKind::Operator);
    reg2.register(make_entry("Beta")).unwrap();
    reg2.register(make_entry("Alpha")).unwrap();
    assert_eq!(reg1.digest, reg2.digest);
}

// ─── Manifest Tests ───────────────────────────────────────────────────────────

#[test]
fn harness_at_m2_full_verify() {
    let rd = make_rd();
    let archive = Archive::new();
    let registries = RegistrySet::new();
    let traces: Vec<TraceEntry> = vec![];
    let obs_log: Vec<Vec<Vec<u8>>> = vec![];

    let manifest = build_manifest(&rd, &traces, &archive, &registries, "discovery", &obs_log);
    let result = verify_manifest(&manifest, &rd, &archive, &traces, &registries);
    assert!(result.is_ok(), "manifest verification failed: {:?}", result);
}

#[test]
fn harness_at_m5_determinism() {
    let rd = make_rd();
    let archive = Archive::new();
    let registries = RegistrySet::new();
    let traces: Vec<TraceEntry> = vec![];
    let obs_log: Vec<Vec<Vec<u8>>> = vec![];

    let m1 = build_manifest(&rd, &traces, &archive, &registries, "discovery", &obs_log);
    let m2 = build_manifest(&rd, &traces, &archive, &registries, "discovery", &obs_log);
    assert_eq!(m1.trace_digests, m2.trace_digests);
    assert_eq!(m1.registry_digests, m2.registry_digests);
}

// ─── Capsule Tests ────────────────────────────────────────────────────────────

const MASTER_KEY: &[u8; 32] = b"harness-master-key-32bytes-test!";

fn make_manifest_for_capsule() -> isls_manifest::ExecutionManifest {
    let rd = make_rd();
    let archive = Archive::new();
    let registries = RegistrySet::new();
    let traces: Vec<TraceEntry> = vec![];
    let obs_log: Vec<Vec<Vec<u8>>> = vec![];
    build_manifest(&rd, &traces, &archive, &registries, "discovery", &obs_log)
}

#[test]
fn harness_at_c1_roundtrip() {
    let manifest = make_manifest_for_capsule();
    let policy = CapsulePolicy {
        require_lock_program_id: [0u8; 32],
        require_rd_digest: manifest.rd_digest,
        require_gate_proofs: vec![],
        require_manifest_id: Some(manifest.run_id),
        expires_at: None,
        max_uses: None,
    };
    let secret = b"harness-secret";
    let capsule = seal(secret, policy, BTreeMap::new(), MASTER_KEY, &manifest).unwrap();
    let recovered = open(&capsule, MASTER_KEY, &manifest, None).unwrap();
    assert_eq!(recovered, secret);
}

#[test]
fn harness_at_c4_expiry() {
    let manifest = make_manifest_for_capsule();
    let policy = CapsulePolicy {
        require_lock_program_id: [0u8; 32],
        require_rd_digest: manifest.rd_digest,
        require_gate_proofs: vec![],
        require_manifest_id: None,
        expires_at: Some(1.0), // long expired
        max_uses: None,
    };
    let capsule = seal(b"secret", policy, BTreeMap::new(), MASTER_KEY, &manifest).unwrap();
    let result = open(&capsule, MASTER_KEY, &manifest, Some(999_999.0));
    assert!(result.is_err());
}

// AT-M4: Replay pack sufficiency — engine replay produces identical crystals
#[test]
fn harness_at_m4_replay_pack_sufficiency() {
    use isls_engine::{execute, ExecuteInput};
    use isls_manifest::build_replay_pack;

    let rd = make_rd();
    let registries = RegistrySet::new();

    // First execute run
    let input1 = ExecuteInput::Program(vec![]);
    let (_, manifest1) = execute(input1, None, &rd.config, &rd, &registries, 3).unwrap();

    // Build replay pack from first run
    let pack = build_replay_pack(
        manifest1.clone(),
        rd.clone(),
        vec![],
        registries.clone(),
        vec![],
    );

    // Second execute run (replay) using pack's RD and registries
    let input2 = ExecuteInput::Program(vec![]);
    let (_, manifest2) = execute(input2, None, &pack.rd.config, &pack.rd, &pack.registries, 3).unwrap();

    // Deterministic replay: identical trace digests
    assert_eq!(manifest1.trace_digests, manifest2.trace_digests,
        "replay must produce identical trace digests");
    assert_eq!(manifest1.crystal_digests, manifest2.crystal_digests,
        "replay must produce identical crystal digests");
}

// ─── Scheduler Tests ─────────────────────────────────────────────────────────

#[test]
fn harness_at_s1_disabled() {
    let cfg = SchedulerConfig { enabled: false, n_min: 1, n_max: 10, ..SchedulerConfig::default() };
    for (d, f, s) in [(0.0, 0.0, 0.0), (1.0, 1.0, 1.0)] {
        assert_eq!(compute_substeps(d, f, s, &cfg), 1);
    }
}

#[test]
fn harness_at_s2_adaptive() {
    let cfg = SchedulerConfig {
        enabled: true,
        n_min: 1,
        n_max: 8,
        strategy: "max_pressure".to_string(),
        ..SchedulerConfig::default()
    };
    assert!(compute_substeps(1.0, 0.0, 0.0, &cfg) > 1);
    assert_eq!(compute_substeps(0.0, 0.0, 0.0, &cfg), 1);
}

#[test]
fn harness_at_s3_determinism() {
    let cfg = SchedulerConfig {
        enabled: true,
        n_min: 1,
        n_max: 5,
        strategy: "max_pressure".to_string(),
        ..SchedulerConfig::default()
    };
    let n1 = compute_substeps(0.6, 0.3, 0.1, &cfg);
    let n2 = compute_substeps(0.6, 0.3, 0.1, &cfg);
    assert_eq!(n1, n2);
}
