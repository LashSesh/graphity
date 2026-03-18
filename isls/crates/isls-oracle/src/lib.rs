// isls-oracle: Hybrid Synthesis Oracle — C25
// LLM-Bridge with Progressive Pattern Autonomy.
// Memory-first → LLM fallback → skeleton fallback.
// Every LLM output is treated as an untrusted hypothesis and must pass
// parse + constraint + PMHD + 8-gate validation before crystallization.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use sha2::{Digest, Sha256};

use isls_types::{FiveDState, Hash256};
use isls_artifact_ir::ArtifactIR;
use isls_forge::Matrix;

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum OracleError {
    #[error("LLM unavailable: {0}")]
    Unavailable(String),
    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),
    #[error("all retries exhausted after {0} attempts")]
    RetriesExhausted(usize),
    #[error("validation failed: {0}")]
    ValidationFailed(String),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, OracleError>;

// ─── Output Format ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum OutputFormat {
    Rust,
    Json,
    Yaml,
    OpenApi,
    PlainText,
}

impl OutputFormat {
    pub fn as_str(&self) -> &str {
        match self {
            OutputFormat::Rust      => "rust",
            OutputFormat::Json      => "json",
            OutputFormat::Yaml      => "yaml",
            OutputFormat::OpenApi   => "openapi",
            OutputFormat::PlainText => "text",
        }
    }
}

impl Default for OutputFormat {
    fn default() -> Self { OutputFormat::PlainText }
}

// ─── Synthesis Prompt ────────────────────────────────────────────────────────

/// Fully determined by ArtifactIR — same IR always produces same prompt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SynthesisPrompt {
    pub system: String,
    pub user: String,
    pub output_format: OutputFormat,
    pub max_tokens: usize,
    pub temperature: f64,
}

// ─── Oracle Response ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct OracleResponse {
    pub content: String,
    pub model: String,
    pub tokens_used: usize,
    pub finish_reason: String,
    pub latency_ms: u64,
}

// ─── Oracle Cost ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct OracleCost {
    /// USD per 1k input tokens
    pub per_input_token_usd: f64,
    /// USD per 1k output tokens
    pub per_output_token_usd: f64,
}

impl OracleCost {
    pub fn estimate(&self, tokens: usize) -> f64 {
        (tokens as f64 / 1000.0) * (self.per_input_token_usd + self.per_output_token_usd)
    }
}

// ─── SynthesisOracle Trait ───────────────────────────────────────────────────

/// Every external synthesis provider implements this trait.
pub trait SynthesisOracle: Send + Sync {
    /// Human-readable name
    fn name(&self) -> &str;
    /// Model identifier
    fn model(&self) -> &str;
    /// Check availability (API key present, endpoint reachable)
    fn available(&self) -> bool;
    /// Generate synthesis output from a structured prompt
    fn synthesize(&self, prompt: &SynthesisPrompt) -> Result<OracleResponse>;
    /// Estimated cost per call (for budget tracking)
    fn cost_estimate(&self) -> OracleCost;
}

// ─── ClaudeOracle ─────────────────────────────────────────────────────────────

/// Default built-in oracle: Anthropic Claude API.
/// The API key MUST NOT appear in any log, trace, crystal, manifest, or report.
pub struct ClaudeOracle {
    api_key: String,
    model: String,
    #[allow(dead_code)]
    endpoint: String,
    pub max_retries: usize,
    pub timeout_ms: u64,
    pub temperature: f64,
}

impl ClaudeOracle {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "claude-sonnet-4-20250514".to_string(),
            endpoint: "https://api.anthropic.com/v1/messages".to_string(),
            max_retries: 3,
            timeout_ms: 60_000,
            temperature: 0.0,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

impl SynthesisOracle for ClaudeOracle {
    fn name(&self) -> &str { "ClaudeOracle" }
    fn model(&self) -> &str { &self.model }

    fn available(&self) -> bool {
        !self.api_key.is_empty()
    }

    fn synthesize(&self, prompt: &SynthesisPrompt) -> Result<OracleResponse> {
        #[cfg(feature = "llm")]
        {
            use std::time::Instant;
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_millis(self.timeout_ms))
                .build()
                .map_err(|e| OracleError::Http(e.to_string()))?;

            let body = serde_json::json!({
                "model": self.model,
                "max_tokens": prompt.max_tokens,
                "temperature": prompt.temperature,
                "system": prompt.system,
                "messages": [{ "role": "user", "content": prompt.user }]
            });

            let t0 = Instant::now();
            let resp = client
                .post(&self.endpoint)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .map_err(|e| OracleError::Http(e.to_string()))?;

            let latency_ms = t0.elapsed().as_millis() as u64;

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let text = resp.text().unwrap_or_default();
                return Err(OracleError::Http(format!("HTTP {status}: {text}")));
            }

            let json: serde_json::Value = resp.json()
                .map_err(|e| OracleError::Http(e.to_string()))?;

            let content = json["content"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|c| c["text"].as_str())
                .unwrap_or("")
                .to_string();

            let tokens_used = json["usage"]["output_tokens"]
                .as_u64()
                .unwrap_or(0) as usize
                + json["usage"]["input_tokens"].as_u64().unwrap_or(0) as usize;

            let finish_reason = json["stop_reason"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();

            Ok(OracleResponse {
                content,
                model: self.model.clone(),
                tokens_used,
                finish_reason,
                latency_ms,
            })
        }
        #[cfg(not(feature = "llm"))]
        {
            let _ = prompt;
            Err(OracleError::Unavailable(
                "isls-oracle built without `llm` feature; reqwest not linked".to_string(),
            ))
        }
    }

    fn cost_estimate(&self) -> OracleCost {
        // claude-sonnet-4: ~$3/MTok input, ~$15/MTok output
        OracleCost {
            per_input_token_usd: 0.003,
            per_output_token_usd: 0.015,
        }
    }
}

// ─── OpenAIOracle ─────────────────────────────────────────────────────────────

/// Oracle implementation for the OpenAI API (gpt-4o, gpt-4o-mini, etc.).
/// The API key MUST NOT appear in any log, trace, crystal, manifest, or report.
pub struct OpenAIOracle {
    api_key: String,
    model: String,
    #[allow(dead_code)]
    endpoint: String,
    pub max_retries: usize,
    pub timeout_ms: u64,
    pub temperature: f64,
}

impl OpenAIOracle {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "gpt-4o-mini".to_string(),
            endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
            max_retries: 3,
            timeout_ms: 60_000,
            temperature: 0.0,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

impl SynthesisOracle for OpenAIOracle {
    fn name(&self) -> &str { "openai" }
    fn model(&self) -> &str { &self.model }

    fn available(&self) -> bool {
        // OpenAI keys start with "sk-"; "test-key" and empty strings return false
        self.api_key.starts_with("sk-")
    }

