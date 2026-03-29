// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! TOML-to-HyperCube parser for ISLS v2.
//!
//! Reads an application requirements TOML file, detects the domain,
//! enriches entities from the domain registry, and constructs a
//! full HyperCube with dimensions and couplings.

use std::collections::BTreeMap;
use std::path::Path;
use serde::Deserialize;

use crate::{
    Coupling, CouplingDir, DimCategory, DimState, DimValue, Dimension, DomainRegistry,
    EntityTemplate, HyperCube, Result,
};

// ─── Raw TOML structures ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RawSpec {
    app: RawApp,
    backend: Option<RawBackend>,
    frontend: Option<RawFrontend>,
    deployment: Option<RawDeployment>,
    #[allow(dead_code)]
    constraints: Option<RawConstraints>,
    /// D2: explicit entity definitions parsed from `[[entities]]` arrays.
    entities: Option<Vec<RawEntity>>,
}

// ─── D2 entity TOML structures ──────────────────────────────────────────────

#[derive(Deserialize)]
struct RawEntity {
    name: String,
    #[serde(default)]
    fields: Vec<RawEntityField>,
    #[serde(default)]
    foreign_keys: Vec<RawForeignKey>,
}

#[derive(Deserialize)]
struct RawEntityField {
    name: String,
    field_type: String,
    #[serde(default)]
    nullable: bool,
    #[serde(default)]
    unique: bool,
    #[serde(default)]
    default: Option<String>,
}

#[derive(Deserialize)]
struct RawForeignKey {
    target: String,
    #[serde(default)]
    nullable: bool,
}

