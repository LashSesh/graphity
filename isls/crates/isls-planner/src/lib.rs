// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Architecture planning for ISLS full-stack generation — Stages 0 and 1.
//!
//! Stage 0 (DESCRIBE): Parses a constraint TOML file into a structured `AppSpec`,
//! inferring entities, operations, dependencies, and frontend pages.
//!
//! Stage 1 (PLAN): Converts an `AppSpec` into an `Architecture` using topological
//! sort on module dependencies to determine the optimal generation order.

use std::path::Path;
use std::fs;
use std::collections::{BTreeMap, VecDeque};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use isls_blueprint::{BlueprintRegistry, GenerationRequest};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("planning failed: {0}")]
    Planning(String),
}

pub type Result<T> = std::result::Result<T, PlannerError>;

// ─── TOML raw structures (for deserialization) ────────────────────────────────

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
    app_type: Option<String>,
    framework: Option<String>,
    styling: Option<String>,
}

#[derive(Deserialize)]
struct RawDeployment {
    containerized: Option<bool>,
    compose: Option<bool>,
}

#[derive(Deserialize)]
struct RawConstraints {
    max_crates: Option<usize>,
    test_coverage: Option<String>,
    evidence_chain: Option<bool>,
}

#[derive(Deserialize)]
struct RawSpec {
    app: RawApp,
    backend: Option<RawBackend>,
    frontend: Option<RawFrontend>,
    deployment: Option<RawDeployment>,
    constraints: Option<RawConstraints>,
}

// ─── AppSpec (Stage 0 output) ─────────────────────────────────────────────────

/// Structured, validated application specification (Stage 0 output).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSpec {
    pub name: String,
    pub description: String,
    pub modules: Vec<ModuleSpec>,
    pub backend: BackendSpec,
    pub frontend: FrontendSpec,
    pub deployment: DeploymentSpec,
    pub constraints: AppConstraints,
}

/// Specification for a single application module (e.g. "inventory").
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModuleSpec {
    pub name: String,
    pub description: String,
    /// Inferred entity names (nouns from description), e.g. ["Product", "Warehouse"].
    pub entities: Vec<String>,
    /// Inferred CRUD operations present in this module.
    pub operations: Vec<String>,
    /// Modules this module depends on (inferred from entity references).
    pub dependencies: Vec<String>,
}

/// Backend technology specification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackendSpec {
    pub language: String,
    pub framework: String,
    pub database: String,
    pub auth: AuthSpec,
}

/// Authentication specification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthSpec {
    pub method: String,
}

/// Frontend technology specification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrontendSpec {
    pub app_type: String,
    pub framework: String,
    pub styling: String,
    pub pages: Vec<PageSpec>,
}

/// A single SPA page specification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageSpec {
    pub name: String,
    pub route: String,
    pub components: Vec<String>,
}

/// Deployment configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeploymentSpec {
    pub containerized: bool,
    pub compose: bool,
}

/// Application-level constraints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConstraints {
    pub max_crates: usize,
    pub test_coverage: String,
    pub evidence_chain: bool,
}

// ─── Architecture (Stage 1 output) ───────────────────────────────────────────

/// Complete architecture plan for a full-stack application (Stage 1 output).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Architecture {
    pub app_name: String,
    pub layers: Vec<Layer>,
    pub generation_order: Vec<GenerationStep>,
    pub interfaces: Vec<Interface>,
    pub estimated_files: usize,
    pub estimated_loc: usize,
}

/// A horizontal architectural layer (models, database, services, api, frontend, tests, deploy).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub components: Vec<Component>,
}

/// A single generateable code component.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub layer: String,
    pub file_path: String,
    pub depends_on: Vec<String>,
    pub blueprint_id: Option<String>,
    pub estimated_loc: usize,
}

/// One step in the generation sequence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerationStep {
    pub order: usize,
    pub component: String,
    pub layer: String,
    pub file_path: String,
    pub reason: String,
    pub can_parallel: Vec<String>,
}

/// An interface contract between two components.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Interface {
    pub from: String,
    pub to: String,
    pub interface_type: InterfaceType,
    pub contract: String,
}

/// How two components communicate.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum InterfaceType {
    FunctionCall,
    HttpEndpoint,
    DatabaseQuery,
    EventEmission,
}

/// Size estimate for the full application.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Estimate {
    pub files: usize,
    pub loc: usize,
    pub oracle_calls_expected: usize,
}

// ─── Stage 0: DESCRIBE ───────────────────────────────────────────────────────

