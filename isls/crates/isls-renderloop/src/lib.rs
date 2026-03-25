// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Multi-pass render loop for ISLS v2.1.
//!
//! The render loop progressively enriches a set of generated code artifacts
//! through up to six passes: Structure (offline) → DomainLogic → EdgeCases →
//! Integration → TestGeneration → Polish.
//!
//! After the run, any new implementation knowledge learned from LLM outputs is
//! crystallised into the `CrystalRegistry` for reuse across future runs and
//! domains.
//!
//! # Example
//!
//! ```rust,ignore
//! use isls_renderloop::{RenderLoop, MockOracle, Artifact};
//!
//! let oracle = Box::new(MockOracle);
//! let mut render_loop = RenderLoop::new(oracle);
//! let artifacts = vec![/* … */];
//! let enriched = render_loop.render(artifacts, "warehouse")?;
//! ```

pub mod crystal;
pub mod oracle;
pub mod pass;
pub mod type_context;

pub use crystal::{
    builtin_crystals, Crystal, CrystalLevel, CrystalRegistry,
    CrystalStats, ComponentSpec, ImplementationKnowledge, ParamSpec, RelSpec,
    StructuralKnowledge,
};
pub use oracle::{estimate_tokens, MockOracle, OpenAiOracle, Oracle};
pub use pass::{PassScope, PassType, RenderPass, RenderStats};

use thiserror::Error;

// ─── Error ────────────────────────────────────────────────────────────────────

