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
/// Only fields from the `{Entity}` template model are applied to all entities.
/// Fields from concrete-name models (e.g. `PaginationParams`, `AppError`,
/// `Product`) are only merged into entities whose name matches exactly.
/// Infrastructure models with generic parameters (e.g. `PaginatedResponse<T>`)
/// are always skipped. Never removes or overrides existing fields.
fn merge_norm_fields(entities_json: &mut serde_json::Value, plan: &ComposedPlan) {
    let entities = match entities_json["entities"].as_array_mut() {
        Some(arr) => arr,
        None => return,
    };

    // Split models into: template (apply to all) vs. entity-specific (apply by name)
    // Skip anything that looks like an infrastructure model or generic type.
    let mut template_fields: Vec<(&str, &str)> = Vec::new();
    let mut entity_specific: std::collections::HashMap<String, Vec<(&str, &str)>> =
        std::collections::HashMap::new();

    for model in &plan.models {
        let name = model.struct_name.as_str();

        // Skip generic/infrastructure models — these must never be flattened
        // into domain entities.
        if name.contains('<') || is_infrastructure_model(name) {
            continue;
        }

        if name == "{Entity}" {
            for field in &model.fields {
                template_fields.push((&field.name, &field.rust_type));
            }
        } else {
            // Concrete name: merge only into an entity with the same name
            let entry = entity_specific.entry(name.to_string()).or_default();
            for field in &model.fields {
                entry.push((&field.name, &field.rust_type));
            }
        }
    }

    if template_fields.is_empty() && entity_specific.is_empty() {
        return;
    }

    for entity in entities.iter_mut() {
        let entity_name = entity["name"].as_str().unwrap_or("").to_string();
        let fields = match entity["fields"].as_array_mut() {
            Some(arr) => arr,
            None => continue,
        };

        // Collect existing field names (unique)
        let mut existing: std::collections::HashSet<String> = fields
            .iter()
            .filter_map(|f| f["name"].as_str().map(|s| s.to_string()))
            .collect();

        // Apply template fields to every entity
        for (name, rust_type) in &template_fields {
            if existing.insert(name.to_string()) {
                let field_type = rust_type_to_field_type(rust_type);
                fields.push(serde_json::json!({
                    "name": name,
                    "field_type": field_type,
                    "nullable": true,
                    "unique": false,
                }));
            }
        }

        // Apply entity-specific fields only when names match
        if let Some(specific) = entity_specific.get(&entity_name) {
            for (name, rust_type) in specific {
                if existing.insert(name.to_string()) {
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
}

/// Names of infrastructure models that must never be merged into entities.
/// These are cross-cutting concerns (pagination, errors, auth payloads, etc.)
/// that have their own structural files — not entity fields.
fn is_infrastructure_model(name: &str) -> bool {
    matches!(
        name,
        "PaginationParams"
            | "PaginatedResponse"
            | "AppError"
            | "ErrorResponse"
            | "LoginRequest"
            | "LoginResponse"
            | "RegisterRequest"
            | "TokenResponse"
            | "JwtClaims"
            | "Claims"
            | "HealthCheck"
            | "HealthStatus"
            | "ApiResponse"
    )
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
    fn test_infrastructure_fields_never_leak_into_entities() {
        // Regression test for the field pollution bug: pagination/error fields
        // must not be added to domain entities.
        let mut json = serde_json::json!({
            "app_name": "warehouse-inventory",
            "description": "Warehouse inventory",
            "entities": [
                {
                    "name": "Product",
                    "fields": [
                        { "name": "name", "field_type": "String", "nullable": false, "unique": false },
                        { "name": "unit_price", "field_type": "i64", "nullable": false, "unique": false }
                    ],
                    "foreign_keys": []
                }
            ]
        });

        enrich_with_norms(
            "Warehouse inventory with products, locations, stock movements, and pagination",
            &mut json,
        );

        let product = &json["entities"][0];
        let fields = product["fields"].as_array().unwrap();
        let field_names: Vec<&str> = fields.iter().filter_map(|f| f["name"].as_str()).collect();

        // Original fields preserved
        assert!(field_names.contains(&"name"));
        assert!(field_names.contains(&"unit_price"));

        // Infrastructure fields must NOT be present
        for forbidden in &[
            "page", "per_page", "sort", "sort_desc", "search",
            "items", "total", "total_pages",
            "NotFound", "ValidationError", "Unauthorized",
            "Forbidden", "Conflict", "InternalError",
        ] {
            assert!(
                !field_names.contains(forbidden),
                "forbidden infrastructure field '{}' leaked into Product entity",
                forbidden,
            );
        }

        // No duplicate fields
        let mut seen = std::collections::HashSet::new();
        for name in &field_names {
            assert!(seen.insert(*name), "duplicate field: {}", name);
        }
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
