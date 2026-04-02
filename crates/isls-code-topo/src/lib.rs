// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Code topology computation for ISLS — ported from Barbara codex-topo.
//!
//! Computes topological signatures from `CodeObservation` sets and measures
//! similarity between code topologies. Used by the orchestrator to verify that
//! generated code matches the intended architectural blueprint.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use isls_reader::CodeObservation;

pub mod bridge;

// ─── CodeTopology ─────────────────────────────────────────────────────────────

/// Topological signature of a code corpus.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CodeTopology {
    /// Number of distinct code units (functions + structs + tables).
    pub node_count: usize,
    /// Number of relationships (imports + calls + references).
    pub edge_count: usize,
    /// Normalised function signature fingerprints: `name/arity`.
    pub function_signatures: Vec<String>,
    /// All struct/class/type names found.
    pub struct_names: Vec<String>,
    /// Detected architectural layers based on path and naming conventions.
    pub layers: Vec<String>,
    /// Import graph edges: (importer_file, imported_module).
    pub call_graph: Vec<(String, String)>,
    /// Per-language breakdown: language -> LOC.
    pub language_breakdown: BTreeMap<String, usize>,
    /// Fiedler-like connectivity measure (approximated).
    pub connectivity: f64,
}

// ─── Topology computation ─────────────────────────────────────────────────────

/// Compute the topological signature of a set of code observations.
pub fn compute_code_topology(observations: &[CodeObservation]) -> CodeTopology {
    let mut function_signatures = Vec::new();
    let mut struct_names = Vec::new();
    let mut call_graph = Vec::new();
    let mut language_breakdown: BTreeMap<String, usize> = BTreeMap::new();
    let mut layers: Vec<String> = Vec::new();

    for obs in observations {
        *language_breakdown.entry(obs.language.as_str().to_string()).or_insert(0) += obs.loc;

        for f in &obs.functions {
            let sig = format!("{}/{}", f.name, f.params.len());
            function_signatures.push(sig);
        }

        for s in &obs.structs {
            struct_names.push(s.name.clone());
        }

        // Build import edges
        let file_stem = obs.file_path.file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        for import in &obs.imports {
            call_graph.push((file_stem.clone(), import.clone()));
        }

        // Detect layers from path components
        let path_str = obs.file_path.to_string_lossy().to_lowercase();
        let detected_layer = detect_layer(&path_str);
        if !detected_layer.is_empty() && !layers.contains(&detected_layer) {
            layers.push(detected_layer);
        }
    }

    function_signatures.sort();
    function_signatures.dedup();
    struct_names.sort();
    struct_names.dedup();
    layers.sort();

    let node_count = function_signatures.len() + struct_names.len();
    let edge_count = call_graph.len();

    // Approximate connectivity: ratio of edges to possible edges
    let connectivity = if node_count > 1 {
        let max_edges = node_count * (node_count - 1);
        (edge_count as f64 / max_edges as f64).min(1.0)
    } else {
        0.0
    };

    CodeTopology {
        node_count,
        edge_count,
        function_signatures,
        struct_names,
        layers,
        call_graph,
        language_breakdown,
        connectivity,
    }
}

fn detect_layer(path: &str) -> String {
    if path.contains("/models/") || path.contains("/model/") { "model".to_string() }
    else if path.contains("/services/") || path.contains("/service/") { "service".to_string() }
    else if path.contains("/api/") || path.contains("/routes/") || path.contains("/handlers/") { "api".to_string() }
    else if path.contains("/database/") || path.contains("/db/") || path.contains("/migrations/") { "database".to_string() }
    else if path.contains("/tests/") || path.contains("_test.rs") || path.contains("_tests.rs") { "test".to_string() }
    else if path.contains("/frontend/") || path.contains("/pages/") || path.contains("/components/") { "frontend".to_string() }
    else { String::new() }
}

// ─── Similarity ───────────────────────────────────────────────────────────────

/// Compute Jaccard-based similarity between two code topologies.
/// Returns a value in [0.0, 1.0] where 1.0 is identical topology.
pub fn topology_similarity(a: &CodeTopology, b: &CodeTopology) -> f64 {
    // Weighted combination of multiple similarity signals
    let fn_sim = jaccard_similarity(&a.function_signatures, &b.function_signatures);
    let struct_sim = jaccard_similarity(&a.struct_names, &b.struct_names);
    let layer_sim = jaccard_similarity(&a.layers, &b.layers);

    let size_sim = {
        let na = a.node_count as f64;
        let nb = b.node_count as f64;
        if na == 0.0 && nb == 0.0 { 1.0 }
        else if na == 0.0 || nb == 0.0 { 0.0 }
        else { na.min(nb) / na.max(nb) }
    };

    // Hardened weights from Barbara (95% cross-language recognition)
    0.40 * fn_sim + 0.25 * struct_sim + 0.20 * layer_sim + 0.15 * size_sim
}

fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let a_set: std::collections::BTreeSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let b_set: std::collections::BTreeSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let intersection = a_set.intersection(&b_set).count();
    let union = a_set.union(&b_set).count();
    if union == 0 { 1.0 } else { intersection as f64 / union as f64 }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_reader::{parse_string, Language};

    #[test]
    fn compute_topology_basic() {
        let obs = parse_string(
            "pub fn foo(x: i32) -> i32 { x }\npub fn bar() {}",
            Language::Rust,
        ).unwrap();
        let topo = compute_code_topology(&[obs]);
        assert!(topo.function_signatures.contains(&"foo/1".to_string()));
        assert!(topo.function_signatures.contains(&"bar/0".to_string()));
    }

    #[test]
    fn similarity_identical() {
        let src = "pub fn foo(x: i32) {} pub struct Bar {}";
        let obs = parse_string(src, Language::Rust).unwrap();
        let topo = compute_code_topology(&[obs]);
        assert!((topology_similarity(&topo, &topo) - 1.0).abs() < 0.01);
    }

    #[test]
    fn similarity_empty() {
        let a = CodeTopology::default();
        let b = CodeTopology::default();
        assert_eq!(topology_similarity(&a, &b), 1.0);
    }
}
