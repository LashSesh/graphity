//! Stub types for removed `isls-oracle` and `isls-templates` crate dependencies.
//!
//! These provide the same interface surface used by the agent's architecture,
//! pipeline, and feature modules so that the crate compiles stand-alone.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ═══════════════════════════════════════════════════════════════════════════════
// Oracle stubs (previously in isls-oracle)
// ═══════════════════════════════════════════════════════════════════════════════

// ─── OracleError ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum OracleError {
    Unavailable(String),
    BudgetExceeded(String),
    RetriesExhausted(usize),
    ValidationFailed(String),
    ParseError(String),
    Http(String),
    Serde(serde_json::Error),
}

impl fmt::Display for OracleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OracleError::Unavailable(s) => write!(f, "LLM unavailable: {}", s),
            OracleError::BudgetExceeded(s) => write!(f, "budget exceeded: {}", s),
            OracleError::RetriesExhausted(n) => {
                write!(f, "all retries exhausted after {} attempts", n)
            }
            OracleError::ValidationFailed(s) => write!(f, "validation failed: {}", s),
            OracleError::ParseError(s) => write!(f, "parse error: {}", s),
            OracleError::Http(s) => write!(f, "HTTP error: {}", s),
            OracleError::Serde(e) => write!(f, "serialization: {}", e),
        }
    }
}

impl std::error::Error for OracleError {}

pub type OracleResult<T> = std::result::Result<T, OracleError>;

// ─── OutputFormat ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub enum OutputFormat {
    Rust,
    Json,
    Yaml,
    OpenApi,
    #[default]
    PlainText,
}

impl OutputFormat {
    pub fn as_str(&self) -> &str {
        match self {
            OutputFormat::Rust => "rust",
            OutputFormat::Json => "json",
            OutputFormat::Yaml => "yaml",
            OutputFormat::OpenApi => "openapi",
            OutputFormat::PlainText => "text",
        }
    }
}

// ─── SynthesisPrompt ────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SynthesisPrompt {
    pub system: String,
    pub user: String,
    pub output_format: OutputFormat,
    pub max_tokens: usize,
    pub temperature: f64,
}

// ─── OracleResponse ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct OracleResponse {
    pub content: String,
    pub model: String,
    pub tokens_used: usize,
    pub finish_reason: String,
    pub latency_ms: u64,
}

// ─── OracleCost ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct OracleCost {
    pub per_input_token_usd: f64,
    pub per_output_token_usd: f64,
}

impl OracleCost {
    pub fn estimate(&self, tokens: usize) -> f64 {
        (tokens as f64 / 1000.0) * (self.per_input_token_usd + self.per_output_token_usd)
    }
}

// ─── SynthesisOracle Trait ──────────────────────────────────────────────────

pub trait SynthesisOracle: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    fn available(&self) -> bool;
    fn synthesize(&self, prompt: &SynthesisPrompt) -> OracleResult<OracleResponse>;
    fn cost_estimate(&self) -> OracleCost;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Template stubs (previously in isls-templates)
// ═══════════════════════════════════════════════════════════════════════════════

// ─── Archetype ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Archetype {
    RestApi,
    CliTool,
    Library,
    Microservice,
    DatabaseBackend,
    WebSocketService,
    WorkerQueue,
    FullStackApp,
    DataPipeline,
    PluginSystem,
    Custom(String),
}

impl Archetype {
    pub fn as_str(&self) -> &str {
        match self {
            Archetype::RestApi => "rest-api",
            Archetype::CliTool => "cli-tool",
            Archetype::Library => "library",
            Archetype::Microservice => "microservice",
            Archetype::DatabaseBackend => "database-backend",
            Archetype::WebSocketService => "websocket-service",
            Archetype::WorkerQueue => "worker-queue",
            Archetype::FullStackApp => "fullstack-app",
            Archetype::DataPipeline => "data-pipeline",
            Archetype::PluginSystem => "plugin-system",
            Archetype::Custom(s) => s.as_str(),
        }
    }
}

// ─── FillStrategy ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum FillStrategy {
    Oracle,
    Pattern,
    Static { content: String },
    Derive { source_atom: String, transform: String },
}

// ─── TemplateConfig ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateConfig {
    pub catalog_dir: String,
    pub auto_match: bool,
    pub match_threshold: f64,
    pub distill_on_success: bool,
    pub active_templates: BTreeMap<String, String>,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            catalog_dir: "~/.isls/templates/".to_string(),
            auto_match: true,
            match_threshold: 0.3,
            distill_on_success: true,
            active_templates: BTreeMap::new(),
        }
    }
}

// ─── ArchitectureTemplate (stub) ────────────────────────────────────────────

/// Minimal stub — only the fields accessed by agent code.
#[derive(Clone, Debug)]
pub struct ArchitectureTemplate {
    pub name: String,
    pub archetype: Archetype,
}

// ─── TemplateCatalog ────────────────────────────────────────────────────────

pub struct TemplateCatalog {
    templates: BTreeMap<String, ArchitectureTemplate>,
    #[allow(dead_code)]
    config: TemplateConfig,
}

impl TemplateCatalog {
    pub fn new(config: TemplateConfig) -> Self {
        Self {
            templates: BTreeMap::new(),
            config,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    pub fn find_by_archetype(&self, arch: &Archetype) -> Vec<&ArchitectureTemplate> {
        self.templates
            .values()
            .filter(|t| &t.archetype == arch)
            .collect()
    }
}
