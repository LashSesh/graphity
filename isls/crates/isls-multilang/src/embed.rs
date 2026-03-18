// isls-multilang/src/embed.rs
//
// H5 structural embedding — 5-axis validation.
//
// Mirrors glyph-embed (Babylon Compiler) structural contract.
// Axes: structural coupling, functional density, topological complexity,
//       symmetry, entropy.  All values in Q16-equivalent f64 [0.0, 1.0].

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::glyph_ir::{IrDocument, NodeKind};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum EmbedError {
    #[error("H5 axis {axis} value {value:.4} outside range [{min:.4}, {max:.4}]: {reason}")]
    AxisOutOfRange {
        axis: String,
        value: f64,
        min: f64,
        max: f64,
        reason: String,
    },
}

// ─── H5 Embedding ────────────────────────────────────────────────────────────

/// 5-axis structural embedding.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct H5Embedding {
    /// a1: structural coupling — ratio of edges to (nodes^2)
    pub structural_coupling: f64,
    /// a2: functional density — ratio of Function nodes to total nodes
    pub functional_density: f64,
    /// a3: topological complexity — avg out-degree / log2(n+1)
    pub topological_complexity: f64,
    /// a4: symmetry — fraction of edges with bidirectional counterpart
    pub symmetry: f64,
    /// a5: entropy — normalized Shannon entropy of node kind distribution
    pub entropy: f64,
}

impl H5Embedding {
    pub fn axes(&self) -> [f64; 5] {
        [
            self.structural_coupling,
            self.functional_density,
            self.topological_complexity,
            self.symmetry,
            self.entropy,
        ]
    }
}

// ─── Config ───────────────────────────────────────────────────────────────────

/// Range configuration for each axis.  Defaults from spec §6.
#[derive(Clone, Debug)]
pub struct EmbedConfig {
    pub a1_range: [f64; 2],
    pub a2_range: [f64; 2],
    pub a3_range: [f64; 2],
    pub a4_range: [f64; 2],
    pub a5_range: [f64; 2],
    /// If true, out-of-range axes are errors; otherwise warnings only.
    pub hard_gate: bool,
}

impl Default for EmbedConfig {
    fn default() -> Self {
        Self {
            a1_range: [0.05, 0.80],
            a2_range: [0.10, 0.90],
            a3_range: [0.0,  0.70],
            a4_range: [0.20, 1.0],
            a5_range: [0.10, 0.90],
            hard_gate: false, // diagnostic, not a hard gate per spec
        }
    }
}

// ─── compute_embedding ────────────────────────────────────────────────────────

/// Compute the 5-axis H5 embedding for an IrDocument.
pub fn compute_embedding(doc: &IrDocument) -> H5Embedding {
    let n = doc.nodes.len();
    let e = doc.edges.len();

    // a1: structural coupling = edges / max_possible_edges
    let max_edges = if n > 1 { (n * (n - 1)) as f64 } else { 1.0 };
    let a1 = clamp01(e as f64 / max_edges);

    // a2: functional density = function_nodes / total_nodes
    let fn_count = doc.nodes.iter().filter(|node| node.kind == NodeKind::Function).count();
    let a2 = if n == 0 { 0.0 } else { clamp01(fn_count as f64 / n as f64) };

    // a3: topological complexity = avg_out_degree / log2(n+1)
    let mut out_degree: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for edge in &doc.edges {
        *out_degree.entry(edge.from.as_str()).or_insert(0) += 1;
    }
    let avg_out = if n == 0 { 0.0 } else { out_degree.values().sum::<usize>() as f64 / n as f64 };
    let log_n = ((n + 1) as f64).log2().max(1.0);
    let a3 = clamp01(avg_out / log_n);

    // a4: symmetry = edges with reverse counterpart / total edges
    use std::collections::HashSet;
    let edge_set: HashSet<(&str, &str)> = doc.edges.iter()
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    let sym_count = doc.edges.iter()
        .filter(|e| edge_set.contains(&(e.to.as_str(), e.from.as_str())))
        .count();
    let a4 = if e == 0 { 1.0 } else { clamp01(sym_count as f64 / e as f64) };

    // a5: entropy = normalized Shannon entropy of node kind distribution
    let mut kind_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for node in &doc.nodes {
        *kind_counts.entry(node.kind.as_str()).or_insert(0) += 1;
    }
    let entropy = if n == 0 {
        0.0
    } else {
        let h: f64 = kind_counts.values()
            .map(|&c| {
                let p = c as f64 / n as f64;
                if p > 0.0 { -p * p.log2() } else { 0.0 }
            })
            .sum();
        let max_h = (kind_counts.len() as f64).log2().max(1.0);
        clamp01(h / max_h)
    };

    H5Embedding {
        structural_coupling: a1,
        functional_density: a2,
        topological_complexity: a3,
        symmetry: a4,
        entropy,
    }
}

