// isls-gateway/src/architect.rs — D7/W1: LLM Conversation Protocol
//
// Builds prompts for the architect conversation, parses LLM responses,
// and applies structured updates to the session's AppSpec.

use isls_forge_llm::{EntityDef, ForeignKeyDef, to_snake_case};
use isls_forge_llm::oracle::Oracle;
use isls_hypercube::domain::FieldDef;

use crate::session::{ArchitectSession, SpecUpdate};

// ─── Prompt Builder ─────────────────────────────────────────────────────────

/// Build the LLM prompt for an architect conversation turn.
pub fn build_architect_prompt(session: &ArchitectSession, user_message: &str) -> String {
    let mut p = String::new();

    // System instruction
    p.push_str("You are an architecture assistant for ISLS (Intelligent Semantic Ledger Substrate).\n");
    p.push_str("You help users design application specifications by extracting entities, fields, relationships, and infrastructure requirements from their descriptions.\n\n");
    p.push_str("IMPORTANT: Your response MUST contain two parts:\n");
    p.push_str("1. A human-readable message explaining what you understood and did.\n");
    p.push_str("2. A JSON block (fenced with ```json ... ```) containing structured updates.\n\n");
    p.push_str("The JSON block MUST follow this exact schema:\n");
    p.push_str("```json\n");
    p.push_str("{\n");
    p.push_str("  \"app_name\": \"kebab-case-name\",\n");
    p.push_str("  \"description\": \"Brief app description\",\n");
    p.push_str("  \"message\": \"Human-readable response to the user.\",\n");
    p.push_str("  \"updates\": {\n");
    p.push_str("    \"entities_add\": [\n");
    p.push_str("      { \"name\": \"EntityName\", \"fields\": [\n");
    p.push_str("        { \"name\": \"field_name\", \"field_type\": \"String\" }\n");
    p.push_str("      ], \"foreign_keys\": [{ \"target\": \"OtherEntity\", \"nullable\": false }] }\n");
    p.push_str("    ],\n");
    p.push_str("    \"entities_modify\": [\n");
    p.push_str("      { \"name\": \"EntityName\", \"add_fields\": [\n");
    p.push_str("        { \"name\": \"new_field\", \"field_type\": \"f64\" }\n");
    p.push_str("      ], \"remove_fields\": [\"old_field\"] }\n");
    p.push_str("    ],\n");
    p.push_str("    \"entities_remove\": [\"UnwantedEntity\"],\n");
    p.push_str("    \"infra\": {\n");
    p.push_str("      \"remove_frontend\": false,\n");
    p.push_str("      \"add_cli\": false\n");
    p.push_str("    }\n");
    p.push_str("  }\n");
    p.push_str("}\n");
    p.push_str("```\n\n");
    p.push_str("Rules:\n");
    p.push_str("- Entity names MUST be PascalCase (e.g. Trade, Portfolio, PerformanceMetric).\n");
    p.push_str("- Field names MUST be snake_case.\n");
    p.push_str("- Valid field types: String, i32, i64, f64, bool.\n");
    p.push_str("- Always include id (i64), created_at (String), updated_at (String) implicitly — do NOT list them.\n");
    p.push_str("- If the user hasn't specified an app name, infer one from the description.\n");
    p.push_str("- Only include entities_add for NEW entities, entities_modify for changes to EXISTING ones.\n");
    p.push_str("- If no changes are needed for a category, use an empty array.\n\n");

    // Current AppSpec context
    if !session.spec.entities.is_empty() {
        p.push_str("## CURRENT APP SPEC:\n");
        p.push_str(&format!("App name: {}\n", session.spec.app_name));
        p.push_str(&format!("Description: {}\n", session.spec.description));
        p.push_str(&format!("Entities ({}):\n", session.spec.entities.len()));
        for entity in &session.spec.entities {
            p.push_str(&format!("  - {} (fields: {})\n", entity.name,
                entity.fields.iter().map(|f| format!("{}: {}", f.name, f.rust_type)).collect::<Vec<_>>().join(", ")));
            for fk in &entity.foreign_keys {
                p.push_str(&format!("    FK -> {}{}\n", fk.target, if fk.nullable { " (nullable)" } else { "" }));
            }
        }
        p.push('\n');
    }

    // Conversation history
    if !session.messages.is_empty() {
        p.push_str("## CONVERSATION HISTORY:\n");
        for msg in &session.messages {
            p.push_str(&format!("{}: {}\n", msg.role.to_uppercase(), msg.content));
        }
        p.push('\n');
    }

    // New user message
    p.push_str(&format!("USER: {}\n", user_message));

    p
}

