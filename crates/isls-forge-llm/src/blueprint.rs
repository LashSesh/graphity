// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! D6: InfraBlueprint — infrastructure description derived from activated norms.
//!
//! An InfraBlueprint is a pure-data struct that describes WHAT the generated
//! project contains (HTTP server? Database? CLI? Frontend?). It is derived
//! from infrastructure norms at generation time. `CodegenHdag::build()` reads
//! the blueprint to decide which nodes to create.
//!
//! Key principle: booleans, not enums. A project can have BOTH an HTTP server
//! AND a CLI. The blueprint describes capabilities, not categories.

use isls_norms::types::ActivatedNorm;

/// Infrastructure blueprint derived from activated norms.
/// Describes WHAT the generated project contains — not HOW
/// to generate it (that's the structural generators' job).
#[derive(Clone, Debug, Default)]
pub struct InfraBlueprint {
    // ── Output shape ─────────────────────────────────
    /// Project produces a runnable binary.
    pub has_binary: bool,
    /// Project produces a library crate (pub API).
    pub has_library: bool,

    // ── Server ───────────────────────────────────────
    /// Project includes an HTTP server.
    pub has_http_server: bool,
    /// Server framework crate name (e.g. "actix-web").
    pub server_framework: Option<String>,

    // ── Persistence ──────────────────────────────────
    /// Project uses a database.
    pub has_database: bool,
    /// Database type (e.g. "postgresql", "sqlite").
    pub database_type: Option<String>,
    /// Database driver crate (e.g. "sqlx").
    pub database_driver: Option<String>,

    // ── Auth ─────────────────────────────────────────
    /// Project includes authentication.
    pub has_auth: bool,
    /// Auth mechanism (e.g. "jwt").
    pub auth_type: Option<String>,

    // ── Frontend ─────────────────────────────────────
    /// Project includes a web frontend.
    pub has_frontend: bool,
    /// Frontend type (e.g. "vanilla-js", "react").
    pub frontend_type: Option<String>,

    // ── Deployment ───────────────────────────────────
    /// Project includes Docker files.
    pub has_docker: bool,

    // ── CLI ──────────────────────────────────────────
    /// Project includes CLI argument parsing.
    pub has_cli: bool,
    /// CLI framework (e.g. "clap").
    pub cli_framework: Option<String>,

    // ── Crate dependencies ───────────────────────────
    /// Additional Cargo.toml dependencies from norms.
    pub extra_dependencies: Vec<CrateDep>,
}

#[derive(Clone, Debug)]
pub struct CrateDep {
    pub name: String,
    pub version: String,
    pub features: Vec<String>,
}

/// Return the default web-app blueprint matching D1-D5 behavior.
///
/// This is the exact configuration that `CodegenHdag::build()` assumed
/// before D6: Actix-Web, PostgreSQL, JWT, vanilla-js frontend, Docker.
pub fn default_web_blueprint() -> InfraBlueprint {
    InfraBlueprint {
        has_binary: true,
        has_library: false,
        has_http_server: true,
        server_framework: Some("actix-web".into()),
        has_database: true,
        database_type: Some("postgresql".into()),
        database_driver: Some("sqlx".into()),
        has_auth: true,
        auth_type: Some("jwt".into()),
        has_frontend: true,
        frontend_type: Some("vanilla-js".into()),
        has_docker: true,
        has_cli: false,
        cli_framework: None,
        extra_dependencies: Vec::new(),
    }
}

/// Derive an InfraBlueprint from activated infrastructure norms.
///
/// If no infrastructure norms are activated (backward compatibility with
/// D1-D5 TOML files), returns the default web-app blueprint.
pub fn derive_blueprint(activated: &[ActivatedNorm]) -> InfraBlueprint {
    let mut bp = InfraBlueprint::default();
    let mut any_infra = false;

    for norm in activated {
        match norm.norm.id.as_str() {
            "ISLS-NORM-INFRA-WEB" => {
                bp.has_binary = true;
                bp.has_http_server = true;
                bp.server_framework = Some("actix-web".into());
                any_infra = true;
            }
            "ISLS-NORM-INFRA-DB" => {
                bp.has_database = true;
                bp.database_type = Some("postgresql".into());
                bp.database_driver = Some("sqlx".into());
                any_infra = true;
            }
            "ISLS-NORM-INFRA-AUTH" => {
                bp.has_auth = true;
                bp.auth_type = Some("jwt".into());
                any_infra = true;
            }
            "ISLS-NORM-INFRA-FRONTEND" => {
                bp.has_frontend = true;
                bp.frontend_type = Some("vanilla-js".into());
                any_infra = true;
            }
            "ISLS-NORM-INFRA-DOCKER" => {
                bp.has_docker = true;
                any_infra = true;
            }
            "ISLS-NORM-INFRA-CLI" => {
                bp.has_binary = true;
                bp.has_cli = true;
                bp.cli_framework = Some("clap".into());
                any_infra = true;
            }
            "ISLS-NORM-INFRA-LIB" => {
                bp.has_library = true;
                any_infra = true;
            }
            _ => {} // Entity/feature norms don't affect blueprint
        }
    }

    // Default: if nothing activated, assume web app (backward compat with D1-D5)
    if !any_infra {
        return default_web_blueprint();
    }

    // Web-app implication: if an HTTP server is present, imply the standard
    // web-app companions (database, auth, frontend, docker) unless they were
    // explicitly excluded by a different infra norm configuration.
    // This prevents partial blueprints when only INFRA-WEB activates from a
    // generic description like "warehouse application".
    if bp.has_http_server {
        if !bp.has_database {
            bp.has_database = true;
            bp.database_type = Some("postgresql".into());
            bp.database_driver = Some("sqlx".into());
        }
        if !bp.has_auth {
            bp.has_auth = true;
            bp.auth_type = Some("jwt".into());
        }
        if !bp.has_frontend {
            bp.has_frontend = true;
            bp.frontend_type = Some("vanilla-js".into());
        }
        if !bp.has_docker {
            bp.has_docker = true;
        }
    }

    bp
}

