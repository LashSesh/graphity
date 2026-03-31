// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! D4 Norm-guided generation enrichment for ISLS.
//!
//! Matches a user's description against the [`NormRegistry`] (including
//! auto-discovered norms), composes matched norms into a [`ComposedPlan`],
//! and merges norm-derived fields into the LLM-extracted entity JSON.
//!
//! **Constraints:**
//! - Pure function: no IO except loading the NormRegistry.
//! - Additive only: norms never remove or override LLM-produced fields.
//! - Fallback to D3: if matching or composition fails, entities pass through unchanged.

use std::collections::HashMap;

use isls_norms::{NormRegistry, ComposedPlan};
use isls_norms::composition::compose_norms_with_registry;

/// Enrich LLM-extracted entities with norm-derived fields.
///
/// Loads the [`NormRegistry`], matches the description, composes a plan,
/// and additively merges suggested fields into `entities_json`.
///
/// If no norms match or composition fails, `entities_json` is unchanged
/// (D3 fallback).
pub fn enrich_with_norms(message: &str, entities_json: &mut serde_json::Value) {
    let registry = NormRegistry::new();

    // Match description against norm catalog
    let activated = registry.match_description(message);
    if activated.is_empty() {
        tracing::debug!("D4 norm enrichment: no norms matched");
        return;
    }

    // Filter to norms with confidence >= 0.5
    let high_conf: Vec<_> = activated
        .iter()
        .filter(|a| a.confidence >= 0.5)
        .cloned()
        .collect();

    if high_conf.is_empty() {
        tracing::debug!("D4 norm enrichment: no norms with confidence >= 0.5");
        return;
    }

    let norm_ids: Vec<&str> = high_conf.iter().map(|a| a.norm.id.as_str()).collect();
    tracing::info!("D4 norm enrichment: {}", norm_ids.join(", "));

    // Compose norms
    let params = HashMap::new();
    let plan = match compose_norms_with_registry(&high_conf, &params, &registry) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("D4 norm enrichment: composition failed ({}), proceeding as D3", e);
            return;
        }
    };

    // Merge norm-suggested fields into entities (additive only)
    merge_norm_fields(entities_json, &plan);
}

/// Build norm hint text to append to the LLM extraction prompt.
///
/// Generated dynamically from the `ComposedPlan.models` — not hardcoded.
pub fn build_norm_hints(composed: &ComposedPlan) -> String {
    if composed.models.is_empty() {
        return String::new();
    }

    let mut hints = String::new();
    hints.push_str("\n## Norm Hints (optional structural guidance)\n");
    hints.push_str("The following patterns have been observed across similar applications.\n");
    hints.push_str("Consider including these fields if they fit the domain:\n\n");

    for model in &composed.models {
        if model.fields.is_empty() {
            continue;
        }
        let field_list: Vec<String> = model
            .fields
            .iter()
            .map(|f| format!("{} ({})", f.name, f.rust_type))
            .collect();
        hints.push_str(&format!(
            "- Entities matching '{}' commonly include:\n  {}\n",
            model.struct_name,
            field_list.join(", ")
        ));
    }

    hints.push_str("\nThese are suggestions, not requirements. Extract entities that fit\n");
    hints.push_str("the user's description first, then add norm-suggested fields only\n");
    hints.push_str("if they are relevant.\n");

    hints
}

/// Additively merge norm-derived fields into entity JSON.
///
/// For each model in the composed plan, find matching entities in the JSON
/// and add any fields that the LLM didn't produce. Never removes or
/// overrides existing fields.
fn merge_norm_fields(entities_json: &mut serde_json::Value, plan: &ComposedPlan) {
    let entities = match entities_json["entities"].as_array_mut() {
        Some(arr) => arr,
        None => return,
    };

    // Collect suggested fields from all model artifacts in the plan
    // (These are template fields — they apply to any entity)
    let mut suggested_fields: Vec<(&str, &str)> = Vec::new();
    for model in &plan.models {
        for field in &model.fields {
            suggested_fields.push((&field.name, &field.rust_type));
        }
    }

    if suggested_fields.is_empty() {
        return;
    }

    for entity in entities.iter_mut() {
        let fields = match entity["fields"].as_array_mut() {
            Some(arr) => arr,
            None => continue,
        };

        // Get existing field names
        let existing_names: Vec<String> = fields
            .iter()
            .filter_map(|f| f["name"].as_str().map(|s| s.to_string()))
            .collect();

        // Add norm-suggested fields that don't already exist (additive only)
        for (name, rust_type) in &suggested_fields {
            if !existing_names.contains(&name.to_string()) {
                // Map Rust type to the field_type format used by the extraction schema
                let field_type = rust_type_to_field_type(rust_type);
                fields.push(serde_json::json!({
                    "name": name,
                    "field_type": field_type,
                    "nullable": true,
                    "unique": false,
                }));
            }
        }
    }
}