/// Validate embedding axes against config ranges.
/// Returns Err only if hard_gate is true and an axis is out of range.
/// Always logs warnings for out-of-range axes.
pub fn validate_embedding(emb: &H5Embedding, cfg: &EmbedConfig) -> Result<(), EmbedError> {
    let checks = [
        ("a1_structural_coupling",   emb.structural_coupling,    cfg.a1_range, "zero=disconnected, high=spaghetti"),
        ("a2_functional_density",    emb.functional_density,     cfg.a2_range, "zero=no functions, high=fragmented"),
        ("a3_topological_complexity",emb.topological_complexity, cfg.a3_range, "high=over-nested"),
        ("a4_symmetry",              emb.symmetry,               cfg.a4_range, "low=lopsided modules"),
        ("a5_entropy",               emb.entropy,                cfg.a5_range, "low=repetitive, high=incoherent"),
    ];

    for (name, value, range, reason) in checks {
        if value < range[0] || value > range[1] {
            tracing::warn!(
                axis = name, value = value, min = range[0], max = range[1],
                "H5 embedding axis out of range: {}", reason
            );
            if cfg.hard_gate {
                return Err(EmbedError::AxisOutOfRange {
                    axis: name.to_string(),
                    value,
                    min: range[0],
                    max: range[1],
                    reason: reason.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph_ir::{IrDocument, IrNode, IrEdge, NodeKind, EdgeKind};

    fn small_doc() -> IrDocument {
        let mut doc = IrDocument::new("test", "test-artifact");
        let root = IrNode::new("n_root", NodeKind::Module, "root");
        let f1 = IrNode::new("n_fn_0", NodeKind::Function, "foo");
        let f2 = IrNode::new("n_fn_1", NodeKind::Function, "bar");
        doc.nodes.push(root);
        doc.nodes.push(f1);
        doc.nodes.push(f2);
        doc.edges.push(IrEdge::new("n_root", "n_fn_0", EdgeKind::Contains));
        doc.edges.push(IrEdge::new("n_root", "n_fn_1", EdgeKind::Contains));
        doc.edges.push(IrEdge::new("n_fn_0", "n_fn_1", EdgeKind::CalleeRef));
        doc.canonicalize();
        doc
    }

    // AT-BB3: Compute embedding; verify all 5 axes in valid range [0.0, 1.0].
    #[test]
    fn at_bb3_h5_embedding() {
        let doc = small_doc();
        let emb = compute_embedding(&doc);
        for &axis in emb.axes().iter() {
            assert!((0.0..=1.0).contains(&axis),
                "AT-BB3: H5 axis {axis} out of [0,1]");
        }
    }

    #[test]
    fn embedding_soft_gate_passes() {
        let doc = small_doc();
        let emb = compute_embedding(&doc);
        let cfg = EmbedConfig { hard_gate: false, ..Default::default() };
        // Should not error even if some axes are marginal
        let _ = validate_embedding(&emb, &cfg);
    }
}