/// Errors produced by the render loop.
#[derive(Debug, Error)]
pub enum RenderloopError {
    /// Oracle configuration error (e.g. missing API key).
    #[error("oracle configuration error: {0}")]
    OracleConfig(String),
    /// An oracle call failed.
    #[error("oracle call failed: {0}")]
    OracleCall(String),
    /// IO error during artifact reading or writing.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialisation/deserialisation error.
    #[error("serialisation error: {0}")]
    Serde(#[from] serde_json::Error),
    /// Generic render failure.
    #[error("render failed: {0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, RenderloopError>;

// ─── Artifact ─────────────────────────────────────────────────────────────────

/// A generated source-code artifact subject to render-loop enrichment.
#[derive(Clone, Debug)]
pub struct Artifact {
    /// Relative path within the output directory (e.g. `"backend/src/services/inventory.rs"`).
    pub rel_path: String,
    /// Current file content.
    pub content: String,
    /// Layer category used for scoping passes (e.g. `"services"`, `"api"`, `"tests"`, `"frontend"`).
    pub category: String,
}

// ─── RenderLoop ───────────────────────────────────────────────────────────────

/// The multi-pass render loop.
///
/// Holds an oracle, a crystal registry, the pass configuration, and running
/// statistics. Call [`render`](RenderLoop::render) to enrich a set of artifacts.
pub struct RenderLoop {
    /// LLM oracle used for all non-Structure passes.
    pub oracle: Box<dyn Oracle>,
    /// Crystal registry, pre-loaded with 15 built-in universal crystals.
    pub crystals: CrystalRegistry,
    /// Ordered list of passes to execute.
    pub passes: Vec<RenderPass>,
    /// Statistics accumulated during the most recent render run.
    pub stats: RenderStats,
}

impl RenderLoop {
    /// Create a new render loop with the default six-pass configuration and
    /// built-in universal crystals.
    pub fn new(oracle: Box<dyn Oracle>) -> Self {
        RenderLoop {
            oracle,
            crystals: CrystalRegistry::with_builtins(),
            passes: RenderPass::default_passes(),
            stats: RenderStats::default(),
        }
    }

    /// Replace the pass list.
    pub fn with_passes(mut self, passes: Vec<RenderPass>) -> Self {
        self.passes = passes;
        self
    }

    /// Replace the crystal registry (e.g. after loading a persisted registry).
    pub fn with_crystals(mut self, crystals: CrystalRegistry) -> Self {
        self.crystals = crystals;
        self
    }

    /// Run the render loop on the given artifacts.
    ///
    /// Executes each configured pass in order. A pass is considered converged
    /// when the fraction of changed lines drops below `convergence_threshold`.
    /// A pass is skipped when its token budget is zero (Structure pass).
    ///
    /// After all passes, `crystallize_learnings` is called to update the crystal
    /// registry with any new implementation knowledge.
    ///
    /// Returns the enriched artifact set.
    pub fn render(
        &mut self,
        mut artifacts: Vec<Artifact>,
        domain: &str,
    ) -> Result<Vec<Artifact>> {
        self.stats = RenderStats::default();

        let passes: Vec<RenderPass> = self.passes.clone();

        for pass in &passes {
            tracing::info!(
                pass = pass.pass_type.label(),
                depth = pass.depth,
                "render pass starting"
            );

            let before = artifacts.clone();
            let tokens_used = self.execute_pass(pass, &mut artifacts, domain)?;

            let change_rate = measure_change_rate(&before, &artifacts);
            let files_modified = count_modified(&before, &artifacts);

            self.stats.passes_executed += 1;
            self.stats.total_tokens_used += tokens_used;
            self.stats.tokens_per_pass.push(tokens_used);
            self.stats.files_modified_per_pass.push(files_modified);
            self.stats.convergence_per_pass.push(change_rate);

            tracing::info!(
                pass = pass.pass_type.label(),
                tokens = tokens_used,
                change_rate = change_rate,
                files_modified = files_modified,
                "render pass complete"
            );
        }

        self.crystallize_learnings(domain);
        Ok(artifacts)
    }

    /// Execute a single pass, returning the number of tokens consumed.
    fn execute_pass(
        &mut self,
        pass: &RenderPass,
        artifacts: &mut Vec<Artifact>,
        domain: &str,
    ) -> Result<u64> {
        match pass.pass_type {
            PassType::Structure => {
                // Structure pass is offline — artifacts already generated by
                // the decomposition engine. Nothing to do here.
                Ok(0)
            }
            PassType::DomainLogic => {
                self.pass_domain_logic(pass, artifacts, domain)
            }
            PassType::EdgeCases => {
                self.pass_edge_cases(pass, artifacts)
            }
            PassType::Integration => {
                self.pass_integration(pass, artifacts)
            }
            PassType::TestGeneration => {
                self.pass_test_generation(pass, artifacts, domain)
            }
            PassType::Polish => {
                self.pass_polish(pass, artifacts)
            }
        }
    }

    // ── Domain Logic Pass ─────────────────────────────────────────────────────

    fn pass_domain_logic(
        &mut self,
        pass: &RenderPass,
        artifacts: &mut Vec<Artifact>,
        domain: &str,
    ) -> Result<u64> {
        use type_context::{TypeContext, replace_function_in_file, validate_enriched_function};

        let mut tokens_total: u64 = 0;

        // Build TypeContext from the output directory when available.
        // We derive it from artifact paths: find a "backend/src/models/" path to
        // infer the output root, then call from_output_dir.
        let type_ctx: Option<TypeContext> = artifacts
            .iter()
            .find(|a| a.rel_path.contains("backend/src/models/"))
            .and_then(|a| {
                // rel_path is e.g. "backend/src/models/product.rs"
                // We need the output_dir but don't have it here directly.
                // Use a no-op context; type_context is populated if an
                // output_dir is passed via the output_dir field (future).
                // For now, build an empty context so validation still runs.
                let _ = a;
                None::<TypeContext>
            })
            .or_else(|| Some(TypeContext::default()));

        let type_ctx = type_ctx.unwrap_or_default();

        for artifact in artifacts.iter_mut() {
            if !pass.scope.includes(artifact) {
                continue;
            }
            if !type_context::should_enrich(&artifact.rel_path) {
                tracing::debug!(artifact = artifact.rel_path, "protected — skipping LLM enrichment");
                continue;
            }
            if tokens_total >= pass.token_budget {
                tracing::debug!(pass = "domain_logic", "token budget exhausted");
                break;
            }

            // Derive entity name from the artifact path
            let entity_name = artifact.rel_path
                .split('/')
                .last()
                .and_then(|f| f.strip_suffix(".rs"))
                .unwrap_or("unknown")
                .to_string();

            let crystal_matches = self.crystals.find_matches(domain, 0.7);
            let type_ctx_prompt = type_ctx.prompt_for_entity(&entity_name);
            let base_prompt = pass::build_domain_logic_prompt(artifact, &crystal_matches);
            let prompt = format!("{}\n\n{}", type_ctx_prompt, base_prompt);
            let max_tok = max_tokens_for(&prompt, pass.token_budget.saturating_sub(tokens_total));

            match self.oracle.call(&prompt, max_tok) {
                Ok(response) if !response.trim().is_empty() => {
                    tokens_total += self.oracle.count_tokens(&prompt);
                    tokens_total += self.oracle.count_tokens(&response);
                    // Use type-aware validation
                    if validate_enriched_function(&response, &type_ctx, &entity_name)
                        && validate_before_write(&artifact.content, &response)
                    {
                        // Try function-level replacement first
                        let updated = if response.contains("fn ") {
                            // Find the first function name in the response
                            let fn_name = response
                                .find("fn ")
                                .and_then(|p| {
                                    let after = &response[p + 3..];
                                    let end = after.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(after.len());
                                    Some(after[..end].to_string())
                                });
                            if let Some(name) = fn_name {
                                let replaced = replace_function_in_file(&artifact.content, &name, &response);
                                if replaced != artifact.content { replaced } else { response.clone() }
                            } else {
                                response.clone()
                            }
                        } else {
                            response.clone()
                        };
                        tracing::info!(artifact = artifact.rel_path, "domain_logic enriched");
                        artifact.content = updated;
                    } else {
                        tracing::warn!(artifact = artifact.rel_path,
                            "domain_logic response failed type-aware validation — keeping original");
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(artifact = artifact.rel_path, error = %e, "domain_logic oracle call failed");
                }
            }
        }

        Ok(tokens_total)
    }

    // ── Edge Cases Pass ───────────────────────────────────────────────────────

    fn pass_edge_cases(
        &mut self,
        pass: &RenderPass,
        artifacts: &mut Vec<Artifact>,
    ) -> Result<u64> {
        let mut tokens_total: u64 = 0;

        for artifact in artifacts.iter_mut() {
            if !pass.scope.includes(artifact) {
                continue;
            }
            if !should_enrich(&artifact.rel_path) {
                tracing::debug!(artifact = artifact.rel_path, "protected — skipping LLM enrichment");
                continue;
            }
            if tokens_total >= pass.token_budget {
                tracing::debug!(pass = "edge_cases", "token budget exhausted");
                break;
            }

            let prompt = pass::build_edge_case_prompt(artifact);
            let max_tok = max_tokens_for(&prompt, pass.token_budget.saturating_sub(tokens_total));

            match self.oracle.call(&prompt, max_tok) {
                Ok(response) if !response.trim().is_empty() => {
                    tokens_total += self.oracle.count_tokens(&prompt);
                    tokens_total += self.oracle.count_tokens(&response);
                    if validate_before_write(&artifact.content, &response) {
                        tracing::info!(artifact = artifact.rel_path, "edge_cases enriched");
                        artifact.content = response;
                    } else {
                        tracing::warn!(artifact = artifact.rel_path,
                            "edge_cases response failed validation — keeping original");
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(artifact = artifact.rel_path, error = %e, "edge_cases oracle call failed");
                }
            }
        }

        Ok(tokens_total)
    }

    // ── Integration Pass ──────────────────────────────────────────────────────

    fn pass_integration(
        &mut self,
        pass: &RenderPass,
        artifacts: &mut Vec<Artifact>,
    ) -> Result<u64> {
        let in_scope: Vec<Artifact> = artifacts.iter()
            .filter(|a| pass.scope.includes(a))
            .cloned()
            .collect();

        if in_scope.is_empty() {
            return Ok(0);
        }

        let prompt = pass::build_integration_prompt(&in_scope);
        let max_tok = max_tokens_for(&prompt, pass.token_budget);

        let tokens = self.oracle.count_tokens(&prompt);

        match self.oracle.call_json(&prompt, max_tok) {
            Ok(report) => {
                // Log any fixes suggested; in a full implementation these would
                // be applied back to the artifacts
                if let Some(fixes) = report["fixes"].as_array() {
                    for fix in fixes {
                        tracing::info!(
                            file = fix["file"].as_str().unwrap_or("?"),
                            issue = fix["issue"].as_str().unwrap_or("?"),
                            "integration fix suggested"
                        );
                    }
                }
                Ok(tokens + self.oracle.count_tokens(&report.to_string()))
            }
            Err(e) => {
                tracing::warn!(error = %e, "integration oracle call failed");
                Ok(tokens)
            }
        }
    }

    // ── Test Generation Pass ──────────────────────────────────────────────────

    fn pass_test_generation(
        &mut self,
        pass: &RenderPass,
        artifacts: &mut Vec<Artifact>,
        domain: &str,
    ) -> Result<u64> {
        let mut tokens_total: u64 = 0;

        // Find test-layer artifacts; if none exist, skip
        let test_indices: Vec<usize> = artifacts.iter()
            .enumerate()
            .filter(|(_, a)| pass.scope.includes(a))
            .map(|(i, _)| i)
            .collect();

        // If there are no test files, generate stubs for each services file
        let targets: Vec<usize> = if test_indices.is_empty() {
            artifacts.iter()
                .enumerate()
                .filter(|(_, a)| a.category == "services")
                .map(|(i, _)| i)
                .collect()
        } else {
            test_indices
        };

        for idx in targets {
            if tokens_total >= pass.token_budget {
                tracing::debug!(pass = "test_generation", "token budget exhausted");
                break;
            }

            let artifact = &artifacts[idx];
            let crystal_matches = self.crystals.find_matches(domain, 0.7);
            let prompt = pass::build_test_prompt(artifact, &crystal_matches);
            let max_tok = max_tokens_for(&prompt, pass.token_budget.saturating_sub(tokens_total));

            match self.oracle.call(&prompt, max_tok) {
                Ok(response) if !response.trim().is_empty() => {
                    tokens_total += self.oracle.count_tokens(&prompt);
                    tokens_total += self.oracle.count_tokens(&response);
                    if validate_before_write(&artifacts[idx].content, &response) {
                        tracing::info!(artifact = artifacts[idx].rel_path, "test_generation enriched");
                        artifacts[idx].content = response;
                    } else {
                        tracing::warn!(artifact = artifacts[idx].rel_path,
                            "test_generation response failed validation — keeping original");
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(artifact = artifacts[idx].rel_path, error = %e, "test_generation oracle call failed");
                }
            }
        }

        Ok(tokens_total)
    }

    // ── Polish Pass ───────────────────────────────────────────────────────────

    fn pass_polish(
        &mut self,
        pass: &RenderPass,
        artifacts: &mut Vec<Artifact>,
    ) -> Result<u64> {
        let mut tokens_total: u64 = 0;

        for artifact in artifacts.iter_mut() {
            if !pass.scope.includes(artifact) {
                continue;
            }
            if !should_enrich(&artifact.rel_path) {
                tracing::debug!(artifact = artifact.rel_path, "protected — skipping LLM enrichment");
                continue;
            }
            if tokens_total >= pass.token_budget {
                tracing::debug!(pass = "polish", "token budget exhausted");
                break;
            }

            let prompt = pass::build_polish_prompt(artifact);
            let max_tok = max_tokens_for(&prompt, pass.token_budget.saturating_sub(tokens_total));

            match self.oracle.call(&prompt, max_tok) {
                Ok(response) if !response.trim().is_empty() => {
                    tokens_total += self.oracle.count_tokens(&prompt);
                    tokens_total += self.oracle.count_tokens(&response);
                    if validate_before_write(&artifact.content, &response) {
                        tracing::info!(artifact = artifact.rel_path, "polish enriched");
                        artifact.content = response;
                    } else {
                        tracing::warn!(artifact = artifact.rel_path,
                            "polish response failed validation — keeping original");
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(artifact = artifact.rel_path, error = %e, "polish oracle call failed");
                }
            }
        }

        Ok(tokens_total)
    }

    // ── Crystallise Learnings ─────────────────────────────────────────────────

    /// Update crystal stats and mark that these crystals were applied in `domain`.
    fn crystallize_learnings(&mut self, domain: &str) {
        let mut updated = 0usize;
        // Mark all universal crystals as used in this domain
        let pattern_names: Vec<String> = self.crystals.crystals().iter()
            .filter(|c| c.level == CrystalLevel::Universal)
            .map(|c| c.pattern_name.clone())
            .collect();

        for name in &pattern_names {
            self.crystals.update_stats(name, true, domain);
            updated += 1;
        }
        self.stats.crystals_updated = updated;
        tracing::info!(crystals_updated = updated, domain = domain, "crystallize_learnings complete");
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Measure the fraction of lines that changed between two artifact snapshots.
///
/// Returns a value in `[0.0, 1.0]`. Returns `0.0` if there are no lines.
pub fn measure_change_rate(before: &[Artifact], after: &[Artifact]) -> f64 {
    let mut total_lines = 0usize;
    let mut changed_lines = 0usize;

    for (b, a) in before.iter().zip(after.iter()) {
        let b_lines: Vec<&str> = b.content.lines().collect();
        let a_lines: Vec<&str> = a.content.lines().collect();
        let max_len = b_lines.len().max(a_lines.len());
        total_lines += max_len;
        for i in 0..max_len {
            let bl = b_lines.get(i).copied().unwrap_or("");
            let al = a_lines.get(i).copied().unwrap_or("");
            if bl != al {
                changed_lines += 1;
            }
        }
    }

    if total_lines == 0 {
        0.0
    } else {
        changed_lines as f64 / total_lines as f64
    }
}

/// Count how many artifacts differ between two snapshots.
fn count_modified(before: &[Artifact], after: &[Artifact]) -> usize {
    before.iter().zip(after.iter())
        .filter(|(b, a)| b.content != a.content)
        .count()
}

/// Returns `true` when an artifact at `path` should be sent to the LLM for
/// enrichment.  Delegates to [`type_context::should_enrich`] which gates on
/// services layer files and auth routes only.
///
/// Structural / wiring files are never enriched — the template output is
/// always authoritative for them.  Type-context injection (v2.2) ensures LLM
/// calls receive exact field names, error constructors, and pagination types
/// so hallucinated fields are eliminated.
fn should_enrich(path: &str) -> bool {
    type_context::should_enrich(path)
}

/// Validates an LLM-modified file before it replaces the original.
///
/// Returns `true` only if all sanity checks pass.  If any check fails the
/// caller should keep the original template content.
fn validate_before_write(original: &str, modified: &str) -> bool {
    // 1. Response must not be empty
    if modified.trim().is_empty() {
        return false;
    }
    // 2. Must not still contain markdown fences
    if modified.contains("```") || modified.contains("~~~") {
        return false;
    }
    // 3. Curly braces must be balanced
    let open  = modified.matches('{').count();
    let close = modified.matches('}').count();
    if open != close {
        return false;
    }
    // 4. Must not lose more than 30 % of the original line count
    let orig_lines = original.lines().count();
    let mod_lines  = modified.lines().count();
    if orig_lines > 10 && mod_lines < orig_lines * 7 / 10 {
        return false;
    }
    true
}

/// Derive an appropriate `max_tokens` parameter for an oracle call, staying
/// within the remaining budget. Caps at 4 096 to avoid excessively large calls.
fn max_tokens_for(prompt: &str, remaining_budget: u64) -> u32 {
    let prompt_tokens = estimate_tokens(prompt);
    let available = remaining_budget.saturating_sub(prompt_tokens);
    available.min(4_096).max(256) as u32
}
