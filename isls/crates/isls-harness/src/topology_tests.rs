// isls-harness: Topology integration tests (C16)
// Harness-level smoke tests that complement the AT-T1..T12 unit tests in isls-topology.

use isls_topology::{
    compute_laplacian, spectral_decompose, compute_topological_signature,
    kuramoto_order_parameter, TopologyConfig,
};
use isls_persist::PersistentGraph;

fn make_chain(n: usize) -> PersistentGraph {
    let mut g = PersistentGraph::new();
    for i in 0..n { g.upsert_vertex(i as u64, 0.0); }
    for i in 0..(n - 1) { g.upsert_edge(i as u64, (i + 1) as u64, 0.0); }
    g
}

/// Harness integration: topology signature on a well-known graph has expected properties.
#[test]
fn harness_at_t_topology_integration() {
    let g = make_chain(5);
    let config = TopologyConfig::default();
    let sig = compute_topological_signature(&g, &config);
    assert!(sig.spectral_gap > 0.0, "chain graph must be connected: spectral_gap > 0");
    assert!(!sig.betti_numbers.is_empty());
    assert!(sig.dtl_predicates.contains_key("Connected"));
    assert_eq!(sig.dtl_predicates["Connected"], true);
}

/// Harness integration: Kuramoto order parameter of all-same phases = 1.
#[test]
fn harness_at_t_kuramoto_integration() {
    let phases = vec![std::f64::consts::FRAC_PI_4; 8];
    let (r, _) = kuramoto_order_parameter(&phases);
    assert!((r - 1.0).abs() < 1e-10);
}

/// Harness integration: spectral gap > 0 for connected graph, = 0 for disconnected.
#[test]
fn harness_at_t_spectral_gap_integration() {
    let g_conn = make_chain(4);
    let lap = compute_laplacian(&g_conn);
    let spec = spectral_decompose(&lap, 4);
    assert!(spec.spectral_gap > 0.0);

    let mut g_disc = PersistentGraph::new();
    g_disc.upsert_vertex(0, 0.0);
    g_disc.upsert_vertex(1, 0.0);
    // No edges → disconnected
    let lap_d = compute_laplacian(&g_disc);
    let spec_d = spectral_decompose(&lap_d, 2);
    assert!(spec_d.spectral_gap < 1e-8);
}
