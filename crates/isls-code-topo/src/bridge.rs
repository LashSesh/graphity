// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! D5 Topology-to-Observation Bridge.
//!
//! Converts Barbara's `CodeObservation`s into `ObservedArtifact`s for the
//! norm learning pipeline. Pure function — no IO, no filesystem access.

use isls_reader::CodeObservation;
use isls_norms::learning::ObservedArtifact;
use isls_norms::types::LayerType;

// ─── Layer Detection ─────────────────────────────────────────────────────────

/// Detect the architectural layer from a file path.
/// Returns `None` for unmapped directories (file should be skipped).
fn detect_layer(path: &str) -> Option<LayerType> {
    let lower = path.to_lowercase();

    // Models / entities
    if lower.contains("/models/") || lower.contains("/model/") || lower.contains("/entities/") {
        return Some(LayerType::Model);
    }
    // Database / queries / repositories
    if lower.contains("/database/") || lower.contains("/db/") || lower.contains("/queries/")
        || lower.contains("/repository/") || lower.contains("/repositories/")
    {
        return Some(LayerType::Query);
    }
    // Services / domain
    if lower.contains("/services/") || lower.contains("/service/") || lower.contains("/domain/") {
        return Some(LayerType::Service);
    }
    // API / routes / handlers / controllers / endpoints
    if lower.contains("/api/") || lower.contains("/routes/") || lower.contains("/handlers/")
        || lower.contains("/controllers/") || lower.contains("/endpoints/")
    {
        return Some(LayerType::Api);
    }
    // Frontend / pages / components / views / templates
    if lower.contains("/frontend/") || lower.contains("/pages/") || lower.contains("/components/")
        || lower.contains("/views/") || lower.contains("/templates/")
    {
        return Some(LayerType::Frontend);
    }
    // Tests
    if lower.contains("/tests/") || lower.contains("/test/")
        || lower.contains("_test.") || lower.contains("_tests.")
        || lower.contains("/spec/")
    {
        return Some(LayerType::Test);
    }
    // Migrations / SQL in root
    if lower.contains("/migrations/") || lower.contains("schema.sql")
        || (lower.ends_with(".sql") && !lower.contains('/'))
    {
        return Some(LayerType::Database);
    }
    // Config / settings / TOML/YAML in root
    if lower.contains("/config/") || lower.contains("/settings/")
        || ((lower.ends_with(".toml") || lower.ends_with(".yaml") || lower.ends_with(".yml"))
            && !lower.contains('/'))
    {
        return Some(LayerType::Config);
    }

    None
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Convert Barbara `CodeObservation`s into `ObservedArtifact`s for the norm
/// learning pipeline.
///
/// Skips files that cannot be mapped to a known `LayerType`.
/// Returns `(artifacts, skipped_count)`.
pub fn observations_from_code(
    code_obs: &[CodeObservation],
) -> (Vec<ObservedArtifact>, usize) {
    let mut artifacts = Vec::new();
    let mut skipped = 0usize;

    for obs in code_obs {
        let path_str = obs.file_path.to_string_lossy();
        let layer = match detect_layer(&path_str) {
            Some(l) => l,
            None => {
                // SQL tables always map to Database regardless of path
                if !obs.sql_tables.is_empty() {
                    LayerType::Database
                } else {
                    skipped += 1;
                    continue;
                }
            }
        };

        // Structs → ObservedArtifact
        for s in &obs.structs {
            artifacts.push(ObservedArtifact {
                layer: layer.clone(),
                artifact_type: "struct".to_string(),
                name: s.name.clone(),
                signature: obs.sha256.clone(),
                field_names: s.fields.clone(),
            });
        }

        // Functions → ObservedArtifact
        for f in &obs.functions {
            artifacts.push(ObservedArtifact {
                layer: layer.clone(),
                artifact_type: "fn".to_string(),
                name: f.name.clone(),
                signature: obs.sha256.clone(),
                field_names: f.params.clone(),
            });
        }

        // SQL tables → ObservedArtifact (always Database layer)
        for t in &obs.sql_tables {
            artifacts.push(ObservedArtifact {
                layer: LayerType::Database,
                artifact_type: "table".to_string(),
                name: t.name.clone(),
                signature: obs.sha256.clone(),
                field_names: t.columns.clone(),
            });
        }
    }

    (artifacts, skipped)
}

/// Infer a domain name from a local path.
/// Uses the last directory component (e.g. `./repos/petshop-api` → `"petshop-api"`).
pub fn domain_from_path(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Infer a domain name from a git URL.
/// Extracts the repository name (e.g. `https://github.com/user/petshop-api.git` → `"petshop-api"`).
pub fn domain_from_url(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("unknown")
        .trim_end_matches(".git")
        .to_string()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_reader::{parse_string, Language, CodeObservation};
    use std::path::PathBuf;

    fn make_obs(path: &str, src: &str, lang: Language) -> CodeObservation {
        let mut obs = parse_string(src, lang).unwrap();
        obs.file_path = PathBuf::from(path);
        obs
    }

    #[test]
    fn test_bridge_rust_model() {
        let obs = make_obs(
            "src/models/product.rs",
            "pub struct Product { pub name: String, pub price: f64 }",
            Language::Rust,
        );
        let (artifacts, skipped) = observations_from_code(&[obs]);
        assert_eq!(skipped, 0);
        assert!(artifacts.iter().any(|a|
            a.artifact_type == "struct"
            && a.name == "Product"
            && matches!(a.layer, LayerType::Model)
        ));
    }

    #[test]
    fn test_bridge_rust_query() {
        let obs = make_obs(
            "src/database/queries.rs",
            "pub async fn get_product(id: i32, db: Pool) -> Product { todo!() }",
            Language::Rust,
        );
        let (artifacts, skipped) = observations_from_code(&[obs]);
        assert_eq!(skipped, 0);
        assert!(artifacts.iter().any(|a|
            a.artifact_type == "fn"
            && a.name == "get_product"
            && matches!(a.layer, LayerType::Query)
        ));
    }

    #[test]
    fn test_bridge_sql_table() {
        let obs = make_obs(
            "migrations/001_create_products.sql",
            "CREATE TABLE products (\n    id INTEGER PRIMARY KEY,\n    name TEXT,\n    price REAL\n);",
            Language::Sql,
        );
        let (artifacts, skipped) = observations_from_code(&[obs]);
        assert_eq!(skipped, 0);
        assert!(artifacts.iter().any(|a|
            a.artifact_type == "table"
            && a.name == "products"
            && matches!(a.layer, LayerType::Database)
            && a.field_names.contains(&"name".to_string())
        ));
    }

    #[test]
    fn test_bridge_python_controller() {
        let obs = make_obs(
            "src/api/products.py",
            "def get_products(request):\n    pass",
            Language::Python,
        );
        let (artifacts, skipped) = observations_from_code(&[obs]);
        assert_eq!(skipped, 0);
        assert!(artifacts.iter().any(|a|
            a.artifact_type == "fn"
            && matches!(a.layer, LayerType::Api)
        ));
    }

    #[test]
    fn test_bridge_js_component() {
        let obs = make_obs(
            "src/components/ProductList.js",
            "class ProductList { render() {} }",
            Language::JavaScript,
        );
        let (artifacts, skipped) = observations_from_code(&[obs]);
        assert_eq!(skipped, 0);
        assert!(artifacts.iter().any(|a|
            matches!(a.layer, LayerType::Frontend)
        ));
    }

    #[test]
    fn test_bridge_skip_unmapped() {
        let obs = make_obs(
            "src/utils/helpers.rs",
            "pub fn format_date(ts: i64) -> String { todo!() }",
            Language::Rust,
        );
        let (artifacts, skipped) = observations_from_code(&[obs]);
        assert_eq!(skipped, 1);
        assert!(artifacts.is_empty());
    }

    #[test]
    fn test_bridge_multi_entity() {
        let obs_model = make_obs(
            "src/models/product.rs",
            "pub struct Product { pub name: String }\npub struct Category { pub label: String }",
            Language::Rust,
        );
        let obs_service = make_obs(
            "src/services/product_service.rs",
            "pub fn create_product(name: String) {}\npub fn list_products() {}",
            Language::Rust,
        );
        let obs_api = make_obs(
            "src/api/product_handler.rs",
            "pub async fn get_product(id: i32) {}\npub async fn delete_product(id: i32) {}",
            Language::Rust,
        );
        let obs_db = make_obs(
            "migrations/001.sql",
            "CREATE TABLE products (\n    id INTEGER,\n    name TEXT\n);\nCREATE TABLE categories (\n    id INTEGER,\n    label TEXT\n);",
            Language::Sql,
        );
        let (artifacts, skipped) = observations_from_code(&[obs_model, obs_service, obs_api, obs_db]);
        assert_eq!(skipped, 0);
        // 2 structs + 2 service fns + 2 api fns + 2 tables = 8+
        assert!(artifacts.len() >= 8);
    }

    #[test]
    fn test_domain_from_path() {
        assert_eq!(domain_from_path(std::path::Path::new("./repos/my-app")), "my-app");
        assert_eq!(domain_from_path(std::path::Path::new("/home/user/petshop-api")), "petshop-api");
    }

    #[test]
    fn test_domain_from_url() {
        assert_eq!(domain_from_url("https://github.com/user/my-app.git"), "my-app");
        assert_eq!(domain_from_url("https://github.com/user/my-app"), "my-app");
        assert_eq!(domain_from_url("https://github.com/user/my-app/"), "my-app");
    }

    #[test]
    fn test_manifest_parse() {
        // Manifest parsing is tested in cmd_scrape, but verify TOML structure here
        let toml_str = r#"
[[repo]]
url = "https://github.com/user/project-a.git"
domain = "project-a"

[[repo]]
path = "./local/project-b"
"#;
        let manifest: toml::Value = toml::from_str(toml_str).unwrap();
        let repos = manifest["repo"].as_array().unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0]["domain"].as_str().unwrap(), "project-a");
    }
}
