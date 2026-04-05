// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Self-defining norm system for ISLS v3.0.
//!
//! Norms are composable, reusable software patterns expressed as pure data.
//! They span all layers of a full-stack application: database migrations,
//! models, queries, services, API handlers, frontend components, tests, and
//! config.  The [`NormRegistry`] holds built-in norms and auto-discovers new
//! ones from synthesis runs via [`NormRegistry::observe_and_learn`].
//!
//! **Constraints:** this crate is pure data — no tokio, no reqwest, no axum.
//! Only `std::fs` is used for persisting auto-discovered norms to
//! `~/.isls/norms.json`.
//!
//! # Example
//!
//! ```rust
//! use isls_norms::NormRegistry;
//!
//! let registry = NormRegistry::default();
//! let activated = registry.match_description("I need a warehouse inventory system");
//! assert!(!activated.is_empty());
//! ```

pub mod catalog;
pub mod composition;
pub mod fitness;
pub mod genome;
pub mod learning;
pub mod types;

pub use catalog::builtin_norms;
pub use composition::{compose_norms, ComposedPlan};
pub use learning::{
    AbstractedArtifact, CandidateStatus, CrossLayerPattern, NormCandidate,
    ObservedArtifact, PromotionCriteria,
};
pub use types::{
    ActivatedNorm, ActivationSource, ApiArtifact, ConfigArtifact, DatabaseArtifact,
    FieldSource, FieldSpec, FrontendArtifact, FrontendComponent, InterfaceContract,
    LayerType, ModelArtifact, Norm, NormEvidence, NormLevel, NormLayers,
    NormModification, NormParameter, NormVariant, NormWiring, ParamType,
    QueryArtifact, ServiceArtifact, TestArtifact, TriggerPattern, ValidationSpec,
};

use std::collections::HashMap;
use std::path::PathBuf;

use thiserror::Error;

/// Errors produced by the norm system.
#[derive(Debug, Error)]
pub enum NormError {
    /// A required norm dependency is missing from the registry.
    #[error("missing norm dependency: {0}")]
    MissingDependency(String),
    /// Serialisation/deserialisation error.
    #[error("serialisation error: {0}")]
    Serde(#[from] serde_json::Error),
    /// IO error when persisting norms.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Composition conflict between two norms.
    #[error("composition conflict: {0}")]
    Conflict(String),
}

pub type Result<T> = std::result::Result<T, NormError>;

// ─── NormRegistry ─────────────────────────────────────────────────────────────

/// Registry of built-in and auto-discovered norms.
///
/// Holds the 20+ molecule norms, 3 organism norms, cross-norm wirings, and
/// the candidate pool for self-discovery.  Auto-discovered norms and candidates
/// are persisted to `~/.isls/norms.json`.
pub struct NormRegistry {
    norms: HashMap<String, Norm>,
    wirings: Vec<NormWiring>,
    candidates: HashMap<String, NormCandidate>,
    auto_id_counter: u32,
    persistence_path: Option<PathBuf>,
}

impl NormRegistry {
    /// Create a new registry pre-loaded with built-in norms and default
    /// persistence path (`~/.isls/norms.json`).
    pub fn new() -> Self {
        let mut registry = Self {
            norms: HashMap::new(),
            wirings: Vec::new(),
            candidates: HashMap::new(),
            auto_id_counter: 0,
            persistence_path: Self::default_persistence_path(),
        };
        for norm in builtin_norms() {
            registry.norms.insert(norm.id.clone(), norm);
        }
        registry.wirings = builtin_wirings();
        // Load auto-discovered norms if available
        let _ = registry.load();
        registry
    }

    /// Create an empty registry without builtins or persistence (for tests).
    pub fn empty_without_persistence() -> Self {
        Self {
            norms: HashMap::new(),
            wirings: Vec::new(),
            candidates: HashMap::new(),
            auto_id_counter: 0,
            persistence_path: None,
        }
    }

    /// Create a new registry pre-loaded with built-in norms but without
    /// disk persistence (for tests).
    pub fn new_without_persistence() -> Self {
        let mut registry = Self {
            norms: HashMap::new(),
            wirings: Vec::new(),
            candidates: HashMap::new(),
            auto_id_counter: 0,
            persistence_path: None,
        };
        for norm in builtin_norms() {
            registry.norms.insert(norm.id.clone(), norm);
        }
        registry.wirings = builtin_wirings();
        registry
    }

    fn default_persistence_path() -> Option<PathBuf> {
        dirs_path().map(|d| d.join("norms.json"))
    }

    /// Register a norm.
    pub fn register(&mut self, norm: Norm) {
        self.norms.insert(norm.id.clone(), norm);
    }

    /// Look up a norm by ID.
    pub fn get(&self, id: &str) -> Option<&Norm> {
        self.norms.get(id)
    }

    /// All registered norms.
    pub fn all_norms(&self) -> Vec<&Norm> {
        self.norms.values().collect()
    }

