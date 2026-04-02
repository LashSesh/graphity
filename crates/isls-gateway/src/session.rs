// isls-gateway/src/session.rs — D7/W1: Architect Session Management
//
// Multi-turn conversation sessions that incrementally build an AppSpec.
// Sessions are stored in-memory (HashMap in AppState).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use isls_forge_llm::{AppSpec, EntityDef, ForeignKeyDef, ValidationRule};
use isls_forge_llm::blueprint::InfraBlueprint;

// ─── Session Types ──────────────────────────────────────────────────────────

/// A conversation session accumulating toward an AppSpec.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectSession {
    pub id: String,
    pub created_at: String,
    /// Full conversation history.
    pub messages: Vec<SessionMessage>,
    /// The evolving AppSpec (starts empty, grows per turn).
    pub spec: AppSpec,
    /// Activated infrastructure norms.
    pub infra_norms: Vec<String>,
    /// API key for LLM calls (provided at session creation).
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    /// LLM model name.
    pub model: String,
    /// Generation result summary (after forge).
    pub forge_result: Option<SessionForgeResult>,
}

/// A single message in the conversation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String, // "user" or "assistant"
    pub content: String,
    pub timestamp: String,
    /// Structured updates extracted from this message.
    pub updates: Option<SpecUpdate>,
}

/// Structured updates extracted from an LLM response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpecUpdate {
    pub entities_added: Vec<String>,
    pub entities_modified: Vec<String>,
    pub entities_removed: Vec<String>,
    pub infra_changes: Vec<String>,
}

/// Summary of a forge run stored on the session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionForgeResult {
    pub success: bool,
    pub files_generated: usize,
    pub total_loc: usize,
    pub total_tokens: u64,
    pub duration_secs: f64,
    pub output_dir: String,
}

/// In-memory store for architect sessions.
pub type SessionStore = Arc<RwLock<HashMap<String, ArchitectSession>>>;

// ─── Session Management ─────────────────────────────────────────────────────

impl ArchitectSession {
    /// Create a new empty session.
    pub fn new(id: String, api_key: Option<String>, model: String) -> Self {
        Self {
            id,
            created_at: chrono::Utc::now().to_rfc3339(),
            messages: Vec::new(),
            spec: AppSpec {
                app_name: String::new(),
                description: String::new(),
                domain_name: String::new(),
                entities: Vec::new(),
                business_rules: Vec::new(),
            },
            infra_norms: Vec::new(),
            api_key,
            model,
            forge_result: None,
        }
    }

    /// Add a user message to the conversation.
    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(SessionMessage {
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            updates: None,
        });
    }

    /// Add an assistant message with optional spec updates.
    pub fn add_assistant_message(&mut self, content: &str, updates: Option<SpecUpdate>) {
        self.messages.push(SessionMessage {
            role: "assistant".to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            updates,
        });
    }

    /// Apply structured updates to the session's AppSpec.
    pub fn apply_updates(&mut self, update: &SpecUpdate) {
        // Remove entities
        for name in &update.entities_removed {
            self.spec.entities.retain(|e| e.name != *name);
        }

        // Note: entities_added and entities_modified are tracked for the SpecUpdate record.
        // Actual entity data comes from the architect module's parsed LLM response.

        // Process infra changes
        for change in &update.infra_changes {
            let lower = change.to_lowercase();
            if lower.contains("remove") && lower.contains("frontend") {
                self.infra_norms.retain(|n| !n.to_lowercase().contains("frontend"));
            } else if lower.contains("add") {
                self.infra_norms.push(change.clone());
            }
        }
    }

    /// Add or update an entity in the spec.
    pub fn upsert_entity(&mut self, entity: EntityDef) {
        if let Some(existing) = self.spec.entities.iter_mut().find(|e| e.name == entity.name) {
            *existing = entity;
        } else {
            self.spec.entities.push(entity);
        }
    }

    /// Set the app name (derived from conversation or explicit).
    pub fn set_app_name(&mut self, name: &str) {
        self.spec.app_name = name.to_string();
        self.spec.domain_name = name
            .replace('-', " ")
            .split_whitespace()
            .next()
            .unwrap_or("app")
            .to_string();
    }
}

// ─── Session Summary (for list endpoint) ────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub created_at: String,
    pub message_count: usize,
    pub entity_count: usize,
    pub app_name: String,
    pub has_forge_result: bool,
}

