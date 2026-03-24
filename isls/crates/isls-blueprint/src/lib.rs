// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Blueprint registry for ISLS full-stack code generation.
//!
//! Stores structural code generation patterns (blueprints) and matches them
//! against generation requests. Confidence scores are updated based on usage
//! outcomes, enabling progressive autonomy: common patterns converge toward
//! fully offline generation without LLM calls.

use std::path::Path;
use std::fs;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use sha2::{Digest, Sha256};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum BlueprintError {
    #[error("blueprint not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, BlueprintError>;

// ─── GenerationRequest ───────────────────────────────────────────────────────

/// Describes a code generation request for blueprint matching.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerationRequest {
    /// Component archetype, e.g. "crud_service", "rest_endpoint", "model", "migration".
    pub component_type: String,
    /// Target language, e.g. "rust", "javascript", "sql".
    pub language: String,
    /// Framework hint, e.g. "actix-web", "vanilla".
    pub framework: String,
    /// Module name, e.g. "inventory", "orders".
    pub module_name: String,
    /// Primary entities involved, e.g. ["Product", "Warehouse"].
    pub entities: Vec<String>,
}

// ─── Blueprint ───────────────────────────────────────────────────────────────

/// A crystallised structural code generation pattern.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Blueprint {
    /// Content-addressed identifier (SHA-256 of name + pattern_type + language).
    pub id: String,
    /// Human-readable name, e.g. "rust-crud-service".
    pub name: String,
    /// Component archetype this blueprint handles.
    pub pattern_type: String,
    /// Target language.
    pub language: String,
    /// Framework hint.
    pub framework: String,
    /// Template sketch with `{{placeholder}}` variables.
    pub template_sketch: String,
    /// Confidence score in [0.0, 1.0]. Updated after each usage.
    pub confidence: f64,
    /// Total number of times this blueprint was matched.
    pub usage_count: usize,
    /// Number of times this blueprint led to a successful (first-attempt) generation.
    pub success_count: usize,
}

impl Blueprint {
    /// Create a new blueprint with an auto-generated id.
    pub fn new(
        name: impl Into<String>,
        pattern_type: impl Into<String>,
        language: impl Into<String>,
        framework: impl Into<String>,
        template_sketch: impl Into<String>,
        confidence: f64,
    ) -> Self {
        let name = name.into();
        let pattern_type = pattern_type.into();
        let language = language.into();
        let framework = framework.into();
        let template_sketch = template_sketch.into();
        let id = make_id(&name, &pattern_type, &language);
        Blueprint { id, name, pattern_type, language, framework, template_sketch, confidence, usage_count: 0, success_count: 0 }
    }

    /// Matching score against a generation request (0.0 – 1.0).
    fn match_score(&self, req: &GenerationRequest) -> f64 {
        let type_match = if self.pattern_type == req.component_type { 1.0 } else { 0.0 };
        if type_match == 0.0 { return 0.0; }
        let lang_match = if self.language == req.language { 1.0 } else { 0.5 };
        let fw_match = if self.framework.is_empty() || self.framework == req.framework { 1.0 } else { 0.7 };
        (type_match * 0.5 + lang_match * 0.3 + fw_match * 0.2) * self.confidence
    }
}

fn make_id(name: &str, pattern_type: &str, language: &str) -> String {
    let mut h = Sha256::new();
    h.update(name.as_bytes());
    h.update(b"|");
    h.update(pattern_type.as_bytes());
    h.update(b"|");
    h.update(language.as_bytes());
    let bytes = h.finalize();
    bytes.iter().take(8).map(|b| format!("{:02x}", b)).collect()
}

// ─── BlueprintRegistry ───────────────────────────────────────────────────────

/// Registry that accumulates and matches code generation blueprints.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BlueprintRegistry {
    blueprints: Vec<Blueprint>,
}

