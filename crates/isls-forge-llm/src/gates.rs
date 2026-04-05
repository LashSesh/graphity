// isls-forge-llm/src/gates.rs — I1: Multiskalen-Gate (Mikro + Meso)
//
// Mikro-Gate: per-file validation immediately after generation.
// Meso-Gate: cross-file consistency check after each layer completes.
// Both are informational in I1 (warnings, not blockers).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::codematrix::{
    extract_resonites, compute_codematrix, infer_artifact_type, infer_layer_depth,
    ArtifactType, Codematrix, Resonite,
};
use crate::hdag::{HdagNode, NodeType, ProvidedSymbol};

// ═══════════════════════════════════════════════════════════════════
// Mikro-Gate: Per-File Validation
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MikroGateResult {
    pub pass: bool,
    pub codematrix: Codematrix,
    pub violations: Vec<MikroViolation>,
    pub resonite_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MikroViolation {
    EmptyContent,
    UnknownImport(String),
    NamingViolation { name: String, expected_pattern: String },
    NoImports,
    TopologyMismatch { expected: String, actual: String },
    LowResonance { value: f64, threshold: f64 },
}

/// Default Mikro-Gate threshold (very permissive — only catches garbage).
pub const MIKRO_THRESHOLD: f64 = 0.15;

/// Run Mikro-Gate on a single generated file.
pub fn mikro_gate(
    code: &str,
    node: &HdagNode,
    provided_symbols: &[ProvidedSymbol],
    threshold: f64,
) -> MikroGateResult {
    let resonites = extract_resonites(code, &node.path, isls_reader::Language::Rust);
    let codematrix = compute_codematrix(&resonites, Some(node.layer));
    let mut violations = Vec::new();

    // Check 1: Non-empty content (LLM nodes should have functions)
    if node.node_type == NodeType::Llm {
        let fn_count = resonites.iter()
            .filter(|r| matches!(r, Resonite::Fn { .. }))
            .count();
        if fn_count == 0 {
            violations.push(MikroViolation::EmptyContent);
        }
    }

    // Check 2: All crate:: imports resolve to ProvidedSymbols
    for r in &resonites {
        if let Resonite::Import { path, is_external: false } = r {
            let resolved = provided_symbols.iter().any(|ps| path.contains(&ps.import_path));
            if !resolved {
                violations.push(MikroViolation::UnknownImport(path.clone()));
            }
        }
    }

    // Check 3: LLM files should have imports (unless trivially simple)
    if node.node_type == NodeType::Llm && node.is_rust {
        let import_count = resonites.iter()
            .filter(|r| matches!(r, Resonite::Import { .. }))
            .count();
        if import_count == 0 {
            violations.push(MikroViolation::NoImports);
        }
    }

    // Check 4: Topology conformance
    if let Some(actual_type) = infer_artifact_type(&node.path) {
        let expected_layer = node.layer;
        let actual_layer = infer_layer_depth(&node.path);
        if expected_layer != actual_layer && (expected_layer as i16 - actual_layer as i16).unsigned_abs() > 2 {
            violations.push(MikroViolation::TopologyMismatch {
                expected: format!("layer {}", expected_layer),
                actual: format!("layer {} ({:?})", actual_layer, actual_type),
            });
        }
    }

    // Check 5: Codematrix resonance above threshold
    let res = codematrix.resonance();
    if res < threshold && !resonites.is_empty() {
        violations.push(MikroViolation::LowResonance {
            value: res,
            threshold,
        });
    }

    MikroGateResult {
        pass: violations.is_empty(),
        codematrix,
        violations,
        resonite_count: resonites.len(),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Meso-Gate: Cross-File Consistency
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MesoGateResult {
    pub pass: bool,
    pub layer: u8,
    pub file_count: usize,
    pub consistency_score: f64,
    pub violations: Vec<MesoViolation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MesoViolation {
    DuplicateFunction { name: String, files: Vec<String> },
    MissingDependency { caller: String, callee: String },
    InconsistentNaming { files: Vec<String>, pattern: String },
    HighVariance { axis: String, variance: f64 },
}

/// Run Meso-Gate on all files in a completed layer.
pub fn meso_gate(
    layer_files: &[(String, String, Codematrix)], // (path, code, codematrix)
) -> MesoGateResult {
    let mut violations = Vec::new();

    if layer_files.is_empty() {
        return MesoGateResult {
            pass: true,
            layer: 0,
            file_count: 0,
            consistency_score: 1.0,
            violations,
        };
    }

    // Check 1: No duplicate function definitions across files in layer
    let mut all_fns: HashMap<String, Vec<String>> = HashMap::new();
    for (path, code, _) in layer_files {
        let resonites = extract_resonites(code, path, isls_reader::Language::Rust);
        for r in &resonites {
            if let Resonite::Fn { name, .. } = r {
                all_fns.entry(name.clone()).or_default().push(path.clone());
            }
        }
    }
    for (name, files) in &all_fns {
        if files.len() > 1 {
            violations.push(MesoViolation::DuplicateFunction {
                name: name.clone(),
                files: files.clone(),
            });
        }
    }

    // Check 2: Codematrix variance within layer (R, F, S axes)
    if layer_files.len() >= 2 {
        let cms: Vec<&Codematrix> = layer_files.iter().map(|(_, _, cm)| cm).collect();
        for (axis_name, values) in &[
            ("R", cms.iter().map(|cm| cm.r).collect::<Vec<_>>()),
            ("F", cms.iter().map(|cm| cm.f).collect::<Vec<_>>()),
            ("S", cms.iter().map(|cm| cm.s).collect::<Vec<_>>()),
        ] {
            let n = values.len() as f64;
            let mean = values.iter().sum::<f64>() / n;
            let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
            if var > 0.15 {
                violations.push(MesoViolation::HighVariance {
                    axis: axis_name.to_string(),
                    variance: var,
                });
            }
        }
    }

    let consistency = (1.0 - violations.len() as f64 * 0.2).max(0.0);
    let layer = infer_layer_depth(&layer_files[0].0);

    MesoGateResult {
        pass: violations.is_empty(),
        layer,
        file_count: layer_files.len(),
        consistency_score: consistency,
        violations,
    }
}

// ═══════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hdag::NodeType;

    fn make_node(path: &str, layer: u8, node_type: NodeType) -> HdagNode {
        HdagNode {
            index: 0,
            path: path.to_string(),
            node_type,
            layer,
            entity: None,
            is_rust: true,
            purpose: String::new(),
        }
    }

    #[test]
    fn test_mikro_gate_passes_for_valid_code() {
        let code = r#"
use crate::errors::AppError;
pub fn get_item(id: i64) -> Result<String, AppError> { Ok("item".into()) }
pub fn list_items() -> Vec<String> { vec![] }
"#;
        let node = make_node("backend/src/services/item.rs", 5, NodeType::Llm);
        let provided = vec![ProvidedSymbol {
            import_path: "crate::errors::AppError".into(),
            kind: crate::hdag::SymbolKind::Enum,
            signature: String::new(),
        }];
        let result = mikro_gate(code, &node, &provided, MIKRO_THRESHOLD);
        assert!(result.pass, "Mikro-gate should pass: {:?}", result.violations);
    }

    #[test]
    fn test_mikro_gate_fails_for_empty_content() {
        let code = "// empty file\n";
        let node = make_node("backend/src/services/item.rs", 5, NodeType::Llm);
        let result = mikro_gate(code, &node, &[], MIKRO_THRESHOLD);
        assert!(!result.pass);
        assert!(result.violations.iter().any(|v| matches!(v, MikroViolation::EmptyContent)));
    }

    #[test]
    fn test_meso_gate_detects_duplicate_functions() {
        let code_a = "pub fn get_item() {}\npub fn list_items() {}";
        let code_b = "pub fn get_item() {}\npub fn delete_item() {}";
        let cm = Codematrix { r: 0.5, f: 0.5, t: 0.5, s: 0.5, e: 0.5 };
        let files = vec![
            ("a.rs".into(), code_a.into(), cm),
            ("b.rs".into(), code_b.into(), cm),
        ];
        let result = meso_gate(&files);
        assert!(!result.pass);
        assert!(result.violations.iter().any(|v| matches!(v, MesoViolation::DuplicateFunction { .. })));
    }

    #[test]
    fn test_meso_gate_passes_for_consistent_layer() {
        let code_a = "pub fn get_pet() {}\npub fn list_pets() {}";
        let code_b = "pub fn get_owner() {}\npub fn list_owners() {}";
        let cm = Codematrix { r: 0.5, f: 0.6, t: 0.9, s: 0.8, e: 0.5 };
        let files = vec![
            ("backend/src/database/pet_queries.rs".into(), code_a.into(), cm),
            ("backend/src/database/owner_queries.rs".into(), code_b.into(), cm),
        ];
        let result = meso_gate(&files);
        assert!(result.pass, "Meso-gate should pass: {:?}", result.violations);
    }
}
