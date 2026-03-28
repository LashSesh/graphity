// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Chat-driven intent recognition and norm operation mapping for ISLS v3.0.
//!
//! Translates natural language user messages into typed [`ChatIntent`] values
//! and maps them to [`NormOperation`]s that drive incremental code generation.
//!
//! Two extraction modes are provided:
//! - [`extract_intent`]: uses an LLM oracle for precise intent recognition.
//! - [`extract_intent_keywords`]: keyword-based fallback (works with `--mock-oracle`).
//!
//! # Example
//!
//! ```rust
//! use isls_chat::{extract_intent_keywords, intent_to_norm_ops};
//! use isls_norms::NormRegistry;
//!
//! let intent = extract_intent_keywords("I need a warehouse inventory app");
//! let registry = NormRegistry::default();
//! let ops = intent_to_norm_ops(&intent, &registry);
//! assert!(!ops.is_empty());
//! ```

use serde::{Deserialize, Serialize};
use thiserror::Error;

use isls_norms::{ActivatedNorm, FieldSpec, FieldSource, NormRegistry};
use isls_forge_llm::oracle::Oracle;

// ─── Error ────────────────────────────────────────────────────────────────────

/// Errors produced by the chat module.
#[derive(Debug, Error)]
pub enum ChatError {
    /// Oracle call failed.
    #[error("oracle error: {0}")]
    Oracle(String),
    /// JSON parsing of oracle response failed.
    #[error("parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ChatError>;

// ─── Intent types ─────────────────────────────────────────────────────────────

/// Recognised intent from a user message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatIntent {
    /// Primary intent type.
    pub intent_type: IntentType,
    /// Entity names mentioned (PascalCase).
    pub entities: Vec<String>,
    /// Field additions described by the user.
    pub fields: Vec<FieldIntent>,
    /// Feature names mentioned (e.g. `"barcode scanning"`).
    pub features: Vec<String>,
    /// Business rules described by the user.
    pub rules: Vec<RuleIntent>,
    /// Roles mentioned (e.g. `"admin"`, `"manager"`).
    pub roles: Vec<String>,
    /// Verbatim user message.
    pub raw_text: String,
}

/// Primary intent of a user message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentType {
    /// User wants to generate a new application from scratch.
    CreateApplication,
    /// User wants to add a field to an entity.
    AddField,
    /// User wants to remove a feature or entity.
    RemoveFeature,
    /// User wants to modify an existing entity.
    ModifyEntity,
    /// User wants to add a business rule.
    AddBusinessRule,
    /// User wants to view / inspect the application.
    ViewApplication,
    /// User wants to deploy the application.
    Deploy,
    /// User is asking for help.
    Help,
    /// Intent is unclear; ISLS should ask for clarification.
    Clarify,
}

/// A field addition described by the user.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldIntent {
    /// Entity the field belongs to.
    pub entity: String,
    /// Field name (snake_case).
    pub field_name: String,
    /// Rust type inferred from context (default: `"Option<String>"`).
    pub rust_type: String,
}

/// A business rule described by the user.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuleIntent {
    pub trigger: String,
    pub condition: String,
    pub action: String,
}

// ─── Norm operations ──────────────────────────────────────────────────────────

/// An operation on the norm set, produced by mapping a [`ChatIntent`].
#[derive(Clone, Debug)]
pub enum NormOperation {
    /// Compose a new application from the given activated norms.
    ComposeNew(Vec<ActivatedNorm>),
    /// Add a field to an existing entity.
    AmendEntity { entity: String, add_field: FieldSpec },
    /// Add a norm by ID.
    AddNorm(String),
    /// Remove a norm by ID.
    RemoveNorm(String),
    /// Add a business rule.
    AddRule { trigger: String, condition: String, action: String },
}

// ─── LLM-based extraction ─────────────────────────────────────────────────────

/// Extract a [`ChatIntent`] from a user message using an LLM oracle.
///
/// The oracle is asked to return a JSON object with intent, entities, fields,
/// features, rules, and roles.  Falls back to [`extract_intent_keywords`] if
/// the oracle call or JSON parsing fails.
pub fn extract_intent(oracle: &dyn Oracle, message: &str) -> Result<ChatIntent> {
    let prompt = format!(
        r#"Extract software requirements from this user message and return ONLY valid JSON.

User message: "{}"

Return JSON with these fields:
{{
  "intent": "CreateApplication|AddField|RemoveFeature|ModifyEntity|AddBusinessRule|ViewApplication|Deploy|Help|Clarify",
  "entities": ["PascalCaseEntityName"],
  "fields": [{{"entity": "EntityName", "field_name": "snake_name", "rust_type": "Rust type"}}],
  "features": ["feature description"],
  "rules": [{{"trigger": "event", "condition": "bool expr", "action": "description"}}],
  "roles": ["role_name"]
}}

Respond with JSON only, no markdown."#,
        message
    );

    let response = oracle.call(&prompt, 512)
        .map_err(|e| ChatError::Oracle(e.to_string()))?;

    parse_intent_json(&response, message)
        .unwrap_or_else(|_| Ok(extract_intent_keywords(message)))
}

