// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Ophanim Coherer — structural code similarity across n candidates.
//!
//! Parses each candidate using `isls_reader::parse_string(c_k, Language::Rust)`
//! and computes per-Thronengel resonance product D_k from:
//!
//! - ψ_k: agreement (mean similarity to all other candidates)
//! - ρ_k: content richness (capped function + struct count)
//! - ω_k: structural validity (has functions AND use statements?)
//!
//! Barbara weights (from `isls-code-topo`): 0.40 fn + 0.25 struct + 0.20 import + 0.15 size.

use std::collections::HashSet;

use isls_reader::{CodeObservation, Language};

/// Extracted structural features from a code candidate.
#[derive(Clone, Debug)]
pub struct CodeFeatures {
    /// Function names with arity, e.g. `"get_trade/2"`.
    pub functions: HashSet<String>,
    /// Struct and enum names.
    pub structs: HashSet<String>,
    /// Import paths (from `use` statements).
    pub imports: HashSet<String>,
    /// Line count.
    pub loc: usize,
    /// Whether parsing succeeded (false for empty/unparseable).
    pub parsed: bool,
}

/// Extract structural features from a code string using `isls_reader`.
pub fn extract_features(code: &str) -> CodeFeatures {
    if code.trim().is_empty() {
        return CodeFeatures {
            functions: HashSet::new(),
            structs: HashSet::new(),
            imports: HashSet::new(),
            loc: 0,
            parsed: false,
        };
    }

    match isls_reader::parse_string(code, Language::Rust) {
        Ok(obs) => features_from_observation(&obs),
        Err(_) => {
            // Fallback: count lines, no structural info
            CodeFeatures {
                functions: HashSet::new(),
                structs: HashSet::new(),
                imports: HashSet::new(),
                loc: code.lines().count(),
                parsed: false,
            }
        }
    }
}

fn features_from_observation(obs: &CodeObservation) -> CodeFeatures {
    let functions: HashSet<String> = obs
        .functions
        .iter()
        .map(|f| format!("{}/{}", f.name, f.params.len()))
        .collect();

    let structs: HashSet<String> = obs
        .structs
        .iter()
        .map(|s| s.name.clone())
        .collect();

    let imports: HashSet<String> = obs.imports.iter().cloned().collect();

    CodeFeatures {
        functions,
        structs,
        imports,
        loc: obs.loc,
        parsed: true,
    }
}

/// Jaccard similarity: |A ∩ B| / |A ∪ B|.
///
/// Returns 1.0 if both sets are empty (vacuous agreement).
pub fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        return 0.0;
    }
    intersection / union
}

/// Pairwise structural similarity using Barbara weights.
///
/// ```text
/// sim(a,b) = 0.40 * J(Fa,Fb) + 0.25 * J(Sa,Sb) + 0.20 * J(Ia,Ib) + 0.15 * min(La,Lb)/max(La,Lb)
/// ```
pub fn similarity(a: &CodeFeatures, b: &CodeFeatures) -> f64 {
    let fn_sim = jaccard(&a.functions, &b.functions);
    let struct_sim = jaccard(&a.structs, &b.structs);
    let import_sim = jaccard(&a.imports, &b.imports);

    let size_sim = if a.loc == 0 && b.loc == 0 {
        1.0
    } else if a.loc == 0 || b.loc == 0 {
        0.0
    } else {
        let la = a.loc as f64;
        let lb = b.loc as f64;
        la.min(lb) / la.max(lb)
    };

    0.40 * fn_sim + 0.25 * struct_sim + 0.20 * import_sim + 0.15 * size_sim
}