// ─── Response Parser ────────────────────────────────────────────────────────

/// Parsed response from the LLM architect call.
pub struct ArchitectResponse {
    /// Human-readable message for the user.
    pub message: String,
    /// App name (if provided/updated).
    pub app_name: Option<String>,
    /// App description (if provided/updated).
    pub description: Option<String>,
    /// Entities to add.
    pub entities_add: Vec<EntityDef>,
    /// Entity modifications.
    pub entities_modify: Vec<EntityModification>,
    /// Entities to remove.
    pub entities_remove: Vec<String>,
    /// Infrastructure changes.
    pub infra_changes: Vec<String>,
}

/// A modification to an existing entity.
pub struct EntityModification {
    pub name: String,
    pub add_fields: Vec<FieldDef>,
    pub remove_fields: Vec<String>,
}

/// Parse the LLM response into structured data.
pub fn parse_llm_response(response: &str) -> ArchitectResponse {
    // Try to extract JSON block from response
    let json_block = extract_json_block(response);

    let (message, app_name, description, entities_add, entities_modify, entities_remove, infra_changes) =
        if let Some(json_str) = json_block {
            parse_json_updates(&json_str, response)
        } else {
            // No JSON block found — use the whole response as message
            (response.to_string(), None, None, vec![], vec![], vec![], vec![])
        };

    ArchitectResponse {
        message,
        app_name,
        description,
        entities_add,
        entities_modify,
        entities_remove,
        infra_changes,
    }
}

/// Extract JSON block fenced with ```json ... ``` from the response.
fn extract_json_block(response: &str) -> Option<String> {
    // Try ```json ... ```
    if let Some(start) = response.find("```json") {
        let after = &response[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim().to_string());
        }
    }
    // Try bare JSON object
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            if end > start {
                let candidate = &response[start..=end];
                if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                    return Some(candidate.to_string());
                }
            }
        }
    }
    None
}

/// Parse JSON updates into structured types.
fn parse_json_updates(
    json_str: &str,
    full_response: &str,
) -> (String, Option<String>, Option<String>, Vec<EntityDef>, Vec<EntityModification>, Vec<String>, Vec<String>) {
    let json: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return (full_response.to_string(), None, None, vec![], vec![], vec![], vec![]),
    };

    let message = json.get("message")
        .and_then(|v| v.as_str())
        .unwrap_or(full_response)
        .to_string();

    let app_name = json.get("app_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let description = json.get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let updates = json.get("updates");

    // Parse entities_add
    let entities_add: Vec<EntityDef> = updates
        .and_then(|u| u.get("entities_add"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().filter_map(|e| parse_entity_add(e)).collect()
        })
        .unwrap_or_default();

    // Parse entities_modify
    let entities_modify: Vec<EntityModification> = updates
        .and_then(|u| u.get("entities_modify"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().filter_map(|e| parse_entity_modify(e)).collect()
        })
        .unwrap_or_default();

    // Parse entities_remove
    let entities_remove: Vec<String> = updates
        .and_then(|u| u.get("entities_remove"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        })
        .unwrap_or_default();

    // Parse infra changes
    let mut infra_changes = Vec::new();
    if let Some(infra) = updates.and_then(|u| u.get("infra")) {
        if infra.get("remove_frontend").and_then(|v| v.as_bool()).unwrap_or(false) {
            infra_changes.push("remove frontend".to_string());
        }
        if infra.get("add_cli").and_then(|v| v.as_bool()).unwrap_or(false) {
            infra_changes.push("add CLI".to_string());
        }
    }

    (message, app_name, description, entities_add, entities_modify, entities_remove, infra_changes)
}

