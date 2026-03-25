// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Self-defining learning system for ISLS v3.0.
//!
//! Observes cross-layer patterns from synthesis runs and promotes recurring
//! patterns to reusable norms automatically.

use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};

use crate::types::{
    LayerType, Norm, NormEvidence, NormLevel, NormLayers, NormParameter,
    NormVariant, TriggerPattern,
};

// ─── Observed Artifact ────────────────────────────────────────────────────────

/// An artifact observed from a synthesis run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObservedArtifact {
    /// Layer this artifact belongs to.
    pub layer: LayerType,
    /// Artifact type (e.g. `"struct"`, `"endpoint"`, `"migration"`).
    pub artifact_type: String,
    /// Artifact name (e.g. `"Product"`, `"list_products"`).
    pub name: String,
    /// SHA-256 of the artifact content.
    pub signature: String,
    /// Field names (for structs) or parameter names (for functions).
    pub field_names: Vec<String>,
}

// ─── Cross-Layer Pattern ─────────────────────────────────────────────────────

/// A cross-layer pattern observed in a single synthesis run.
///
/// Represents a coherent set of artifacts spanning ≥2 layers that appear
/// together for a specific entity/concept in a given domain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrossLayerPattern {
    /// SHA-256 of the combined artifact signatures.
    pub signature: String,
    /// Layers that are present in this pattern.
    pub layers_present: Vec<LayerType>,
    /// The observed artifacts.
    pub artifacts: Vec<ObservedArtifact>,
    /// Entity/concept this pattern was observed on (lower-snake).
    pub observed_on: String,
    /// Domain name.
    pub domain: String,
    /// Run identifier.
    pub run_id: String,
}

/// Extract cross-layer patterns from a flat list of observed artifacts.
///
/// Groups artifacts by their entity/concept name (inferred from the artifact
/// name), then creates a pattern for each group that spans ≥2 layers.
pub fn extract_cross_layer_patterns(
    artifacts: &[ObservedArtifact],
    domain: &str,
    run_id: &str,
) -> Vec<CrossLayerPattern> {
    // Group artifacts by their entity name (first word of name, lower-snake)
    let mut by_entity: HashMap<String, Vec<ObservedArtifact>> = HashMap::new();
    for a in artifacts {
        let entity = entity_from_name(&a.name);
        by_entity.entry(entity).or_default().push(a.clone());
    }

    let mut patterns = Vec::new();
    for (entity, group) in by_entity {
        let layers: HashSet<_> = group.iter().map(|a| a.layer.clone()).collect();
        if layers.len() < 2 { continue; } // Only patterns spanning ≥2 layers

        let sig = compute_pattern_signature(&group);
        patterns.push(CrossLayerPattern {
            signature: sig,
            layers_present: layers.into_iter().collect(),
            artifacts: group,
            observed_on: entity,
            domain: domain.to_string(),
            run_id: run_id.to_string(),
        });
    }
    patterns
}

fn entity_from_name(name: &str) -> String {
    // e.g. "Product" → "product", "list_products" → "product"
    let lower = name.to_lowercase();
    // Strip common suffixes
    let stripped = lower
        .trim_end_matches("s")
        .trim_start_matches("list_")
        .trim_start_matches("get_")
        .trim_start_matches("create_")
        .trim_start_matches("update_")
        .trim_start_matches("delete_");
    stripped.to_string()
}

fn compute_pattern_signature(artifacts: &[ObservedArtifact]) -> String {
    let mut hasher = Sha256::new();
    let mut sigs: Vec<&str> = artifacts.iter().map(|a| a.signature.as_str()).collect();
    sigs.sort();
    for s in sigs { hasher.update(s.as_bytes()); }
    format!("{:x}", hasher.finalize())[..16].to_string()
}

// ─── Norm Candidate ───────────────────────────────────────────────────────────

/// A candidate norm discovered through self-observation.
///
/// A candidate starts as `Observing`, becomes `Eligible` when it meets the
/// [`PromotionCriteria`], and is `Promoted` after [`synthesize_norm`] runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NormCandidate {
    /// Candidate identifier (e.g. `"ISLS-CAND-0001"`).
    pub id: String,
    /// All cross-layer patterns that contributed to this candidate.
    pub observations: Vec<CrossLayerPattern>,
    /// Distinct domain names where this pattern was observed.
    pub domains: Vec<String>,
    /// Total number of observations.
    pub observation_count: usize,
    /// Average topological similarity across observations (0.0–1.0).
    pub consistency: f64,
    /// Layers consistently present across observations.
    pub consistent_layers: Vec<LayerType>,
    /// Abstracted artifact templates.
    pub common_artifacts: Vec<AbstractedArtifact>,
    /// Current status.
    pub status: CandidateStatus,
}