/// Map Rust type strings to the field_type format used by the extraction schema.
fn rust_type_to_field_type(rust_type: &str) -> &str {
    match rust_type {
        "i32" => "i32",
        "i64" => "i64",
        "f64" => "f64",
        "bool" => "bool",
        "String" => "String",
        _ => "String",
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entities() -> serde_json::Value {
        serde_json::json!({
            "app_name": "hotel-manager",
            "description": "Hotel management system",
            "entities": [
                {
                    "name": "Room",
                    "fields": [
                        { "name": "room_number", "field_type": "String", "nullable": false, "unique": true },
                        { "name": "floor", "field_type": "i32", "nullable": false, "unique": false }
                    ],
                    "foreign_keys": []
                },
                {
                    "name": "User",
                    "fields": [
                        { "name": "email", "field_type": "String", "nullable": false, "unique": true },
                        { "name": "password_hash", "field_type": "String", "nullable": false, "unique": false }
                    ],
                    "foreign_keys": []
                }
            ]
        })
    }

    #[test]
    fn test_norm_enrichment_additive() {
        let mut json = sample_entities();

        // Call enrichment — with builtin norms, "hotel management" should match
        enrich_with_norms("Hotel management with rooms and bookings", &mut json);

        // Verify original fields are preserved (never removed)
        let room = &json["entities"][0];
        let fields = room["fields"].as_array().unwrap();
        assert!(
            fields.iter().any(|f| f["name"] == "room_number"),
            "original field 'room_number' must be preserved"
        );
        assert!(
            fields.iter().any(|f| f["name"] == "floor"),
            "original field 'floor' must be preserved"
        );

        // Verify User entity is also preserved
        let user = &json["entities"][1];
        let user_fields = user["fields"].as_array().unwrap();
        assert!(
            user_fields.iter().any(|f| f["name"] == "email"),
            "User email must be preserved"
        );
    }

    #[test]
    fn test_norm_enrichment_fallback() {
        let mut json = sample_entities();
        let original = json.clone();

        // A very obscure description that shouldn't match any norms strongly
        enrich_with_norms("xyzzy plugh nothing", &mut json);

        // Entities should pass through unchanged
        assert_eq!(
            json["entities"].as_array().unwrap().len(),
            original["entities"].as_array().unwrap().len(),
            "entity count should be unchanged on fallback"
        );
    }

    #[test]
    fn test_build_norm_hints_empty() {
        let plan = ComposedPlan::default();
        assert!(build_norm_hints(&plan).is_empty());
    }

    #[test]
    fn test_build_norm_hints_with_models() {
        use isls_norms::types::{ModelArtifact, FieldSpec, FieldSource};

        let plan = ComposedPlan {
            models: vec![ModelArtifact {
                struct_name: "{Entity}".into(),
                fields: vec![
                    FieldSpec {
                        name: "status".into(),
                        rust_type: "String".into(),
                        sql_type: "TEXT".into(),
                        nullable: false,
                        default_value: None,
                        indexed: false,
                        unique: false,
                        source: FieldSource::UserInput,
                        description: String::new(),
                    },
                ],
                derives: vec![],
                validations: vec![],
            }],
            ..Default::default()
        };

        let hints = build_norm_hints(&plan);
        assert!(hints.contains("status"), "hints should mention suggested fields");
        assert!(hints.contains("Norm Hints"), "hints should have header");
    }
}