/// Parse a single entity from the entities_add array.
fn parse_entity_add(value: &serde_json::Value) -> Option<EntityDef> {
    let name = value.get("name")?.as_str()?.to_string();
    let snake_name = to_snake_case(&name);

    let fields: Vec<FieldDef> = value.get("fields")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().filter_map(|f| {
                let fname = f.get("name")?.as_str()?.to_string();
                let ftype = f.get("field_type")?.as_str()?.to_string();
                let sql_type = rust_type_to_sql(&ftype);
                Some(FieldDef {
                    name: fname,
                    rust_type: ftype,
                    sql_type,
                    nullable: f.get("nullable").and_then(|v| v.as_bool()).unwrap_or(false),
                    default_value: None,
                    description: String::new(),
                })
            }).collect()
        })
        .unwrap_or_default();

    let foreign_keys: Vec<ForeignKeyDef> = value.get("foreign_keys")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().filter_map(|fk| {
                let target = fk.get("target")?.as_str()?.to_string();
                let nullable = fk.get("nullable").and_then(|v| v.as_bool()).unwrap_or(false);
                Some(ForeignKeyDef { target, nullable })
            }).collect()
        })
        .unwrap_or_default();

    Some(EntityDef {
        name,
        snake_name,
        fields,
        foreign_keys,
        validations: vec![],
        business_rules: vec![],
        relationships: vec![],
        plural_name: None,
    })
}

/// Parse an entity modification.
fn parse_entity_modify(value: &serde_json::Value) -> Option<EntityModification> {
    let name = value.get("name")?.as_str()?.to_string();

    let add_fields: Vec<FieldDef> = value.get("add_fields")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().filter_map(|f| {
                let fname = f.get("name")?.as_str()?.to_string();
                let ftype = f.get("field_type")?.as_str()?.to_string();
                let sql_type = rust_type_to_sql(&ftype);
                Some(FieldDef {
                    name: fname,
                    rust_type: ftype,
                    sql_type,
                    nullable: f.get("nullable").and_then(|v| v.as_bool()).unwrap_or(false),
                    default_value: None,
                    description: String::new(),
                })
            }).collect()
        })
        .unwrap_or_default();

    let remove_fields: Vec<String> = value.get("remove_fields")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        })
        .unwrap_or_default();

    Some(EntityModification {
        name,
        add_fields,
        remove_fields,
    })
}

/// Map Rust type to SQL type.
fn rust_type_to_sql(rust_type: &str) -> String {
    match rust_type {
        "String" => "VARCHAR(255) NOT NULL".to_string(),
        "i32" => "INTEGER NOT NULL".to_string(),
        "i64" => "BIGINT NOT NULL".to_string(),
        "f64" => "DOUBLE PRECISION NOT NULL".to_string(),
        "bool" => "BOOLEAN NOT NULL DEFAULT false".to_string(),
        other => format!("VARCHAR(255) NOT NULL /* {} */", other),
    }
}

// ─── Apply Updates ──────────────────────────────────────────────────────────

/// Process the full architect response: apply updates to the session.
pub fn apply_architect_response(session: &mut ArchitectSession, response: &ArchitectResponse) {
    // Update app name if provided
    if let Some(ref name) = response.app_name {
        if !name.is_empty() && session.spec.app_name.is_empty() {
            session.set_app_name(name);
        }
    }

    // Update description if provided
    if let Some(ref desc) = response.description {
        if !desc.is_empty() {
            session.spec.description = desc.clone();
        }
    }

    // Add new entities
    for entity in &response.entities_add {
        session.upsert_entity(entity.clone());
    }

    // Modify existing entities
    for modification in &response.entities_modify {
        if let Some(existing) = session.spec.entities.iter_mut().find(|e| e.name == modification.name) {
            // Add new fields
            for field in &modification.add_fields {
                if !existing.fields.iter().any(|f| f.name == field.name) {
                    existing.fields.push(field.clone());
                }
            }
            // Remove fields
            for field_name in &modification.remove_fields {
                existing.fields.retain(|f| f.name != *field_name);
            }
        }
    }

    // Remove entities
    for name in &response.entities_remove {
        session.spec.entities.retain(|e| e.name != *name);
    }

    // Build spec update record
    let spec_update = SpecUpdate {
        entities_added: response.entities_add.iter().map(|e| e.name.clone()).collect(),
        entities_modified: response.entities_modify.iter().map(|e| e.name.clone()).collect(),
        entities_removed: response.entities_remove.clone(),
        infra_changes: response.infra_changes.clone(),
    };

    // Apply infra changes
    session.apply_updates(&spec_update);

    // Record the assistant message
    session.add_assistant_message(&response.message, Some(spec_update));
}