impl BlueprintRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self { blueprints: Vec::new() }
    }

    /// Create a registry pre-populated with built-in templates for common patterns.
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        reg.add(Blueprint::new(
            "rust-crud-service",
            "crud_service",
            "rust",
            "",
            "// CRUD service for {{entity}}",
            0.9,
        ));
        reg.add(Blueprint::new(
            "rust-actix-rest-endpoint",
            "rest_endpoint",
            "rust",
            "actix-web",
            "// REST endpoints for {{entity}}",
            0.9,
        ));
        reg.add(Blueprint::new(
            "rust-model",
            "model",
            "rust",
            "",
            "// Rust struct for {{entity}}",
            0.95,
        ));
        reg.add(Blueprint::new(
            "sql-crud-migration",
            "migration",
            "sql",
            "",
            "-- Migration for {{entity}}",
            0.95,
        ));
        reg.add(Blueprint::new(
            "js-vanilla-spa-page",
            "frontend_page",
            "javascript",
            "vanilla",
            "// Vanilla JS page for {{module}}",
            0.85,
        ));
        reg.add(Blueprint::new(
            "rust-integration-test",
            "integration_test",
            "rust",
            "actix-web",
            "// Integration test for {{entity}}",
            0.80,
        ));
        reg
    }

    /// Find the best matching blueprint for the given request, or `None` if
    /// no blueprint achieves a match score above the threshold.
    pub fn find_match(&self, req: &GenerationRequest) -> Option<&Blueprint> {
        const MIN_SCORE: f64 = 0.4;
        self.blueprints.iter()
            .map(|bp| (bp.match_score(req), bp))
            .filter(|(score, _)| *score >= MIN_SCORE)
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, bp)| bp)
    }

    /// Add a blueprint to the registry (replaces existing with same id).
    pub fn add(&mut self, blueprint: Blueprint) {
        if let Some(existing) = self.blueprints.iter_mut().find(|b| b.id == blueprint.id) {
            *existing = blueprint;
        } else {
            self.blueprints.push(blueprint);
        }
    }

    /// Record a usage outcome for a blueprint.
    ///
    /// - `success = true` → first-attempt success, confidence increases.
    /// - `success = false` → generation required fixes, confidence decreases.
    pub fn record_usage(&mut self, id: &str, success: bool) {
        if let Some(bp) = self.blueprints.iter_mut().find(|b| b.id == id) {
            bp.usage_count += 1;
            if success {
                bp.success_count += 1;
            }
        }
    }

    /// Recompute confidence scores from usage history (Bayesian update).
    pub fn update_confidences(&mut self) {
        for bp in &mut self.blueprints {
            if bp.usage_count > 0 {
                // Beta distribution mean: (successes + alpha) / (total + alpha + beta)
                let alpha = 1.0_f64; // prior successes
                let beta = 1.0_f64;  // prior failures
                let failures = bp.usage_count - bp.success_count;
                bp.confidence = (bp.success_count as f64 + alpha)
                    / (bp.usage_count as f64 + alpha + beta);
                let _ = failures;
            }
        }
    }

    /// Persist the registry to a JSON file.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load a registry from a JSON file.
    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read(path)?;
        let reg: Self = serde_json::from_slice(&data)?;
        Ok(reg)
    }

    /// Number of blueprints in the registry.
    pub fn len(&self) -> usize {
        self.blueprints.len()
    }

    /// True if the registry contains no blueprints.
    pub fn is_empty(&self) -> bool {
        self.blueprints.is_empty()
    }

    /// Iterate over all blueprints.
    pub fn iter(&self) -> impl Iterator<Item = &Blueprint> {
        self.blueprints.iter()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_match_crud_service() {
        let reg = BlueprintRegistry::with_builtins();
        let req = GenerationRequest {
            component_type: "crud_service".to_string(),
            language: "rust".to_string(),
            framework: "actix-web".to_string(),
            module_name: "inventory".to_string(),
            entities: vec!["Product".to_string()],
        };
        let bp = reg.find_match(&req);
        assert!(bp.is_some());
        assert_eq!(bp.unwrap().pattern_type, "crud_service");
    }

    #[test]
    fn no_match_for_unknown_type() {
        let reg = BlueprintRegistry::with_builtins();
        let req = GenerationRequest {
            component_type: "quantum_entangler".to_string(),
            language: "rust".to_string(),
            framework: "".to_string(),
            module_name: "test".to_string(),
            entities: vec![],
        };
        assert!(reg.find_match(&req).is_none());
    }

    #[test]
    fn save_and_load() {
        let reg = BlueprintRegistry::with_builtins();
        let dir = std::env::temp_dir().join("isls-blueprint-test");
        let path = dir.join("registry.json");
        reg.save(&path).unwrap();
        let loaded = BlueprintRegistry::load(&path).unwrap();
        assert_eq!(loaded.len(), reg.len());
    }

    #[test]
    fn confidence_update() {
        let mut reg = BlueprintRegistry::with_builtins();
        let id = reg.blueprints[0].id.clone();
        reg.record_usage(&id, true);
        reg.record_usage(&id, true);
        reg.record_usage(&id, false);
        reg.update_confidences();
        let bp = reg.blueprints.iter().find(|b| b.id == id).unwrap();
        assert!(bp.confidence > 0.5 && bp.confidence < 1.0);
    }
}
