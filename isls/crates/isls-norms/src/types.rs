// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Core norm type definitions for ISLS v3.0.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

// ─── Norm ─────────────────────────────────────────────────────────────────────

/// A composable software pattern spanning all application layers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Norm {
    /// Unique norm identifier (e.g. `"ISLS-NORM-0042"`).
    pub id: String,
    /// Human-readable name (e.g. `"CRUD-Entity"`).
    pub name: String,
    /// Abstraction level.
    pub level: NormLevel,
    /// Patterns that trigger norm activation.
    pub triggers: Vec<TriggerPattern>,
    /// Cross-layer artifact definitions.
    pub layers: NormLayers,
    /// Configurable parameters.
    pub parameters: Vec<NormParameter>,
    /// IDs of norms this norm depends on.
    pub requires: Vec<String>,
    /// Optional implementation variants.
    pub variants: Vec<NormVariant>,
    /// Semantic version string.
    pub version: String,
    /// Evidence/provenance metadata.
    pub evidence: NormEvidence,
}

/// Abstraction level of a norm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NormLevel {
    /// Single field, endpoint, or component.
    Atom,
    /// Coherent feature (CRUD entity, auth, pagination).
    Molecule,
    /// Composed domain (warehouse, e-commerce, project tracker).
    Organism,
    /// Multi-tenant platform.
    Ecosystem,
}

/// A pattern that activates a norm.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TriggerPattern {
    /// Keywords (lowercase) in the requirement description.
    pub keywords: Vec<String>,
    /// Higher-level concepts (lower-cased).
    pub concepts: Vec<String>,
    /// Minimum confidence score to activate.
    pub min_confidence: f64,
    /// Keywords that prevent activation even when others match.
    pub excludes: Vec<String>,
}

// ─── Norm Layers ─────────────────────────────────────────────────────────────

/// Cross-layer artifact definitions for a norm.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NormLayers {
    pub database: Vec<DatabaseArtifact>,
    pub model: Vec<ModelArtifact>,
    pub query: Vec<QueryArtifact>,
    pub service: Vec<ServiceArtifact>,
    pub api: Vec<ApiArtifact>,
    pub frontend: Vec<FrontendArtifact>,
    pub test: Vec<TestArtifact>,
    pub config: Vec<ConfigArtifact>,
}

/// Database migration/schema artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatabaseArtifact {
    /// Table name.
    pub table: String,
    /// SQL DDL snippet.
    pub ddl: String,
}

/// Data model (struct) artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelArtifact {
    /// Struct name (PascalCase).
    pub struct_name: String,
    /// Fields of the struct.
    pub fields: Vec<FieldSpec>,
    /// Derive macros (e.g. `["Debug", "Clone", "Serialize"]`).
    pub derives: Vec<String>,
    /// Validation rules.
    pub validations: Vec<ValidationSpec>,
}

/// A single struct field.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldSpec {
    pub name: String,
    pub rust_type: String,
    pub sql_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub indexed: bool,
    pub unique: bool,
    /// Who controls this field's value.
    pub source: FieldSource,
    pub description: String,
}

/// Who controls a field's value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldSource {
    /// Supplied by the API caller on creation.
    UserInput,
    /// Optional input from the API caller.
    UserOptional,
    /// Set by the database or system on creation (id, created_at).
    SystemGenerated,
    /// Computed by the system at runtime (totals, derived fields).
    SystemComputed,
}

/// A validation rule for a model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationSpec {
    pub name: String,
    pub condition: String,
    pub message: String,
}

/// Database query artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryArtifact {
    pub name: String,
    pub description: String,
    pub sql_template: String,
    pub parameters: Vec<String>,
    pub return_type: String,
}

/// Service-layer artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceArtifact {
    pub name: String,
    pub description: String,
    pub method_signatures: Vec<String>,
    pub business_rules: Vec<String>,
}

/// REST API endpoint artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiArtifact {
    pub method: String,
    pub path: String,
    pub auth_required: bool,
    pub min_role: String,
    pub request_body: Option<String>,
    pub response_type: String,
    pub description: String,
}

