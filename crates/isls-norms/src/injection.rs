// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! I3/W2 — Norm Injection.
//!
//! Load a JSON blueprint, validate it and register it in the
//! [`NormRegistry`] under the reserved ID prefix `ISLS-NORM-INJECT-`.
//! Injected norms participate in the same fitness tracking as auto-
//! discovered norms — bad injections die, good ones thrive.

use serde::{Deserialize, Serialize};

use crate::types::{Norm, NormEvidence, NormLayers, NormLevel, TriggerPattern};
use crate::NormRegistry;

/// Reserved ID prefix for manually injected norms.
pub const INJECT_PREFIX: &str = "ISLS-NORM-INJECT-";

// ─── Blueprint ──────────────────────────────────────────────────────────────

/// A norm described by a JSON blueprint file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormBlueprint {
    pub id: String,
    pub name: String,
    /// Abstraction level — `"Atom" | "Molecule" | "Organism" | "Ecosystem"`.
    pub level: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default)]
    pub expected_resonites: Vec<ResonitePattern>,
    #[serde(default)]
    pub activation_keywords: Vec<String>,
    #[serde(default)]
    pub constraints: BlueprintConstraints,
}

/// A pattern description inside a blueprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonitePattern {
    /// `"Fn"`, `"Type"`, or `"Import"`.
    pub kind: String,
    /// Name pattern (may contain glob wildcards like `emit_*`).
    pub pattern: String,
    #[serde(default)]
    pub arity: Option<usize>,
    #[serde(default)]
    pub type_kind: Option<String>,
}

/// Blueprint constraints — interpreted by the fitness system.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlueprintConstraints {
    #[serde(default)]
    pub min_layers: Option<usize>,
    #[serde(default)]
    pub requires: Vec<String>,
}

// ─── Errors ─────────────────────────────────────────────────────────────────