    /// Wirings between two norm IDs (order-independent).
    pub fn wirings_for(&self, a: &str, b: &str) -> Vec<&NormWiring> {
        self.wirings.iter().filter(|w| {
            (w.when.0 == a && w.when.1 == b) || (w.when.0 == b && w.when.1 == a)
        }).collect()
    }

    /// All norm wirings.
    pub fn all_wirings(&self) -> &[NormWiring] {
        &self.wirings
    }

    /// All candidates in the learning pool.
    pub fn candidates(&self) -> Vec<&NormCandidate> {
        self.candidates.values().collect()
    }

    /// Match a free-text description against the norm catalog using keyword
    /// scoring.  Returns norms sorted by descending confidence, filtered to
    /// those with confidence ≥ 0.3.
    pub fn match_description(&self, description: &str) -> Vec<ActivatedNorm> {
        let lower = description.to_lowercase();
        let mut scored: Vec<(f64, &Norm)> = self.norms.values().filter_map(|norm| {
            let confidence = score_norm(norm, &lower);
            if confidence >= 0.3 { Some((confidence, norm)) } else { None }
        }).collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().map(|(confidence, norm)| ActivatedNorm {
            norm: norm.clone(),
            confidence,
            source: ActivationSource::KeywordMatch,
        }).collect()
    }

    /// Persist auto-discovered norms and candidates to disk.
    pub fn save(&self) -> Result<()> {
        let path = match &self.persistence_path {
            Some(p) => p,
            None => return Ok(()),
        };
        // Collect only auto-discovered norms (non-builtin)
        let auto_norms: Vec<&Norm> = self.norms.values()
            .filter(|n| n.id.starts_with("ISLS-NORM-AUTO-"))
            .collect();
        let payload = serde_json::json!({
            "auto_norms": auto_norms,
            "candidates": self.candidates.values().collect::<Vec<_>>(),
            "auto_id_counter": self.auto_id_counter,
        });
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&payload)?)?;
        Ok(())
    }

    /// Load auto-discovered norms and candidates from disk.
    pub fn load(&mut self) -> Result<()> {
        let path = match &self.persistence_path {
            Some(p) => p.clone(),
            None => return Ok(()),
        };
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&path)?;
        let payload: serde_json::Value = serde_json::from_str(&content)?;

        if let Some(norms) = payload["auto_norms"].as_array() {
            for v in norms {
                if let Ok(norm) = serde_json::from_value::<Norm>(v.clone()) {
                    self.norms.insert(norm.id.clone(), norm);
                }
            }
        }
        if let Some(cands) = payload["candidates"].as_array() {
            for v in cands {
                if let Ok(cand) = serde_json::from_value::<NormCandidate>(v.clone()) {
                    // Key must be the pattern signature (same key used in observe_and_learn)
                    let key = cand.observations.first()
                        .map(|o| o.signature.clone())
                        .unwrap_or_else(|| cand.id.clone());
                    self.candidates.insert(key, cand);
                }
            }
        }
        if let Some(counter) = payload["auto_id_counter"].as_u64() {
            self.auto_id_counter = counter as u32;
        }
        Ok(())
    }

    /// Observe generated artifacts from a synthesis run and update the
    /// learning pool.  Eligible candidates are auto-promoted to norms.
    ///
    /// # Parameters
    /// - `artifacts`: layer→name→fields triples describing generated files
    /// - `domain`: domain name (e.g. "warehouse")
    /// - `run_id`: unique run identifier
    pub fn observe_and_learn(
        &mut self,
        artifacts: &[learning::ObservedArtifact],
        domain: &str,
        run_id: &str,
    ) {
        use learning::{extract_cross_layer_patterns, synthesize_norm, PromotionCriteria};
        let criteria = PromotionCriteria::default();

        let patterns = extract_cross_layer_patterns(artifacts, domain, run_id);

        for pattern in patterns {
            // Add to or update candidate pool
            let sig = pattern.signature.clone();
            let next_id = format!("ISLS-CAND-{:04}", self.candidates.len() + 1);
            let cand = self.candidates.entry(sig.clone()).or_insert_with(|| {
                NormCandidate::new(next_id, &pattern)
            });
            cand.observe(pattern);

            // Check promotion criteria
            if cand.meets_criteria(&criteria) {
                cand.status = CandidateStatus::Eligible;
                let promoted_norm = synthesize_norm(cand, &mut self.auto_id_counter);
                cand.status = CandidateStatus::Promoted;
                self.norms.insert(promoted_norm.id.clone(), promoted_norm);
            }
        }

        let _ = self.save();
    }
}