impl NormCandidate {
    /// Create a new candidate from its first observation.
    pub fn new(id: String, pattern: &CrossLayerPattern) -> Self {
        let mut c = NormCandidate {
            id,
            observations: vec![],
            domains: vec![],
            observation_count: 0,
            consistency: 0.0,
            consistent_layers: pattern.layers_present.clone(),
            common_artifacts: abstract_artifacts(&pattern.artifacts),
            status: CandidateStatus::Observing,
        };
        c.observe(pattern.clone());
        c
    }

    /// Record a new observation of this pattern.
    pub fn observe(&mut self, pattern: CrossLayerPattern) {
        if !self.domains.contains(&pattern.domain) {
            self.domains.push(pattern.domain.clone());
        }
        self.observation_count += 1;
        self.consistency = compute_consistency(&self.observations, &pattern);
        self.consistent_layers = intersect_layers(&self.consistent_layers, &pattern.layers_present);
        self.common_artifacts = merge_abstracted(
            &self.common_artifacts,
            &abstract_artifacts(&pattern.artifacts),
            self.observation_count,
        );
        self.observations.push(pattern);
    }

    /// Check whether this candidate meets all promotion criteria.
    pub fn meets_criteria(&self, criteria: &PromotionCriteria) -> bool {
        self.status == CandidateStatus::Observing
            && self.consistency >= criteria.min_consistency
            && self.domains.len() >= criteria.min_domains
            && self.consistent_layers.len() >= criteria.min_layers
            && self.observation_count >= criteria.min_observations
    }
}

// ─── Abstracted Artifact ─────────────────────────────────────────────────────

/// An abstracted artifact template inferred from multiple observations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbstractedArtifact {
    pub layer: LayerType,
    /// Templatised name (e.g. `"{entity}_status"`).
    pub name_template: String,
    /// Fields consistently present across observations.
    pub common_fields: Vec<AbstractedField>,
    /// Fraction of observations where this artifact was present.
    pub confidence: f64,
}

/// An abstracted field.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbstractedField {
    pub name: String,
    pub observed_types: Vec<String>,
    pub presence_rate: f64,
}

// ─── Candidate Status ─────────────────────────────────────────────────────────

/// Lifecycle status of a norm candidate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateStatus {
    /// Accumulating observations.
    Observing,
    /// Meets all promotion criteria; ready for synthesis.
    Eligible,
    /// Has been synthesised into a norm.
    Promoted,
    /// Manually rejected.
    Rejected,
}

// ─── Promotion Criteria ───────────────────────────────────────────────────────

/// Thresholds that a candidate must meet for automatic promotion.
#[derive(Clone, Debug)]
pub struct PromotionCriteria {
    /// Minimum average consistency (0.0–1.0).
    pub min_consistency: f64,
    /// Minimum number of distinct domains.
    pub min_domains: usize,
    /// Minimum number of consistent layers.
    pub min_layers: usize,
    /// Minimum total observations.
    pub min_observations: usize,
    /// Minimum artifact presence rate.
    pub min_artifact_presence: f64,
}

impl Default for PromotionCriteria {
    fn default() -> Self {
        Self {
            min_consistency: 0.85,
            min_domains: 3,
            min_layers: 4,
            min_observations: 5,
            min_artifact_presence: 0.80,
        }
    }
}

// ─── Norm Synthesis ───────────────────────────────────────────────────────────

