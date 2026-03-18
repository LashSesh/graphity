// isls-multilang/src/templates.rs
//
// Six full-stack templates T11–T16 (spec §7).
// Each template is an IR tree with multi-language atoms.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use crate::codegen::{FillStrategy, MultiLangAtom};

// ─── Template ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MultiLangTemplate {
    /// Template ID (T11–T16)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Slug used in CLI (e.g. "saas-starter")
    pub slug: String,
    /// Languages this template targets
    pub languages: Vec<String>,
    /// Number of atoms
    pub atom_count: usize,
    /// Number of molecules
    pub molecule_count: usize,
    /// Atoms with backend assignments
    pub atoms: Vec<MultiLangAtom>,
}

impl MultiLangTemplate {
    pub fn languages_display(&self) -> String {
        self.languages.join(" + ")
    }
}

// ─── Catalog ─────────────────────────────────────────────────────────────────

pub struct TemplateCatalog {
    templates: BTreeMap<String, MultiLangTemplate>,
}

impl Default for TemplateCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateCatalog {
    pub fn new() -> Self {
        let mut cat = Self { templates: BTreeMap::new() };
        for t in builtin_templates() {
            cat.templates.insert(t.slug.clone(), t);
        }
        cat
    }

    pub fn get(&self, slug: &str) -> Option<&MultiLangTemplate> {
        self.templates.get(slug)
    }

    pub fn list(&self) -> Vec<&MultiLangTemplate> {
        self.templates.values().collect()
    }

    /// Find the best matching template for an intent string.
    pub fn best_match_for_intent(&self, intent: &str) -> Option<&MultiLangTemplate> {
        let intent_lower = intent.to_lowercase();
        // Score each template by keyword overlap
        let mut best_score = 0usize;
        let mut best_template: Option<&MultiLangTemplate> = None;

        for template in self.templates.values() {
            let mut score = 0;
            let name_lower = template.name.to_lowercase();
            let slug_lower = template.slug.to_lowercase();

            // Check name/slug keyword overlap
            for word in intent_lower.split_whitespace() {
                if name_lower.contains(word) || slug_lower.contains(word) {
                    score += 2;
                }
            }
            // Check language overlap
            for lang in &template.languages {
                if intent_lower.contains(lang.to_lowercase().as_str()) {
                    score += 1;
                }
            }

            if score > best_score {
                best_score = score;
                best_template = Some(template);
            }
        }

        best_template
    }
}

// ─── Built-in Templates ───────────────────────────────────────────────────────

