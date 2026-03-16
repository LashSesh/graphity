// isls-harness: Scale integration tests (C18)
// Harness-level smoke tests that complement the AT-SC1..SC15 unit tests in isls-scale.

use isls_scale::{
    HyperBounds, HypercubeUniverse, ScaleConfig, MultiScaleState,
    multi_scale_tick, isls_engine_types,
};
use isls_topology::{
    compute_laplacian, spectral_decompose, init_kuramoto_state, TopologyConfig,
};
use isls_persist::PersistentGraph;

fn make_chain(n: usize) -> PersistentGraph {
    let mut g = PersistentGraph::new();
    for i in 0..n { g.upsert_vertex(i as u64, 0.0); }
    for i in 0..(n - 1) { g.upsert_edge(i as u64, (i + 1) as u64, 1.0); }
    g
}

/// Harness AT-SC-H1: multi_scale_tick returns without panic on empty graph.
#[test]
fn harness_at_sc_empty_graph() {
    let g = PersistentGraph::new();
    let laplacian = compute_laplacian(&g);
    let spectral = spectral_decompose(&laplacian, 4);
    let kuramoto = init_kuramoto_state(&g);
    let micro = isls_engine_types::MicroState::from_graph(&g);
    let mut scale_state = MultiScaleState::default();
    let config = ScaleConfig { enabled: true, ..ScaleConfig::default() };
    let result = multi_scale_tick(&micro, &mut scale_state, &spectral, &kuramoto, &config, &[], 1);
    // With no vertices, meso and macro crystal counts are 0
    assert_eq!(result.meso_crystals.len(), 0);
    assert_eq!(result.macro_crystals.len(), 0);
}

/// Harness AT-SC-H2: HyperBounds split_all produces 32 children.
#[test]
fn harness_at_sc_hyperbounds_split_all() {
    use isls_types::FiveDState;
    let min = FiveDState { p: 0.0, rho: 0.0, omega: 0.0, chi: 0.0, eta: 0.0 };
    let max = FiveDState { p: 1.0, rho: 1.0, omega: 1.0, chi: 1.0, eta: 1.0 };
    let bounds = HyperBounds::new(min, max);
    let children = bounds.split_all();
    assert_eq!(children.len(), 32);
}

/// Harness AT-SC-H3: disabled config produces no crystals.
#[test]
fn harness_at_sc_disabled_returns_empty() {
    let g = make_chain(6);
    let laplacian = compute_laplacian(&g);
    let spectral = spectral_decompose(&laplacian, 4);
    let kuramoto = init_kuramoto_state(&g);
    let micro = isls_engine_types::MicroState::from_graph(&g);
    let mut scale_state = MultiScaleState::default();
    let config = ScaleConfig { enabled: false, ..ScaleConfig::default() };
    let result = multi_scale_tick(&micro, &mut scale_state, &spectral, &kuramoto, &config, &[], 1);
    assert_eq!(result.meso_crystals.len(), 0);
    assert_eq!(result.macro_crystals.len(), 0);
}