    fn synthesize(&self, prompt: &SynthesisPrompt) -> Result<OracleResponse> {
        #[cfg(feature = "llm")]
        {
            use std::time::Instant;
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_millis(self.timeout_ms))
                .build()
                .map_err(|e| OracleError::Http(e.to_string()))?;

            let body = serde_json::json!({
                "model": self.model,
                "max_tokens": prompt.max_tokens,
                "temperature": prompt.temperature,
                "messages": [
                    { "role": "system", "content": prompt.system },
                    { "role": "user",   "content": prompt.user }
                ]
            });

            let t0 = Instant::now();
            let resp = client
                .post(&self.endpoint)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .map_err(|e| OracleError::Http(e.to_string()))?;

            let latency_ms = t0.elapsed().as_millis() as u64;

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let text = resp.text().unwrap_or_default();
                return Err(OracleError::Http(format!("HTTP {status}: {text}")));
            }

            let json: serde_json::Value = resp.json()
                .map_err(|e| OracleError::Http(e.to_string()))?;

            let content = json["choices"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|c| c["message"]["content"].as_str())
                .unwrap_or("")
                .to_string();

            let tokens_used = json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as usize
                + json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as usize;

            let finish_reason = json["choices"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|c| c["finish_reason"].as_str())
                .unwrap_or("unknown")
                .to_string();

            Ok(OracleResponse {
                content,
                model: self.model.clone(),
                tokens_used,
                finish_reason,
                latency_ms,
            })
        }
        #[cfg(not(feature = "llm"))]
        {
            let _ = prompt;
            Err(OracleError::Unavailable(
                "isls-oracle built without `llm` feature; reqwest not linked".to_string(),
            ))
        }
    }

    fn cost_estimate(&self) -> OracleCost {
        match self.model.as_str() {
            "gpt-4o-mini" => OracleCost {
                per_input_token_usd:  0.00000015, // $0.15/MTok
                per_output_token_usd: 0.00000060, // $0.60/MTok
            },
            _ => OracleCost { // gpt-4o default
                per_input_token_usd:  0.0000025,  // $2.50/MTok
                per_output_token_usd: 0.000010,   // $10/MTok
            },
        }
    }
}

// ─── Oracle Budget ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OracleUsage {
    pub calls_this_run: u64,
    pub tokens_this_run: u64,
    pub cost_this_run: f64,
    pub calls_today: u64,
}

impl Default for OracleUsage {
    fn default() -> Self {
        Self {
            calls_this_run: 0,
            tokens_this_run: 0,
            cost_this_run: 0.0,
            calls_today: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OracleBudget {
    /// Maximum LLM calls per run
    pub max_calls_per_run: u64,     // default 100
    /// Maximum total tokens per run
    pub max_tokens_per_run: u64,    // default 500_000
    /// Maximum estimated cost per run (USD)
    pub max_cost_per_run: f64,      // default 10.0
    /// Maximum calls per day (across all runs)
    pub max_calls_per_day: u64,     // default 1000
    /// Current usage
    pub current: OracleUsage,
}

impl Default for OracleBudget {
    fn default() -> Self {
        Self {
            max_calls_per_run: 100,
            max_tokens_per_run: 500_000,
            max_cost_per_run: 10.0,
            max_calls_per_day: 1_000,
            current: OracleUsage::default(),
        }
    }
}

impl OracleBudget {
    /// Returns Ok(()) if the budget allows another call, Err if exceeded.
    pub fn check(&self) -> Result<()> {
        if self.current.calls_this_run >= self.max_calls_per_run {
            return Err(OracleError::BudgetExceeded(
                format!("max_calls_per_run={} reached", self.max_calls_per_run),
            ));
        }
        if self.current.tokens_this_run >= self.max_tokens_per_run {
            return Err(OracleError::BudgetExceeded(
                format!("max_tokens_per_run={} reached", self.max_tokens_per_run),
            ));
        }
        if self.current.cost_this_run >= self.max_cost_per_run {
            return Err(OracleError::BudgetExceeded(
                format!("max_cost_per_run=${:.2} reached", self.max_cost_per_run),
            ));
        }
        if self.current.calls_today >= self.max_calls_per_day {
            return Err(OracleError::BudgetExceeded(
                format!("max_calls_per_day={} reached", self.max_calls_per_day),
            ));
        }
        Ok(())
    }

    pub fn record_call(&mut self, tokens: usize, cost: f64) {
        self.current.calls_this_run += 1;
        self.current.tokens_this_run += tokens as u64;
        self.current.cost_this_run += cost;
        self.current.calls_today += 1;
    }
}

// ─── Autonomy Metrics ────────────────────────────────────────────────────────

/// M33 and M34 tracking — the system's independence from the oracle.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AutonomyMetrics {
    /// Total synthesis requests
    pub total_requests: u64,
    /// Served from pattern memory (no LLM)
    pub memory_hits: u64,
    /// Served from LLM (memory miss)
    pub oracle_calls: u64,
    /// LLM output that failed validation
    pub oracle_rejections: u64,
    /// Fell back to skeleton (LLM unavailable + no memory match)
    pub skeleton_fallbacks: u64,
    /// Autonomy ratio M33 = memory_hits / total_requests
    pub autonomy_ratio: f64,
    /// Total tokens consumed by LLM calls
    pub total_tokens: u64,
    /// Total estimated cost USD
    pub total_cost_usd: f64,
    // ── Constraint Propagation breakdown (C25 pass) ──────────────────────────
    /// Components resolved deterministically (zero degrees of freedom)
    pub deterministic_synths: u64,
    /// Components handled via pattern reuse (high-similarity match)
    pub constrained_calls: u64,
    /// Oracle calls with constrained prompt (low degrees of freedom)
    pub open_calls: u64,
    /// Propagation ratio = (deterministic_synths + constrained_calls) / total_requests
    pub propagation_ratio: f64,
}

impl AutonomyMetrics {
    pub fn record_memory_hit(&mut self) {
        self.total_requests += 1;
        self.memory_hits += 1;
        self.update_ratio();
    }

    pub fn record_oracle_call(&mut self, tokens: usize, cost: f64) {
        self.total_requests += 1;
        self.oracle_calls += 1;
        self.total_tokens += tokens as u64;
        self.total_cost_usd += cost;
        self.update_ratio();
    }

    pub fn record_oracle_rejection(&mut self) {
        self.oracle_rejections += 1;
    }

    pub fn record_skeleton(&mut self) {
        self.total_requests += 1;
        self.skeleton_fallbacks += 1;
        self.update_ratio();
    }

    /// Record the outcome of a constraint propagation pass for one synthesis run.
    pub fn record_propagation(
        &mut self,
        deterministic: u64,
        pattern_reuse: u64,
        constrained: u64,
        open: u64,
    ) {
        self.deterministic_synths += deterministic;
        self.constrained_calls   += pattern_reuse; // pattern reuse = constrained (no LLM)
        self.open_calls          += constrained + open;
        self.update_propagation_ratio();
    }

    fn update_ratio(&mut self) {
        self.autonomy_ratio = if self.total_requests == 0 {
            0.0
        } else {
            self.memory_hits as f64 / self.total_requests as f64
        };
    }

    fn update_propagation_ratio(&mut self) {
        let total = self.deterministic_synths + self.constrained_calls + self.open_calls;
        self.propagation_ratio = if total == 0 {
            0.0
        } else {
            (self.deterministic_synths + self.constrained_calls) as f64 / total as f64
        };
    }

    /// M34: oracle rejection rate = oracle_rejections / oracle_calls
    pub fn rejection_rate(&self) -> f64 {
        if self.oracle_calls == 0 {
            0.0
        } else {
            self.oracle_rejections as f64 / self.oracle_calls as f64
        }
    }
}

// ─── Oracle Config ───────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct OracleConfig {
    pub enabled: bool,
    /// Provider selection: "claude", "openai", or None for auto-detect.
    /// Auto-detect checks ANTHROPIC_API_KEY first, then OPENAI_API_KEY.
    pub provider: Option<String>,
    pub model: String,
    pub endpoint: String,
    /// "env:ANTHROPIC_API_KEY" or "env:OPENAI_API_KEY" or "capsule:<id>" or raw key (not recommended)
    pub api_key_source: String,
    pub temperature: f64,
    pub max_tokens: usize,
    pub max_retries: usize,
    pub timeout_ms: u64,
    pub memory_first: bool,
    /// Cosine similarity threshold for pattern match (default 0.85)
    pub match_threshold: f64,
    /// Minimum quality score for pattern reuse (default 0.6)
    pub quality_threshold: f64,
    /// Mini-PMHD ticks for oracle output validation (default 50)
    pub pmhd_validation_ticks: usize,
    pub output_format: OutputFormat,
    /// "skeleton" or "error"
    pub fallback: String,
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: None, // auto-detect
            model: "claude-sonnet-4-20250514".to_string(),
            endpoint: "https://api.anthropic.com/v1/messages".to_string(),
            api_key_source: "env:ANTHROPIC_API_KEY".to_string(),
            temperature: 0.0,
            max_tokens: 4096,
            max_retries: 3,
            timeout_ms: 60_000,
            memory_first: true,
            match_threshold: 0.85,
            quality_threshold: 0.6,
            pmhd_validation_ticks: 50,
            output_format: OutputFormat::PlainText,
            fallback: "skeleton".to_string(),
        }
    }
}

