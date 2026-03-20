/// Full-pipeline integration smoke test.
///
/// Validates: init → ingest → 10 ticks → crystal production → SHA-256 id → archive integrity.
/// Must pass in <30 s, without API keys, without external dependencies.

use std::collections::BTreeMap;
use isls_types::Config;
use isls_observe::PassthroughAdapter;
use isls_engine::{macro_step, GlobalState};
use isls_archive::verify_crystal;

#[test]
fn smoke_pipeline_produces_valid_crystal() {
    // 1. Init — zero all gates so synthetic data can crystallise.
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
    let adapter = PassthroughAdapter::new("smoke-test");

    // 2–3. Ingest synthetic payloads and run 10 ticks.
    let mut crystals = Vec::new();
    for tick in 0..10u64 {
        let payload = format!("smoke-observation-tick-{tick}").into_bytes();
        let result = macro_step(&mut state, &[payload], &config, &adapter)
            .expect("macro_step must not error");
        if let Some(crystal) = result {
            crystals.push(crystal);
        }
    }

    // 4. At least one crystal must have been produced.
    assert!(
        !crystals.is_empty(),
        "at least 1 crystal must emerge within 10 ticks"
    );

    // 5. Crystal id must be a valid SHA-256 hash (32 bytes, non-zero).
    let crystal = &crystals[0];
    assert_eq!(crystal.crystal_id.len(), 32, "crystal_id must be 32 bytes (SHA-256)");
    assert_ne!(crystal.crystal_id, [0u8; 32], "crystal_id must not be all zeros");

    // Verify hex representation is 64 characters.
    let hex_id: String = crystal.crystal_id.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex_id.len(), 64, "hex-encoded crystal_id must be 64 chars");

    // 6. Archive integrity — verify the crystal against its evidence chain.
    assert!(
        !state.archive.is_empty(),
        "archive must contain committed crystal(s)"
    );
    let pinned = BTreeMap::new();
    verify_crystal(crystal, &pinned)
        .expect("crystal must pass archive verification");
}
