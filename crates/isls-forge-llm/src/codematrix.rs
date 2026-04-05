// isls-forge-llm/src/codematrix.rs — I1: 5D Codematrix + Resonite Observables
//
// Every generated Rust file is measured on 5 axes:
//   R (Relationality), F (Functional Cohesion), T (Topology conformance),
//   S (Symmetry), E (Entropy).
// Result: a [f64; 5] fingerprint per file.
//
// Resonites are the atomic observables extracted from Rust source code.
// The Codematrix is computed FROM the Resonite set.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════
// Resonite: Atomic Observable Primitives
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum Resonite {
    Fn {
        name: String,
        arity: usize,
        return_type: String,
        is_pub: bool,
        is_async: bool,
    },
    Type {
        name: String,
        kind: TypeKind,
        field_count: usize,
        derive_count: usize,
    },
    Import {
        path: String,
        is_external: bool,
    },
    Layer {
        depth: u8,
        artifact_type: ArtifactType,
    },
    Relation {
        source: String,
        target: String,
        kind: RelationKind,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum TypeKind {
    Struct,
    Enum,
    Trait,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum ArtifactType {
    Model,
    Query,
    Service,
    Api,
    Config,
    Auth,
    Migration,
    Frontend,
    Test,
    Static,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum RelationKind {
    ForeignKey,
    Implements,
    Uses,
    Contains,
}

// ═══════════════════════════════════════════════════════════════════
// 5D Codematrix
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Codematrix {
    pub r: f64, // Relationality [0, 1]
    pub f: f64, // Functional Cohesion [0, 1]
    pub t: f64, // Topology conformance [0, 1]
    pub s: f64, // Symmetry [0, 1]
    pub e: f64, // Entropy [0, 1]
}

impl Codematrix {
    /// Resonance product (geometric mean of all 5 axes).
    pub fn resonance(&self) -> f64 {
        (self.r * self.f * self.t * self.s * self.e).powf(0.2)
    }

    /// Euclidean distance between two codematrix points.
    pub fn distance(&self, other: &Codematrix) -> f64 {
        ((self.r - other.r).powi(2)
            + (self.f - other.f).powi(2)
            + (self.t - other.t).powi(2)
            + (self.s - other.s).powi(2)
            + (self.e - other.e).powi(2))
        .sqrt()
    }

    /// Average of multiple codematrix points.
    pub fn average(points: &[Codematrix]) -> Codematrix {
        if points.is_empty() {
            return Codematrix::default();
        }
        let n = points.len() as f64;
        Codematrix {
            r: points.iter().map(|p| p.r).sum::<f64>() / n,
            f: points.iter().map(|p| p.f).sum::<f64>() / n,
            t: points.iter().map(|p| p.t).sum::<f64>() / n,
            s: points.iter().map(|p| p.s).sum::<f64>() / n,
            e: points.iter().map(|p| p.e).sum::<f64>() / n,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Resonite Extraction
// ═══════════════════════════════════════════════════════════════════

/// Extract all Resonites from source code in any supported language.
///
/// `lang` selects the parser. Pass `isls_reader::Language::Rust` to preserve
/// the previous behaviour. All languages produce the same Resonite variants.
pub fn extract_resonites(
    code: &str,
    file_path: &str,
    lang: isls_reader::Language,
) -> Vec<Resonite> {
    let parsed = match isls_reader::parse_string(code, lang) {
        Ok(obs) => obs,
        Err(_) => return vec![],
    };

    let mut resonites = Vec::new();

    // Functions
    for func in &parsed.functions {
        resonites.push(Resonite::Fn {
            name: func.name.clone(),
            arity: func.params.len(),
            return_type: func.return_type.clone().unwrap_or_default(),
            is_pub: func.is_public,
            is_async: func.is_async,
        });
    }

    // Types (structs / classes / interfaces)
    for s in &parsed.structs {
        let kind = if s.derives.iter().any(|d| d == "interface") {
            TypeKind::Trait
        } else if s.derives.iter().any(|d| d.contains("Enum")) {
            TypeKind::Enum
        } else {
            TypeKind::Struct
        };
        resonites.push(Resonite::Type {
            name: s.name.clone(),
            kind,
            field_count: s.fields.len(),
            derive_count: s.derives.len(),
        });
    }

    // Imports
    for import in &parsed.imports {
        let is_external = import.starts_with("ext:")
            || (!import.starts_with("crate::") && !import.starts_with('.'));
        let path = import.trim_start_matches("ext:").to_string();
        resonites.push(Resonite::Import { path, is_external });
    }

    // Layer (inferred from file path)
    if let Some(artifact_type) = infer_artifact_type(file_path) {
        resonites.push(Resonite::Layer {
            depth: infer_layer_depth(file_path),
            artifact_type,
        });
    }

    // Relations (FK from struct field patterns like "-> Entity")
    for s in &parsed.structs {
        for field in &s.fields {
            if field.ends_with("_id") || field.contains("-> ") {
                resonites.push(Resonite::Relation {
                    source: s.name.clone(),
                    target: field.clone(),
                    kind: RelationKind::ForeignKey,
                });
            }
        }
    }

    resonites
}

/// Infer artifact type from file path.
pub fn infer_artifact_type(path: &str) -> Option<ArtifactType> {
    let lower = path.to_lowercase();
    if lower.contains("/models/") || lower.contains("/model/") || lower.contains("/entities/") {
        Some(ArtifactType::Model)
    } else if lower.contains("/database/") || lower.contains("/queries/") || lower.contains("/db/") {
        Some(ArtifactType::Query)
    } else if lower.contains("/services/") || lower.contains("/service/") {
        Some(ArtifactType::Service)
    } else if lower.contains("/api/") || lower.contains("/handlers/") || lower.contains("/routes/") {
        Some(ArtifactType::Api)
    } else if lower.contains("/config") || lower.ends_with("config.rs") {
        Some(ArtifactType::Config)
    } else if lower.contains("/auth") || lower.contains("jwt") {
        Some(ArtifactType::Auth)
    } else if lower.contains("/migration") {
        Some(ArtifactType::Migration)
    } else if lower.contains("/frontend/") || lower.contains("/pages/") || lower.contains("/components/") {
        Some(ArtifactType::Frontend)
    } else if lower.contains("/tests/") || lower.contains("_test") {
        Some(ArtifactType::Test)
    } else if lower.contains("cargo.toml") || lower.contains(".gitignore") || lower.contains("dockerfile") {
        Some(ArtifactType::Static)
    } else {
        None
    }
}

/// Infer layer depth from file path (matches HDAG layer convention).
pub fn infer_layer_depth(path: &str) -> u8 {
    let lower = path.to_lowercase();
    if lower.contains("cargo.toml") || lower.contains(".gitignore") || lower.contains("dockerfile") {
        0 // Static/structural
    } else if lower.contains("errors") || lower.contains("config") || lower.contains("pagination") {
        1 // Foundation
    } else if lower.contains("auth") || lower.contains("jwt") {
        2 // Auth
    } else if lower.contains("/models/") || lower.contains("/model/") {
        3 // Models
    } else if lower.contains("/database/") || lower.contains("/queries/") || lower.contains("/db/") {
        4 // Database/queries
    } else if lower.contains("/services/") || lower.contains("/service/") {
        5 // Services
    } else if lower.contains("/api/") || lower.contains("/handlers/") || lower.contains("/routes/") {
        6 // API
    } else if lower.contains("/frontend/") || lower.contains("/pages/") {
        8 // Frontend
    } else if lower.contains("/tests/") || lower.contains("_test") {
        9 // Tests
    } else {
        0
    }
}

// ═══════════════════════════════════════════════════════════════════
// Codematrix Computation
// ═══════════════════════════════════════════════════════════════════

/// Compute the 5D Codematrix from a set of Resonites and optional layer info.
pub fn compute_codematrix(resonites: &[Resonite], expected_layer: Option<u8>) -> Codematrix {
    Codematrix {
        r: compute_r(resonites),
        f: compute_f(resonites),
        t: compute_t(resonites, expected_layer),
        s: compute_s(resonites),
        e: compute_e(resonites),
    }
}

/// Compute the full Codematrix from raw source code and file path.
///
/// The language is inferred from the file extension; unknown extensions fall
/// back to Rust (existing behaviour).
pub fn compute_codematrix_from_code(code: &str, file_path: &str, expected_layer: Option<u8>) -> Codematrix {
    let lang = isls_reader::Language::from_filename(
        std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(""),
    );
    let lang = if matches!(lang, isls_reader::Language::Unknown) {
        isls_reader::Language::Rust
    } else {
        lang
    };
    let resonites = extract_resonites(code, file_path, lang);
    compute_codematrix(&resonites, expected_layer)
}

// ── R: Relationality ───────────────────────────────────────────────

const R_MAX: f64 = 15.0;

fn compute_r(resonites: &[Resonite]) -> f64 {
    let import_count = resonites
        .iter()
        .filter(|r| matches!(r, Resonite::Import { .. }))
        .count();
    let fk_count = resonites
        .iter()
        .filter(|r| matches!(r, Resonite::Relation { kind: RelationKind::ForeignKey, .. }))
        .count();
    clamp01((import_count + fk_count) as f64 / R_MAX)
}

// ── F: Functional Cohesion ─────────────────────────────────────────

fn compute_f(resonites: &[Resonite]) -> f64 {
    let import_count = resonites
        .iter()
        .filter(|r| matches!(r, Resonite::Import { .. }))
        .count() as f64;
    let fn_count = resonites
        .iter()
        .filter(|r| matches!(r, Resonite::Fn { .. }))
        .count() as f64;
    let epsilon = 1.0;
    // Approximation: F = 1 - imports / (imports + functions + eps)
    clamp01(1.0 - import_count / (import_count + fn_count + epsilon))
}

// ── T: Topology Conformance ────────────────────────────────────────

fn compute_t(resonites: &[Resonite], expected_layer: Option<u8>) -> f64 {
    let expected = match expected_layer {
        Some(l) => l,
        None => {
            // Infer from resonites
            resonites
                .iter()
                .find_map(|r| {
                    if let Resonite::Layer { depth, .. } = r {
                        Some(*depth)
                    } else {
                        None
                    }
                })
                .unwrap_or(0)
        }
    };

    let actual = resonites
        .iter()
        .find_map(|r| {
            if let Resonite::Layer { depth, .. } = r {
                Some(*depth)
            } else {
                None
            }
        })
        .unwrap_or(0);

    // Perfect match = 1.0, off by 1 = 0.7, off by 2+ = max(0.2, ...)
    if expected == actual {
        1.0
    } else {
        let diff = (expected as i16 - actual as i16).unsigned_abs() as f64;
        clamp01(1.0 - diff * 0.3)
    }
}

// ── S: Symmetry ────────────────────────────────────────────────────

fn compute_s(resonites: &[Resonite]) -> f64 {
    let fns: Vec<&str> = resonites
        .iter()
        .filter_map(|r| {
            if let Resonite::Fn { name, .. } = r {
                Some(name.as_str())
            } else {
                None
            }
        })
        .collect();

    if fns.is_empty() {
        return 0.5; // Neutral for files without functions
    }

    let conforming_prefixes = [
        "get_", "list_", "create_", "update_", "delete_", "new", "from_", "into_",
        "is_", "has_", "with_", "set_", "add_", "remove_", "find_", "search_",
        "validate_", "parse_", "build_", "handle_", "process_", "run_", "start_",
        "stop_", "init_", "load_", "save_", "fetch_", "send_", "check_",
    ];

    let conforming = fns
        .iter()
        .filter(|name| {
            conforming_prefixes.iter().any(|p| name.starts_with(p))
                || name.chars().next().map_or(false, |c| c.is_lowercase())
        })
        .count();

    clamp01(conforming as f64 / fns.len() as f64)
}

// ── E: Entropy ─────────────────────────────────────────────────────

fn compute_e(resonites: &[Resonite]) -> f64 {
    // Build a token distribution from resonite names
    let mut token_counts: HashMap<String, usize> = HashMap::new();

    for r in resonites {
        let token = match r {
            Resonite::Fn { name, .. } => format!("fn:{}", name),
            Resonite::Type { name, kind, .. } => format!("{:?}:{}", kind, name),
            Resonite::Import { path, .. } => format!("use:{}", path),
            Resonite::Layer { artifact_type, .. } => format!("layer:{:?}", artifact_type),
            Resonite::Relation { kind, .. } => format!("rel:{:?}", kind),
        };
        *token_counts.entry(token).or_insert(0) += 1;
    }

    if token_counts.is_empty() {
        return 0.0;
    }

    let total: f64 = token_counts.values().sum::<usize>() as f64;
    let vocab_size = token_counts.len() as f64;

    if vocab_size <= 1.0 {
        return 0.0;
    }

    // Shannon entropy
    let h: f64 = token_counts
        .values()
        .map(|&count| {
            let p = count as f64 / total;
            if p > 0.0 {
                -p * p.log2()
            } else {
                0.0
            }
        })
        .sum();

    let h_max = vocab_size.log2();
    if h_max <= 0.0 {
        return 0.0;
    }

    clamp01(h / h_max)
}

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

// ═══════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_QUERY: &str = r#"
use sqlx::PgPool;
use crate::errors::AppError;
use crate::models::pet::Pet;

pub async fn get_pet(pool: &PgPool, id: i64) -> Result<Pet, AppError> {
    sqlx::query_as!(Pet, "SELECT * FROM pets WHERE id = $1", id)
        .fetch_one(pool)
        .await
        .map_err(|_| AppError::NotFound("Pet not found".into()))
}

pub async fn list_pets(pool: &PgPool) -> Result<Vec<Pet>, AppError> {
    sqlx::query_as!(Pet, "SELECT * FROM pets ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
}

pub async fn create_pet(pool: &PgPool, name: &str) -> Result<Pet, AppError> {
    sqlx::query_as!(Pet, "INSERT INTO pets (name) VALUES ($1) RETURNING *", name)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
}
"#;

    const SAMPLE_MODEL: &str = r#"
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Pet {
    pub id: i64,
    pub name: String,
    pub species: String,
    pub owner_id: Option<i64>,
}
"#;

    #[test]
    fn test_extract_resonites_query() {
        let resonites = extract_resonites(SAMPLE_QUERY, "backend/src/database/pet_queries.rs", isls_reader::Language::Rust);
        let fn_count = resonites.iter().filter(|r| matches!(r, Resonite::Fn { .. })).count();
        let import_count = resonites.iter().filter(|r| matches!(r, Resonite::Import { .. })).count();
        assert!(fn_count >= 3, "Expected >=3 functions, got {}", fn_count);
        assert!(import_count >= 2, "Expected >=2 imports, got {}", import_count);
    }

    #[test]
    fn test_extract_resonites_model() {
        let resonites = extract_resonites(SAMPLE_MODEL, "backend/src/models/pet.rs", isls_reader::Language::Rust);
        let type_count = resonites.iter().filter(|r| matches!(r, Resonite::Type { .. })).count();
        assert!(type_count >= 1, "Expected >=1 type, got {}", type_count);
    }

    #[test]
    fn test_codematrix_values_in_range() {
        let cm = compute_codematrix_from_code(SAMPLE_QUERY, "backend/src/database/pet_queries.rs", Some(4));
        assert!(cm.r >= 0.0 && cm.r <= 1.0, "R out of range: {}", cm.r);
        assert!(cm.f >= 0.0 && cm.f <= 1.0, "F out of range: {}", cm.f);
        assert!(cm.t >= 0.0 && cm.t <= 1.0, "T out of range: {}", cm.t);
        assert!(cm.s >= 0.0 && cm.s <= 1.0, "S out of range: {}", cm.s);
        assert!(cm.e >= 0.0 && cm.e <= 1.0, "E out of range: {}", cm.e);
    }

    #[test]
    fn test_codematrix_distance_identical() {
        let cm = compute_codematrix_from_code(SAMPLE_QUERY, "backend/src/database/pet_queries.rs", Some(4));
        assert!((cm.distance(&cm) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_codematrix_resonance_well_formed() {
        let cm = compute_codematrix_from_code(SAMPLE_QUERY, "backend/src/database/pet_queries.rs", Some(4));
        let res = cm.resonance();
        assert!(res > 0.0, "Resonance should be positive for a well-formed file: {}", res);
    }

    #[test]
    fn test_codematrix_average() {
        let a = Codematrix { r: 0.6, f: 0.8, t: 0.9, s: 0.7, e: 0.5 };
        let b = Codematrix { r: 0.4, f: 0.6, t: 0.7, s: 0.9, e: 0.3 };
        let avg = Codematrix::average(&[a, b]);
        assert!((avg.r - 0.5).abs() < 1e-10);
        assert!((avg.f - 0.7).abs() < 1e-10);
    }

    #[test]
    fn test_infer_artifact_type() {
        assert_eq!(infer_artifact_type("backend/src/models/pet.rs"), Some(ArtifactType::Model));
        assert_eq!(infer_artifact_type("backend/src/database/pet_queries.rs"), Some(ArtifactType::Query));
        assert_eq!(infer_artifact_type("backend/src/services/pet.rs"), Some(ArtifactType::Service));
        assert_eq!(infer_artifact_type("backend/src/api/pet.rs"), Some(ArtifactType::Api));
    }
}