impl Default for NormRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Score a norm against a lowercase description string.
///
/// Returns a value in [0.0, 1.0].  Even a single keyword match yields ≥ 0.2
/// so that organism norms (which have many keywords) are not penalised for
/// the keywords that don't appear in the description.
fn score_norm(norm: &Norm, lower_desc: &str) -> f64 {
    let mut hits = 0usize;
    for trigger in &norm.triggers {
        for kw in &trigger.keywords {
            if lower_desc.contains(kw.as_str()) {
                hits += 1;
            }
        }
        for concept in &trigger.concepts {
            if lower_desc.contains(concept.as_str()) {
                hits += 1;
            }
        }
    }
    if hits == 0 { return 0.0; }
    // Scale: each hit adds 0.25, capped at 1.0
    f64::min(1.0, hits as f64 * 0.25)
}

/// Check if a norm structurally matches a cross-layer pattern.
fn norm_matches_pattern(norm: &Norm, pattern: &CrossLayerPattern) -> bool {
    // A norm matches if ≥60% of its layer types are present in the pattern
    let norm_layers = norm_layer_types(norm);
    if norm_layers.is_empty() { return false; }
    let matches = norm_layers.iter()
        .filter(|lt| pattern.layers_present.contains(lt))
        .count();
    matches as f64 / norm_layers.len() as f64 >= 0.6
}

fn norm_layer_types(norm: &Norm) -> Vec<LayerType> {
    let mut layers = Vec::new();
    if !norm.layers.database.is_empty() { layers.push(LayerType::Database); }
    if !norm.layers.model.is_empty()    { layers.push(LayerType::Model); }
    if !norm.layers.query.is_empty()    { layers.push(LayerType::Query); }
    if !norm.layers.service.is_empty()  { layers.push(LayerType::Service); }
    if !norm.layers.api.is_empty()      { layers.push(LayerType::Api); }
    if !norm.layers.frontend.is_empty() { layers.push(LayerType::Frontend); }
    if !norm.layers.test.is_empty()     { layers.push(LayerType::Test); }
    if !norm.layers.config.is_empty()   { layers.push(LayerType::Config); }
    layers
}

/// Returns the ISLS data directory (`~/.isls`).
fn dirs_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|h| PathBuf::from(h).join(".isls"))
}

fn builtin_wirings() -> Vec<NormWiring> {
    vec![
        // Order + Inventory → fulfill_order calls adjust_stock
        NormWiring {
            when: ("ISLS-NORM-0042".into(), "ISLS-NORM-0112".into()),
            description: "Order fulfillment calls inventory adjustment".into(),
            add_services: vec![ServiceArtifact {
                name: "fulfill_order_stock".into(),
                description: "Deduct inventory for each order line on fulfillment".into(),
                method_signatures: vec![
                    "pub async fn fulfill_order(pool: &PgPool, order_id: i64) -> Result<(), AppError>".into(),
                ],
                business_rules: vec!["for each order line: adjust_stock(product_id, -quantity)".into()],
            }],
            add_rules: vec![],
            add_tests: vec![],
        },
        // Auth + All entities → inject auth check on all endpoints
        NormWiring {
            when: ("ISLS-NORM-0088".into(), "ISLS-NORM-0042".into()),
            description: "Auth norm injects require_role check on CRUD endpoints".into(),
            add_services: vec![],
            add_rules: vec![types::BusinessRule {
                name: "auth_required".into(),
                trigger: "on_request".into(),
                condition: "user.is_authenticated()".into(),
                action: "require_role(user, min_role)?".into(),
            }],
            add_tests: vec![],
        },
    ]
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_loads_builtins() {
        let reg = NormRegistry::default();
        assert!(reg.norms.len() >= 20, "should have at least 20 built-in norms, got {}", reg.norms.len());
    }

    #[test]
    fn test_match_warehouse_description() {
        let reg = NormRegistry::default();
        let activated = reg.match_description("warehouse inventory management with stock tracking");
        assert!(!activated.is_empty(), "should match at least one norm for warehouse");
        // The warehouse organism should be among the top matches
        let has_warehouse = activated.iter().any(|a| a.norm.id == "ISLS-NORM-0500");
        assert!(has_warehouse, "warehouse organism norm should be activated");
    }

    #[test]
    fn test_match_returns_sorted_by_confidence() {
        let reg = NormRegistry::default();
        let activated = reg.match_description("crud entity with pagination and search");
        let confidences: Vec<f64> = activated.iter().map(|a| a.confidence).collect();
        for i in 1..confidences.len() {
            assert!(confidences[i - 1] >= confidences[i], "should be sorted desc");
        }
    }

    #[test]
    fn test_compose_warehouse_norms() {
        let reg = NormRegistry::default();
        let activated = reg.match_description("warehouse inventory management with orders");
        assert!(!activated.is_empty());
        let params = HashMap::new();
        let plan = compose_norms(&activated, &params).expect("composition should succeed");
        // Warehouse organism should produce multiple model artifacts
        assert!(!plan.models.is_empty() || !plan.api.is_empty(),
            "composed plan should have models or api artifacts");
    }
}