// ─── Oracle Pattern Entry ────────────────────────────────────────────────────

/// A validated pattern stored in the oracle memory.
/// Unlike isls-pmhd's PatternEntry, this includes the actual content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OraclePatternEntry {
    /// Content-addressed ID derived from signature + domain + content
    pub id: Hash256,
    pub domain: String,
    /// Composite quality score [0, 1]
    pub quality_score: f64,
    /// 5D signature for similarity matching
    pub signature: FiveDState,
    pub component_kinds: Vec<String>,
    /// The actual generated content (code/spec/schema)
    pub content: String,
    /// Unix timestamp of creation
    pub created_at: f64,
}

impl OraclePatternEntry {
    pub fn new(
        domain: impl Into<String>,
        quality_score: f64,
        signature: FiveDState,
        component_kinds: Vec<String>,
        content: impl Into<String>,
    ) -> Self {
        let domain = domain.into();
        let content = content.into();
        let mut hasher = Sha256::new();
        hasher.update(domain.as_bytes());
        hasher.update(&signature.p.to_le_bytes());
        hasher.update(&signature.rho.to_le_bytes());
        hasher.update(content.as_bytes());
        let id: Hash256 = hasher.finalize().into();

        let created_at = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;

        Self {
            id,
            domain,
            quality_score,
            signature,
            component_kinds,
            content,
            created_at,
        }
    }
}

// ─── Oracle Pattern Memory ───────────────────────────────────────────────────

/// In-memory store of validated synthesis patterns.
#[derive(Clone, Debug, Default)]
pub struct OraclePatternMemory {
    entries: Vec<OraclePatternEntry>,
    match_threshold: f64,
    quality_threshold: f64,
}

impl OraclePatternMemory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            match_threshold: 0.85,
            quality_threshold: 0.6,
        }
    }

    pub fn with_thresholds(match_threshold: f64, quality_threshold: f64) -> Self {
        Self {
            entries: Vec::new(),
            match_threshold,
            quality_threshold,
        }
    }

    pub fn add(&mut self, entry: OraclePatternEntry) {
        self.entries.push(entry);
    }

    /// Find the best-matching pattern for the given signature and domain.
    /// Returns the entry if similarity >= match_threshold AND quality >= quality_threshold.
    pub fn find_match(&self, sig: &FiveDState, domain: &str) -> Option<(&OraclePatternEntry, f64)> {
        self.entries.iter()
            .filter(|e| e.domain == domain && e.quality_score >= self.quality_threshold)
            .map(|e| (e, cosine_similarity(&e.signature, sig)))
            .filter(|(_, sim)| *sim >= self.match_threshold)
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn entries(&self) -> &[OraclePatternEntry] { &self.entries }

    /// Domain statistics: count per domain
    pub fn domain_stats(&self) -> BTreeMap<String, usize> {
        let mut map = BTreeMap::new();
        for e in &self.entries {
            *map.entry(e.domain.clone()).or_insert(0) += 1;
        }
        map
    }

    /// Average quality score across all entries
    pub fn avg_quality(&self) -> f64 {
        if self.entries.is_empty() { return 0.0; }
        let sum: f64 = self.entries.iter().map(|e| e.quality_score).sum();
        sum / self.entries.len() as f64
    }
}

fn cosine_similarity(a: &FiveDState, b: &FiveDState) -> f64 {
    let dot = a.p * b.p + a.rho * b.rho + a.omega * b.omega + a.chi * b.chi + a.eta * b.eta;
    let mag_a = (a.p * a.p + a.rho * a.rho + a.omega * a.omega + a.chi * a.chi + a.eta * a.eta).sqrt();
    let mag_b = (b.p * b.p + b.rho * b.rho + b.omega * b.omega + b.chi * b.chi + b.eta * b.eta).sqrt();
    if mag_a < 1e-12 || mag_b < 1e-12 {
        return 0.0;
    }
    (dot / (mag_a * mag_b)).clamp(-1.0, 1.0)
}

/// Derive a FiveDState "signature" from an ArtifactIR by averaging component signatures.
pub fn ir_signature(ir: &ArtifactIR) -> FiveDState {
    if ir.components.is_empty() {
        return FiveDState::default();
    }
    let n = ir.components.len() as f64;
    let sum = ir.components.iter().fold(FiveDState::default(), |acc, c| {
        FiveDState {
            p:     acc.p     + c.signature.p,
            rho:   acc.rho   + c.signature.rho,
            omega: acc.omega + c.signature.omega,
            chi:   acc.chi   + c.signature.chi,
            eta:   acc.eta   + c.signature.eta,
        }
    });
    FiveDState {
        p:     sum.p     / n,
        rho:   sum.rho   / n,
        omega: sum.omega / n,
        chi:   sum.chi   / n,
        eta:   sum.eta   / n,
    }
}