/// Synthesise a new norm from a promoted candidate.
///
/// The resulting norm gets an auto-generated ID and is registered with
/// `NormLevel::Molecule`.
pub fn synthesize_norm(candidate: &NormCandidate, auto_id_counter: &mut u32) -> Norm {
    *auto_id_counter += 1;
    let id = format!("ISLS-NORM-AUTO-{:04}", auto_id_counter);
    let name = infer_norm_name(&candidate.common_artifacts);
    let triggers = infer_triggers(&candidate.observations);
    let layers = build_norm_layers(&candidate.common_artifacts);
    let parameters = infer_parameters(&candidate.observations);

    Norm {
        id,
        name,
        level: NormLevel::Molecule,
        triggers,
        layers,
        parameters,
        requires: vec![],
        variants: vec![],
        version: "1.0.0-auto".to_string(),
        evidence: NormEvidence {
            usage_count: candidate.observation_count as u32,
            domains_used: candidate.domains.clone(),
            builtin: false,
            signature: candidate.observations.first()
                .map(|o| o.signature.clone())
                .unwrap_or_default(),
        },
    }
}

fn infer_norm_name(artifacts: &[AbstractedArtifact]) -> String {
    // Use the most common layer as a hint
    let has_status = artifacts.iter().any(|a| {
        a.common_fields.iter().any(|f| f.name == "status")
    });
    let has_inventory = artifacts.iter().any(|a| {
        a.common_fields.iter().any(|f| f.name == "quantity" || f.name == "inventory_count")
    });
    if has_status { "Auto-StateMachine".to_string() }
    else if has_inventory { "Auto-Inventory".to_string() }
    else { "Auto-Pattern".to_string() }
}

fn infer_triggers(observations: &[CrossLayerPattern]) -> Vec<TriggerPattern> {
    let mut keywords: HashSet<String> = HashSet::new();
    for obs in observations {
        // Extract entity name as keyword
        keywords.insert(obs.observed_on.clone());
    }
    vec![TriggerPattern {
        keywords: keywords.into_iter().collect(),
        concepts: vec![],
        min_confidence: 0.3,
        excludes: vec![],
    }]
}

fn build_norm_layers(artifacts: &[AbstractedArtifact]) -> NormLayers {
    // Build minimal NormLayers from abstracted artifacts
    // (just tracking which layers are present; full artifact generation
    //  would require more context)
    let _ = artifacts;
    NormLayers::default()
}

fn infer_parameters(observations: &[CrossLayerPattern]) -> Vec<NormParameter> {
    let _ = observations;
    vec![]
}

// ─── Helper computations ─────────────────────────────────────────────────────

fn compute_consistency(existing: &[CrossLayerPattern], new: &CrossLayerPattern) -> f64 {
    if existing.is_empty() { return 1.0; }
    let existing_layers: HashSet<_> = existing
        .last()
        .map(|o| o.layers_present.iter().cloned().collect::<HashSet<_>>())
        .unwrap_or_default();
    let new_layers: HashSet<_> = new.layers_present.iter().cloned().collect();
    let intersection = existing_layers.intersection(&new_layers).count();
    let union = existing_layers.union(&new_layers).count();
    if union == 0 { 1.0 } else { intersection as f64 / union as f64 }
}

fn intersect_layers(a: &[LayerType], b: &[LayerType]) -> Vec<LayerType> {
    a.iter().filter(|lt| b.contains(lt)).cloned().collect()
}

fn abstract_artifacts(artifacts: &[ObservedArtifact]) -> Vec<AbstractedArtifact> {
    artifacts.iter().map(|a| AbstractedArtifact {
        layer: a.layer.clone(),
        name_template: generalize_name(&a.name),
        common_fields: a.field_names.iter().map(|f| AbstractedField {
            name: f.clone(),
            observed_types: vec![],
            presence_rate: 1.0,
        }).collect(),
        confidence: 1.0,
    }).collect()
}

fn generalize_name(name: &str) -> String {
    // Replace entity-specific words with `{entity}` placeholder
    // (very naive: lowercase first, strip common suffixes)
    name.to_lowercase()
        .replace("product", "{entity}")
        .replace("order", "{entity}")
        .replace("task", "{entity}")
}

fn merge_abstracted(
    existing: &[AbstractedArtifact],
    new: &[AbstractedArtifact],
    total_count: usize,
) -> Vec<AbstractedArtifact> {
    let mut merged = existing.to_vec();
    for n in new {
        if let Some(e) = merged.iter_mut().find(|e| e.layer == n.layer) {
            // Update confidence
            e.confidence = (e.confidence * (total_count - 1) as f64 + n.confidence)
                / total_count as f64;
        } else {
            let mut a = n.clone();
            a.confidence /= total_count as f64;
            merged.push(a);
        }
    }
    merged
}
