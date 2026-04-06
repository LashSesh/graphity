// isls-norms/src/relations.rs — N1/W1: Norm Relation Engine
//
// Computes pairwise relations between norms: Compatible, Dependent,
// Conflicting, or Independent.  Relations are derived from layer-artifact
// overlap (exports/imports), not manually defined.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::Norm;

// ─── Types ──────────────────────────────────────────────────────────────────

/// Relation between two norms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormRelation {
    pub norm_a: String,
    pub norm_b: String,
    pub relation: RelationType,
    pub strength: f64,
    pub shared_resonites: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationType {
    Compatible,
    Dependent,
    Conflicting,
    Independent,
}

/// Complete relation matrix for all norms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormRelationMatrix {
    pub relations: Vec<NormRelation>,
    pub computed_at: String,
    pub norm_count: usize,
}

// ─── Export / Import extraction ─────────────────────────────────────────────

/// Extract "exports" from a norm: struct names, service names, query names,
/// API paths — anything this norm defines that others might reference.
pub fn extract_exports(norm: &Norm) -> BTreeSet<String> {
    let mut exports = BTreeSet::new();
    for m in &norm.layers.model {
        exports.insert(m.struct_name.clone());
    }
    for q in &norm.layers.query {
        exports.insert(q.name.clone());
    }
    for s in &norm.layers.service {
        exports.insert(s.name.clone());
    }
    for a in &norm.layers.api {
        exports.insert(format!("{} {}", a.method, a.path));
    }
    // Also export from trigger keywords (these serve as the norm's "identity")
    for trigger in &norm.triggers {
        for kw in &trigger.keywords {
            exports.insert(kw.clone());
        }
    }
    exports
}

/// Extract "imports" from a norm: its explicit `requires` list plus any
/// type names referenced in query return types or service signatures that
/// are not defined within this norm.
pub fn extract_imports(norm: &Norm) -> BTreeSet<String> {
    let mut imports = BTreeSet::new();
    // Explicit dependencies
    for dep in &norm.requires {
        imports.insert(dep.clone());
    }
    // Referenced return types not defined in this norm
    let defined: BTreeSet<String> = norm.layers.model.iter()
        .map(|m| m.struct_name.clone())
        .collect();
    for q in &norm.layers.query {
        let rt = q.return_type.replace("Vec<", "").replace("Option<", "").replace('>', "");
        let rt = rt.trim().to_string();
        if !rt.is_empty() && !rt.starts_with('{') && !defined.contains(&rt) && rt != "()" {
            imports.insert(rt);
        }
    }
    imports
}

/// Get keyword-based "resonite classes" for a norm — used for compatibility.
fn classify_norm_resonites(norm: &Norm) -> BTreeSet<String> {
    let mut classes = BTreeSet::new();
    for trigger in &norm.triggers {
        for kw in &trigger.keywords {
            classes.insert(kw.clone());
        }
        for concept in &trigger.concepts {
            classes.insert(concept.clone());
        }
    }
    classes
}

/// Get a simple "signature" for a named export within a norm.
/// Uses the first service method matching the name, or a fallback string.
fn get_signature(norm: &Norm, name: &str) -> String {
    for s in &norm.layers.service {
        for sig in &s.method_signatures {
            if sig.contains(name) {
                return sig.clone();
            }
        }
    }
    for q in &norm.layers.query {
        if q.name == name {
            return format!("{} -> {}", q.parameters.join(", "), q.return_type);
        }
    }
    name.to_string()
}

// ─── Pair relation ──────────────────────────────────────────────────────────

pub fn compute_pair_relation(a: &Norm, b: &Norm) -> NormRelation {
    let a_exports = extract_exports(a);
    let b_exports = extract_exports(b);
    let a_imports = extract_imports(a);
    let b_imports = extract_imports(b);

    // Check explicit dependency via `requires`
    let a_requires_b = a.requires.contains(&b.id);
    let b_requires_a = b.requires.contains(&a.id);

    // Dependency: a imports what b exports (by norm ID)
    let a_needs_b: Vec<String> = a_imports.intersection(&b_exports)
        .cloned()
        .chain(if a_requires_b { vec![b.id.clone()] } else { vec![] })
        .collect();
    let b_needs_a: Vec<String> = b_imports.intersection(&a_exports)
        .cloned()
        .chain(if b_requires_a { vec![a.id.clone()] } else { vec![] })
        .collect();

    // Conflict: both export same non-keyword names with different signatures
    let shared_exports: Vec<String> = a_exports.intersection(&b_exports)
        .filter(|name| {
            // Skip keywords — only structural exports matter for conflicts
            !name.contains(' ') || name.starts_with("GET ") || name.starts_with("POST ")
                || name.starts_with("PUT ") || name.starts_with("DELETE ")
        })
        .cloned()
        .collect();
    let conflicting = shared_exports.iter().any(|name| {
        let sig_a = get_signature(a, name);
        let sig_b = get_signature(b, name);
        sig_a != sig_b && sig_a != *name && sig_b != *name
    });

    // Compatible: shared resonite classes (keywords/concepts)
    let a_classes = classify_norm_resonites(a);
    let b_classes = classify_norm_resonites(b);
    let shared_classes: Vec<String> = a_classes.intersection(&b_classes)
        .cloned()
        .collect();

    let (relation, strength, shared) = if conflicting {
        let s = shared_exports.len() as f64 / a_exports.len().max(1) as f64;
        (RelationType::Conflicting, s, shared_exports)
    } else if a_requires_b || !a_needs_b.is_empty() {
        let s = a_needs_b.len() as f64 / a_imports.len().max(1) as f64;
        (RelationType::Dependent, s, a_needs_b)
    } else if b_requires_a || !b_needs_a.is_empty() {
        let s = b_needs_a.len() as f64 / b_imports.len().max(1) as f64;
        (RelationType::Dependent, s, b_needs_a)
    } else if !shared_classes.is_empty() {
        let union_count = a_classes.union(&b_classes).count().max(1);
        let s = shared_classes.len() as f64 / union_count as f64;
        (RelationType::Compatible, s, shared_classes)
    } else {
        (RelationType::Independent, 0.0, vec![])
    };

    NormRelation {
        norm_a: a.id.clone(),
        norm_b: b.id.clone(),
        relation,
        strength,
        shared_resonites: shared,
    }
}