fn parse_intent_json(response: &str, raw_text: &str) -> std::result::Result<Result<ChatIntent>, serde_json::Error> {
    let trimmed = response.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let v: serde_json::Value = serde_json::from_str(trimmed)?;

    let intent_str = v["intent"].as_str().unwrap_or("Clarify");
    let intent_type = match intent_str {
        "CreateApplication" => IntentType::CreateApplication,
        "AddField"          => IntentType::AddField,
        "RemoveFeature"     => IntentType::RemoveFeature,
        "ModifyEntity"      => IntentType::ModifyEntity,
        "AddBusinessRule"   => IntentType::AddBusinessRule,
        "ViewApplication"   => IntentType::ViewApplication,
        "Deploy"            => IntentType::Deploy,
        "Help"              => IntentType::Help,
        _                   => IntentType::Clarify,
    };

    let entities = v["entities"].as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let fields = v["fields"].as_array()
        .map(|a| a.iter().filter_map(|x| {
            Some(FieldIntent {
                entity:     x["entity"].as_str()?.to_string(),
                field_name: x["field_name"].as_str()?.to_string(),
                rust_type:  x["rust_type"].as_str().unwrap_or("Option<String>").to_string(),
            })
        }).collect())
        .unwrap_or_default();

    let features = v["features"].as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let rules = v["rules"].as_array()
        .map(|a| a.iter().filter_map(|x| {
            Some(RuleIntent {
                trigger:   x["trigger"].as_str()?.to_string(),
                condition: x["condition"].as_str()?.to_string(),
                action:    x["action"].as_str()?.to_string(),
            })
        }).collect())
        .unwrap_or_default();

    let roles = v["roles"].as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    Ok(Ok(ChatIntent { intent_type, entities, fields, features, rules, roles, raw_text: raw_text.to_string() }))
}

// ─── Keyword-based extraction (fallback / --mock-oracle) ────────────────────