/// Derive a blueprint from a free-text description by matching against
/// infrastructure norms in the NormRegistry.
///
/// This is the main entry point for pipeline integration (W4). It creates
/// a NormRegistry, matches the description, and derives the blueprint.
/// If no infrastructure norms activate, returns the default web blueprint.
pub fn derive_blueprint_from_description(description: &str) -> InfraBlueprint {
    let registry = isls_norms::NormRegistry::new();
    let activated = registry.match_description(description);
    derive_blueprint(&activated)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_norms::types::{
        ActivatedNorm, ActivationSource, Norm, NormEvidence, NormLevel,
        NormLayers, TriggerPattern,
    };

    fn infra_norm(id: &str, name: &str) -> ActivatedNorm {
        ActivatedNorm {
            norm: Norm {
                id: id.to_string(),
                name: name.to_string(),
                level: NormLevel::Atom,
                triggers: vec![TriggerPattern::default()],
                layers: NormLayers::default(),
                parameters: vec![],
                requires: vec![],
                variants: vec![],
                version: "1.0.0".to_string(),
                evidence: NormEvidence { builtin: true, ..Default::default() },
            },
            confidence: 1.0,
            source: ActivationSource::KeywordMatch,
        }
    }

    #[test]
    fn test_default_web_blueprint() {
        let bp = default_web_blueprint();
        assert!(bp.has_binary);
        assert!(bp.has_http_server);
        assert!(bp.has_database);
        assert!(bp.has_auth);
        assert!(bp.has_frontend);
        assert!(bp.has_docker);
        assert!(!bp.has_cli);
        assert!(!bp.has_library);
    }

    #[test]
    fn test_derive_empty_norms_gives_default() {
        let bp = derive_blueprint(&[]);
        assert!(bp.has_http_server);
        assert!(bp.has_database);
        assert!(bp.has_auth);
        assert!(bp.has_frontend);
        assert!(bp.has_docker);
    }

    #[test]
    fn test_derive_web_infra_norms() {
        let activated = vec![
            infra_norm("ISLS-NORM-INFRA-WEB", "Web-Server"),
            infra_norm("ISLS-NORM-INFRA-DB", "Database"),
            infra_norm("ISLS-NORM-INFRA-AUTH", "Authentication"),
            infra_norm("ISLS-NORM-INFRA-FRONTEND", "Frontend"),
            infra_norm("ISLS-NORM-INFRA-DOCKER", "Docker"),
        ];
        let bp = derive_blueprint(&activated);
        assert!(bp.has_binary);
        assert!(bp.has_http_server);
        assert!(bp.has_database);
        assert!(bp.has_auth);
        assert!(bp.has_frontend);
        assert!(bp.has_docker);
        assert!(!bp.has_cli);
        assert!(!bp.has_library);
    }

    #[test]
    fn test_derive_cli_blueprint() {
        let activated = vec![
            infra_norm("ISLS-NORM-INFRA-CLI", "CLI-Tool"),
        ];
        let bp = derive_blueprint(&activated);
        assert!(bp.has_binary);
        assert!(bp.has_cli);
        assert!(!bp.has_http_server);
        assert!(!bp.has_database);
        assert!(!bp.has_frontend);
    }

    #[test]
    fn test_derive_library_blueprint() {
        let activated = vec![
            infra_norm("ISLS-NORM-INFRA-LIB", "Library"),
        ];
        let bp = derive_blueprint(&activated);
        assert!(bp.has_library);
        assert!(!bp.has_binary);
        assert!(!bp.has_http_server);
    }

    #[test]
    fn test_derive_from_description_warehouse() {
        let bp = derive_blueprint_from_description(
            "A warehouse application generated by ISLS v3.4."
        );
        // With generic description, should fall back to default web blueprint
        assert!(bp.has_http_server, "warehouse app must have HTTP server (bp={:?})", bp);
        assert!(bp.has_database, "warehouse app must have database");
        assert!(bp.has_docker, "warehouse app must have docker");
    }

    #[test]
    fn test_derive_from_description_petshop() {
        let bp = derive_blueprint_from_description(
            "Pet shop with animals, owners, and veterinary appointments"
        );
        // Petshop description may or may not activate infra norms
        // but should always result in a usable web blueprint
        assert!(bp.has_http_server, "petshop app must have HTTP server (bp={:?})", bp);
        assert!(bp.has_database, "petshop app must have database");
    }
}