// ─── Full matrix ────────────────────────────────────────────────────────────

pub fn compute_relations(norms: &[Norm]) -> NormRelationMatrix {
    let mut relations = Vec::new();
    for i in 0..norms.len() {
        for j in (i + 1)..norms.len() {
            let rel = compute_pair_relation(&norms[i], &norms[j]);
            if rel.relation != RelationType::Independent {
                relations.push(rel);
            }
        }
    }
    NormRelationMatrix {
        relations,
        computed_at: now_iso(),
        norm_count: norms.len(),
    }
}

fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

// ─── Persistence ────────────────────────────────────────────────────────────

fn relations_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|h| PathBuf::from(h).join(".isls").join("relations.json"))
}

pub fn save_relations(matrix: &NormRelationMatrix, path: Option<&std::path::Path>) -> std::io::Result<()> {
    let p = match path {
        Some(p) => p.to_path_buf(),
        None => match relations_path() {
            Some(p) => p,
            None => return Ok(()),
        },
    };
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(matrix).unwrap_or_default())?;
    Ok(())
}

pub fn load_relations(path: Option<&std::path::Path>) -> Option<NormRelationMatrix> {
    let p = match path {
        Some(p) => p.to_path_buf(),
        None => relations_path()?,
    };
    let content = std::fs::read_to_string(&p).ok()?;
    serde_json::from_str(&content).ok()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::builtin_norms;

    fn find_norm<'a>(norms: &'a [Norm], id: &str) -> &'a Norm {
        norms.iter().find(|n| n.id == id).unwrap()
    }

    #[test]
    fn test_compatible_pair() {
        // CRUD-Entity variants (0042 and 0042-SD) share many keywords
        let norms = builtin_norms();
        let crud = find_norm(&norms, "ISLS-NORM-0042");
        let crud_sd = find_norm(&norms, "ISLS-NORM-0042-SD");
        let rel = compute_pair_relation(crud, crud_sd);
        // Both share the same base keywords ("entity", "crud", etc.)
        // They should be Compatible or Conflicting (same exports).
        assert_ne!(rel.relation, RelationType::Independent,
            "CRUD and CRUD-SoftDelete should have some relation, got Independent");
    }

    #[test]
    fn test_dependency_pair() {
        // CRUD-Entity requires Pagination (ISLS-NORM-0100) and Error-System (ISLS-NORM-0101)
        let norms = builtin_norms();
        let crud = find_norm(&norms, "ISLS-NORM-0042");
        let pagination = find_norm(&norms, "ISLS-NORM-0100");
        let rel = compute_pair_relation(crud, pagination);
        assert_eq!(rel.relation, RelationType::Dependent,
            "CRUD should depend on Pagination, got {:?}", rel.relation);
    }

    #[test]
    fn test_compute_full_matrix() {
        let norms = builtin_norms();
        let matrix = compute_relations(&norms);
        assert_eq!(matrix.norm_count, norms.len());
        // With 20+ norms, there should be many non-independent relations
        assert!(!matrix.relations.is_empty(),
            "Should have some relations, got 0");
        // Check relation types are present
        let has_dep = matrix.relations.iter().any(|r| r.relation == RelationType::Dependent);
        assert!(has_dep, "Should have at least one Dependent relation");
    }

    #[test]
    fn test_extract_exports_nonempty() {
        let norms = builtin_norms();
        let crud = find_norm(&norms, "ISLS-NORM-0042");
        let exports = extract_exports(crud);
        assert!(!exports.is_empty(), "CRUD norm should have exports");
        // Should contain model struct name
        assert!(exports.contains("{Entity}") || exports.iter().any(|e| e.contains("entity")),
            "CRUD exports should include entity-related items");
    }
}
