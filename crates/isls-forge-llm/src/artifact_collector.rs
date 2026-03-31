// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Post-emission artifact collector for ISLS D4 observation pipeline.
//!
//! Walks the generated project directory after S7 Emit, inspects each
//! generated file, and produces a `Vec<ObservedArtifact>` for feeding
//! into [`NormRegistry::observe_and_learn()`].
//!
//! **Constraints:** Uses only `std::fs` and regex — no tokio, no external
//! parsing crates.

use std::path::{Path, PathBuf};

use regex::Regex;
use sha2::{Digest, Sha256};

use isls_norms::learning::ObservedArtifact;
use isls_norms::LayerType;

// ─── ArtifactCollector ───────────────────────────────────────────────────────

/// Collects artifact metadata from a generated project directory.
pub struct ArtifactCollector {
    output_dir: PathBuf,
}

impl ArtifactCollector {
    /// Create a new collector rooted at the given output directory.
    pub fn new(output_dir: &Path) -> Self {
        Self {
            output_dir: output_dir.to_path_buf(),
        }
    }

    /// Walk the generated project and extract artifact metadata.
    ///
    /// Returns a flat list of observed artifacts spanning all layers.
    /// On IO errors for individual files, the file is skipped (logged).
    pub fn collect(&self) -> Vec<ObservedArtifact> {
        let mut artifacts = Vec::new();

        // backend/src/ — Rust source files (Model, Query, Service, Api, Test)
        let backend_src = self.output_dir.join("backend/src");
        if backend_src.exists() {
            self.walk_rust_dir(&backend_src, &mut artifacts);
        }

        // backend/database/migrations/ — SQL migrations (Database layer)
        let migrations = self.output_dir.join("backend/database/migrations");
        if migrations.exists() {
            self.collect_migrations(&migrations, &mut artifacts);
        }

        // frontend/pages/ or frontend/src/pages/ — Frontend pages
        for pages_dir in &[
            self.output_dir.join("frontend/pages"),
            self.output_dir.join("frontend/src/pages"),
        ] {
            if pages_dir.exists() {
                self.collect_frontend(pages_dir, &mut artifacts);
            }
        }

        artifacts
    }

    // ── Rust source walker ───────────────────────────────────────────────────