/// Parse a constraint TOML file into a structured `AppSpec`.
///
/// Infers entities (capitalised nouns), operations (verbs like create/list/update),
/// module dependencies, and frontend pages.  No LLM required.
pub fn parse_toml(path: &Path) -> Result<AppSpec> {
    let content = fs::read_to_string(path)?;
    parse_toml_str(&content)
}

/// Parse a constraint TOML string into an `AppSpec`.
pub fn parse_toml_str(content: &str) -> Result<AppSpec> {
    let raw: RawSpec = toml::from_str(content)?;

    let backend_raw = raw.backend.unwrap_or(RawBackend {
        language: None, framework: None, database: None, auth_method: None,
    });
    let frontend_raw = raw.frontend.unwrap_or(RawFrontend {
        app_type: None, framework: None, styling: None,
    });
    let deployment_raw = raw.deployment.unwrap_or(RawDeployment {
        containerized: None, compose: None,
    });
    let constraints_raw = raw.constraints.unwrap_or(RawConstraints {
        max_crates: None, test_coverage: None, evidence_chain: None,
    });

    let backend = BackendSpec {
        language: backend_raw.language.unwrap_or_else(|| "rust".to_string()),
        framework: backend_raw.framework.unwrap_or_else(|| "actix-web".to_string()),
        database: backend_raw.database.unwrap_or_else(|| "postgresql".to_string()),
        auth: AuthSpec {
            method: backend_raw.auth_method.unwrap_or_else(|| "jwt".to_string()),
        },
    };

    // Parse modules and infer entities/operations/dependencies
    let module_map = raw.app.modules.unwrap_or_default();
    let mut modules: Vec<ModuleSpec> = module_map.iter().map(|(name, desc)| {
        let entities = infer_entities(desc);
        let operations = infer_operations(desc);
        ModuleSpec {
            name: name.clone(),
            description: desc.clone(),
            entities,
            operations,
            dependencies: vec![], // filled in below
        }
    }).collect();

    // Infer cross-module dependencies
    let all_entities: BTreeMap<String, String> = modules.iter()
        .flat_map(|m| m.entities.iter().map(|e| (e.clone(), m.name.clone())))
        .collect();

    for module in &mut modules {
        let mut deps: Vec<String> = module.description
            .split_whitespace()
            .filter_map(|word| {
                let clean: String = word.chars().filter(|c| c.is_alphabetic()).collect();
                let capitalized = capitalize(&clean);
                all_entities.get(&capitalized)
                    .filter(|owner| **owner != module.name)
                    .cloned()
            })
            .collect();
        deps.sort();
        deps.dedup();
        module.dependencies = deps;
    }
    modules.sort_by(|a, b| a.name.cmp(&b.name));

    // Infer frontend pages: one per module + dashboard
    let mut pages: Vec<PageSpec> = vec![
        PageSpec {
            name: "dashboard".to_string(),
            route: "/".to_string(),
            components: vec!["kpi-cards".to_string(), "summary-charts".to_string()],
        },
    ];
    for m in &modules {
        pages.push(PageSpec {
            name: m.name.clone(),
            route: format!("/{}", m.name),
            components: vec![
                "data-table".to_string(),
                "entity-form".to_string(),
                "action-bar".to_string(),
            ],
        });
    }

    let frontend = FrontendSpec {
        app_type: frontend_raw.app_type.unwrap_or_else(|| "spa".to_string()),
        framework: frontend_raw.framework.unwrap_or_else(|| "vanilla".to_string()),
        styling: frontend_raw.styling.unwrap_or_else(|| "minimal".to_string()),
        pages,
    };

    Ok(AppSpec {
        name: raw.app.name,
        description: raw.app.description,
        modules,
        backend,
        frontend,
        deployment: DeploymentSpec {
            containerized: deployment_raw.containerized.unwrap_or(true),
            compose: deployment_raw.compose.unwrap_or(true),
        },
        constraints: AppConstraints {
            max_crates: constraints_raw.max_crates.unwrap_or(1),
            test_coverage: constraints_raw.test_coverage.unwrap_or_else(|| "integration".to_string()),
            evidence_chain: constraints_raw.evidence_chain.unwrap_or(true),
        },
    })
}

// ─── Inference helpers ────────────────────────────────────────────────────────

