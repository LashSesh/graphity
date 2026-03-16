// isls-harness: Store integration tests (C17)
// Harness-level smoke tests that complement the AT-D1..D8 unit tests in isls-store.

use isls_store::{IslandStore, CrystalRow};

fn make_store() -> IslandStore {
    IslandStore::open_memory().unwrap()
}

fn mk_crystal(run_id: &str, id: &str) -> CrystalRow {
    CrystalRow {
        crystal_id: id.to_string(),
        run_id: run_id.to_string(),
        stability_score: 0.8,
        free_energy: -0.5,
        created_at_tick: 1,
        carrier_instance: 0,
        constraint_count: 2,
        region_size: 3,
        topology_signature: "{}".to_string(),
        validation_status: "pending".to_string(),
        data: "{}".to_string(),
    }
}

/// Harness integration: full lifecycle — project → run → crystals → finish → query.
#[test]
fn harness_at_d_full_lifecycle() {
    let store = make_store();
    let proj_id = store.create_project("harness-proj", "integration test").unwrap();
    let run_id = store.create_run(&proj_id, "discover", "rdXYZ", 100).unwrap();

    for i in 0..5 {
        store.insert_crystal(&mk_crystal(&run_id, &format!("hc{i}"))).unwrap();
    }

    store.finish_run(&run_id, 5).unwrap();
    let run = store.get_run(&run_id).unwrap();
    assert_eq!(run.crystal_count, 5);
    assert_eq!(run.status, "completed");

    let crystals = store.list_crystals(&run_id).unwrap();
    assert_eq!(crystals.len(), 5);
}

/// Harness integration: settings round-trip.
#[test]
fn harness_at_d_settings() {
    let store = make_store();
    store.set_setting("backend", "sqlite").unwrap();
    let v = store.get_setting("backend").unwrap();
    assert_eq!(v, Some("sqlite".to_string()));
    // Missing key returns None
    assert_eq!(store.get_setting("nonexistent").unwrap(), None);
}

/// Harness integration: alerts lifecycle.
#[test]
fn harness_at_d_alerts() {
    use isls_store::AlertRow;
    let store = make_store();
    let proj_id = store.create_project("alert-proj", "").unwrap();
    let run_id = store.create_run(&proj_id, "discover", "rdA", 10).unwrap();
    store.insert_alert(&AlertRow {
        run_id: run_id.clone(),
        tick: 3,
        metric_id: "M25".to_string(),
        level: "yellow".to_string(),
        message: "spectral gap below 0.01".to_string(),
    }).unwrap();
    let alerts = store.get_alerts(&run_id).unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].metric_id, "M25");
}