/// Compute per-candidate resonance product D_k for all candidates.
///
/// Returns a Vec of D_k values, one per candidate. Also returns the pairwise
/// similarity matrix (flattened upper triangle) for use by the Konus lens.
///
/// # Algorithm
///
/// For each candidate k:
/// - ψ_k = mean similarity to all other candidates (agreement)
/// - ρ_k = min(1.0, (|F| + |S|) / 5) (content richness, capped)
/// - ω_k = structural validity multiplier (1.0/0.5/0.0)
/// - D_k = ψ_k * ρ_k * ω_k (with ψ_k < 0.15 outlier cutoff)
pub fn compute_resonance(candidates: &[String]) -> (Vec<f64>, Vec<f64>) {
    let n = candidates.len();
    if n == 0 {
        return (vec![], vec![]);
    }
    if n == 1 {
        let feat = extract_features(&candidates[0]);
        let rho = (feat.functions.len() + feat.structs.len()) as f64 / 5.0;
        let rho = rho.min(1.0);
        let omega = structural_validity(&feat);
        return (vec![rho * omega], vec![]);
    }

    // Extract features for all candidates
    let features: Vec<CodeFeatures> = candidates.iter().map(|c| extract_features(c)).collect();

    // Compute pairwise similarity matrix (upper triangle)
    let mut pairwise: Vec<f64> = Vec::new();
    let mut sim_matrix = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let s = similarity(&features[i], &features[j]);
            sim_matrix[i][j] = s;
            sim_matrix[j][i] = s;
            pairwise.push(s);
        }
    }

    // Compute D_k for each candidate
    let mut dk = Vec::with_capacity(n);
    for k in 0..n {
        // ψ_k: mean similarity to all others (agreement)
        let psi: f64 = (0..n)
            .filter(|&j| j != k)
            .map(|j| sim_matrix[k][j])
            .sum::<f64>()
            / (n - 1) as f64;

        // ρ_k: content richness (capped at 1.0)
        let rho = ((features[k].functions.len() + features[k].structs.len()) as f64 / 5.0)
            .min(1.0);

        // ω_k: structural validity
        let omega = structural_validity(&features[k]);

        // D_k with outlier cutoff
        let d = if psi < 0.15 { 0.0 } else { psi * rho * omega };
        dk.push(d);
    }

    (dk, pairwise)
}

/// Extract features for all candidates (exposed for Monolith's Coagula).
pub fn extract_all_features(candidates: &[String]) -> Vec<CodeFeatures> {
    candidates.iter().map(|c| extract_features(c)).collect()
}