/// Extract a [`ChatIntent`] using keyword matching — no LLM call required.
///
/// Used when `--mock-oracle` is active or as a fallback when LLM extraction
/// fails.  Accuracy is lower than [`extract_intent`] but sufficient for
/// common commands.
pub fn extract_intent_keywords(message: &str) -> ChatIntent {
    let lower = message.to_lowercase();
    let mut intent_type = IntentType::Clarify;
    let mut entities: Vec<String> = Vec::new();
    let mut fields: Vec<FieldIntent> = Vec::new();
    let mut features: Vec<String> = Vec::new();
    let mut rules: Vec<RuleIntent> = Vec::new();

    // ── Classify intent ──────────────────────────────────────────────────────
    if lower.contains("deploy") || lower.contains("ship") || lower.contains("publish") {
        intent_type = IntentType::Deploy;
    } else if lower.contains("show me") || lower.contains("view") || lower.contains("display") || lower.contains("list") {
        intent_type = IntentType::ViewApplication;
    } else if lower.contains("help") || lower.contains("what can") || lower.contains("how do") {
        intent_type = IntentType::Help;
    } else if lower.contains("remove") || lower.contains("delete") || lower.contains("disable") {
        intent_type = IntentType::RemoveFeature;
    } else if lower.contains("add") && (lower.contains("field") || lower.contains("column") || lower.contains("attribute")) {
        intent_type = IntentType::AddField;
    } else if lower.contains("add") && lower.contains(" to ") && !lower.contains("rule") {
        // "add <field> to <entity>" pattern without explicit "field" keyword
        intent_type = IntentType::AddField;
    } else if lower.contains("add") && (lower.contains("rule") || lower.contains("validate") || lower.contains("warn") || lower.contains("alert")) {
        intent_type = IntentType::AddBusinessRule;
    } else if lower.contains("change") || lower.contains("modify") || lower.contains("update") || lower.contains("rename") {
        intent_type = IntentType::ModifyEntity;
    } else if lower.contains("i need") || lower.contains("build") || lower.contains("create") || lower.contains("make")
        || lower.contains("app") || lower.contains("system") || lower.contains("platform")
        || lower.contains("management") || lower.contains("manager") || lower.contains("tracker")
        || lower.contains("inventory") || lower.contains("warehouse") || lower.contains("shop")
        || lower.contains("store") || lower.contains("portal") || lower.contains("service") {
        intent_type = IntentType::CreateApplication;
    }

    // ── Extract entity names ─────────────────────────────────────────────────
    // Common entity keywords
    let entity_keywords = [
        "product", "order", "inventory", "warehouse", "user", "customer",
        "cart", "category", "task", "project", "sprint", "team", "part",
        "supplier", "shipment", "address", "review", "comment", "label",
    ];
    for kw in &entity_keywords {
        if lower.contains(kw) {
            let entity = to_pascal_case(kw);
            if !entities.contains(&entity) {
                entities.push(entity.clone());
            }
        }
    }

    // ── Extract AddField operations ──────────────────────────────────────────
    // Pattern: "add <field> to <entity>" or "add <field> field to <entity>"
    if intent_type == IntentType::AddField {
        // Very simple heuristic: look for "add X to Y" patterns
        let words: Vec<&str> = lower.split_whitespace().collect();
        if let Some(add_idx) = words.iter().position(|w| *w == "add") {
            if let Some(to_idx) = words[add_idx..].iter().position(|w| *w == "to") {
                let to_idx = add_idx + to_idx;
                let field_words = &words[add_idx + 1..to_idx];
                let entity_words = &words[to_idx + 1..];
                if !field_words.is_empty() && !entity_words.is_empty() {
                    let field_name = field_words
                        .iter()
                        .filter(|w| **w != "field" && **w != "column" && **w != "a" && **w != "an" && **w != "the")
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("_");
                    let entity = to_pascal_case(entity_words[0].trim_end_matches('s'));
                    if !field_name.is_empty() && !entity.is_empty() {
                        fields.push(FieldIntent {
                            entity: entity.clone(),
                            field_name,
                            rust_type: "Option<String>".to_string(),
                        });
                        if !entities.contains(&entity) {
                            entities.push(entity);
                        }
                    }
                }
            }
        }
    }

    // ── Extract features ─────────────────────────────────────────────────────
    let feature_keywords = [
        ("barcode", "barcode scanning"),
        ("scan", "barcode scanning"),
        ("qr", "qr code scanning"),
        ("export", "csv/json export"),
        ("chart", "analytics charts"),
        ("dashboard", "kpi dashboard"),
        ("notification", "push notifications"),
        ("search", "full-text search"),
        ("pagination", "pagination"),
        ("auth", "jwt authentication"),
        ("jwt", "jwt authentication"),
    ];
    for (kw, feat) in &feature_keywords {
        if lower.contains(kw) {
            let feat = feat.to_string();
            if !features.contains(&feat) {
                features.push(feat);
            }
        }
    }

    // ── Extract business rules ────────────────────────────────────────────────
    if lower.contains("warn when") || lower.contains("alert when") || lower.contains("notify when") {
        rules.push(RuleIntent {
            trigger: "on_condition".to_string(),
            condition: "threshold_exceeded".to_string(),
            action: "send_alert".to_string(),
        });
    }
    if lower.contains("low stock") || lower.contains("reorder") {
        rules.push(RuleIntent {
            trigger: "on_stock_change".to_string(),
            condition: "quantity < reorder_level".to_string(),
            action: "warn_reorder_needed".to_string(),
        });
    }

    ChatIntent {
        intent_type,
        entities,
        fields,
        features,
        rules,
        roles: Vec::new(),
        raw_text: message.to_string(),
    }
}

// ─── Intent → NormOperation mapping ─────────────────────────────────────────

/// Map a [`ChatIntent`] to a list of [`NormOperation`]s.
pub fn intent_to_norm_ops(
    intent: &ChatIntent,
    registry: &NormRegistry,
) -> Vec<NormOperation> {
    match intent.intent_type {
        IntentType::CreateApplication => {
            let activated = registry.match_description(&intent.raw_text);
            if activated.is_empty() {
                vec![]
            } else {
                vec![NormOperation::ComposeNew(activated)]
            }
        }
        IntentType::AddField => {
            intent.fields.iter().map(|fi| {
                NormOperation::AmendEntity {
                    entity: fi.entity.clone(),
                    add_field: FieldSpec {
                        name: fi.field_name.clone(),
                        rust_type: fi.rust_type.clone(),
                        sql_type: infer_sql_type(&fi.rust_type),
                        nullable: fi.rust_type.starts_with("Option<"),
                        default_value: None,
                        indexed: false,
                        unique: false,
                        source: FieldSource::UserInput,
                        description: format!("User-added field: {}", fi.field_name),
                    },
                }
            }).collect()
        }
        IntentType::AddBusinessRule => {
            intent.rules.iter().map(|r| NormOperation::AddRule {
                trigger: r.trigger.clone(),
                condition: r.condition.clone(),
                action: r.action.clone(),
            }).collect()
        }
        _ => vec![],
    }
}