/// Result of attempting to inject a blueprint.
#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("blueprint ID must start with '{INJECT_PREFIX}', got '{0}'")]
    BadIdPrefix(String),
    #[error("blueprint field '{0}' is empty or missing")]
    MissingField(&'static str),
    #[error("unknown level '{0}' (expected Atom, Molecule, Organism or Ecosystem)")]
    UnknownLevel(String),
    #[error("a norm with id '{0}' already exists")]
    Duplicate(String),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Parse a [`NormBlueprint`] from a JSON string.
pub fn parse_blueprint(json: &str) -> Result<NormBlueprint, InjectError> {
    let bp: NormBlueprint = serde_json::from_str(json)?;
    Ok(bp)
}

/// Load a blueprint from a file on disk.
pub fn load_blueprint(path: &std::path::Path) -> Result<NormBlueprint, InjectError> {
    let content = std::fs::read_to_string(path)?;
    parse_blueprint(&content)
}

/// Validate + convert + insert an injected norm.
///
/// Returns the ID of the freshly registered norm on success.
pub fn inject_norm(
    registry: &mut NormRegistry,
    blueprint: NormBlueprint,
) -> Result<String, InjectError> {
    // 1. ID prefix.
    if !blueprint.id.starts_with(INJECT_PREFIX) {
        return Err(InjectError::BadIdPrefix(blueprint.id));
    }
    // 2. Required fields.
    if blueprint.name.trim().is_empty() {
        return Err(InjectError::MissingField("name"));
    }
    if blueprint.level.trim().is_empty() {
        return Err(InjectError::MissingField("level"));
    }
    // 3. Level.
    let level = parse_level(&blueprint.level)?;

    // 4. Duplicate check.
    if registry.get(&blueprint.id).is_some() {
        return Err(InjectError::Duplicate(blueprint.id));
    }

    // 5. Build an internal Norm from the blueprint.
    let id = blueprint.id.clone();
    let keywords_lower: Vec<String> = blueprint
        .activation_keywords
        .iter()
        .map(|k| k.to_lowercase())
        .collect();
    let norm = Norm {
        id: id.clone(),
        name: blueprint.name,
        level,
        triggers: vec![TriggerPattern {
            keywords: keywords_lower,
            concepts: blueprint
                .layers
                .iter()
                .map(|l| l.to_lowercase())
                .collect(),
            min_confidence: 0.3,
            excludes: vec![],
        }],
        layers: NormLayers::default(),
        parameters: vec![],
        requires: blueprint.constraints.requires.clone(),
        variants: vec![],
        version: "1.0.0".to_string(),
        evidence: NormEvidence {
            usage_count: 0,
            domains_used: vec![],
            builtin: false,
            signature: compute_blueprint_signature(&id, &blueprint.description),
        },
    };

    // 6. Store it.
    registry.register(norm);
    Ok(id)
}

/// Remove an injected norm. Returns `true` if a matching norm was removed.
pub fn remove_injected(registry: &mut NormRegistry, id: &str) -> bool {
    if !id.starts_with(INJECT_PREFIX) {
        return false;
    }
    registry.remove(id).is_some()
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn parse_level(s: &str) -> Result<NormLevel, InjectError> {
    match s.trim().to_lowercase().as_str() {
        "atom" => Ok(NormLevel::Atom),
        "molecule" => Ok(NormLevel::Molecule),
        "organism" => Ok(NormLevel::Organism),
        "ecosystem" => Ok(NormLevel::Ecosystem),
        _ => Err(InjectError::UnknownLevel(s.to_string())),
    }
}

fn compute_blueprint_signature(id: &str, description: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(id.as_bytes());
    h.update(b"\n");
    h.update(description.as_bytes());
    let sig = h.finalize();
    format!("{:x}", sig)[..16].to_string()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_JSON: &str = r#"{
        "id": "ISLS-NORM-INJECT-001",
        "name": "EventDrivenArchitecture",
        "level": "System",
        "description": "Event bus with handlers, dispatcher, store",
        "layers": ["EventBus", "EventHandler"],
        "activation_keywords": ["event", "publish", "subscribe"]
    }"#;

    #[test]
    fn parse_blueprint_ok() {
        let bp = parse_blueprint(VALID_JSON).unwrap();
        assert_eq!(bp.id, "ISLS-NORM-INJECT-001");
        assert_eq!(bp.activation_keywords.len(), 3);
    }

    #[test]
    fn inject_and_retrieve() {
        let mut reg = NormRegistry::empty_without_persistence();
        let mut bp = parse_blueprint(VALID_JSON).unwrap();
        // The README example uses "System" — remap to Organism for tests.
        bp.level = "Organism".into();
        let id = inject_norm(&mut reg, bp).expect("inject should succeed");
        assert_eq!(id, "ISLS-NORM-INJECT-001");
        assert!(reg.get(&id).is_some());
    }

    #[test]
    fn reject_bad_prefix() {
        let mut reg = NormRegistry::empty_without_persistence();
        let bp = NormBlueprint {
            id: "ISLS-NORM-AUTO-0001".into(),
            name: "X".into(),
            level: "Molecule".into(),
            description: String::new(),
            layers: vec![],
            expected_resonites: vec![],
            activation_keywords: vec![],
            constraints: Default::default(),
        };
        assert!(matches!(
            inject_norm(&mut reg, bp),
            Err(InjectError::BadIdPrefix(_))
        ));
    }

    #[test]
    fn reject_unknown_level() {
        let mut reg = NormRegistry::empty_without_persistence();
        let bp = NormBlueprint {
            id: "ISLS-NORM-INJECT-777".into(),
            name: "X".into(),
            level: "Supernova".into(),
            description: String::new(),
            layers: vec![],
            expected_resonites: vec![],
            activation_keywords: vec![],
            constraints: Default::default(),
        };
        assert!(matches!(
            inject_norm(&mut reg, bp),
            Err(InjectError::UnknownLevel(_))
        ));
    }

    #[test]
    fn reject_duplicate() {
        let mut reg = NormRegistry::empty_without_persistence();
        let bp = NormBlueprint {
            id: "ISLS-NORM-INJECT-010".into(),
            name: "X".into(),
            level: "Molecule".into(),
            description: String::new(),
            layers: vec![],
            expected_resonites: vec![],
            activation_keywords: vec!["event".into()],
            constraints: Default::default(),
        };
        inject_norm(&mut reg, bp.clone()).unwrap();
        assert!(matches!(
            inject_norm(&mut reg, bp),
            Err(InjectError::Duplicate(_))
        ));
    }

    #[test]
    fn remove_injected_ok() {
        let mut reg = NormRegistry::empty_without_persistence();
        let bp = NormBlueprint {
            id: "ISLS-NORM-INJECT-020".into(),
            name: "Y".into(),
            level: "Molecule".into(),
            description: String::new(),
            layers: vec![],
            expected_resonites: vec![],
            activation_keywords: vec!["kw".into()],
            constraints: Default::default(),
        };
        inject_norm(&mut reg, bp).unwrap();
        assert!(remove_injected(&mut reg, "ISLS-NORM-INJECT-020"));
        assert!(reg.get("ISLS-NORM-INJECT-020").is_none());
        assert!(!remove_injected(&mut reg, "ISLS-NORM-0042"));
    }
}
