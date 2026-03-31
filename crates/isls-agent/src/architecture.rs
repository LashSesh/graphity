// isls-agent: architecture.rs — Feature → Technical Architecture (invisible to operator)
//
// Translates human-readable features into technical components.
// The operator NEVER sees this — it's the Agent's internal planning.

use serde::{Deserialize, Serialize};

use crate::stubs::{
    Archetype, FillStrategy, OutputFormat, SynthesisOracle, SynthesisPrompt, TemplateCatalog,
};

use crate::apply::strip_markdown_fences;
use crate::feature::Feature;
use crate::workspace::AgentWorkspace;

// ─── TechnicalComponent ─────────────────────────────────────────────────────

/// A single technical component the Agent must create or modify.
/// This is INTERNAL — the operator sees Feature names, not these.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TechnicalComponent {
    /// Links back to Feature.name
    pub feature: String,
    /// Internal file path: "src/search.rs"
    pub file_path: String,
    /// "database_query", "route_handler", "model", "test", "config", "main"
    pub component_type: String,
    /// How to fill: Oracle, Pattern, or Static
    pub fill_strategy: FillStrategy,
    /// Technical prompt for the LLM (invisible to operator)
    pub description_for_oracle: String,
}

// ─── TechnicalPlan ──────────────────────────────────────────────────────────

/// The Agent's internal blueprint for implementing the operator's features.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TechnicalPlan {
    pub project_name: String,
    /// Auto-selected template name from catalog (if new project)
    pub template: Option<String>,
    pub components: Vec<TechnicalComponent>,
    pub estimated_oracle_calls: usize,
    pub estimated_cost_usd: f64,
    pub test_count: usize,
}

// ─── Architecture Planning ──────────────────────────────────────────────────