impl From<&ArchitectSession> for SessionSummary {
    fn from(s: &ArchitectSession) -> Self {
        Self {
            id: s.id.clone(),
            created_at: s.created_at.clone(),
            message_count: s.messages.len(),
            entity_count: s.spec.entities.len(),
            app_name: s.spec.app_name.clone(),
            has_forge_result: s.forge_result.is_some(),
        }
    }
}

// ─── Readiness Check (D7/W2) ────────────────────────────────────────────────

/// Result of a readiness check.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadinessResult {
    /// Whether all required criteria are met and forge can proceed.
    pub ready: bool,
    /// Overall readiness score (0-100).
    pub score: u32,
    /// Individual criterion results.
    pub criteria: HashMap<String, CriterionResult>,
    /// Suggestions for improving readiness.
    pub suggestions: Vec<String>,
}

/// Result for a single readiness criterion.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CriterionResult {
    pub met: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u32>,
}

/// Valid field types for the entities_have_types criterion.
const VALID_FIELD_TYPES: &[&str] = &["String", "i32", "i64", "f64", "bool"];

/// Compute the readiness of a session's AppSpec.
pub fn compute_readiness(session: &ArchitectSession) -> ReadinessResult {
    let spec = &session.spec;
    let mut criteria = HashMap::new();
    let mut suggestions = Vec::new();
    let mut all_required = true;
    let mut bonus_score: u32 = 0;

    // Required: has_entities — at least 1 non-User entity
    let non_user_entities: Vec<&EntityDef> = spec.entities.iter()
        .filter(|e| e.name != "User")
        .collect();
    let has_entities = !non_user_entities.is_empty();
    criteria.insert("has_entities".to_string(), CriterionResult {
        met: has_entities,
        detail: Some(format!("{} entities", non_user_entities.len())),
        score: None,
    });
    if !has_entities {
        all_required = false;
        suggestions.push("Add at least one entity (e.g. 'I need a Trade entity')".to_string());
    }

    // Required: entities_have_fields — every entity has >= 1 field
    let entities_without_fields: Vec<&str> = spec.entities.iter()
        .filter(|e| e.name != "User" && e.fields.is_empty())
        .map(|e| e.name.as_str())
        .collect();
    let entities_have_fields = entities_without_fields.is_empty() && has_entities;
    criteria.insert("entities_have_fields".to_string(), CriterionResult {
        met: entities_have_fields,
        detail: if entities_without_fields.is_empty() { None } else {
            Some(format!("Missing fields: {}", entities_without_fields.join(", ")))
        },
        score: None,
    });
    if !entities_have_fields {
        all_required = false;
        for name in &entities_without_fields {
            suggestions.push(format!("Add fields to {} (e.g. 'A {} has a name and description')", name, name));
        }
    }

    // Required: has_app_name
    let has_app_name = !spec.app_name.is_empty();
    criteria.insert("has_app_name".to_string(), CriterionResult {
        met: has_app_name,
        detail: if has_app_name { Some(spec.app_name.clone()) } else { None },
        score: None,
    });
    if !has_app_name {
        all_required = false;
        suggestions.push("Specify an app name (the system will infer one from your description)".to_string());
    }

    // Required: fk_targets_valid — all FK targets reference existing entities
    let entity_names: Vec<&str> = spec.entities.iter().map(|e| e.name.as_str()).collect();
    let invalid_fks: Vec<String> = spec.entities.iter()
        .flat_map(|e| {
            e.foreign_keys.iter()
                .filter(|fk| !entity_names.contains(&fk.target.as_str()))
                .map(|fk| format!("{} -> {}", e.name, fk.target))
                .collect::<Vec<_>>()
        })
        .collect();
    let fk_targets_valid = invalid_fks.is_empty();
    criteria.insert("fk_targets_valid".to_string(), CriterionResult {
        met: fk_targets_valid,
        detail: if invalid_fks.is_empty() { None } else {
            Some(format!("Invalid FK targets: {}", invalid_fks.join(", ")))
        },
        score: None,
    });
    if !fk_targets_valid {
        all_required = false;
        for fk in &invalid_fks {
            suggestions.push(format!("Fix foreign key: {} (target entity does not exist)", fk));
        }
    }

    // Bonus: has_description (+10%)
    let has_description = !spec.description.is_empty();
    let desc_score = if has_description { 10 } else { 0 };
    bonus_score += desc_score;
    criteria.insert("has_description".to_string(), CriterionResult {
        met: has_description,
        detail: None,
        score: Some(desc_score),
    });
    if !has_description {
        suggestions.push("Add a description for your application".to_string());
    }

    // Bonus: entities_have_types (+15%)
    let fields_without_types: Vec<String> = spec.entities.iter()
        .flat_map(|e| {
            e.fields.iter()
                .filter(|f| !VALID_FIELD_TYPES.contains(&f.rust_type.as_str()))
                .map(|f| format!("{}.{}", e.name, f.name))
                .collect::<Vec<_>>()
        })
        .collect();
    let entities_have_types = fields_without_types.is_empty() && has_entities;
    let types_score = if entities_have_types { 15 } else { 0 };
    bonus_score += types_score;
    criteria.insert("entities_have_types".to_string(), CriterionResult {
        met: entities_have_types,
        detail: if fields_without_types.is_empty() { None } else {
            Some(format!("{} has invalid type", fields_without_types.first().unwrap_or(&String::new())))
        },
        score: Some(types_score),
    });
    if !entities_have_types {
        for f in &fields_without_types {
            suggestions.push(format!("Add valid type to {} (String/i32/i64/f64/bool)", f));
        }
    }

    // Bonus: has_relationships (+10%)
    let fk_count: usize = spec.entities.iter().map(|e| e.foreign_keys.len()).sum();
    let has_relationships = fk_count > 0;
    let rel_score = if has_relationships { 10 } else { 0 };
    bonus_score += rel_score;
    criteria.insert("has_relationships".to_string(), CriterionResult {
        met: has_relationships,
        detail: if has_relationships { Some(format!("{} FK(s)", fk_count)) } else { None },
        score: Some(rel_score),
    });

    // Bonus: multi_entity (+15%)
    let multi_entity = non_user_entities.len() >= 3;
    let multi_score = if multi_entity { 15 } else { 0 };
    bonus_score += multi_score;
    criteria.insert("multi_entity".to_string(), CriterionResult {
        met: multi_entity,
        detail: Some(format!("{} entities", non_user_entities.len())),
        score: Some(multi_score),
    });

    // Calculate final score: 50% base when required met + bonuses
    let base = if all_required { 50 } else { 0 };
    let score = (base + bonus_score).min(100);

    ReadinessResult {
        ready: all_required,
        score,
        criteria,
        suggestions,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_hypercube::domain::FieldDef;

    #[test]
    fn test_create_session() {
        let session = ArchitectSession::new(
            "test-001".to_string(),
            None,
            "gpt-4o".to_string(),
        );
        assert_eq!(session.id, "test-001");
        assert!(session.spec.entities.is_empty());
        assert!(session.spec.app_name.is_empty());
        assert!(session.messages.is_empty());
    }

    #[test]
    fn test_add_messages() {
        let mut session = ArchitectSession::new(
            "test-002".to_string(),
            None,
            "gpt-4o".to_string(),
        );
        session.add_user_message("Build me a pet shop app");
        session.add_assistant_message("I'll create a Pet entity.", Some(SpecUpdate {
            entities_added: vec!["Pet".to_string()],
            entities_modified: vec![],
            entities_removed: vec![],
            infra_changes: vec![],
        }));
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].role, "user");
        assert_eq!(session.messages[1].role, "assistant");
        assert!(session.messages[1].updates.is_some());
    }

    #[test]
    fn test_upsert_entity() {
        let mut session = ArchitectSession::new(
            "test-003".to_string(),
            None,
            "gpt-4o".to_string(),
        );
        let pet = EntityDef {
            name: "Pet".to_string(),
            snake_name: "pet".to_string(),
            fields: vec![FieldDef {
                name: "name".to_string(),
                rust_type: "String".to_string(),
                sql_type: "VARCHAR(255)".to_string(),
                nullable: false,
                default_value: None,
                description: "Pet name".to_string(),
            }],
            foreign_keys: vec![],
            validations: vec![],
            business_rules: vec![],
            relationships: vec![],
            plural_name: None,
        };
        session.upsert_entity(pet.clone());
        assert_eq!(session.spec.entities.len(), 1);

        // Update same entity
        let mut updated_pet = pet;
        updated_pet.fields.push(FieldDef {
            name: "breed".to_string(),
            rust_type: "String".to_string(),
            sql_type: "VARCHAR(255)".to_string(),
            nullable: true,
            default_value: None,
            description: "Breed".to_string(),
        });
        session.upsert_entity(updated_pet);
        assert_eq!(session.spec.entities.len(), 1);
        assert_eq!(session.spec.entities[0].fields.len(), 2);
    }

    #[test]
    fn test_apply_updates_remove_entity() {
        let mut session = ArchitectSession::new(
            "test-004".to_string(),
            None,
            "gpt-4o".to_string(),
        );
        session.upsert_entity(EntityDef {
            name: "Pet".to_string(),
            snake_name: "pet".to_string(),
            fields: vec![],
            foreign_keys: vec![],
            validations: vec![],
            business_rules: vec![],
            relationships: vec![],
            plural_name: None,
        });
        session.upsert_entity(EntityDef {
            name: "Owner".to_string(),
            snake_name: "owner".to_string(),
            fields: vec![],
            foreign_keys: vec![],
            validations: vec![],
            business_rules: vec![],
            relationships: vec![],
            plural_name: None,
        });
        assert_eq!(session.spec.entities.len(), 2);

        session.apply_updates(&SpecUpdate {
            entities_added: vec![],
            entities_modified: vec![],
            entities_removed: vec!["Owner".to_string()],
            infra_changes: vec![],
        });
        assert_eq!(session.spec.entities.len(), 1);
        assert_eq!(session.spec.entities[0].name, "Pet");
    }

    #[test]
    fn test_set_app_name() {
        let mut session = ArchitectSession::new(
            "test-005".to_string(),
            None,
            "gpt-4o".to_string(),
        );
        session.set_app_name("crypto-journal");
        assert_eq!(session.spec.app_name, "crypto-journal");
        assert_eq!(session.spec.domain_name, "crypto");
    }

    // ── Readiness tests ──────────────────────────────────────────────────────

    #[test]
    fn test_readiness_empty_session() {
        let session = ArchitectSession::new("r1".into(), None, "gpt-4o".into());
        let r = compute_readiness(&session);
        assert!(!r.ready);
        assert_eq!(r.score, 0);
        assert!(!r.suggestions.is_empty());
    }

    #[test]
    fn test_readiness_with_entity_and_fields() {
        let mut session = ArchitectSession::new("r2".into(), None, "gpt-4o".into());
        session.set_app_name("test-app");
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
        let r = compute_readiness(&session);
        assert!(r.ready);
        assert!(r.score >= 50);
    }

    #[test]
    fn test_readiness_multi_entity_bonus() {
        let mut session = ArchitectSession::new("r3".into(), None, "gpt-4o".into());
        session.set_app_name("test-app");
        session.spec.description = "A test app".to_string();
        for name in &["Pet", "Owner", "Breed"] {
            session.upsert_entity(EntityDef {
                name: name.to_string(),
                snake_name: isls_forge_llm::to_snake_case(name),
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
        }
        let r = compute_readiness(&session);
        assert!(r.ready);
        // Should get base 50 + description 10 + types 15 + multi_entity 15 = 90
        assert!(r.score >= 85, "score was {}", r.score);
    }

    #[test]
    fn test_readiness_invalid_fk() {
        let mut session = ArchitectSession::new("r4".into(), None, "gpt-4o".into());
        session.set_app_name("test-app");
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
            foreign_keys: vec![ForeignKeyDef {
                target: "NonExistent".to_string(),
                nullable: false,
            }],
            validations: vec![],
            business_rules: vec![],
            relationships: vec![],
            plural_name: None,
        });
        let r = compute_readiness(&session);
        assert!(!r.ready);
        assert!(r.suggestions.iter().any(|s| s.contains("NonExistent")));
    }

    #[test]
    fn test_session_summary() {
        let session = ArchitectSession::new(
            "test-006".to_string(),
            None,
            "gpt-4o".to_string(),
        );
        let summary = SessionSummary::from(&session);
        assert_eq!(summary.id, "test-006");
        assert_eq!(summary.message_count, 0);
        assert_eq!(summary.entity_count, 0);
        assert!(!summary.has_forge_result);
    }
}