#[derive(Deserialize)]
struct RawApp {
    name: String,
    description: String,
    modules: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct RawBackend {
    language: Option<String>,
    framework: Option<String>,
    database: Option<String>,
    auth_method: Option<String>,
}

#[derive(Deserialize)]
struct RawFrontend {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    app_type: Option<String>,
    framework: Option<String>,
    #[allow(dead_code)]
    styling: Option<String>,
}

#[derive(Deserialize)]
struct RawDeployment {
    containerized: Option<bool>,
    compose: Option<bool>,
}

#[derive(Deserialize)]
struct RawConstraints {
    #[allow(dead_code)]
    max_crates: Option<usize>,
    #[allow(dead_code)]
    test_coverage: Option<String>,
    #[allow(dead_code)]
    evidence_chain: Option<bool>,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Parse a TOML requirements file into a HyperCube.
///
/// Detects the domain from module descriptions, enriches entities from the
/// domain registry, and auto-generates couplings.
pub fn parse_toml_to_cube(path: &Path) -> Result<HyperCube> {
    let content = std::fs::read_to_string(path)?;
    parse_toml_str_to_cube(&content)
}

/// Parse a TOML string into a HyperCube (useful for testing).
pub fn parse_toml_str_to_cube(content: &str) -> Result<HyperCube> {
    let raw: RawSpec = toml::from_str(content)?;
    let registry = DomainRegistry::new();

    let mut dimensions = Vec::new();
    let mut couplings = Vec::new();

    // ── Architecture dimensions (fixed from [backend]) ───────────────────
    let backend = raw.backend.unwrap_or(RawBackend {
        language: Some("rust".into()),
        framework: Some("actix-web".into()),
        database: Some("postgresql".into()),
        auth_method: Some("jwt".into()),
    });

    let lang = backend.language.unwrap_or_else(|| "rust".into());
    let framework = backend.framework.unwrap_or_else(|| "actix-web".into());
    let database = backend.database.unwrap_or_else(|| "postgresql".into());
    let auth_method = backend.auth_method.unwrap_or_else(|| "jwt".into());

    dimensions.push(dim_fixed("arch.language", DimCategory::Architecture, &lang, 0, "Programming language"));
    dimensions.push(dim_fixed("arch.framework", DimCategory::Architecture, &framework, 0, "Web framework"));
    dimensions.push(dim_fixed("arch.database", DimCategory::Architecture, &database, 0, "Database engine"));
    dimensions.push(dim_fixed("arch.auth_method", DimCategory::Security, &auth_method, 0, "Authentication method"));
    dimensions.push(dim_fixed("arch.app_name", DimCategory::Architecture, &raw.app.name, 0, "Application name"));

    // ── Detect domain or parse explicit entities ──────────────────────────
    let has_toml_entities = raw.entities.as_ref().map_or(false, |e| !e.is_empty());

    let (entities, domain) = if has_toml_entities {
        // D2 path: parse entities directly from TOML [[entities]] arrays
        let toml_entities = raw.entities.unwrap();
        let parsed = parse_raw_entities(&toml_entities);
        (parsed, None)
    } else {
        // D1 path: detect domain from module descriptions
        let all_module_text = raw.app.modules
            .as_ref()
            .map(|m| {
                let mut text = raw.app.description.clone();
                for (k, v) in m {
                    text.push(' ');
                    text.push_str(k);
                    text.push(' ');
                    text.push_str(v);
                }
                text
            })
            .unwrap_or_else(|| raw.app.description.clone());

        let domain = registry.detect(&all_module_text);

        let entities: Vec<EntityTemplate> = if let Some(dom) = domain {
            dom.entities.clone()
        } else {
            infer_entities_from_modules(&raw.app.modules.unwrap_or_default())
        };
        (entities, domain)
    };

    for entity in &entities {
        let snake = to_snake_case(&entity.name);

        // model dimension (carries EntityDef)
        dimensions.push(Dimension {
            name: format!("model.{}", snake),
            category: DimCategory::DataModel,
            state: DimState::Free {
                options: vec![DimValue::EntityDef(entity.clone())],
                default: Some(DimValue::EntityDef(entity.clone())),
            },
            complexity: (entity.fields.len() as u32) * 5 + 30,
            description: format!("{} data model", entity.name),
        });

        // validation dimension
        dimensions.push(Dimension {
            name: format!("validation.{}", snake),
            category: DimCategory::DataModel,
            state: DimState::Derived {
                depends_on: vec![format!("model.{}", snake)],
            },
            complexity: (entity.validations.len() as u32) * 8 + 10,
            description: format!("{} validation rules", entity.name),
        });

        // queries dimension
        dimensions.push(Dimension {
            name: format!("queries.{}", snake),
            category: DimCategory::Storage,
            state: DimState::Free {
                options: vec![DimValue::Choice("crud".into())],
                default: Some(DimValue::Choice("crud".into())),
            },
            complexity: 80 + (entity.fields.len() as u32) * 3,
            description: format!("{} database queries", entity.name),
        });

        // service dimension
        dimensions.push(Dimension {
            name: format!("service.{}", snake),
            category: DimCategory::BusinessLogic,
            state: DimState::Free {
                options: vec![DimValue::Choice("crud_service".into())],
                default: Some(DimValue::Choice("crud_service".into())),
            },
            complexity: 60 + (entity.fields.len() as u32) * 2,
            description: format!("{} service layer", entity.name),
        });

        // api dimension
        dimensions.push(Dimension {
            name: format!("api.{}", snake),
            category: DimCategory::Interface,
            state: DimState::Free {
                options: vec![DimValue::Choice("rest".into())],
                default: Some(DimValue::Choice("rest".into())),
            },
            complexity: 80 + (entity.fields.len() as u32) * 2,
            description: format!("{} API endpoints", entity.name),
        });

        // test dimension
        dimensions.push(Dimension {
            name: format!("tests.{}", snake),
            category: DimCategory::Testing,
            state: DimState::Free {
                options: vec![DimValue::Choice("integration".into())],
                default: Some(DimValue::Choice("integration".into())),
            },
            complexity: 100,
            description: format!("{} integration tests", entity.name),
        });

        // ── Intra-entity couplings ───────────────────────────────────────
        couplings.push(coupling(&format!("model.{}", snake), &format!("validation.{}", snake), 0.95, CouplingDir::Forward));
        couplings.push(coupling(&format!("model.{}", snake), &format!("queries.{}", snake), 0.90, CouplingDir::Forward));
        couplings.push(coupling(&format!("queries.{}", snake), &format!("service.{}", snake), 0.85, CouplingDir::Forward));
        couplings.push(coupling(&format!("service.{}", snake), &format!("api.{}", snake), 0.80, CouplingDir::Forward));
        couplings.push(coupling(&format!("model.{}", snake), &format!("tests.{}", snake), 0.70, CouplingDir::Forward));
    }

    // ── Business logic dimensions ────────────────────────────────────────
    if let Some(dom) = domain {
        for rule in &dom.business_rules {
            dimensions.push(Dimension {
                name: format!("business_logic.{}", rule.name),
                category: DimCategory::BusinessLogic,
                state: DimState::Free {
                    options: vec![DimValue::Choice(rule.logic_pseudocode.clone())],
                    default: None,
                },
                complexity: 40,
                description: rule.description.clone(),
            });

            // Couple business rules to involved entity services
            for entity_name in &rule.entities_involved {
                let snake = to_snake_case(entity_name);
                couplings.push(coupling(
                    &format!("business_logic.{}", rule.name),
                    &format!("service.{}", snake),
                    0.85,
                    CouplingDir::Mutual,
                ));
            }
        }
    }

    // ── Cross-cutting dimensions ─────────────────────────────────────────
    dimensions.push(dim_free("cross.error_handling", DimCategory::BusinessLogic, 80, "Error types and handling"));
    dimensions.push(dim_free("cross.pagination", DimCategory::Interface, 60, "Pagination support"));
    dimensions.push(dim_free("cross.auth", DimCategory::Security, 120, "Authentication and authorisation"));
    dimensions.push(dim_free("cross.config", DimCategory::Architecture, 40, "Application configuration"));
    dimensions.push(dim_free("cross.main", DimCategory::Architecture, 50, "Main entry point"));

    // Cross-cutting couplings
    for entity in &entities {
        let snake = to_snake_case(&entity.name);
        couplings.push(coupling("cross.auth", &format!("api.{}", snake), 0.75, CouplingDir::Mutual));
        couplings.push(coupling("cross.error_handling", &format!("service.{}", snake), 0.70, CouplingDir::Mutual));
        couplings.push(coupling("cross.pagination", &format!("api.{}", snake), 0.85, CouplingDir::Mutual));
        couplings.push(coupling("cross.pagination", &format!("queries.{}", snake), 0.80, CouplingDir::Mutual));
    }
    couplings.push(coupling("cross.config", "cross.main", 0.90, CouplingDir::Forward));
    couplings.push(coupling("cross.auth", "cross.config", 0.70, CouplingDir::Forward));

    // ── Frontend dimensions ──────────────────────────────────────────────
    let frontend = raw.frontend.unwrap_or(RawFrontend {
        app_type: Some("spa".into()),
        framework: Some("vanilla".into()),
        styling: Some("minimal".into()),
    });

    dimensions.push(dim_fixed(
        "frontend.framework",
        DimCategory::Presentation,
        &frontend.framework.unwrap_or_else(|| "vanilla".into()),
        0,
        "Frontend framework",
    ));

    dimensions.push(dim_free("frontend.layout", DimCategory::Presentation, 80, "Navigation and layout"));
    dimensions.push(dim_free("frontend.dashboard", DimCategory::Presentation, 120, "Dashboard page"));
    dimensions.push(dim_free("frontend.login", DimCategory::Presentation, 60, "Login page"));
    dimensions.push(dim_free("frontend.api_client", DimCategory::Presentation, 60, "API client module"));
    dimensions.push(dim_free("frontend.style", DimCategory::Presentation, 100, "CSS styles"));

    for entity in &entities {
        let snake = to_snake_case(&entity.name);
        dimensions.push(dim_free(
            &format!("frontend.page.{}", snake),
            DimCategory::Presentation,
            100,
            &format!("{} management page", entity.name),
        ));
        couplings.push(coupling(
            &format!("frontend.page.{}", snake),
            &format!("api.{}", snake),
            0.75,
            CouplingDir::Forward,
        ));
        couplings.push(coupling(
            "frontend.api_client",
            &format!("frontend.page.{}", snake),
            0.80,
            CouplingDir::Forward,
        ));
    }

    couplings.push(coupling("frontend.layout", "frontend.dashboard", 0.85, CouplingDir::Forward));
    couplings.push(coupling("frontend.layout", "frontend.login", 0.80, CouplingDir::Forward));
    couplings.push(coupling("cross.auth", "frontend.login", 0.85, CouplingDir::Forward));

    // ── Deployment dimensions ────────────────────────────────────────────
    let deployment = raw.deployment.unwrap_or(RawDeployment {
        containerized: Some(true),
        compose: Some(true),
    });

    if deployment.containerized.unwrap_or(true) {
        dimensions.push(dim_free("deploy.dockerfile", DimCategory::Deployment, 30, "Dockerfile"));
    }
    if deployment.compose.unwrap_or(true) {
        dimensions.push(dim_free("deploy.compose", DimCategory::Deployment, 40, "Docker compose"));
    }
    dimensions.push(dim_free("deploy.env", DimCategory::Deployment, 10, "Environment variables"));
    dimensions.push(dim_free("deploy.readme", DimCategory::Documentation, 80, "README documentation"));

    // ── Migration dimension ──────────────────────────────────────────────
    dimensions.push(dim_free("storage.migration", DimCategory::Storage, 100, "SQL migration"));
    for entity in &entities {
        let snake = to_snake_case(&entity.name);
        couplings.push(coupling(
            &format!("model.{}", snake),
            "storage.migration",
            0.90,
            CouplingDir::Forward,
        ));
    }

    Ok(HyperCube {
        dimensions,
        couplings,
        depth: 0,
        parent_signature: None,
        entities_from_toml: has_toml_entities,
    })
}

// ─── D2 Entity Parsing ──────────────────────────────────────────────────────

/// Convert TOML `[[entities]]` into `EntityTemplate`s with auto-generated
/// id, timestamp, and FK fields.
fn parse_raw_entities(raw_entities: &[RawEntity]) -> Vec<EntityTemplate> {
    let mut entities: Vec<EntityTemplate> = Vec::new();

    for raw in raw_entities {
        let mut fields = Vec::new();

        // id field (always first)
        fields.push(crate::domain::FieldDef {
            name: "id".into(),
            rust_type: "i64".into(),
            sql_type: "BIGSERIAL PRIMARY KEY".into(),
            nullable: false,
            default_value: None,
            description: "Primary key".into(),
        });

        // User-defined fields
        for rf in &raw.fields {
            let (rust_type, sql_type) = map_field_type(&rf.field_type, rf.nullable, rf.unique, rf.default.as_deref());
            fields.push(crate::domain::FieldDef {
                name: rf.name.clone(),
                rust_type,
                sql_type,
                nullable: rf.nullable,
                default_value: rf.default.clone(),
                description: format!("{} field", rf.name),
            });
        }

        // Foreign key fields — auto-generated from [[entities.foreign_keys]]
        for fk in &raw.foreign_keys {
            let target_snake = to_snake_case(&fk.target);
            let target_table = pluralize_snake(&target_snake);
            let fk_field_name = format!("{}_id", target_snake);
            let (rust_type, sql_type) = if fk.nullable {
                (
                    "Option<i64>".to_string(),
                    format!("BIGINT REFERENCES {}(id)", target_table),
                )
            } else {
                (
                    "i64".to_string(),
                    format!("BIGINT NOT NULL REFERENCES {}(id)", target_table),
                )
            };
            fields.push(crate::domain::FieldDef {
                name: fk_field_name,
                rust_type,
                sql_type,
                nullable: fk.nullable,
                default_value: None,
                description: format!("FK to {}", fk.target),
            });
        }

        // Timestamps
        fields.push(crate::domain::FieldDef {
            name: "created_at".into(),
            rust_type: "String".into(),
            sql_type: "TIMESTAMPTZ NOT NULL DEFAULT NOW()".into(),
            nullable: false,
            default_value: Some("NOW()".into()),
            description: "Creation timestamp".into(),
        });
        fields.push(crate::domain::FieldDef {
            name: "updated_at".into(),
            rust_type: "String".into(),
            sql_type: "TIMESTAMPTZ NOT NULL DEFAULT NOW()".into(),
            nullable: false,
            default_value: Some("NOW()".into()),
            description: "Update timestamp".into(),
        });

        entities.push(EntityTemplate {
            name: raw.name.clone(),
            description: format!("{} entity", raw.name),
            fields,
            validations: vec![],
            indices: vec![],
        });
    }

    entities
}

/// Map a D2 spec field_type to (rust_type, sql_type).
fn map_field_type(field_type: &str, nullable: bool, unique: bool, default: Option<&str>) -> (String, String) {
    let (base_rust, base_sql) = match field_type {
        "String" => ("String", "VARCHAR(255)"),
        "i64" => ("i64", "BIGINT"),
        "i32" => ("i32", "INTEGER"),
        "f64" => ("f64", "DOUBLE PRECISION"),
        "bool" => ("bool", "BOOLEAN"),
        _ => ("String", "VARCHAR(255)"),
    };

    let rust_type = if nullable {
        format!("Option<{}>", base_rust)
    } else {
        base_rust.to_string()
    };

    let mut sql_parts = base_sql.to_string();
    if !nullable {
        sql_parts.push_str(" NOT NULL");
    }
    if unique {
        sql_parts.push_str(" UNIQUE");
    }
    if let Some(d) = default {
        sql_parts.push_str(&format!(" DEFAULT {}", d));
    }

    (rust_type, sql_parts)
}

/// Pluralize a snake_case name (mirrors isls_forge_llm::pluralize).
fn pluralize_snake(name: &str) -> String {
    if name.is_empty() {
        return name.to_string();
    }
    if name.ends_with("ss") || name.ends_with("sh") || name.ends_with("ch") || name.ends_with("x") {
        return format!("{}es", name);
    }
    if name.ends_with('y') {
        let before_y = &name[..name.len() - 1];
        if let Some(c) = before_y.chars().last() {
            if !"aeiou".contains(c) {
                return format!("{}ies", before_y);
            }
        }
    }
    if name.ends_with('s') {
        return name.to_string();
    }
    format!("{}s", name)
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn dim_fixed(name: &str, cat: DimCategory, value: &str, complexity: u32, desc: &str) -> Dimension {
    Dimension {
        name: name.into(),
        category: cat,
        state: DimState::Fixed(DimValue::Choice(value.into())),
        complexity,
        description: desc.into(),
    }
}

fn dim_free(name: &str, cat: DimCategory, complexity: u32, desc: &str) -> Dimension {
    Dimension {
        name: name.into(),
        category: cat,
        state: DimState::Free {
            options: vec![],
            default: None,
        },
        complexity,
        description: desc.into(),
    }
}

fn coupling(from: &str, to: &str, strength: f64, dir: CouplingDir) -> Coupling {
    Coupling {
        from: from.into(),
        to: to.into(),
        strength,
        direction: dir,
    }
}

/// Convert PascalCase to snake_case.
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap_or(ch));
        } else {
            result.push(ch);
        }
    }
    result
}