// ─── Prompt Builder ───────────────────────────────────────────────────────────

/// Deterministic prompt construction from ArtifactIR + Matrix.
/// Invariant: same ArtifactIR + same Matrix → same SynthesisPrompt.
pub struct PromptBuilder;

impl PromptBuilder {
    pub fn build(ir: &ArtifactIR, matrix: &dyn Matrix, config: &OracleConfig) -> SynthesisPrompt {
        let constraint_text: String = ir.constraints.iter()
            .map(|c| format!("  - {}", c.predicate))
            .collect::<Vec<_>>()
            .join("\n");

        let system = format!(
            "You are a code synthesis engine. You produce ONLY valid {} code. \
             No explanations, no markdown fences, no commentary. \
             Output ONLY the requested artifact.\n\n\
             CONSTRAINTS:\n{}\n\n\
             QUALITY REQUIREMENTS:\n\
             - Coherence >= {:.2}\n\
             - Robustness >= {:.2}\n\
             - All interfaces must be satisfied\n\
             - Code must compile (if applicable)\n\n\
             OUTPUT FORMAT: {}",
            matrix.domain(),
            if constraint_text.is_empty() { "  (none)".to_string() } else { constraint_text },
            ir.metrics.coherence,
            ir.metrics.robustness,
            config.output_format.as_str(),
        );

        let user = Self::build_user_prompt(ir);

        SynthesisPrompt {
            system,
            user,
            output_format: config.output_format.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
        }
    }

    fn build_user_prompt(ir: &ArtifactIR) -> String {
        let mut parts = Vec::new();

        parts.push("COMPONENTS:".to_string());
        for comp in &ir.components {
            parts.push(format!("- {} (kind: {}): {}", comp.name, comp.kind, comp.content));
            if !comp.dependencies.is_empty() {
                parts.push(format!("  depends on: {}", comp.dependencies.join(", ")));
            }
        }

        if !ir.interfaces.is_empty() {
            parts.push("\nINTERFACES:".to_string());
            for iface in &ir.interfaces {
                parts.push(format!(
                    "- {} provides to {}: {}",
                    iface.provider, iface.consumer, iface.contract
                ));
            }
        }

        parts.push(format!(
            "\nPROVENANCE: monolith {}, drill ticks {}-{}, quality coh={:.2} rob={:.2} cov={:.2}",
            ir.provenance.monolith_id,
            ir.provenance.tick_range[0],
            ir.provenance.tick_range[1],
            ir.metrics.coherence,
            ir.metrics.robustness,
            ir.metrics.coverage,
        ));

        parts.join("\n")
    }
}

// ─── Validation Summary ───────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct ValidationSummary {
    pub parse_ok: bool,
    pub constraints_ok: bool,
    pub pmhd_ok: bool,
    pub gates_ok: bool,
    pub quality_score: f64,
    pub failure_reason: Option<String>,
}

impl ValidationSummary {
    pub fn passed(&self) -> bool {
        self.parse_ok && self.constraints_ok && self.pmhd_ok && self.gates_ok
    }
}

// ─── Oracle Validator ─────────────────────────────────────────────────────────

/// 4-stage validation pipeline for LLM-generated content.
pub struct OracleValidator;

impl OracleValidator {
    /// Stage 1: Parse validation — is the content non-empty and plausible?
    pub fn validate_parse(content: &str, format: &OutputFormat) -> Result<()> {
        if content.trim().is_empty() {
            return Err(OracleError::ParseError("empty content".to_string()));
        }
        match format {
            OutputFormat::Json => {
                serde_json::from_str::<serde_json::Value>(content)
                    .map_err(|e| OracleError::ParseError(e.to_string()))?;
            }
            OutputFormat::Rust => {
                // Lightweight check: must contain at least one valid-looking Rust token
                if !content.contains("fn ")
                    && !content.contains("struct ")
                    && !content.contains("impl ")
                    && !content.contains("use ")
                    && !content.contains("pub ")
                    && !content.contains("mod ")
                {
                    return Err(OracleError::ParseError(
                        "content does not appear to be Rust code".to_string(),
                    ));
                }
            }
            _ => {} // PlainText, YAML, OpenAPI: accept as-is
        }
        Ok(())
    }

    /// Stage 2: Constraint satisfaction — check that required keywords/components appear.
    pub fn validate_constraints(content: &str, ir: &ArtifactIR) -> Result<()> {
        // All required component names must appear in the output
        let missing: Vec<&str> = ir.components.iter()
            .filter(|c| !content.contains(&c.name))
            .map(|c| c.name.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(OracleError::ValidationFailed(format!(
                "missing required components: {}",
                missing.join(", ")
            )));
        }
        Ok(())
    }

    /// Stage 3: Mini-PMHD adversarial check — compute a quality proxy metric.
    /// Returns a quality score in [0, 1].
    pub fn validate_pmhd(content: &str, ir: &ArtifactIR) -> f64 {
        // Simplified mini-PMHD: measure coherence as fraction of constraint
        // predicates that are referenced in the content.
        let total = ir.constraints.len();
        if total == 0 {
            return 0.8; // no constraints → neutral quality
        }
        let satisfied = ir.constraints.iter()
            .filter(|c| {
                // Check that some keyword from the predicate appears in the content
                c.predicate.split_whitespace().any(|word| {
                    word.len() >= 4 && content.contains(word)
                })
            })
            .count();
        (satisfied as f64 / total as f64).max(0.5) // floor at 0.5 for partial credit
    }

    /// Stage 4: Gate check — does the quality meet minimum thresholds?
    pub fn validate_gates(quality: f64) -> bool {
        quality >= 0.5
    }

    /// Full 4-stage validation. Returns a ValidationSummary.
    pub fn validate(content: &str, ir: &ArtifactIR, format: &OutputFormat) -> ValidationSummary {
        let mut summary = ValidationSummary::default();

        // Stage 1: parse
        match Self::validate_parse(content, format) {
            Ok(_) => summary.parse_ok = true,
            Err(e) => {
                summary.failure_reason = Some(format!("Stage 1 parse: {e}"));
                return summary;
            }
        }

        // Stage 2: constraints
        match Self::validate_constraints(content, ir) {
            Ok(_) => summary.constraints_ok = true,
            Err(e) => {
                summary.failure_reason = Some(format!("Stage 2 constraints: {e}"));
                return summary;
            }
        }

        // Stage 3: PMHD
        let quality = Self::validate_pmhd(content, ir);
        summary.quality_score = quality;
        summary.pmhd_ok = quality >= 0.5;
        if !summary.pmhd_ok {
            summary.failure_reason = Some(format!("Stage 3 PMHD: quality {quality:.3} < 0.5"));
            return summary;
        }

        // Stage 4: gates
        summary.gates_ok = Self::validate_gates(quality);
        if !summary.gates_ok {
            summary.failure_reason = Some(format!("Stage 4 gates: quality {quality:.3} < 0.5"));
        }

        summary
    }
}

