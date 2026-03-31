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
    /// Validation of extracted specification failed.
    #[error("validation error: {0}")]
    Validation(String),
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

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap_or(ch));
        } else {
            result.push(ch);
        }
    }
    result
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

// ─── D3: Chat-to-App entity extraction ──────────────────────────────────────

/// Build the LLM prompt for entity extraction from a natural language message.
///
/// Returns the exact prompt from the D3 specification (§2.1).
/// A single LLM call with this prompt extracts the complete application structure.
pub fn build_extraction_prompt(message: &str) -> String {
    let template = r#"You are an expert software architect. Extract the data model
from this application description. Return ONLY valid JSON,
no markdown fences, no explanation.

The JSON must follow this exact schema:
{
  "app_name": "kebab-case-name",
  "description": "one line description",
  "entities": [
    {
      "name": "PascalCaseName",
      "fields": [
        {
          "name": "snake_case_name",
          "field_type": "String|i32|i64|f64|bool",
          "nullable": false,
          "unique": false,
          "default": null
        }
      ],
      "foreign_keys": [
        {
          "target": "OtherEntityName",
          "nullable": false
        }
      ]
    }
  ]
}

Rules:
- Every app needs a User entity with: email (String, unique),
  password_hash (String), role (String, default "user"),
  is_active (bool, default true).
- Use i64 for money (cents), i32 for quantities, String for text,
  bool for flags, f64 for measurements.
- Add created_at/updated_at automatically - do NOT include them.
- id is automatic - do NOT include it.
- Foreign keys reference other entities by PascalCase name.
- If an entity "belongs to" another, add a foreign_key.
- Name entities in PascalCase singular: "Product", not "products".
- Name fields in snake_case: "unit_price", not "unitPrice".
- Include 4-12 entities. Not less, not more.
- Every entity needs at least 1 own field besides foreign keys.

User's description:
{user_message}"#;

    template.replace("{user_message}", message)
}

/// Rust reserved keywords — entity and field names must not collide.
const RUST_KEYWORDS: &[&str] = &[
    "return", "type", "match", "move", "ref", "self", "super",
    "crate", "mod", "pub", "use", "fn", "let", "mut", "const",
    "static", "struct", "enum", "trait", "impl", "where", "loop",
    "while", "for", "if", "else", "break", "continue", "async",
    "await", "unsafe", "extern", "dyn", "box", "yield", "macro",
    "abstract", "become", "do", "final", "override", "priv",
    "try", "typeof", "unsized", "virtual",
];