// ─── Incremental regeneration ────────────────────────────────────────────────

/// Return the list of relative file paths that must be regenerated for a
/// given [`NormOperation`].
///
/// Used by incremental regen: only affected files are re-emitted, keeping
/// compilation fast.
pub fn affected_files(op: &NormOperation) -> Vec<String> {
    match op {
        NormOperation::AmendEntity { entity, .. } => {
            let e = entity.to_lowercase();
            vec![
                format!("backend/src/models/{}.rs", e),
                format!("backend/src/database/{}_queries.rs", e),
                format!("backend/src/services/{}.rs", e),
                format!("backend/src/api/{}.rs", e),
                "backend/database/migrations/001_initial.sql".to_string(),
                format!("frontend/src/pages/{}.js", e),
            ]
        }
        NormOperation::ComposeNew(_) => vec![],   // full generation
        NormOperation::AddNorm(_) | NormOperation::RemoveNorm(_) => vec![],
        NormOperation::AddRule { .. } => vec!["backend/src/services/".to_string()],
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn to_pascal_case(s: &str) -> String {
    s.split('_').filter(|p| !p.is_empty()).map(|p| {
        let mut c = p.chars();
        match c.next() {
            Some(first) => first.to_uppercase().to_string() + c.as_str(),
            None => String::new(),
        }
    }).collect()
}

fn infer_sql_type(rust_type: &str) -> String {
    let inner = rust_type.trim_start_matches("Option<").trim_end_matches('>');
    match inner {
        "i64" | "i32"           => "BIGINT".to_string(),
        "f64" | "f32"           => "NUMERIC(10,2)".to_string(),
        "bool"                  => "BOOLEAN NOT NULL DEFAULT false".to_string(),
        "String"                => "VARCHAR(255) NOT NULL".to_string(),
        _                       => "TEXT".to_string(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_norms::NormRegistry;

    #[test]
    fn test_create_app_intent() {
        let intent = extract_intent_keywords("I need a warehouse inventory app with barcode scanning");
        assert_eq!(intent.intent_type, IntentType::CreateApplication);
        assert!(intent.features.iter().any(|f| f.contains("barcode")), "should detect barcode feature");
    }

    #[test]
    fn test_add_field_intent() {
        let intent = extract_intent_keywords("add weight to parts");
        assert_eq!(intent.intent_type, IntentType::AddField);
        assert!(!intent.fields.is_empty(), "should extract field");
        assert_eq!(intent.fields[0].field_name, "weight");
    }

    #[test]
    fn test_extract_entities() {
        let intent = extract_intent_keywords("I need a project tracker with tasks and sprints");
        assert!(intent.entities.iter().any(|e| e == "Task" || e == "Project"));
    }

    #[test]
    fn test_create_app_maps_to_norms() {
        let intent = extract_intent_keywords("warehouse inventory management with orders");
        let registry = NormRegistry::default();
        let ops = intent_to_norm_ops(&intent, &registry);
        assert!(!ops.is_empty(), "should produce norm operations");
        let has_compose = ops.iter().any(|op| matches!(op, NormOperation::ComposeNew(_)));
        assert!(has_compose, "should produce ComposeNew operation");
    }

    #[test]
    fn test_affected_files_amend_entity() {
        let op = NormOperation::AmendEntity {
            entity: "Product".to_string(),
            add_field: FieldSpec {
                name: "weight".to_string(),
                rust_type: "Option<i32>".to_string(),
                sql_type: "INTEGER".to_string(),
                nullable: true,
                default_value: None,
                indexed: false,
                unique: false,
                source: FieldSource::UserInput,
                description: "Product weight".to_string(),
            },
        };
        let files = affected_files(&op);
        assert_eq!(files.len(), 6);
        assert!(files.iter().any(|f| f.contains("models/product")));
        assert!(files.iter().any(|f| f.contains("services/product")));
        assert!(files.iter().any(|f| f.contains("migrations")));
    }
}