/// Frontend UI component artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrontendArtifact {
    pub component_type: FrontendComponent,
    pub name: String,
    pub api_calls: Vec<String>,
    pub description: String,
}

/// Frontend component category.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrontendComponent {
    Page,
    Table,
    Form,
    DetailView,
    Modal,
    DashboardCard,
    Chart,
    SearchBar,
    StatusBadge,
    ActionButton,
    Scanner,
}

/// Integration/unit test artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TestArtifact {
    pub name: String,
    pub description: String,
    pub test_type: String,
    pub scenario: String,
}

/// Configuration artifact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigArtifact {
    pub name: String,
    pub description: String,
    pub template: String,
}

// ─── Norm Parameters & Variants ───────────────────────────────────────────────

/// A configurable parameter for a norm.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NormParameter {
    pub name: String,
    pub param_type: ParamType,
    pub default: Option<String>,
    pub description: String,
}

/// Type of a norm parameter value.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ParamType {
    EntityName,
    FieldList,
    RelationshipList,
    RoleRequirement,
    Boolean,
    Choice(Vec<String>),
}

/// A named variant of a norm (extends the base).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NormVariant {
    pub id: String,
    pub name: String,
    pub modifications: Vec<NormModification>,
}

/// A modification applied by a norm variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NormModification {
    AddField(FieldSpec),
    AddEndpoint(ApiArtifact),
    AddComponent(FrontendArtifact),
    AddTest(TestArtifact),
    AddMigration(String),
    ModifyService(String, String),
}

// ─── Norm Evidence ─────────────────────────────────────────────────────────────

/// Provenance metadata for a norm.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NormEvidence {
    /// How many synthesis runs have used this norm.
    pub usage_count: u32,
    /// Domains this norm has been applied in.
    pub domains_used: Vec<String>,
    /// Whether this norm was hand-authored (`true`) or auto-discovered.
    pub builtin: bool,
    /// SHA-256 content signature.
    pub signature: String,
}

// ─── Activated Norm ───────────────────────────────────────────────────────────

/// A norm that has been activated for a particular synthesis run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivatedNorm {
    pub norm: Norm,
    /// Activation confidence [0.0, 1.0].
    pub confidence: f64,
    /// How this norm was activated.
    pub source: ActivationSource,
}

/// How a norm was activated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivationSource {
    /// User explicitly named this norm.
    UserExplicit,
    /// Keyword matching against the description.
    KeywordMatch,
    /// Required by another activated norm.
    Dependency,
    /// Added by a chat amendment.
    ChatAmendment,
}

// ─── Norm Wiring ─────────────────────────────────────────────────────────────

/// Cross-norm wiring: when norms A and B are both active, apply additional
/// service logic, business rules, and tests.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NormWiring {
    /// (norm_a_id, norm_b_id) — order-independent.
    pub when: (String, String),
    /// Human-readable description.
    pub description: String,
    /// Additional service artifacts to inject.
    pub add_services: Vec<ServiceArtifact>,
    /// Additional business rules to inject.
    pub add_rules: Vec<BusinessRule>,
    /// Additional tests to inject.
    pub add_tests: Vec<TestArtifact>,
}

/// A business rule in a norm wiring.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusinessRule {
    pub name: String,
    pub trigger: String,
    pub condition: String,
    pub action: String,
}

// ─── Layer Type ───────────────────────────────────────────────────────────────

/// Identifies a layer in the full-stack architecture.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayerType {
    Database,
    Model,
    Query,
    Service,
    Api,
    Frontend,
    Test,
    Config,
}

// ─── Interface Contract ───────────────────────────────────────────────────────

/// An interface contract between two norm layers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InterfaceContract {
    pub from_norm: String,
    pub to_norm: String,
    pub contract_type: String,
    pub description: String,
    pub types_shared: Vec<String>,
}

// ─── Composed Plan ─────────────────────────────────────────────────────────────
// (defined in composition.rs — re-exported from lib.rs)

/// Parameter map for norm instantiation.
pub type NormParams = HashMap<String, String>;