/// Validate the JSON structure extracted by the LLM.
///
/// Checks entity array, User entity presence, PascalCase names, Rust keyword
/// collisions, field types, and FK referential integrity.
pub fn validate_extracted_spec(json: &serde_json::Value) -> Result<()> {
    let entities = json["entities"].as_array()
        .ok_or_else(|| ChatError::Validation("no entities array".into()))?;

    if entities.is_empty() {
        return Err(ChatError::Validation("no entities extracted".into()));
    }

    // Must have a User entity
    let has_user = entities.iter().any(|e| e["name"].as_str() == Some("User"));
    if !has_user {
        return Err(ChatError::Validation("no User entity — required for auth".into()));
    }

    // Collect all entity names for FK validation
    let entity_names: Vec<&str> = entities.iter()
        .filter_map(|e| e["name"].as_str())
        .collect();

    for entity in entities {
        let name = entity["name"].as_str()
            .ok_or_else(|| ChatError::Validation("entity without name".into()))?;

        // PascalCase check
        if name.chars().next().map_or(true, |c| !c.is_uppercase()) {
            return Err(ChatError::Validation(format!("entity '{}' not PascalCase", name)));
        }

        // Must have fields or foreign_keys (or both)
        let field_count = entity["fields"].as_array().map_or(0, |f| f.len());
        let fk_count = entity["foreign_keys"].as_array().map_or(0, |f| f.len());
        if field_count == 0 && fk_count == 0 {
            return Err(ChatError::Validation(
                format!("entity '{}' has no fields and no FKs", name),
            ));
        }

        // FK targets must reference existing entities
        if let Some(fks) = entity["foreign_keys"].as_array() {
            for fk in fks {
                let target = fk["target"].as_str()
                    .ok_or_else(|| ChatError::Validation("FK without target".into()))?;
                if !entity_names.contains(&target) {
                    return Err(ChatError::Validation(
                        format!("FK target '{}' not in entities", target),
                    ));
                }
            }
        }

        // Field names and types must be valid
        if let Some(fields) = entity["fields"].as_array() {
            for field in fields {
                let fname = field["name"].as_str().unwrap_or("");
                if RUST_KEYWORDS.contains(&fname) {
                    return Err(ChatError::Validation(
                        format!("field name '{}' in {} is a Rust keyword", fname, name),
                    ));
                }
                let ft = field["field_type"].as_str().unwrap_or("");
                if !["String", "i32", "i64", "f64", "bool"].contains(&ft) {
                    return Err(ChatError::Validation(
                        format!("invalid field type '{}' in {}", ft, name),
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Convert LLM-extracted JSON to a valid TOML string.
///
/// Deterministic conversion — no LLM call. Produces `[app]`, `[backend]`,
/// `[frontend]`, `[deployment]`, and `[[entities]]` sections.
pub fn json_to_toml(json: &serde_json::Value) -> Result<String> {
    let mut toml = String::new();

    // [app] section
    toml.push_str("[app]\n");
    toml.push_str(&format!("name = \"{}\"\n",
        json["app_name"].as_str().unwrap_or("my-app")));
    toml.push_str(&format!("description = \"{}\"\n\n",
        json["description"].as_str().unwrap_or("Generated application")));

    // [backend] section
    toml.push_str("[backend]\n");
    toml.push_str("language = \"rust\"\n");
    toml.push_str("framework = \"actix-web\"\n");
    toml.push_str("database = \"postgresql\"\n");
    toml.push_str("auth_method = \"jwt\"\n\n");

    // [frontend] section
    toml.push_str("[frontend]\n");
    toml.push_str("type = \"spa\"\n");
    toml.push_str("framework = \"vanilla\"\n");
    toml.push_str("styling = \"minimal\"\n\n");

    // [deployment] section
    toml.push_str("[deployment]\n");
    toml.push_str("containerized = true\n");
    toml.push_str("compose = true\n\n");

    // [[entities]] sections
    if let Some(entities) = json["entities"].as_array() {
        // Build rename map: keyword entity names get "Record" suffix
        let rename = |name: &str| -> String {
            if RUST_KEYWORDS.contains(&to_snake_case(name).as_str()) {
                format!("{}Record", name)
            } else {
                name.to_string()
            }
        };

        for entity in entities {
            toml.push_str("[[entities]]\n");
            let raw_name = entity["name"].as_str().unwrap_or("Unknown");
            toml.push_str(&format!("name = \"{}\"\n", rename(raw_name)));

            // Compute FK-generated field names to skip duplicates
            let fk_field_names: Vec<String> = entity["foreign_keys"].as_array()
                .map(|fks| fks.iter().filter_map(|fk| {
                    fk["target"].as_str().map(|t| {
                        let renamed = rename(t);
                        format!("{}_id", to_snake_case(&renamed))
                    })
                }).collect())
                .unwrap_or_default();

            // Fields
            if let Some(fields) = entity["fields"].as_array() {
                toml.push_str("fields = [\n");
                for field in fields {
                    let fname = field["name"].as_str().unwrap_or("field");
                    if fk_field_names.contains(&fname.to_string()) {
                        continue; // FK has priority — skip duplicate user field
                    }
                    toml.push_str(&format!(
                        "    {{ name = \"{}\", field_type = \"{}\"",
                        fname,
                        field["field_type"].as_str().unwrap_or("String"),
                    ));
                    if field["nullable"].as_bool().unwrap_or(false) {
                        toml.push_str(", nullable = true");
                    }
                    if field["unique"].as_bool().unwrap_or(false) {
                        toml.push_str(", unique = true");
                    }
                    if let Some(def) = field["default"].as_str() {
                        toml.push_str(&format!(", default = \"{}\"", def));
                    }
                    toml.push_str(" },\n");
                }
                toml.push_str("]\n");
            }

            // Foreign keys (with renamed targets)
            if let Some(fks) = entity["foreign_keys"].as_array() {
                if !fks.is_empty() {
                    toml.push_str("foreign_keys = [\n");
                    for fk in fks {
                        let target = fk["target"].as_str().unwrap_or("Unknown");
                        toml.push_str(&format!(
                            "    {{ target = \"{}\"",
                            rename(target),
                        ));
                        if fk["nullable"].as_bool().unwrap_or(false) {
                            toml.push_str(", nullable = true");
                        }
                        toml.push_str(" },\n");
                    }
                    toml.push_str("]\n");
                }
            }
            toml.push_str("\n");
        }
    }

    Ok(toml)
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

    // ─── D3 tests ────────────────────────────────────────────────────────────

    fn sample_valid_json() -> serde_json::Value {
        serde_json::json!({
            "app_name": "test-app",
            "description": "A test application",
            "entities": [
                {
                    "name": "User",
                    "fields": [
                        { "name": "email", "field_type": "String", "nullable": false, "unique": true },
                        { "name": "password_hash", "field_type": "String", "nullable": false, "unique": false },
                        { "name": "role", "field_type": "String", "nullable": false, "unique": false, "default": "user" },
                        { "name": "is_active", "field_type": "bool", "nullable": false, "unique": false, "default": "true" }
                    ],
                    "foreign_keys": []
                },
                {
                    "name": "Product",
                    "fields": [
                        { "name": "name", "field_type": "String", "nullable": false, "unique": false },
                        { "name": "price", "field_type": "i64", "nullable": false, "unique": false },
                        { "name": "in_stock", "field_type": "bool", "nullable": false, "unique": false }
                    ],
                    "foreign_keys": [
                        { "target": "User", "nullable": false }
                    ]
                }
            ]
        })
    }

    #[test]
    fn test_build_extraction_prompt() {
        let prompt = build_extraction_prompt("Restaurant with reservations");
        assert!(prompt.contains("Restaurant with reservations"), "should contain user message");
        assert!(prompt.contains("User entity"), "should mention User entity requirement");
        assert!(prompt.contains("PascalCase"), "should mention PascalCase");
        assert!(prompt.contains("snake_case"), "should mention snake_case");
        assert!(prompt.contains("4-12 entities"), "should mention entity count bounds");
    }

    #[test]
    fn test_validate_valid_spec() {
        let json = sample_valid_json();
        assert!(validate_extracted_spec(&json).is_ok(), "valid spec should pass validation");
    }

    #[test]
    fn test_validate_missing_user() {
        let json = serde_json::json!({
            "entities": [
                {
                    "name": "Product",
                    "fields": [
                        { "name": "name", "field_type": "String" }
                    ]
                }
            ]
        });
        let err = validate_extracted_spec(&json).unwrap_err();
        assert!(err.to_string().contains("User"), "should mention missing User entity");
    }

    #[test]
    fn test_validate_invalid_type() {
        let json = serde_json::json!({
            "entities": [
                {
                    "name": "User",
                    "fields": [
                        { "name": "email", "field_type": "VARCHAR" }
                    ]
                }
            ]
        });
        let err = validate_extracted_spec(&json).unwrap_err();
        assert!(err.to_string().contains("invalid field type"), "should reject invalid type");
    }

    #[test]
    fn test_validate_bad_fk_target() {
        let json = serde_json::json!({
            "entities": [
                {
                    "name": "User",
                    "fields": [
                        { "name": "email", "field_type": "String" }
                    ],
                    "foreign_keys": [
                        { "target": "NonExistent" }
                    ]
                }
            ]
        });
        let err = validate_extracted_spec(&json).unwrap_err();
        assert!(err.to_string().contains("not in entities"), "should reject bad FK target");
    }

    #[test]
    fn test_json_to_toml() {
        let json = sample_valid_json();
        let toml = json_to_toml(&json).unwrap();

        // [app] section
        assert!(toml.contains("[app]"));
        assert!(toml.contains("name = \"test-app\""));
        assert!(toml.contains("description = \"A test application\""));

        // [backend] section
        assert!(toml.contains("[backend]"));
        assert!(toml.contains("language = \"rust\""));
        assert!(toml.contains("framework = \"actix-web\""));
        assert!(toml.contains("database = \"postgresql\""));
        assert!(toml.contains("auth_method = \"jwt\""));

        // [frontend] section
        assert!(toml.contains("[frontend]"));
        assert!(toml.contains("framework = \"vanilla\""));

        // [deployment] section
        assert!(toml.contains("[deployment]"));
        assert!(toml.contains("containerized = true"));

        // [[entities]] sections
        assert!(toml.contains("[[entities]]"));
        assert!(toml.contains("name = \"User\""));
        assert!(toml.contains("name = \"Product\""));

        // Field details
        assert!(toml.contains("name = \"email\""));
        assert!(toml.contains("unique = true"));
        assert!(toml.contains("default = \"user\""));

        // FK
        assert!(toml.contains("target = \"User\""));
    }

    #[test]
    fn test_validate_keyword_entity_passes() {
        // Entity names that are Rust keywords (PascalCase) pass validation —
        // they are auto-renamed in json_to_toml, not rejected here.
        let json = serde_json::json!({
            "entities": [
                {
                    "name": "User",
                    "fields": [
                        { "name": "email", "field_type": "String" }
                    ]
                },
                {
                    "name": "Return",
                    "fields": [
                        { "name": "reason", "field_type": "String" }
                    ]
                }
            ]
        });
        assert!(validate_extracted_spec(&json).is_ok(),
            "keyword entity names should pass validation (renamed in TOML)");
    }

    #[test]
    fn test_json_to_toml_keyword_entity_rename() {
        let json = serde_json::json!({
            "app_name": "library",
            "description": "Library system",
            "entities": [
                {
                    "name": "User",
                    "fields": [
                        { "name": "email", "field_type": "String" }
                    ]
                },
                {
                    "name": "Return",
                    "fields": [
                        { "name": "reason", "field_type": "String" }
                    ],
                    "foreign_keys": [
                        { "target": "User" }
                    ]
                },
                {
                    "name": "Loan",
                    "fields": [
                        { "name": "due_date", "field_type": "String" }
                    ],
                    "foreign_keys": [
                        { "target": "Return" }
                    ]
                }
            ]
        });
        let toml = json_to_toml(&json).unwrap();
        // Return → ReturnRecord (keyword auto-rename)
        assert!(toml.contains("name = \"ReturnRecord\""),
            "keyword entity should be renamed to ReturnRecord");
        assert!(!toml.contains("name = \"Return\""),
            "original keyword entity name should not appear");
        // FK targets must also be renamed
        assert!(toml.contains("target = \"ReturnRecord\""),
            "FK target should reference renamed entity");
        // Non-keyword entities unchanged
        assert!(toml.contains("name = \"User\""));
        assert!(toml.contains("name = \"Loan\""));
    }

    #[test]
    fn test_validate_keyword_field() {
        let json = serde_json::json!({
            "entities": [
                {
                    "name": "User",
                    "fields": [
                        { "name": "email", "field_type": "String" },
                        { "name": "match", "field_type": "String" }
                    ]
                }
            ]
        });
        let err = validate_extracted_spec(&json).unwrap_err();
        assert!(err.to_string().contains("Rust keyword"), "should reject keyword field name");
    }

    #[test]
    fn test_json_to_toml_dedup_fk_field() {
        let json = serde_json::json!({
            "app_name": "dedup-test",
            "description": "Test FK dedup",
            "entities": [
                {
                    "name": "Category",
                    "fields": [
                        { "name": "label", "field_type": "String" }
                    ]
                },
                {
                    "name": "Product",
                    "fields": [
                        { "name": "name", "field_type": "String" },
                        { "name": "category_id", "field_type": "i64" }
                    ],
                    "foreign_keys": [
                        { "target": "Category" }
                    ]
                }
            ]
        });
        let toml = json_to_toml(&json).unwrap();
        // FK target must be present
        assert!(toml.contains("target = \"Category\""));
        // The duplicate category_id field must be removed (FK has priority)
        assert!(!toml.contains("name = \"category_id\""),
            "duplicate field should be removed when FK generates same name");
        // Other fields must remain
        assert!(toml.contains("name = \"name\""));
    }
}