/// Process a user message: build prompt, call oracle, parse, apply.
///
/// Returns the assistant's human-readable message.
pub fn process_message(
    session: &mut ArchitectSession,
    user_message: &str,
    oracle: &dyn Oracle,
) -> Result<String, String> {
    // Add user message to history
    session.add_user_message(user_message);

    // Build prompt
    let prompt = build_architect_prompt(session, user_message);

    // Call oracle
    let response_text = oracle
        .call(&prompt, 4096)
        .map_err(|e| format!("Oracle call failed: {}", e))?;

    // Parse response
    let parsed = parse_llm_response(&response_text);

    // Apply updates
    let message = parsed.message.clone();
    apply_architect_response(session, &parsed);

    Ok(message)
}

/// Process a user message without an LLM — manual/fallback mode.
pub fn process_message_manual(session: &mut ArchitectSession, user_message: &str) -> String {
    session.add_user_message(user_message);
    let message = format!(
        "Manual mode (no API key). Your message has been recorded. \
         You can add entities directly via POST /api/session/{}/entity, \
         or provide an API key to enable AI-assisted architecture.",
        session.id
    );
    session.add_assistant_message(&message, None);
    message
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prompt_empty_session() {
        let session = ArchitectSession::new("t1".into(), None, "gpt-4o".into());
        let prompt = build_architect_prompt(&session, "Build me a pet shop app");
        assert!(prompt.contains("architecture assistant"));
        assert!(prompt.contains("Build me a pet shop app"));
        assert!(!prompt.contains("CURRENT APP SPEC"));
    }

    #[test]
    fn test_build_prompt_with_entities() {
        let mut session = ArchitectSession::new("t2".into(), None, "gpt-4o".into());
        session.upsert_entity(EntityDef {
            name: "Pet".to_string(),
            snake_name: "pet".to_string(),
            fields: vec![FieldDef {
                name: "name".to_string(),
                rust_type: "String".to_string(),
                sql_type: "VARCHAR(255)".to_string(),
                nullable: false,
                default_value: None,
                description: String::new(),
            }],
            foreign_keys: vec![],
            validations: vec![],
            business_rules: vec![],
            relationships: vec![],
            plural_name: None,
        });
        let prompt = build_architect_prompt(&session, "Add a breed field");
        assert!(prompt.contains("CURRENT APP SPEC"));
        assert!(prompt.contains("Pet"));
        assert!(prompt.contains("name: String"));
    }

    #[test]
    fn test_parse_llm_response_with_json() {
        let response = r#"I found 2 entities for your crypto trading journal.

```json
{
  "app_name": "crypto-journal",
  "description": "A crypto trading journal",
  "message": "Found 2 entities: Trade and Portfolio.",
  "updates": {
    "entities_add": [
      { "name": "Trade", "fields": [
        { "name": "pair", "field_type": "String" },
        { "name": "entry_price", "field_type": "f64" }
      ], "foreign_keys": [] },
      { "name": "Portfolio", "fields": [
        { "name": "name", "field_type": "String" }
      ], "foreign_keys": [] }
    ],
    "entities_modify": [],
    "entities_remove": [],
    "infra": { "remove_frontend": false, "add_cli": false }
  }
}
```"#;

        let parsed = parse_llm_response(response);
        assert_eq!(parsed.message, "Found 2 entities: Trade and Portfolio.");
        assert_eq!(parsed.app_name, Some("crypto-journal".to_string()));
        assert_eq!(parsed.entities_add.len(), 2);
        assert_eq!(parsed.entities_add[0].name, "Trade");
        assert_eq!(parsed.entities_add[0].fields.len(), 2);
        assert_eq!(parsed.entities_add[1].name, "Portfolio");
    }

    #[test]
    fn test_parse_llm_response_no_json() {
        let response = "I don't understand your request. Can you be more specific?";
        let parsed = parse_llm_response(response);
        assert_eq!(parsed.message, response);
        assert!(parsed.entities_add.is_empty());
    }

    #[test]
    fn test_apply_architect_response() {
        let mut session = ArchitectSession::new("t3".into(), None, "gpt-4o".into());

        let response = ArchitectResponse {
            message: "Added Trade entity.".to_string(),
            app_name: Some("crypto-journal".to_string()),
            description: Some("A trading journal app".to_string()),
            entities_add: vec![EntityDef {
                name: "Trade".to_string(),
                snake_name: "trade".to_string(),
                fields: vec![FieldDef {
                    name: "pair".to_string(),
                    rust_type: "String".to_string(),
                    sql_type: "VARCHAR(255)".to_string(),
                    nullable: false,
                    default_value: None,
                    description: String::new(),
                }],
                foreign_keys: vec![],
                validations: vec![],
                business_rules: vec![],
                relationships: vec![],
                plural_name: None,
            }],
            entities_modify: vec![],
            entities_remove: vec![],
            infra_changes: vec![],
        };

        apply_architect_response(&mut session, &response);
        assert_eq!(session.spec.app_name, "crypto-journal");
        assert_eq!(session.spec.description, "A trading journal app");
        assert_eq!(session.spec.entities.len(), 1);
        assert_eq!(session.spec.entities[0].name, "Trade");
        assert_eq!(session.messages.len(), 1); // assistant message
        assert!(session.messages[0].updates.is_some());
    }

    #[test]
    fn test_entity_modification() {
        let mut session = ArchitectSession::new("t4".into(), None, "gpt-4o".into());
        session.upsert_entity(EntityDef {
            name: "Trade".to_string(),
            snake_name: "trade".to_string(),
            fields: vec![FieldDef {
                name: "pair".to_string(),
                rust_type: "String".to_string(),
                sql_type: "VARCHAR(255)".to_string(),
                nullable: false,
                default_value: None,
                description: String::new(),
            }],
            foreign_keys: vec![],
            validations: vec![],
            business_rules: vec![],
            relationships: vec![],
            plural_name: None,
        });

        let response = ArchitectResponse {
            message: "Added quantity field to Trade.".to_string(),
            app_name: None,
            description: None,
            entities_add: vec![],
            entities_modify: vec![EntityModification {
                name: "Trade".to_string(),
                add_fields: vec![FieldDef {
                    name: "quantity".to_string(),
                    rust_type: "f64".to_string(),
                    sql_type: "DOUBLE PRECISION".to_string(),
                    nullable: false,
                    default_value: None,
                    description: String::new(),
                }],
                remove_fields: vec![],
            }],
            entities_remove: vec![],
            infra_changes: vec![],
        };

        apply_architect_response(&mut session, &response);
        assert_eq!(session.spec.entities[0].fields.len(), 2);
        assert_eq!(session.spec.entities[0].fields[1].name, "quantity");
    }

    #[test]
    fn test_manual_mode() {
        let mut session = ArchitectSession::new("t5".into(), None, "gpt-4o".into());
        let msg = process_message_manual(&mut session, "Build me an app");
        assert!(msg.contains("Manual mode"));
        assert_eq!(session.messages.len(), 2); // user + assistant
    }

    #[test]
    fn test_rust_type_to_sql() {
        assert!(rust_type_to_sql("String").contains("VARCHAR"));
        assert!(rust_type_to_sql("i32").contains("INTEGER"));
        assert!(rust_type_to_sql("i64").contains("BIGINT"));
        assert!(rust_type_to_sql("f64").contains("DOUBLE PRECISION"));
        assert!(rust_type_to_sql("bool").contains("BOOLEAN"));
    }

    #[test]
    fn test_extract_json_block() {
        let with_fence = "Some text\n```json\n{\"key\": \"value\"}\n```\nMore text";
        assert_eq!(extract_json_block(with_fence), Some("{\"key\": \"value\"}".to_string()));

        let bare_json = "Some text {\"key\": \"value\"} more text";
        assert_eq!(extract_json_block(bare_json), Some("{\"key\": \"value\"}".to_string()));

        let no_json = "Just plain text without any JSON";
        assert_eq!(extract_json_block(no_json), None);
    }
}