/// Infer simple entity names from module descriptions (fallback when no domain matches).
fn infer_entities_from_modules(modules: &BTreeMap<String, String>) -> Vec<EntityTemplate> {
    let mut entities = Vec::new();
    for (name, desc) in modules {
        // Extract capitalized nouns as potential entity names
        let words: Vec<&str> = desc.split_whitespace().collect();
        for word in &words {
            let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
            if clean.len() > 2
                && clean.chars().next().map_or(false, |c| c.is_uppercase())
                && !["The", "And", "For", "With", "From", "Into"].contains(&clean)
            {
                if !entities.iter().any(|e: &EntityTemplate| e.name == clean) {
                    entities.push(EntityTemplate {
                        name: clean.to_string(),
                        description: format!("Entity from module '{}'", name),
                        fields: vec![
                            crate::domain::FieldDef {
                                name: "id".into(), rust_type: "i64".into(),
                                sql_type: "BIGSERIAL PRIMARY KEY".into(),
                                nullable: false, default_value: None, description: "Primary key".into(),
                            },
                            crate::domain::FieldDef {
                                name: "name".into(), rust_type: "String".into(),
                                sql_type: "VARCHAR(255) NOT NULL".into(),
                                nullable: false, default_value: None, description: "Name".into(),
                            },
                            crate::domain::FieldDef {
                                name: "created_at".into(), rust_type: "String".into(),
                                sql_type: "TIMESTAMPTZ NOT NULL DEFAULT NOW()".into(),
                                nullable: false, default_value: Some("NOW()".into()), description: "Created at".into(),
                            },
                            crate::domain::FieldDef {
                                name: "updated_at".into(), rust_type: "String".into(),
                                sql_type: "TIMESTAMPTZ NOT NULL DEFAULT NOW()".into(),
                                nullable: false, default_value: Some("NOW()".into()), description: "Updated at".into(),
                            },
                        ],
                        validations: vec![],
                        indices: vec![],
                    });
                }
            }
        }
    }
    entities
}