fn builtin_templates() -> Vec<MultiLangTemplate> {
    vec![
        MultiLangTemplate {
            id: "T11".to_string(),
            name: "SaaS Starter".to_string(),
            slug: "saas-starter".to_string(),
            languages: vec!["rust".into(), "typescript".into(), "sql".into(), "yaml".into(), "markdown".into()],
            atom_count: 22,
            molecule_count: 6,
            atoms: vec![
                atom("auth-service",     "rust",       FillStrategy::Oracle,  vec!["n_fn_0"]),
                atom("user-model",       "sql",        FillStrategy::Oracle,  vec!["n_fn_1"]),
                atom("api-routes",       "rust",       FillStrategy::Oracle,  vec!["n_fn_2"]),
                atom("frontend-types",   "typescript", FillStrategy::Derive,  vec!["n_fn_3"]),
                atom("docker-infra",     "yaml",       FillStrategy::Static,  vec![]),
                atom("readme",           "markdown",   FillStrategy::Derive,  vec![]),
            ],
        },
        MultiLangTemplate {
            id: "T12".to_string(),
            name: "Dashboard".to_string(),
            slug: "dashboard".to_string(),
            languages: vec!["rust".into(), "typescript".into(), "sql".into(), "yaml".into()],
            atom_count: 18,
            molecule_count: 5,
            atoms: vec![
                atom("data-api",         "rust",       FillStrategy::Oracle,  vec!["n_fn_0"]),
                atom("metrics-model",    "sql",        FillStrategy::Oracle,  vec!["n_fn_1"]),
                atom("dashboard-ui",     "typescript", FillStrategy::Oracle,  vec!["n_fn_2"]),
                atom("deployment",       "yaml",       FillStrategy::Static,  vec![]),
            ],
        },
        MultiLangTemplate {
            id: "T13".to_string(),
            name: "API + Docs".to_string(),
            slug: "api-docs".to_string(),
            languages: vec!["rust".into(), "markdown".into(), "yaml".into()],
            atom_count: 12,
            molecule_count: 3,
            atoms: vec![
                atom("api-server",       "rust",       FillStrategy::Oracle,  vec!["n_fn_0"]),
                atom("api-docs",         "markdown",   FillStrategy::Derive,  vec![]),
                atom("deployment",       "yaml",       FillStrategy::Static,  vec![]),
            ],
        },
        MultiLangTemplate {
            id: "T14".to_string(),
            name: "Python ML".to_string(),
            slug: "python-ml".to_string(),
            languages: vec!["python".into(), "sql".into(), "yaml".into()],
            atom_count: 10,
            molecule_count: 3,
            atoms: vec![
                atom("ml-model",         "python",     FillStrategy::Oracle,  vec!["n_fn_0"]),
                atom("feature-store",    "sql",        FillStrategy::Oracle,  vec!["n_fn_1"]),
                atom("training-infra",   "yaml",       FillStrategy::Static,  vec![]),
            ],
        },
        MultiLangTemplate {
            id: "T15".to_string(),
            name: "Static Site".to_string(),
            slug: "static-site".to_string(),
            languages: vec!["typescript".into(), "markdown".into()],
            atom_count: 8,
            molecule_count: 2,
            atoms: vec![
                atom("site-components",  "typescript", FillStrategy::Oracle,  vec!["n_fn_0"]),
                atom("content-docs",     "markdown",   FillStrategy::Derive,  vec![]),
            ],
        },
        MultiLangTemplate {
            id: "T16".to_string(),
            name: "Monorepo".to_string(),
            slug: "monorepo".to_string(),
            languages: vec!["rust".into(), "typescript".into(), "sql".into(), "yaml".into(), "markdown".into()],
            atom_count: 16,
            molecule_count: 5,
            atoms: vec![
                atom("core-lib",         "rust",       FillStrategy::Oracle,  vec!["n_fn_0"]),
                atom("api-gateway",      "rust",       FillStrategy::Oracle,  vec!["n_fn_1"]),
                atom("web-app",          "typescript", FillStrategy::Oracle,  vec!["n_fn_2"]),
                atom("shared-db",        "sql",        FillStrategy::Oracle,  vec!["n_fn_3"]),
                atom("infra",            "yaml",       FillStrategy::Static,  vec![]),
                atom("mono-docs",        "markdown",   FillStrategy::Derive,  vec![]),
            ],
        },
    ]
}

fn atom(name: &str, backend: &str, fill: FillStrategy, node_ids: Vec<&str>) -> MultiLangAtom {
    MultiLangAtom {
        name: name.to_string(),
        backend: backend.to_string(),
        fill,
        ir_node_ids: node_ids.into_iter().map(|s| s.to_string()).collect(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_all_templates() {
        let cat = TemplateCatalog::new();
        let templates = cat.list();
        assert_eq!(templates.len(), 6, "must have 6 templates T11-T16");
        let ids: Vec<_> = templates.iter().map(|t| t.id.as_str()).collect();
        for id in &["T11", "T12", "T13", "T14", "T15", "T16"] {
            assert!(ids.contains(id), "must contain template {id}");
        }
    }

    #[test]
    fn template_languages_match_spec() {
        let cat = TemplateCatalog::new();
        let t11 = cat.get("saas-starter").expect("T11");
        assert!(t11.languages.contains(&"rust".to_string()));
        assert!(t11.languages.contains(&"typescript".to_string()));
        assert!(t11.languages.contains(&"sql".to_string()));

        let t14 = cat.get("python-ml").expect("T14");
        assert!(t14.languages.contains(&"python".to_string()));
    }

    #[test]
    fn template_match_saas() {
        let cat = TemplateCatalog::new();
        let result = cat.best_match_for_intent("Build a SaaS starter app");
        assert!(result.is_some(), "should match a template");
    }

    // AT-BB12: Catalog knows T11 SaaS Starter with all required languages.
    #[test]
    fn at_bb12_t11_languages() {
        let cat = TemplateCatalog::new();
        let t11 = cat.get("saas-starter").expect("T11 must exist");
        let required = ["rust", "typescript", "sql", "yaml", "markdown"];
        for lang in &required {
            assert!(
                t11.languages.contains(&lang.to_string()),
                "AT-BB12: T11 must include language '{lang}'"
            );
        }
    }
}