    fn walk_rust_dir(&self, dir: &Path, artifacts: &mut Vec<ObservedArtifact>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.walk_rust_dir(&path, artifacts);
            } else if path.extension().map_or(false, |e| e == "rs") {
                self.collect_rust_file(&path, artifacts);
            }
        }
    }

    fn collect_rust_file(&self, path: &Path, artifacts: &mut Vec<ObservedArtifact>) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return,
        };

        let layer = self.layer_from_path(path);
        let layer = match layer {
            Some(l) => l,
            None => return,
        };

        let signature = sha256_hex(&content);

        // Extract structs
        let re_struct = Regex::new(r"pub struct (\w+)").expect("regex compiles");
        let re_field = Regex::new(r"(?m)^\s+pub (\w+):").expect("regex compiles");
        let re_struct_body = Regex::new(r"(?s)pub struct (\w+)\s*\{([^}]*)\}").expect("regex compiles");

        for caps in re_struct_body.captures_iter(&content) {
            let name = caps[1].to_string();
            let body = &caps[2];
            let field_names: Vec<String> = re_field
                .captures_iter(body)
                .map(|c| c[1].to_string())
                .collect();

            artifacts.push(ObservedArtifact {
                layer: layer.clone(),
                artifact_type: "struct".to_string(),
                name,
                signature: signature.clone(),
                field_names,
            });
        }

        // Also capture structs without bodies that re_struct_body missed
        // (e.g. tuple structs) — but only if not already captured
        let captured_names: Vec<String> = artifacts
            .iter()
            .filter(|a| a.signature == signature && a.artifact_type == "struct")
            .map(|a| a.name.clone())
            .collect();

        for caps in re_struct.captures_iter(&content) {
            let name = caps[1].to_string();
            if !captured_names.contains(&name) {
                artifacts.push(ObservedArtifact {
                    layer: layer.clone(),
                    artifact_type: "struct".to_string(),
                    name,
                    signature: signature.clone(),
                    field_names: vec![],
                });
            }
        }

        // Extract functions
        let re_fn = Regex::new(r"pub\s+(?:async\s+)?fn\s+(\w+)").expect("regex compiles");
        for caps in re_fn.captures_iter(&content) {
            let name = caps[1].to_string();
            artifacts.push(ObservedArtifact {
                layer: layer.clone(),
                artifact_type: "fn".to_string(),
                name,
                signature: signature.clone(),
                field_names: vec![],
            });
        }
    }

    fn layer_from_path(&self, path: &Path) -> Option<LayerType> {
        let rel = path.strip_prefix(&self.output_dir).ok()?;
        let rel_str = rel.to_string_lossy();

        // Normalize path separators
        let rel_str = rel_str.replace('\\', "/");

        if rel_str.contains("models/") && !rel_str.contains("mod.rs") {
            Some(LayerType::Model)
        } else if rel_str.contains("database/") && rel_str.contains("_queries") {
            Some(LayerType::Query)
        } else if rel_str.contains("services/") && !rel_str.contains("mod.rs") {
            Some(LayerType::Service)
        } else if rel_str.contains("api/") && !rel_str.contains("mod.rs") {
            Some(LayerType::Api)
        } else if rel_str.contains("api_tests") || rel_str.contains("tests/") {
            Some(LayerType::Test)
        } else {
            None
        }
    }

    // ── SQL migrations ───────────────────────────────────────────────────────

    fn collect_migrations(&self, dir: &Path, artifacts: &mut Vec<ObservedArtifact>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        let re_table = Regex::new(r"(?i)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(\w+)")
            .expect("regex compiles");

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "sql") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let signature = sha256_hex(&content);
                    for caps in re_table.captures_iter(&content) {
                        let table_name = caps[1].to_string();
                        artifacts.push(ObservedArtifact {
                            layer: LayerType::Database,
                            artifact_type: "table".to_string(),
                            name: table_name,
                            signature: signature.clone(),
                            field_names: vec![],
                        });
                    }
                }
            }
        }
    }

    // ── Frontend pages ───────────────────────────────────────────────────────

    fn collect_frontend(&self, dir: &Path, artifacts: &mut Vec<ObservedArtifact>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "js" || e == "jsx" || e == "ts" || e == "tsx") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let signature = sha256_hex(&content);
                    let name = path.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    artifacts.push(ObservedArtifact {
                        layer: LayerType::Frontend,
                        artifact_type: "page".to_string(),
                        name,
                        signature,
                        field_names: vec![],
                    });
                }
            }
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp dir");

        // Create model file
        let models_dir = dir.path().join("backend/src/models");
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(
            models_dir.join("product.rs"),
            r#"
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub price: f64,
    pub active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateProductPayload {
    pub name: String,
    pub price: f64,
}
"#,
        ).unwrap();

        // Create query file
        let db_dir = dir.path().join("backend/src/database");
        fs::create_dir_all(&db_dir).unwrap();
        fs::write(
            db_dir.join("product_queries.rs"),
            r#"
use sqlx::PgPool;

pub async fn get_product(pool: &PgPool, id: i64) -> Result<Product, AppError> {
    todo!()
}

pub async fn list_products(pool: &PgPool) -> Result<Vec<Product>, AppError> {
    todo!()
}

pub async fn create_product(pool: &PgPool, payload: CreateProductPayload) -> Result<Product, AppError> {
    todo!()
}
"#,
        ).unwrap();

        // Create service file
        let svc_dir = dir.path().join("backend/src/services");
        fs::create_dir_all(&svc_dir).unwrap();
        fs::write(
            svc_dir.join("product.rs"),
            r#"
pub async fn get_product(pool: &PgPool, id: i64) -> Result<Product, AppError> {
    todo!()
}
"#,
        ).unwrap();

        // Create API file
        let api_dir = dir.path().join("backend/src/api");
        fs::create_dir_all(&api_dir).unwrap();
        fs::write(
            api_dir.join("product.rs"),
            r#"
pub async fn list_products(pool: web::Data<PgPool>) -> Result<impl Responder, AppError> {
    todo!()
}

pub fn product_routes(cfg: &mut web::ServiceConfig) {
    todo!()
}
"#,
        ).unwrap();

        // Create migration
        let mig_dir = dir.path().join("backend/database/migrations");
        fs::create_dir_all(&mig_dir).unwrap();
        fs::write(
            mig_dir.join("001_initial.sql"),
            r#"
CREATE TABLE IF NOT EXISTS products (
    id BIGSERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    price NUMERIC(10,2) NOT NULL
);

CREATE TABLE IF NOT EXISTS orders (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL
);
"#,
        ).unwrap();

        // Create frontend page
        let pages_dir = dir.path().join("frontend/pages");
        fs::create_dir_all(&pages_dir).unwrap();
        fs::write(pages_dir.join("products.js"), "// products page").unwrap();

        dir
    }

    #[test]
    fn test_artifact_collector_models() {
        let dir = setup_test_dir();
        let collector = ArtifactCollector::new(dir.path());
        let artifacts = collector.collect();

        let model_structs: Vec<_> = artifacts
            .iter()
            .filter(|a| a.layer == LayerType::Model && a.artifact_type == "struct")
            .collect();

        assert!(
            model_structs.iter().any(|a| a.name == "Product"),
            "should find Product struct"
        );
        let product = model_structs.iter().find(|a| a.name == "Product").unwrap();
        assert!(product.field_names.contains(&"name".to_string()));
        assert!(product.field_names.contains(&"price".to_string()));
        assert!(product.field_names.contains(&"active".to_string()));
    }

    #[test]
    fn test_artifact_collector_queries() {
        let dir = setup_test_dir();
        let collector = ArtifactCollector::new(dir.path());
        let artifacts = collector.collect();

        let query_fns: Vec<_> = artifacts
            .iter()
            .filter(|a| a.layer == LayerType::Query && a.artifact_type == "fn")
            .collect();

        assert!(
            query_fns.iter().any(|a| a.name == "get_product"),
            "should find get_product fn"
        );
        assert!(
            query_fns.iter().any(|a| a.name == "list_products"),
            "should find list_products fn"
        );
        assert!(
            query_fns.iter().any(|a| a.name == "create_product"),
            "should find create_product fn"
        );
    }

    #[test]
    fn test_artifact_collector_layers() {
        let dir = setup_test_dir();
        let collector = ArtifactCollector::new(dir.path());
        let artifacts = collector.collect();

        let layers: std::collections::HashSet<_> =
            artifacts.iter().map(|a| a.layer.clone()).collect();

        assert!(layers.contains(&LayerType::Model), "should have Model layer");
        assert!(layers.contains(&LayerType::Query), "should have Query layer");
        assert!(layers.contains(&LayerType::Service), "should have Service layer");
        assert!(layers.contains(&LayerType::Api), "should have Api layer");
        assert!(layers.contains(&LayerType::Database), "should have Database layer");
        assert!(layers.contains(&LayerType::Frontend), "should have Frontend layer");
    }

    #[test]
    fn test_artifact_collector_migrations() {
        let dir = setup_test_dir();
        let collector = ArtifactCollector::new(dir.path());
        let artifacts = collector.collect();

        let tables: Vec<_> = artifacts
            .iter()
            .filter(|a| a.layer == LayerType::Database && a.artifact_type == "table")
            .collect();

        assert!(
            tables.iter().any(|a| a.name == "products"),
            "should find products table"
        );
        assert!(
            tables.iter().any(|a| a.name == "orders"),
            "should find orders table"
        );
    }

    #[test]
    fn test_artifact_collector_frontend() {
        let dir = setup_test_dir();
        let collector = ArtifactCollector::new(dir.path());
        let artifacts = collector.collect();

        let pages: Vec<_> = artifacts
            .iter()
            .filter(|a| a.layer == LayerType::Frontend && a.artifact_type == "page")
            .collect();

        assert!(
            pages.iter().any(|a| a.name == "products"),
            "should find products page"
        );
    }

    #[test]
    fn test_artifact_collector_empty_dir() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let collector = ArtifactCollector::new(dir.path());
        let artifacts = collector.collect();
        assert!(artifacts.is_empty(), "empty dir should produce no artifacts");
    }
}