/// Plan the technical architecture for a set of features.
///
/// If no workspace exists (new project), select a template from the catalog.
/// If a workspace exists, analyze it and plan delta additions.
pub fn plan_architecture(
    features: &[Feature],
    workspace: &Option<AgentWorkspace>,
    catalog: &TemplateCatalog,
    _oracle: &dyn SynthesisOracle,
) -> Result<TechnicalPlan, String> {
    let project_name = derive_project_name(features);

    // Determine template (for new projects)
    let template = if workspace.is_none() {
        select_template(features, catalog)
    } else {
        None
    };

    let mut components = Vec::new();

    // For each feature, generate technical components
    for feature in features {
        let feature_components = if let Some(ref ws) = workspace {
            plan_delta_components(feature, ws)
        } else {
            plan_new_components(feature, &project_name)
        };
        components.extend(feature_components);
    }

    // Add shared components for new projects
    if workspace.is_none() {
        // Main entry point
        components.push(TechnicalComponent {
            feature: "Projekt-Setup".into(),
            file_path: "src/main.rs".into(),
            component_type: "main".into(),
            fill_strategy: FillStrategy::Static {
                content: String::new(),
            },
            description_for_oracle: format!(
                "Create main.rs for project '{}' that initializes and runs the application",
                project_name
            ),
        });

        // Cargo.toml
        components.push(TechnicalComponent {
            feature: "Projekt-Setup".into(),
            file_path: "Cargo.toml".into(),
            component_type: "config".into(),
            fill_strategy: FillStrategy::Static {
                content: String::new(),
            },
            description_for_oracle: format!(
                "Create Cargo.toml for project '{}' with appropriate dependencies",
                project_name
            ),
        });

        // Tests
        components.push(TechnicalComponent {
            feature: "Qualitätssicherung".into(),
            file_path: "tests/integration.rs".into(),
            component_type: "test".into(),
            fill_strategy: FillStrategy::Oracle,
            description_for_oracle: format!(
                "Create integration tests for features: {}",
                features
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }

    let oracle_calls = components
        .iter()
        .filter(|c| c.fill_strategy == FillStrategy::Oracle)
        .count();
    let cost_per_call = 0.003; // estimated average

    Ok(TechnicalPlan {
        project_name,
        template,
        components,
        estimated_oracle_calls: oracle_calls,
        estimated_cost_usd: oracle_calls as f64 * cost_per_call,
        test_count: 0,
    })
}

/// Plan architecture using the Oracle for complex decomposition.
pub fn plan_architecture_with_oracle(
    features: &[Feature],
    workspace: &Option<AgentWorkspace>,
    catalog: &TemplateCatalog,
    oracle: &dyn SynthesisOracle,
) -> Result<TechnicalPlan, String> {
    // For simple cases (≤5 features), use deterministic planning
    if features.len() <= 5 {
        return plan_architecture(features, workspace, catalog, oracle);
    }

    // For complex cases, use the Oracle to help plan
    let feature_summary: String = features
        .iter()
        .map(|f| format!("- {}: {}", f.name, f.description))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = SynthesisPrompt {
        system: "You are a software architect. Given a list of features, output a JSON array \
                 of components needed. Each component has: feature (string), file_path (string), \
                 component_type (string: model/route_handler/database_query/test/config/main), \
                 description (string). Output ONLY valid JSON."
            .into(),
        user: format!(
            "Plan components for these features:\n{}\n\nExisting project: {}",
            feature_summary,
            workspace
                .as_ref()
                .map(|w| w.summary())
                .unwrap_or_else(|| "New project".into())
        ),
        output_format: OutputFormat::Json,
        max_tokens: 2048,
        temperature: 0.0,
    };

    let response = oracle
        .synthesize(&prompt)
        .map_err(|e| format!("Architecture Oracle error: {}", e))?;

    let cleaned = strip_markdown_fences(&response.content);

    // Try to parse Oracle response; fall back to deterministic if it fails
    match serde_json::from_str::<Vec<serde_json::Value>>(&cleaned) {
        Ok(items) => {
            let mut components: Vec<TechnicalComponent> = Vec::new();
            for item in &items {
                components.push(TechnicalComponent {
                    feature: item["feature"].as_str().unwrap_or("unknown").into(),
                    file_path: item["file_path"].as_str().unwrap_or("src/lib.rs").into(),
                    component_type: item["component_type"]
                        .as_str()
                        .unwrap_or("model")
                        .into(),
                    fill_strategy: FillStrategy::Oracle,
                    description_for_oracle: item["description"]
                        .as_str()
                        .unwrap_or("")
                        .into(),
                });
            }
            let oracle_calls = components.len();
            let project_name = derive_project_name(features);
            Ok(TechnicalPlan {
                project_name,
                template: None,
                components,
                estimated_oracle_calls: oracle_calls,
                estimated_cost_usd: oracle_calls as f64 * 0.003,
                test_count: 0,
            })
        }
        Err(_) => plan_architecture(features, workspace, catalog, oracle),
    }
}

// ─── Internal Helpers ───────────────────────────────────────────────────────

/// Derive a kebab-case project name from the features.
fn derive_project_name(features: &[Feature]) -> String {
    if features.is_empty() {
        return "new-project".into();
    }
    // Use data entities from features
    let entities: Vec<&str> = features
        .iter()
        .flat_map(|f| f.data_involved.iter())
        .map(|s| s.as_str())
        .collect();
    if !entities.is_empty() {
        let name = entities[0].to_lowercase().replace(' ', "-");
        format!("{}-manager", name)
    } else {
        let first = &features[0].name;
        first
            .to_lowercase()
            .replace(' ', "-")
            .replace(|c: char| !c.is_alphanumeric() && c != '-', "")
    }
}

/// Select the best matching template from the catalog based on features.
fn select_template(features: &[Feature], catalog: &TemplateCatalog) -> Option<String> {
    if catalog.is_empty() {
        return None;
    }

    // Infer archetype from feature capabilities and data
    let all_caps: String = features
        .iter()
        .flat_map(|f| f.capabilities.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    let archetype = if all_caps.contains("api")
        || all_caps.contains("server")
        || all_caps.contains("http")
        || all_caps.contains("rest")
    {
        Archetype::RestApi
    } else if all_caps.contains("cli") || all_caps.contains("command") {
        Archetype::CliTool
    } else {
        // Default: REST API (most common for CRUD apps)
        Archetype::RestApi
    };

    catalog
        .find_by_archetype(&archetype)
        .first()
        .map(|t| t.name.clone())
}

/// Plan components for a feature in a NEW project.
fn plan_new_components(feature: &Feature, project_name: &str) -> Vec<TechnicalComponent> {
    let mut components = Vec::new();
    let module_name = feature
        .name
        .to_lowercase()
        .replace(' ', "_")
        .replace(|c: char| !c.is_alphanumeric() && c != '_', "");

    // Model/data component
    if !feature.data_involved.is_empty() {
        components.push(TechnicalComponent {
            feature: feature.name.clone(),
            file_path: format!("src/{}.rs", module_name),
            component_type: "model".into(),
            fill_strategy: FillStrategy::Oracle,
            description_for_oracle: format!(
                "Create data models and CRUD operations for {}: {}. \
                 Project: {}. Data: {:?}. Capabilities: {:?}",
                feature.name,
                feature.description,
                project_name,
                feature.data_involved,
                feature.capabilities
            ),
        });
    }

    components
}

/// Plan DELTA components for adding a feature to an EXISTING project.
fn plan_delta_components(feature: &Feature, workspace: &AgentWorkspace) -> Vec<TechnicalComponent> {
    let mut components = Vec::new();

    // Find relevant existing files
    let relevant = workspace.relevant_files(&feature.name);

    if relevant.is_empty() {
        // New file needed
        let module_name = feature
            .name
            .to_lowercase()
            .replace(' ', "_")
            .replace(|c: char| !c.is_alphanumeric() && c != '_', "");
        components.push(TechnicalComponent {
            feature: feature.name.clone(),
            file_path: format!("src/{}.rs", module_name),
            component_type: "model".into(),
            fill_strategy: FillStrategy::Oracle,
            description_for_oracle: format!(
                "Create new module for {}: {}. \
                 Existing project types: {:?}",
                feature.name,
                feature.description,
                workspace.types.iter().map(|t| &t.name).collect::<Vec<_>>()
            ),
        });
    } else {
        // Modify existing files
        for module in relevant.iter().take(2) {
            components.push(TechnicalComponent {
                feature: feature.name.clone(),
                file_path: module.path.clone(),
                component_type: "modification".into(),
                fill_strategy: FillStrategy::Oracle,
                description_for_oracle: format!(
                    "Modify {} to add {}: {}. \
                     Existing items in file: {:?}",
                    module.path,
                    feature.name,
                    feature.description,
                    module.public_items
                ),
            });
        }
    }

    components
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::Feature;
    use crate::stubs::{OracleCost, OracleResponse, OracleResult};

    // Mock Oracle that does nothing (planning is deterministic)
    struct MockOracle;
    impl SynthesisOracle for MockOracle {
        fn name(&self) -> &str { "mock" }
        fn model(&self) -> &str { "mock-v1" }
        fn available(&self) -> bool { true }
        fn synthesize(&self, _: &SynthesisPrompt) -> OracleResult<OracleResponse> {
            Ok(OracleResponse {
                content: "[]".into(),
                model: "mock".into(),
                tokens_used: 0,
                finish_reason: "stop".into(),
                latency_ms: 0,
            })
        }
        fn cost_estimate(&self) -> OracleCost { OracleCost::default() }
    }

    fn sample_features() -> Vec<Feature> {
        vec![
            Feature {
                name: "Bookmarks anlegen".into(),
                description: "Neue Bookmarks mit Titel und URL anlegen".into(),
                capabilities: vec!["Bookmark erstellen".into(), "Bookmark anzeigen".into()],
                data_involved: vec!["Bookmarks".into()],
                priority: 1,
            },
            Feature {
                name: "Bookmarks durchsuchen".into(),
                description: "Bookmarks nach Titel durchsuchen".into(),
                capabilities: vec!["Textsuche".into(), "Ergebnisliste".into()],
                data_involved: vec!["Bookmarks".into()],
                priority: 1,
            },
            Feature {
                name: "Tags verwalten".into(),
                description: "Tags zu Bookmarks hinzufügen und filtern".into(),
                capabilities: vec!["Tags zuweisen".into(), "Nach Tags filtern".into()],
                data_involved: vec!["Tags".into()],
                priority: 2,
            },
        ]
    }

    // AT-AG14: Architecture planning — 3 features → technical plan with components
    #[test]
    fn at_ag14_architecture_planning() {
        let features = sample_features();
        let oracle = MockOracle;
        let catalog = TemplateCatalog::new(crate::stubs::TemplateConfig::default());

        let plan = plan_architecture(&features, &None, &catalog, &oracle)
            .expect("plan should succeed");

        assert!(!plan.project_name.is_empty(), "project name derived");
        assert!(
            plan.components.len() >= 3,
            "AT-AG14: expected ≥3 components for 3 features, got {}",
            plan.components.len()
        );
        assert!(plan.estimated_cost_usd >= 0.0, "cost estimated");

        // Each feature should have at least one component
        for feature in &features {
            let has_component = plan
                .components
                .iter()
                .any(|c| c.feature == feature.name);
            assert!(
                has_component || feature.data_involved.is_empty(),
                "feature '{}' should have a component",
                feature.name
            );
        }
    }

    // AT-AG14b: Project name derivation
    #[test]
    fn at_ag14b_project_name() {
        let features = sample_features();
        let name = derive_project_name(&features);
        assert!(
            name.contains("bookmark") || name.contains("manager"),
            "project name should relate to features, got: {}",
            name
        );
    }

    // AT-AG14c: Delta planning for existing workspace
    #[test]
    fn at_ag14c_delta_planning() {
        // Create a temp workspace
        let dir = std::env::temp_dir().join(format!(
            "isls_arch_test_{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"test\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub struct Bookmark { pub id: i64 }\npub fn list() -> Vec<Bookmark> { vec![] }\n",
        )
        .unwrap();

        let ws = AgentWorkspace::analyze(&dir).expect("analyze");
        let oracle = MockOracle;
        let catalog = TemplateCatalog::new(crate::stubs::TemplateConfig::default());

        // Add a single new feature to existing project
        let new_feature = Feature {
            name: "Bookmarks löschen".into(),
            description: "Bookmarks dauerhaft entfernen".into(),
            capabilities: vec!["Löschen".into()],
            data_involved: vec!["Bookmarks".into()],
            priority: 2,
        };

        let plan =
            plan_architecture(&[new_feature], &Some(ws), &catalog, &oracle)
                .expect("delta plan");

        assert!(
            !plan.components.is_empty(),
            "delta plan should have components"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