/// Extract capitalised nouns from a description as entity names.
fn infer_entities(desc: &str) -> Vec<String> {
    let mut entities: Vec<String> = desc.split_whitespace()
        .filter(|w| {
            let c: String = w.chars().filter(|ch| ch.is_alphabetic()).collect();
            c.len() > 3 && c.chars().next().map(|ch| ch.is_uppercase()).unwrap_or(false)
        })
        .map(|w| w.chars().filter(|ch| ch.is_alphabetic()).collect::<String>())
        .collect();

    // Also infer from common patterns
    let lower = desc.to_lowercase();
    let patterns = [
        ("product", "Product"),
        ("order", "Order"),
        ("warehouse", "Warehouse"),
        ("user", "User"),
        ("report", "Report"),
        ("stock", "Stock"),
        ("inventory", "Inventory"),
        ("item", "Item"),
        ("category", "Category"),
    ];
    for (kw, entity) in patterns {
        if lower.contains(kw) && !entities.iter().any(|e| e == entity) {
            entities.push(entity.to_string());
        }
    }
    entities.sort();
    entities.dedup();
    entities
}

/// Extract CRUD-like operations from a description.
fn infer_operations(desc: &str) -> Vec<String> {
    let lower = desc.to_lowercase();
    let mut ops = Vec::new();
    if lower.contains("creat") || lower.contains("add") || lower.contains("new") { ops.push("create".to_string()); }
    if lower.contains("list") || lower.contains("view") || lower.contains("read") || lower.contains("catalog") { ops.push("list".to_string()); }
    if lower.contains("updat") || lower.contains("edit") || lower.contains("modif") { ops.push("update".to_string()); }
    if lower.contains("delet") || lower.contains("remov") || lower.contains("cancel") { ops.push("delete".to_string()); }
    if lower.contains("search") || lower.contains("filter") { ops.push("search".to_string()); }
    if lower.contains("report") || lower.contains("aggregat") || lower.contains("export") { ops.push("report".to_string()); }
    if ops.is_empty() {
        ops = vec!["create".to_string(), "list".to_string(), "update".to_string(), "delete".to_string()];
    }
    ops
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

// ─── Stage 1: PLAN ───────────────────────────────────────────────────────────

/// Create a full architecture plan from an `AppSpec`.
///
/// Assigns components to layers and determines the generation order via
/// topological sort on dependency relationships. Matches components against
/// the blueprint registry to flag known patterns.
pub fn plan(spec: &AppSpec, blueprints: &BlueprintRegistry) -> Result<Architecture> {
    let mut layers: Vec<Layer> = vec![
        Layer { name: "models".to_string(), components: vec![] },
        Layer { name: "database".to_string(), components: vec![] },
        Layer { name: "services".to_string(), components: vec![] },
        Layer { name: "api".to_string(), components: vec![] },
        Layer { name: "frontend".to_string(), components: vec![] },
        Layer { name: "tests".to_string(), components: vec![] },
        Layer { name: "deploy".to_string(), components: vec![] },
    ];

    let lang = spec.backend.language.as_str();
    let fw = spec.backend.framework.as_str();

    // Models layer — one component per entity per module
    let mut all_entities: Vec<(String, String)> = vec![]; // (entity, module)
    for module in &spec.modules {
        for entity in &module.entities {
            let comp_name = format!("{}_model", entity.to_lowercase());
            let file_path = format!("src/models/{}.rs", entity.to_lowercase());
            let req = GenerationRequest {
                component_type: "model".to_string(),
                language: lang.to_string(),
                framework: fw.to_string(),
                module_name: module.name.clone(),
                entities: vec![entity.clone()],
            };
            let blueprint_id = blueprints.find_match(&req).map(|bp| bp.id.clone());
            layers[0].components.push(Component {
                name: comp_name.clone(),
                layer: "models".to_string(),
                file_path,
                depends_on: vec![],
                blueprint_id,
                estimated_loc: 60,
            });
            all_entities.push((entity.clone(), module.name.clone()));
        }
    }

    // Database layer — migration + queries per module
    for module in &spec.modules {
        let comp_name = format!("{}_queries", module.name);
        let deps: Vec<String> = module.entities.iter()
            .map(|e| format!("{}_model", e.to_lowercase()))
            .collect();
        let req = GenerationRequest {
            component_type: "migration".to_string(),
            language: "sql".to_string(),
            framework: "".to_string(),
            module_name: module.name.clone(),
            entities: module.entities.clone(),
        };
        let blueprint_id = blueprints.find_match(&req).map(|bp| bp.id.clone());
        layers[1].components.push(Component {
            name: comp_name,
            layer: "database".to_string(),
            file_path: format!("src/database/{}_queries.rs", module.name),
            depends_on: deps,
            blueprint_id,
            estimated_loc: 80,
        });
    }
    // Pool component
    layers[1].components.push(Component {
        name: "db_pool".to_string(),
        layer: "database".to_string(),
        file_path: "src/database/pool.rs".to_string(),
        depends_on: vec![],
        blueprint_id: None,
        estimated_loc: 30,
    });

    // Services layer — one per module
    for module in &spec.modules {
        let comp_name = format!("{}_service", module.name);
        let mut deps: Vec<String> = module.entities.iter()
            .map(|e| format!("{}_model", e.to_lowercase()))
            .collect();
        deps.push(format!("{}_queries", module.name));
        let req = GenerationRequest {
            component_type: "crud_service".to_string(),
            language: lang.to_string(),
            framework: fw.to_string(),
            module_name: module.name.clone(),
            entities: module.entities.clone(),
        };
        let blueprint_id = blueprints.find_match(&req).map(|bp| bp.id.clone());
        layers[2].components.push(Component {
            name: comp_name,
            layer: "services".to_string(),
            file_path: format!("src/services/{}.rs", module.name),
            depends_on: deps,
            blueprint_id,
            estimated_loc: 120,
        });
    }

    // API layer — one per module
    for module in &spec.modules {
        let comp_name = format!("{}_api", module.name);
        let deps = vec![format!("{}_service", module.name)];
        let req = GenerationRequest {
            component_type: "rest_endpoint".to_string(),
            language: lang.to_string(),
            framework: fw.to_string(),
            module_name: module.name.clone(),
            entities: module.entities.clone(),
        };
        let blueprint_id = blueprints.find_match(&req).map(|bp| bp.id.clone());
        layers[3].components.push(Component {
            name: comp_name,
            layer: "api".to_string(),
            file_path: format!("src/api/{}.rs", module.name),
            depends_on: deps,
            blueprint_id,
            estimated_loc: 100,
        });
    }

    // Frontend layer — one per page
    for page in &spec.frontend.pages {
        let comp_name = format!("{}_page", page.name);
        let req = GenerationRequest {
            component_type: "frontend_page".to_string(),
            language: "javascript".to_string(),
            framework: spec.frontend.framework.clone(),
            module_name: page.name.clone(),
            entities: vec![],
        };
        let blueprint_id = blueprints.find_match(&req).map(|bp| bp.id.clone());
        layers[4].components.push(Component {
            name: comp_name,
            layer: "frontend".to_string(),
            file_path: format!("src/pages/{}.js", page.name),
            depends_on: vec![],
            blueprint_id,
            estimated_loc: 80,
        });
    }

    // Tests layer
    for module in &spec.modules {
        let comp_name = format!("{}_tests", module.name);
        let deps = vec![
            format!("{}_api", module.name),
            format!("{}_service", module.name),
        ];
        let req = GenerationRequest {
            component_type: "integration_test".to_string(),
            language: lang.to_string(),
            framework: fw.to_string(),
            module_name: module.name.clone(),
            entities: module.entities.clone(),
        };
        let blueprint_id = blueprints.find_match(&req).map(|bp| bp.id.clone());
        layers[5].components.push(Component {
            name: comp_name,
            layer: "tests".to_string(),
            file_path: format!("tests/{}_tests.rs", module.name),
            depends_on: deps,
            blueprint_id,
            estimated_loc: 80,
        });
    }

    // Deploy layer
    layers[6].components.push(Component {
        name: "docker_compose".to_string(),
        layer: "deploy".to_string(),
        file_path: "docker-compose.yml".to_string(),
        depends_on: vec![],
        blueprint_id: None,
        estimated_loc: 30,
    });

    // Build interfaces
    let interfaces = build_interfaces(spec);

    // Compute generation order
    let all_components: Vec<Component> = layers.iter()
        .flat_map(|l| l.components.iter().cloned())
        .collect();
    let generation_order = topological_sort(&all_components);

    let estimated_files: usize = layers.iter().map(|l| l.components.len()).sum::<usize>()
        + 5; // main.rs, config.rs, migrations, index.html, style.css
    let estimated_loc: usize = layers.iter()
        .flat_map(|l| l.components.iter())
        .map(|c| c.estimated_loc)
        .sum::<usize>()
        + 200; // shared infrastructure

    Ok(Architecture {
        app_name: spec.name.clone(),
        layers,
        generation_order,
        interfaces,
        estimated_files,
        estimated_loc,
    })
}

fn build_interfaces(spec: &AppSpec) -> Vec<Interface> {
    let mut interfaces = Vec::new();
    for module in &spec.modules {
        for entity in &module.entities {
            interfaces.push(Interface {
                from: format!("{}_api", module.name),
                to: format!("{}_service", module.name),
                interface_type: InterfaceType::FunctionCall,
                contract: format!("fn list_{}s() -> Vec<{}>", entity.to_lowercase(), entity),
            });
            interfaces.push(Interface {
                from: format!("{}_service", module.name),
                to: format!("{}_queries", module.name),
                interface_type: InterfaceType::DatabaseQuery,
                contract: format!("query_{}_list_all(pool)", entity.to_lowercase()),
            });
        }
    }
    interfaces
}

fn topological_sort(components: &[Component]) -> Vec<GenerationStep> {
    let mut in_degree: BTreeMap<String, usize> = BTreeMap::new();
    let mut adjacency: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for comp in components {
        in_degree.entry(comp.name.clone()).or_insert(0);
        for dep in &comp.depends_on {
            *in_degree.entry(comp.name.clone()).or_insert(0) += 1;
            adjacency.entry(dep.clone()).or_default().push(comp.name.clone());
        }
    }

    let mut queue: VecDeque<String> = in_degree.iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(name, _)| name.clone())
        .collect();

    let mut steps = Vec::new();
    let mut order = 1;

    while let Some(name) = queue.pop_front() {
        let comp = components.iter().find(|c| c.name == name);
        if let Some(comp) = comp {
            let reason = if comp.depends_on.is_empty() {
                format!("{} has no dependencies — generate first", comp.layer)
            } else {
                format!("{} depends on {} — generate after them", comp.layer, comp.depends_on.join(", "))
            };
            steps.push(GenerationStep {
                order,
                component: name.clone(),
                layer: comp.layer.clone(),
                file_path: comp.file_path.clone(),
                reason,
                can_parallel: vec![],
            });
            order += 1;
        }
        if let Some(dependents) = adjacency.get(&name) {
            for dep in dependents {
                let deg = in_degree.entry(dep.clone()).or_insert(0);
                if *deg > 0 { *deg -= 1; }
                if *deg == 0 {
                    queue.push_back(dep.clone());
                }
            }
        }
    }

    steps
}