/// Structural validity multiplier ω_k.
///
/// - 1.0 if candidate has ≥1 function AND ≥1 use statement
/// - 0.5 if only partial structure (has one but not the other)
/// - 0.0 if empty or no Rust structure found
fn structural_validity(feat: &CodeFeatures) -> f64 {
    let has_functions = !feat.functions.is_empty();
    let has_imports = !feat.imports.is_empty();

    if has_functions && has_imports {
        1.0
    } else if has_functions || has_imports {
        0.5
    } else {
        0.0
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const CRUD_CODE_A: &str = r#"
use sqlx::PgPool;
use crate::models::Product;
use crate::errors::AppError;

pub async fn get_product(pool: &PgPool, id: i64) -> Result<Product, AppError> {
    sqlx::query_as::<_, Product>("SELECT * FROM products WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or(AppError::NotFound("Product not found".into()))
}

pub async fn list_products(pool: &PgPool) -> Result<Vec<Product>, AppError> {
    sqlx::query_as::<_, Product>("SELECT * FROM products")
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
}

pub async fn create_product(pool: &PgPool, name: &str) -> Result<Product, AppError> {
    sqlx::query_as::<_, Product>("INSERT INTO products (name) VALUES ($1) RETURNING *")
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
}
"#;

    // Nearly identical to A — same structure, minor wording differences
    const CRUD_CODE_B: &str = r#"
use sqlx::PgPool;
use crate::models::Product;
use crate::errors::AppError;

pub async fn get_product(pool: &PgPool, id: i64) -> Result<Product, AppError> {
    sqlx::query_as::<_, Product>("SELECT * FROM products WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Product {} not found", id)))
}

pub async fn list_products(pool: &PgPool) -> Result<Vec<Product>, AppError> {
    sqlx::query_as::<_, Product>("SELECT * FROM products ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
}

pub async fn create_product(pool: &PgPool, name: &str) -> Result<Product, AppError> {
    sqlx::query_as::<_, Product>("INSERT INTO products (name) VALUES ($1) RETURNING *")
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
}
"#;

    const COMPLETELY_DIFFERENT: &str = r#"
fn fibonacci(n: u32) -> u32 {
    if n <= 1 { return n; }
    fibonacci(n - 1) + fibonacci(n - 2)
}
"#;

    #[test]
    fn identical_candidates_high_resonance() {
        let candidates = vec![
            CRUD_CODE_A.to_string(),
            CRUD_CODE_A.to_string(),
            CRUD_CODE_A.to_string(),
            CRUD_CODE_A.to_string(),
        ];
        let (dk, _) = compute_resonance(&candidates);
        assert_eq!(dk.len(), 4);
        for d in &dk {
            assert!(*d > 0.4, "identical candidates should have high D_k, got {}", d);
        }
        // All D_k should be equal for identical candidates
        let first = dk[0];
        for d in &dk[1..] {
            assert!((d - first).abs() < 0.001, "identical candidates should have equal D_k");
        }
    }

    #[test]
    fn similar_candidates_moderate_resonance() {
        let candidates = vec![
            CRUD_CODE_A.to_string(),
            CRUD_CODE_B.to_string(),
            CRUD_CODE_A.to_string(),
            CRUD_CODE_B.to_string(),
        ];
        let (dk, _) = compute_resonance(&candidates);
        assert_eq!(dk.len(), 4);
        for d in &dk {
            assert!(*d > 0.3, "similar candidates should have moderate D_k, got {}", d);
        }
    }

    #[test]
    fn completely_different_low_resonance() {
        let candidates = vec![
            CRUD_CODE_A.to_string(),
            COMPLETELY_DIFFERENT.to_string(),
            "// empty".to_string(),
            "fn unrelated() {}".to_string(),
        ];
        let (dk, _) = compute_resonance(&candidates);
        assert_eq!(dk.len(), 4);
        // At least some should be low
        let max_d = dk.iter().cloned().fold(0.0_f64, f64::max);
        assert!(max_d < 0.8, "heterogeneous swarm max D_k should be < 0.8, got {}", max_d);
    }

    #[test]
    fn empty_candidate_gets_zero_dk() {
        let candidates = vec![
            CRUD_CODE_A.to_string(),
            CRUD_CODE_A.to_string(),
            String::new(),
            CRUD_CODE_A.to_string(),
        ];
        let (dk, _) = compute_resonance(&candidates);
        assert_eq!(dk[2], 0.0, "empty candidate must have D_k = 0");
    }

    #[test]
    fn jaccard_identical_sets() {
        let a: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(jaccard(&a, &a), 1.0);
    }

    #[test]
    fn jaccard_disjoint_sets() {
        let a: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(jaccard(&a, &b), 0.0);
    }

    #[test]
    fn jaccard_empty_sets() {
        let empty: HashSet<String> = HashSet::new();
        assert_eq!(jaccard(&empty, &empty), 1.0);
    }

    #[test]
    fn jaccard_partial_overlap() {
        let a: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let j = jaccard(&a, &b);
        assert!((j - 0.5).abs() < 0.01, "J({{a,b,c}},{{b,c,d}}) should be 0.5, got {}", j);
    }

    #[test]
    fn single_candidate_returns_richness() {
        let (dk, pairwise) = compute_resonance(&[CRUD_CODE_A.to_string()]);
        assert_eq!(dk.len(), 1);
        assert!(dk[0] > 0.0, "single valid candidate should have nonzero D_k");
        assert!(pairwise.is_empty(), "single candidate has no pairwise sims");
    }

    #[test]
    fn pairwise_sims_returned() {
        let candidates = vec![
            CRUD_CODE_A.to_string(),
            CRUD_CODE_B.to_string(),
            CRUD_CODE_A.to_string(),
        ];
        let (dk, pairwise) = compute_resonance(&candidates);
        assert_eq!(dk.len(), 3);
        // n=3: upper triangle has n*(n-1)/2 = 3 pairs
        assert_eq!(pairwise.len(), 3);
    }
}