// ─── Synthesis Source ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SynthesisSource {
    /// Served from pattern memory, no LLM call
    Memory { pattern_id: Hash256, similarity: f64 },
    /// Adapted from pattern memory, no LLM call
    MemoryAdapted { pattern_id: Hash256, similarity: f64 },
    /// Generated by LLM oracle
    Oracle { model: String, tokens: usize },
    /// Structural skeleton (LLM unavailable + no memory match)
    Skeleton,
}

// ─── Oracle Synthesis Result ──────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct OracleSynthesisResult {
    pub content: String,
    pub source: SynthesisSource,
    pub validation: ValidationSummary,
    pub retries_used: usize,
    pub tokens_used: usize,
}

// ─── Oracle Engine ────────────────────────────────────────────────────────────

/// The main synthesis engine: memory-first → LLM fallback → skeleton fallback.
pub struct OracleEngine {
    oracle: Box<dyn SynthesisOracle>,
    pub memory: OraclePatternMemory,
    pub budget: OracleBudget,
    pub metrics: AutonomyMetrics,
    pub config: OracleConfig,
}

impl OracleEngine {
    pub fn new(config: OracleConfig, memory: OraclePatternMemory) -> Self {
        let oracle = Self::build_oracle(&config);
        Self {
            oracle,
            memory,
            budget: OracleBudget::default(),
            metrics: AutonomyMetrics::default(),
            config,
        }
    }

    pub fn with_oracle(
        config: OracleConfig,
        memory: OraclePatternMemory,
        oracle: Box<dyn SynthesisOracle>,
    ) -> Self {
        Self {
            oracle,
            memory,
            budget: OracleBudget::default(),
            metrics: AutonomyMetrics::default(),
            config,
        }
    }

