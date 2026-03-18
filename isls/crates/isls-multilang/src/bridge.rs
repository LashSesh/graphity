// isls-multilang/src/bridge.rs
//
// ArtifactIR → glyph_ir::IrDocument conversion (Def 3.1 of spec).
//
// Deterministic and content-addressed: same ArtifactIR → same IrDocument
// with identical digest.

use std::collections::BTreeMap;
use thiserror::Error;
use serde_json::json;

use isls_artifact_ir::ArtifactIR;
use crate::glyph_ir::{EdgeKind, IrDocument, IrEdge, IrNode, NodeKind};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("empty ArtifactIR: no components")]
    Empty,
}

// ─── Bridge Conversion ───────────────────────────────────────────────────────

/// Convert ArtifactIR to glyph_ir::IrDocument (Definition 3.1).
///
/// Mapping:
/// - One root Module node.
/// - Each ArtifactIR component → IrNode::Function with metadata.
/// - Each ArtifactIR interface → IrEdge::CalleeRef between named nodes.
/// - After construction: doc.canonicalize() to set digest.
pub fn artifact_to_glyph_ir(ir: &ArtifactIR) -> Result<IrDocument, BridgeError> {
    let artifact_hex = hex_encode(&ir.header.artifact_id);
    let mut doc = IrDocument::new(&ir.header.domain, &artifact_hex);

    // Root module node
    let root = IrNode::new("n_root", NodeKind::Module, "root");
    doc.nodes.push(root);

    // Each component → Function node with metadata
    for (i, comp) in ir.components.iter().enumerate() {
        let id = format!("n_fn_{i}");
        let mut node = IrNode::new(&id, NodeKind::Function, &comp.name);

        // Dependencies become params
        node.params = if comp.dependencies.is_empty() {
            None
        } else {
            Some(comp.dependencies.clone())
        };

        // Store component content as properties for Oracle prompts
        let mut props = BTreeMap::new();
        props.insert("kind".into(), json!(comp.kind));
        props.insert("description".into(), json!(comp.content));
        props.insert("component_id".into(), json!(comp.id));
        node.properties = Some(props);

        doc.nodes.push(node);
        doc.edges.push(IrEdge::new("n_root", &id, EdgeKind::Contains));
    }

    // Interfaces → CalleeRef edges between named nodes
    for iface in &ir.interfaces {
        // Interfaces store provider/consumer as component IDs (hex).
        // Try matching by component ID first, then by name.
        let provider_id = find_node_id_by_comp_id(&doc, &iface.provider)
            .or_else(|| doc.find_node_id_by_name(&iface.provider));
        let consumer_id = find_node_id_by_comp_id(&doc, &iface.consumer)
            .or_else(|| doc.find_node_id_by_name(&iface.consumer));

        if let (Some(p), Some(c)) = (provider_id, consumer_id) {
            doc.edges.push(IrEdge::new(&p, &c, EdgeKind::CalleeRef));
        }
    }

    doc.canonicalize();
    Ok(doc)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn find_node_id_by_comp_id(doc: &IrDocument, comp_id: &str) -> Option<String> {
    doc.nodes.iter()
        .find(|n| {
            n.properties.as_ref()
                .and_then(|p| p.get("component_id"))
                .and_then(|v| v.as_str())
                .map(|id| id == comp_id)
                .unwrap_or(false)
        })
        .map(|n| n.id.clone())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_pmhd::{DecisionSpec, DrillEngine, PmhdConfig, QualityThresholds};
    use std::collections::BTreeMap;

    fn make_ir() -> ArtifactIR {
        let mut goals = BTreeMap::new();
        goals.insert("coherence".to_string(), 0.6);
        let spec = DecisionSpec::new(
            "REST API health check",
            goals,
            vec!["must return JSON".to_string()],
            "rust",
            PmhdConfig {
                ticks: 8,
                pool_size: 4,
                commit_budget: 2,
                thresholds: QualityThresholds::default(),
                ..Default::default()
            },
        );
        let mut eng = DrillEngine::new(spec.config.clone());
        let res = eng.drill(&spec);
        let monolith = res.monoliths.into_iter().next().expect("monolith");
        ArtifactIR::build_from_monolith(&monolith, &spec, 0).expect("ArtifactIR")
    }

    // AT-BB1: node count = component count + 1 (root)
    #[test]
    fn at_bb1_ir_conversion() {
        let ir = make_ir();
        let comp_count = ir.components.len();
        let doc = artifact_to_glyph_ir(&ir).expect("bridge");
        assert_eq!(
            doc.nodes.len(),
            comp_count + 1,
            "AT-BB1: node count must be component_count + 1 (root)"
        );
    }

    // AT-BB2: Convert same ArtifactIR twice → identical digest
    #[test]
    fn at_bb2_ir_determinism() {
        let ir = make_ir();
        let doc1 = artifact_to_glyph_ir(&ir).expect("bridge 1");
        let doc2 = artifact_to_glyph_ir(&ir).expect("bridge 2");
        assert_eq!(
            doc1.digest, doc2.digest,
            "AT-BB2: digest must be identical for same ArtifactIR"
        );
    }
}