/// Estimate the total size of the application.
pub fn estimate(arch: &Architecture) -> Estimate {
    let oracle_calls = arch.layers.iter()
        .find(|l| l.name == "services")
        .map(|l| l.components.len())
        .unwrap_or(0);
    Estimate {
        files: arch.estimated_files,
        loc: arch.estimated_loc,
        oracle_calls_expected: oracle_calls,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WAREHOUSE_TOML: &str = r#"
[app]
name = "warehouse-system"
description = "Warehouse management with inventory, orders, reporting"

[app.modules]
inventory = "Product catalog, stock levels, reorder alerts"
orders = "Order creation, fulfillment, cancellation, tracking"
reporting = "Daily reports, KPI dashboard"
auth = "JWT authentication, role-based access"

[backend]
language = "rust"
framework = "actix-web"
database = "postgresql"
auth_method = "jwt"

[frontend]
type = "spa"
framework = "vanilla"
styling = "minimal"

[deployment]
containerized = true
compose = true
"#;

    #[test]
    fn parse_warehouse_toml() {
        let spec = parse_toml_str(WAREHOUSE_TOML).unwrap();
        assert_eq!(spec.name, "warehouse-system");
        assert!(spec.modules.iter().any(|m| m.name == "inventory"));
        assert!(spec.modules.iter().any(|m| m.name == "orders"));
        assert_eq!(spec.backend.language, "rust");
        assert_eq!(spec.backend.framework, "actix-web");
        assert!(!spec.frontend.pages.is_empty());
    }

    #[test]
    fn plan_generates_all_layers() {
        let spec = parse_toml_str(WAREHOUSE_TOML).unwrap();
        let blueprints = BlueprintRegistry::with_builtins();
        let arch = plan(&spec, &blueprints).unwrap();

        assert!(arch.layers.iter().any(|l| l.name == "models" && !l.components.is_empty()));
        assert!(arch.layers.iter().any(|l| l.name == "services" && !l.components.is_empty()));
        assert!(arch.layers.iter().any(|l| l.name == "api" && !l.components.is_empty()));
        assert!(!arch.generation_order.is_empty());
    }

    #[test]
    fn generation_order_models_first() {
        let spec = parse_toml_str(WAREHOUSE_TOML).unwrap();
        let blueprints = BlueprintRegistry::with_builtins();
        let arch = plan(&spec, &blueprints).unwrap();

        // Find lowest-order step
        let first = arch.generation_order.iter().min_by_key(|s| s.order).unwrap();
        assert_eq!(first.layer, "models");
    }
}