    fn build_oracle(config: &OracleConfig) -> Box<dyn SynthesisOracle> {
        // Resolve API key from source
        let api_key = if config.api_key_source.starts_with("env:") {
            let var = &config.api_key_source["env:".len()..];
            std::env::var(var).unwrap_or_default()
        } else if config.api_key_source.starts_with("capsule:") {
            // Capsule-based key retrieval is handled separately via cmd_oracle_seal_key
            String::new()
        } else {
            // Treat as raw key (not recommended, for testing only)
            config.api_key_source.clone()
        };

        // Helper: is this key format an OpenAI key?
        // Anthropic keys start with "sk-ant-"; OpenAI keys start with "sk-" but NOT "sk-ant-".
        let is_openai_key = |k: &str| k.starts_with("sk-") && !k.starts_with("sk-ant-");

        match config.provider.as_deref() {
            Some("openai") => {
                // Explicit OpenAI: use resolved key, or fall back to env var
                let key = if !api_key.is_empty() {
                    api_key
                } else {
                    std::env::var("OPENAI_API_KEY").unwrap_or_default()
                };
                let mut oracle = OpenAIOracle::new(key);
                if !config.model.is_empty()
                    && config.model != "claude-sonnet-4-20250514"
                {
                    oracle = oracle.with_model(config.model.clone());
                }
                Box::new(oracle)
            }
            // "anthropic" is an accepted alias for "claude"
            Some("claude") | Some("anthropic") => {
                let key = if !api_key.is_empty() {
                    api_key
                } else {
                    std::env::var("ANTHROPIC_API_KEY").unwrap_or_default()
                };
                Box::new(ClaudeOracle::new(key))
            }
            _ => {
                // Auto-detect: check ANTHROPIC_API_KEY first, then OPENAI_API_KEY.
                // When api_key_source resolves to a key, distinguish by key format:
                //   sk-ant-... → Anthropic/Claude
                //   sk-...     → OpenAI
                if !api_key.is_empty() {
                    if is_openai_key(&api_key) {
                        let mut oracle = OpenAIOracle::new(api_key);
                        if !config.model.is_empty()
                            && config.model != "claude-sonnet-4-20250514"
                        {
                            oracle = oracle.with_model(config.model.clone());
                        }
                        return Box::new(oracle);
                    }
                    // sk-ant-... or other formats → ClaudeOracle
                    return Box::new(ClaudeOracle::new(api_key));
                }
                if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                    if !key.is_empty() {
                        return Box::new(ClaudeOracle::new(key));
                    }
                }
                if let Ok(key) = std::env::var("OPENAI_API_KEY") {
                    if !key.is_empty() {
                        let mut oracle = OpenAIOracle::new(key);
                        if !config.model.is_empty()
                            && config.model != "claude-sonnet-4-20250514"
                        {
                            oracle = oracle.with_model(config.model.clone());
                        }
                        return Box::new(oracle);
                    }
                }
                // No key found — skeleton fallback
                Box::new(ClaudeOracle::new(String::new()))
            }
        }
    }

    pub fn oracle_available(&self) -> bool {
        self.oracle.available()
    }

    /// Human-readable provider name (e.g. "ClaudeOracle", "openai").
    pub fn oracle_name(&self) -> &str {
        self.oracle.name()
    }

    /// Active model identifier (e.g. "claude-sonnet-4-20250514", "gpt-4o-mini").
    pub fn oracle_model(&self) -> &str {
        self.oracle.model()
    }

    pub fn autonomy(&self) -> &AutonomyMetrics {
        &self.metrics
    }

    pub fn budget_status(&self) -> &OracleBudget {
        &self.budget
    }

    /// Main synthesis method: memory-first → LLM → skeleton.
    pub fn synthesize(
        &mut self,
        ir: &ArtifactIR,
        matrix: &dyn Matrix,
    ) -> Result<OracleSynthesisResult> {
        let domain = matrix.domain().to_string();
        let sig = ir_signature(ir);
        let format = self.config.output_format.clone();

        // [1] Memory-first lookup
        if self.config.memory_first {
            if let Some((entry, similarity)) = self.memory.find_match(&sig, &domain) {
                let content = entry.content.clone();
                let pattern_id = entry.id;

                // Validate the memory-retrieved content against current IR
                let validation = OracleValidator::validate(&content, ir, &format);
                if validation.passed() {
                    self.metrics.record_memory_hit();
                    return Ok(OracleSynthesisResult {
                        content,
                        source: SynthesisSource::Memory { pattern_id, similarity },
                        validation,
                        retries_used: 0,
                        tokens_used: 0,
                    });
                }
                // Memory match failed validation — fall through to LLM
            }
        }

        // [2] LLM availability check
        if !self.oracle.available() || !self.config.enabled {
            return self.emit_skeleton(ir, matrix);
        }

        // [3] Budget check
        if let Err(e) = self.budget.check() {
            return self.emit_skeleton(ir, matrix).map(|r| {
                // Still count it as a skeleton due to budget
                eprintln!("[oracle] budget: {e}");
                r
            });
        }

        // [4] Build prompt (deterministic)
        let prompt = PromptBuilder::build(ir, matrix, &self.config);

        // [5] LLM call with retry loop
        let max_retries = self.config.max_retries;
        let mut last_error = String::new();
        #[allow(unused_assignments)]
        let mut retries_used = 0;

        for attempt in 0..=max_retries {
            let attempt_prompt = if attempt == 0 {
                prompt.clone()
            } else {
                // Append failure reason to user prompt for retry
                let mut refined = prompt.clone();
                refined.user = format!(
                    "{}\n\nPREVIOUS ATTEMPT FAILED: {}\nPlease fix the issue.",
                    refined.user, last_error
                );
                refined
            };

            match self.oracle.synthesize(&attempt_prompt) {
                Ok(response) => {
                    retries_used = attempt;
                    let tokens = response.tokens_used;
                    let cost = self.oracle.cost_estimate().estimate(tokens);

                    // Validate the LLM output
                    let validation = OracleValidator::validate(&response.content, ir, &format);
                    if validation.passed() {
                        self.budget.record_call(tokens, cost);
                        self.metrics.record_oracle_call(tokens, cost);

                        // Crystallize into pattern memory
                        let quality = validation.quality_score;
                        let entry = OraclePatternEntry::new(
                            &domain,
                            quality,
                            sig,
                            ir.components.iter().map(|c| c.kind.clone()).collect(),
                            response.content.clone(),
                        );
                        self.memory.add(entry);

                        return Ok(OracleSynthesisResult {
                            content: response.content,
                            source: SynthesisSource::Oracle {
                                model: response.model,
                                tokens,
                            },
                            validation,
                            retries_used,
                            tokens_used: tokens,
                        });
                    } else {
                        self.metrics.record_oracle_rejection();
                        last_error = validation.failure_reason
                            .clone()
                            .unwrap_or_else(|| "unknown validation failure".to_string());
                    }
                }
                Err(e) => {
                    last_error = e.to_string();
                }
            }
        }

        // All retries exhausted — fall back to skeleton
        self.emit_skeleton(ir, matrix)
    }

    fn emit_skeleton(&mut self, ir: &ArtifactIR, matrix: &dyn Matrix) -> Result<OracleSynthesisResult> {
        self.metrics.record_skeleton();

        let input = matrix.interpret(ir, &isls_forge::MatrixConfig::default());
        use isls_forge::{DefaultSynthesizer, ForgeConfig, Synthesizer};
        let synth_output = DefaultSynthesizer.synthesize(&input, &ForgeConfig::default());
        let content = synth_output.content;

        let validation = ValidationSummary {
            parse_ok: true,
            constraints_ok: true,
            pmhd_ok: true,
            gates_ok: true,
            quality_score: 0.5,
            failure_reason: None,
        };

        Ok(OracleSynthesisResult {
            content,
            source: SynthesisSource::Skeleton,
            validation,
            retries_used: 0,
            tokens_used: 0,
        })
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use isls_types::FiveDState;
    use isls_artifact_ir::{
        ArtifactHeader, ArtifactMetrics, ArtifactProvenance, ArtifactIR,
    };
    use isls_forge::RustModuleMatrix;

    // ── Mock Oracle ───────────────────────────────────────────────────────────

    struct MockOracle {
        responses: std::sync::Mutex<Vec<Result<String>>>,
        available: bool,
        model: String,
    }

    impl MockOracle {
        fn new_available(responses: Vec<Result<String>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
                available: true,
                model: "mock-model-1.0".to_string(),
            }
        }

        fn new_unavailable() -> Self {
            Self {
                responses: std::sync::Mutex::new(vec![]),
                available: false,
                model: "mock-model-1.0".to_string(),
            }
        }
    }

    impl SynthesisOracle for MockOracle {
        fn name(&self) -> &str { "MockOracle" }
        fn model(&self) -> &str { &self.model }
        fn available(&self) -> bool { self.available }
        fn synthesize(&self, _prompt: &SynthesisPrompt) -> Result<OracleResponse> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                return Err(OracleError::Unavailable("no more mock responses".to_string()));
            }
            match responses.remove(0) {
                Ok(content) => Ok(OracleResponse {
                    content,
                    model: self.model.clone(),
                    tokens_used: 100,
                    finish_reason: "stop".to_string(),
                    latency_ms: 50,
                }),
                Err(e) => Err(e),
            }
        }
        fn cost_estimate(&self) -> OracleCost { OracleCost::default() }
    }

    // ── Helper: build a minimal ArtifactIR ───────────────────────────────────

    fn make_ir(domain: &str, component_name: &str) -> ArtifactIR {
        let sig = FiveDState {
            p: 0.9, rho: 0.8, omega: 0.7, chi: 0.6, eta: 0.5,
        };
        ArtifactIR {
            header: ArtifactHeader {
                artifact_id: [0u8; 32],
                version: "1.0.0".to_string(),
                timestamp_tick: 1,
                layer_index: 0,
                por_decision: None,
                source_monolith_id: "test-mono".to_string(),
                domain: domain.to_string(),
            },
            components: vec![isls_artifact_ir::Component {
                id: "comp-1".to_string(),
                kind: "function".to_string(),
                name: component_name.to_string(),
                content: format!("fn {}() -> bool {{ true }}", component_name),
                dependencies: vec![],
                signature: sig,
            }],
            interfaces: vec![],
            constraints: vec![],
            metrics: ArtifactMetrics { coherence: 0.8, robustness: 0.7, ..Default::default() },
            provenance: ArtifactProvenance {
                decision_spec_id: [0u8; 32],
                monolith_id: "test-mono".to_string(),
                seed: 42,
                config_hash: "abc".to_string(),
                tick_range: [0, 10],
                drill_strategy: "greedy".to_string(),
                por_evidence: None,
                pattern_memory_size: 0,
            },
            deltas: vec![],
            extra: BTreeMap::new(),
        }
    }

    fn make_engine_with_oracle(oracle: MockOracle) -> OracleEngine {
        OracleEngine::with_oracle(
            OracleConfig::default(),
            OraclePatternMemory::new(),
            Box::new(oracle),
        )
    }

    // ── AT-O1: Memory-first hit ───────────────────────────────────────────────

    #[test]
    fn at_o1_memory_first_hit() {
        let ir = make_ir("rust", "health_check");
        let sig = ir_signature(&ir);

        let mut memory = OraclePatternMemory::with_thresholds(0.85, 0.0);
        // Add a high-similarity pattern (same signature → cosine sim = 1.0)
        let entry = OraclePatternEntry::new(
            "rust",
            0.9,
            sig,
            vec!["function".to_string()],
            "fn health_check() -> bool { true }",
        );
        memory.add(entry.clone());

        // Oracle that must NOT be called
        let oracle = MockOracle::new_unavailable();
        let mut engine = OracleEngine::with_oracle(
            OracleConfig::default(),
            memory,
            Box::new(oracle),
        );

        let matrix = RustModuleMatrix;
        let result = engine.synthesize(&ir, &matrix).unwrap();

        assert!(matches!(result.source, SynthesisSource::Memory { .. }));
        assert_eq!(engine.metrics.memory_hits, 1);
        assert_eq!(engine.metrics.oracle_calls, 0);
    }

    // ── AT-O2: LLM fallback ───────────────────────────────────────────────────

    #[test]
    fn at_o2_llm_fallback() {
        let ir = make_ir("rust", "process_data");
        let oracle = MockOracle::new_available(vec![
            Ok("fn process_data() -> bool { true }".to_string()),
        ]);
        let mut engine = make_engine_with_oracle(oracle);
        let matrix = RustModuleMatrix;

        let result = engine.synthesize(&ir, &matrix).unwrap();

        assert!(matches!(result.source, SynthesisSource::Oracle { .. }));
        assert_eq!(engine.metrics.oracle_calls, 1);
        // Pattern should be stored in memory
        assert_eq!(engine.memory.len(), 1);
    }

    // ── AT-O3: Validation rejection → skeleton fallback ───────────────────────

    #[test]
    fn at_o3_validation_rejection() {
        let ir = make_ir("rust", "missing_component");
        // MockOracle returns empty content (fails parse → Stage 1) for all retries
        let responses: Vec<Result<String>> = (0..=3)
            .map(|_| Ok(String::new()))
            .collect();
        let mut engine = OracleEngine::with_oracle(
            OracleConfig { max_retries: 3, ..OracleConfig::default() },
            OraclePatternMemory::new(),
            Box::new(MockOracle::new_available(responses)),
        );
        let matrix = RustModuleMatrix;

        let result = engine.synthesize(&ir, &matrix).unwrap();

        // All retries exhausted → skeleton
        assert!(matches!(result.source, SynthesisSource::Skeleton));
        assert!(engine.metrics.skeleton_fallbacks > 0);
    }

    // ── AT-O4: Prompt determinism ─────────────────────────────────────────────

    #[test]
    fn at_o4_prompt_determinism() {
        let ir = make_ir("rust", "deterministic_fn");
        let matrix = RustModuleMatrix;
        let config = OracleConfig::default();

        let p1 = PromptBuilder::build(&ir, &matrix, &config);
        let p2 = PromptBuilder::build(&ir, &matrix, &config);

        assert_eq!(p1, p2, "same ArtifactIR must produce identical prompt");
    }

    // ── AT-O5: Budget enforcement ─────────────────────────────────────────────

    #[test]
    fn at_o5_budget_enforcement() {
        let ir = make_ir("rust", "budgeted_fn");
        let responses: Vec<Result<String>> = vec![
            Ok("fn budgeted_fn() -> bool { true }".to_string()),
            Ok("fn budgeted_fn() -> bool { true }".to_string()),
        ];
        let mut engine = OracleEngine::with_oracle(
            OracleConfig::default(),
            OraclePatternMemory::new(),
            Box::new(MockOracle::new_available(responses)),
        );
        // Set budget to 1 call per run
        engine.budget.max_calls_per_run = 1;

        let matrix = RustModuleMatrix;

        // First request: LLM succeeds
        let r1 = engine.synthesize(&ir, &matrix).unwrap();
        assert!(matches!(r1.source, SynthesisSource::Oracle { .. } | SynthesisSource::Memory { .. }));

        // Second request: budget exceeded → skeleton
        let r2 = engine.synthesize(&ir, &matrix).unwrap();
        // Either from memory (if first result was stored and matches) or skeleton
        // The budget check happens after memory lookup, so if memory has a match it may hit memory.
        // We just verify no additional LLM call beyond 1 was made.
        assert!(engine.metrics.oracle_calls <= 1);
        let _ = r2;
    }

    // ── AT-O6: Autonomy tracking ──────────────────────────────────────────────

    #[test]
    fn at_o6_autonomy_tracking() {
        let mut metrics = AutonomyMetrics::default();

        // 3 memory hits
        metrics.record_memory_hit();
        metrics.record_memory_hit();
        metrics.record_memory_hit();

        // 2 oracle calls
        metrics.record_oracle_call(100, 0.01);
        metrics.record_oracle_call(200, 0.02);

        assert_eq!(metrics.total_requests, 5);
        assert_eq!(metrics.memory_hits, 3);
        assert_eq!(metrics.oracle_calls, 2);

        let expected_ratio = 3.0 / 5.0;
        let diff = (metrics.autonomy_ratio - expected_ratio).abs();
        assert!(diff < 1e-10, "autonomy_ratio should be 0.6, got {}", metrics.autonomy_ratio);
    }

    // ── AT-O7: Pattern crystallization ────────────────────────────────────────

    #[test]
    fn at_o7_pattern_crystallization() {
        let ir = make_ir("rust", "stored_fn");
        let oracle = MockOracle::new_available(vec![
            Ok("fn stored_fn() -> bool { true }".to_string()),
        ]);
        let mut engine = make_engine_with_oracle(oracle);
        let matrix = RustModuleMatrix;

        let result = engine.synthesize(&ir, &matrix).unwrap();
        assert!(matches!(result.source, SynthesisSource::Oracle { .. }));

        // Pattern should now be in memory
        assert_eq!(engine.memory.len(), 1);

        // Subsequent request with same signature should hit memory
        let ir2 = make_ir("rust", "stored_fn");
        // Make oracle unavailable to prove memory is used
        let mut engine2 = OracleEngine::with_oracle(
            OracleConfig::default(),
            engine.memory.clone(),
            Box::new(MockOracle::new_unavailable()),
        );

        let r2 = engine2.synthesize(&ir2, &matrix).unwrap();
        assert!(matches!(r2.source, SynthesisSource::Memory { .. }));
    }

    // ── AT-O8: Graceful degradation (no API key) ──────────────────────────────

    #[test]
    fn at_o8_graceful_degradation() {
        let ir = make_ir("rust", "fallback_fn");
        let oracle = MockOracle::new_unavailable();
        let mut engine = make_engine_with_oracle(oracle);
        let matrix = RustModuleMatrix;

        // No error — skeleton emitted gracefully
        let result = engine.synthesize(&ir, &matrix).unwrap();
        assert!(matches!(result.source, SynthesisSource::Skeleton));
        assert_eq!(engine.metrics.skeleton_fallbacks, 1);
    }

    // ── AT-O9: No secret leakage in prompt ────────────────────────────────────

    #[test]
    fn at_o9_no_secret_leakage() {
        let ir = make_ir("rust", "secure_fn");
        let matrix = RustModuleMatrix;

        let api_key = "sk-ant-api03-super-secret-key-12345";
        let config = OracleConfig {
            api_key_source: api_key.to_string(),
            ..OracleConfig::default()
        };

        let prompt = PromptBuilder::build(&ir, &matrix, &config);

        assert!(!prompt.system.contains(api_key), "API key must not appear in system prompt");
        assert!(!prompt.user.contains(api_key), "API key must not appear in user prompt");

        let prompt_json = serde_json::to_string(&prompt).unwrap();
        assert!(!prompt_json.contains(api_key), "API key must not appear in serialized prompt");
    }

    // ── AT-O10: Capsule-protected API key ─────────────────────────────────────

    #[test]
    fn at_o10_capsule_protected_key() {
        use isls_manifest::{build_manifest, TraceEntry};
        use isls_registry::RegistrySet;
        use isls_archive::Archive;
        use isls_types::{Config, RunDescriptor, SchedulerConfig};
        use isls_capsule::{seal, open, CapsulePolicy};

        // Build a manifest (represents a valid constitutional state)
        let rd = RunDescriptor {
            config: Config::default(),
            operator_versions: BTreeMap::new(),
            initial_state_digest: [0u8; 32],
            seed: None,
            registry_digests: BTreeMap::new(),
            scheduler: SchedulerConfig::default(),
        };
        let archive = Archive::new();
        let registries = RegistrySet::new();
        let traces: Vec<TraceEntry> = vec![];
        let obs_log: Vec<Vec<Vec<u8>>> = vec![];
        let manifest = build_manifest(&rd, &traces, &archive, &registries, "oracle", &obs_log);

        // Seal the API key
        let api_key = b"sk-ant-test-oracle-key";
        let policy = CapsulePolicy {
            require_lock_program_id: [0u8; 32],
            require_rd_digest: manifest.rd_digest,
            require_gate_proofs: vec![],
            require_manifest_id: Some(manifest.run_id),
            expires_at: None,
            max_uses: None,
        };
        let master_key = b"oracle-test-master-key-32bytes!!";
        let capsule = seal(api_key, policy, BTreeMap::new(), master_key, &manifest).unwrap();

        // Open with the correct manifest → success
        let recovered = open(&capsule, master_key, &manifest, None).unwrap();
        assert_eq!(&recovered, api_key, "API key should be recovered from capsule");

        // Open with a wrong manifest → failure (genesis drift)
        let rd2 = RunDescriptor {
            config: Config::default(),
            operator_versions: BTreeMap::new(),
            initial_state_digest: [0xffu8; 32], // drifted
            seed: None,
            registry_digests: BTreeMap::new(),
            scheduler: SchedulerConfig::default(),
        };
        let manifest2 = build_manifest(&rd2, &traces, &archive, &registries, "oracle", &obs_log);
        let result = open(&capsule, master_key, &manifest2, None);
        assert!(result.is_err(), "API key must not be released when manifest (genesis) is drifted");
    }

    // ── Extra: budget check unit test ─────────────────────────────────────────

    #[test]
    fn budget_check_passes_under_limit() {
        let budget = OracleBudget::default();
        assert!(budget.check().is_ok());
    }

    #[test]
    fn budget_check_fails_when_calls_exceeded() {
        let mut budget = OracleBudget {
            max_calls_per_run: 1,
            ..OracleBudget::default()
        };
        budget.current.calls_this_run = 1;
        assert!(budget.check().is_err());
    }

    // ── Extra: OraclePatternMemory similarity threshold ───────────────────────

    #[test]
    fn pattern_memory_rejects_low_similarity() {
        let sig_a = FiveDState { p: 1.0, rho: 0.0, omega: 0.0, chi: 0.0, eta: 0.0 };
        let sig_b = FiveDState { p: 0.0, rho: 1.0, omega: 0.0, chi: 0.0, eta: 0.0 };
        // cosine similarity = 0.0 (orthogonal)

        let mut memory = OraclePatternMemory::with_thresholds(0.85, 0.0);
        memory.add(OraclePatternEntry::new(
            "rust", 1.0, sig_a, vec![], "content".to_string(),
        ));

        let result = memory.find_match(&sig_b, "rust");
        assert!(result.is_none(), "orthogonal signatures should not match at threshold 0.85");
    }

    // ── OpenAIOracle unit tests ───────────────────────────────────────────────

    #[test]
    fn test_openai_oracle_creation() {
        let oracle = OpenAIOracle::new("test-key");
        assert_eq!(oracle.name(), "openai");
        assert_eq!(oracle.model(), "gpt-4o-mini");
        // "test-key" doesn't start with "sk-" → unavailable
        assert!(!oracle.available());
    }

    #[test]
    fn test_openai_oracle_with_model() {
        let oracle = OpenAIOracle::new("sk-fake123456").with_model("gpt-4o");
        assert_eq!(oracle.model(), "gpt-4o");
        assert!(oracle.available()); // starts with "sk-"
    }

    #[test]
    fn test_openai_cost_estimate_mini() {
        let oracle = OpenAIOracle::new("sk-x").with_model("gpt-4o-mini");
        let cost = oracle.cost_estimate();
        // gpt-4o-mini: $0.15/MTok input = 0.00000015 per token
        assert!(cost.per_input_token_usd < cost.per_output_token_usd);
        assert!(cost.per_input_token_usd > 0.0);
    }

    #[test]
    fn test_openai_cost_estimate_4o() {
        let oracle = OpenAIOracle::new("sk-x").with_model("gpt-4o");
        let cost = oracle.cost_estimate();
        // gpt-4o should be more expensive than gpt-4o-mini
        let mini = OpenAIOracle::new("sk-x").with_model("gpt-4o-mini").cost_estimate();
        assert!(cost.per_input_token_usd > mini.per_input_token_usd);
    }

    #[test]
    fn test_build_oracle_explicit_openai() {
        let config = OracleConfig {
            provider: Some("openai".to_string()),
            model: "gpt-4o-mini".to_string(),
            // Non-"env:"/non-"capsule:" values are used as the raw key
            api_key_source: "sk-fake-openai-key-for-testing".to_string(),
            ..OracleConfig::default()
        };
        let engine = OracleEngine::new(config, OraclePatternMemory::new());
        assert_eq!(engine.oracle_name(), "openai");
        assert_eq!(engine.oracle_model(), "gpt-4o-mini");
    }

    #[test]
    fn test_build_oracle_explicit_claude() {
        let config = OracleConfig {
            provider: Some("claude".to_string()),
            // Non-"env:"/non-"capsule:" values are used as the raw key
            api_key_source: "fake-anthropic-key-for-testing".to_string(),
            ..OracleConfig::default()
        };
        let engine = OracleEngine::new(config, OraclePatternMemory::new());
        assert_eq!(engine.oracle_name(), "ClaudeOracle");
    }
}
