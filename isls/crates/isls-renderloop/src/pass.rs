// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Multi-pass render loop passes for ISLS v2.1.
//!
//! Each pass enriches the generated artifact set in a specific way, from
//! structure-only (offline) through domain logic, edge cases, integration
//! checks, test generation, and final polish.

use serde::{Deserialize, Serialize};

use crate::crystal::Crystal;
use crate::Artifact;

// ─── PassType ─────────────────────────────────────────────────────────────────

/// The type of enrichment performed in a single render pass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PassType {
    /// Pass 0: Offline. Emit file skeletons from templates and universal crystals.
    /// No LLM calls. Token budget: 0.
    Structure,
    /// Pass 1: LLM fills in business-logic function bodies in the services layer.
    DomainLogic,
    /// Pass 2: LLM adds error handling, boundary checks, and edge-case guards.
    EdgeCases,
    /// Pass 3: LLM checks cross-module consistency (API ↔ services ↔ frontend).
    Integration,
    /// Pass 4: LLM generates integration test scenarios.
    TestGeneration,
    /// Pass 5: LLM improves error messages, logging, documentation, naming.
    Polish,
}

impl PassType {
    /// Human-readable label used in trace output.
    pub fn label(&self) -> &str {
        match self {
            PassType::Structure => "structure",
            PassType::DomainLogic => "domain_logic",
            PassType::EdgeCases => "edge_cases",
            PassType::Integration => "integration",
            PassType::TestGeneration => "test_generation",
            PassType::Polish => "polish",
        }
    }

    /// Returns true if this pass requires an LLM oracle call.
    pub fn requires_oracle(&self) -> bool {
        !matches!(self, PassType::Structure)
    }
}

// ─── PassScope ────────────────────────────────────────────────────────────────

/// Which artifacts a pass operates on.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PassScope {
    /// All artifacts in the current set.
    All,
    /// Only artifacts whose category equals the given layer name
    /// (e.g. `"services"`, `"api"`, `"frontend"`, `"tests"`).
    Layer(String),
    /// Only the listed relative file paths.
    Files(Vec<String>),
    /// Only artifacts whose path contains the given function-name pattern
    /// (substring match, supports `*` suffix wildcard).
    Functions(String),
}

impl PassScope {
    /// Returns true if the given artifact is in scope for this pass.
    pub fn includes(&self, artifact: &Artifact) -> bool {
        match self {
            PassScope::All => true,
            PassScope::Layer(layer) => artifact.category == *layer,
            PassScope::Files(paths) => paths.iter().any(|p| artifact.rel_path == *p),
            PassScope::Functions(pattern) => {
                if let Some(prefix) = pattern.strip_suffix('*') {
                    artifact.rel_path.contains(prefix)
                } else {
                    artifact.rel_path.contains(pattern.as_str())
                }
            }
        }
    }
}

// ─── RenderPass ───────────────────────────────────────────────────────────────

/// Configuration for a single pass in the render loop.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderPass {
    /// Pass depth index (0 = Structure, 1 = DomainLogic, …, 5 = Polish).
    pub depth: u32,
    /// Type of enrichment this pass performs.
    pub pass_type: PassType,
    /// Which artifacts this pass operates on.
    pub scope: PassScope,
    /// Maximum tokens this pass may consume before moving to the next pass.
    pub token_budget: u64,
    /// Stop this pass early when the fraction of changed lines drops below
    /// this threshold (convergence criterion).
    pub convergence_threshold: f64,
}

impl RenderPass {
    /// Build the default six-pass sequence as specified in the v2.1 spec.
    pub fn default_passes() -> Vec<RenderPass> {
        vec![
            RenderPass {
                depth: 0,
                pass_type: PassType::Structure,
                scope: PassScope::All,
                token_budget: 0,
                convergence_threshold: 0.0,
            },
            RenderPass {
                depth: 1,
                pass_type: PassType::DomainLogic,
                scope: PassScope::Layer("services".to_string()),
                token_budget: 50_000,
                convergence_threshold: 0.05,
            },
            RenderPass {
                depth: 2,
                pass_type: PassType::EdgeCases,
                scope: PassScope::Layer("services".to_string()),
                token_budget: 20_000,
                convergence_threshold: 0.03,
            },
            RenderPass {
                depth: 3,
                pass_type: PassType::Integration,
                scope: PassScope::All,
                token_budget: 15_000,
                convergence_threshold: 0.02,
            },
            RenderPass {
                depth: 4,
                pass_type: PassType::TestGeneration,
                scope: PassScope::Layer("tests".to_string()),
                token_budget: 20_000,
                convergence_threshold: 0.05,
            },
            RenderPass {
                depth: 5,
                pass_type: PassType::Polish,
                scope: PassScope::All,
                token_budget: 10_000,
                convergence_threshold: 0.01,
            },
        ]
    }
}

// ─── RenderStats ──────────────────────────────────────────────────────────────

/// Accumulated statistics for a completed render loop run.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RenderStats {
    /// Number of passes that were actually executed.
    pub passes_executed: u32,
    /// Total tokens consumed across all passes.
    pub total_tokens_used: u64,
    /// Tokens consumed per pass (indexed by pass depth).
    pub tokens_per_pass: Vec<u64>,
    /// Number of files modified per pass.
    pub files_modified_per_pass: Vec<usize>,
    /// Measured change rate per pass (fraction of lines changed).
    pub convergence_per_pass: Vec<f64>,
    /// Number of crystals updated after this run.
    pub crystals_updated: usize,
}

// ─── Prompt builders ──────────────────────────────────────────────────────────

/// Build a domain-logic enrichment prompt for a single service-layer artifact.
///
/// The prompt asks the LLM to fill in function bodies using the known crystal
/// patterns as guidance.
pub fn build_domain_logic_prompt(artifact: &Artifact, crystals: &[&Crystal]) -> String {
    let crystal_hints = if crystals.is_empty() {
        String::new()
    } else {
        let hints: Vec<String> = crystals.iter()
            .map(|c| format!("- {} ({}): {}", c.pattern_name, c.level.label(), c.description))
            .collect();
        format!("\nRelevant architectural patterns:\n{}", hints.join("\n"))
    };

    let impl_notes = crystals.iter()
        .flat_map(|c| c.implementation.body_patterns.iter())
        .cloned()
        .collect::<Vec<_>>();

    let patterns_section = if impl_notes.is_empty() {
        String::new()
    } else {
        format!("\nKnown implementation patterns:\n{}", impl_notes.join("\n"))
    };

    format!(
        "Fill in the function bodies in the following Rust service file with correct business logic.\n\
         Replace any TODO or placeholder comments with real implementation.\n\
         Use only the types and functions already declared in the file.\n\
         Return the complete file with all function bodies filled in.\n\
         {crystal_hints}{patterns_section}\n\n\
         File: {path}\n\
         ```rust\n{content}\n```",
        crystal_hints = crystal_hints,
        patterns_section = patterns_section,
        path = artifact.rel_path,
        content = artifact.content,
    )
}

/// Build an edge-case enrichment prompt for a service-layer artifact.
pub fn build_edge_case_prompt(artifact: &Artifact) -> String {
    format!(
        "Review the following Rust service file and add missing error handling and edge-case guards.\n\
         Add checks for: empty/zero/negative inputs, referenced entities not found, \
         concurrent modification, database errors, and permission violations.\n\
         Return the complete file with all edge cases handled.\n\n\
         File: {path}\n\
         ```rust\n{content}\n```",
        path = artifact.rel_path,
        content = artifact.content,
    )
}

/// Build a cross-module integration consistency prompt.
pub fn build_integration_prompt(artifacts: &[Artifact]) -> String {
    let summaries: Vec<String> = artifacts.iter()
        .map(|a| format!("--- {} ({}) ---\n{}", a.rel_path, a.category, truncate(&a.content, 500)))
        .collect();

    format!(
        "Review these generated files for cross-module consistency issues.\n\
         Check: API routes calling non-existent service functions, frontend URLs not matching \
         API routes, request/response shape mismatches.\n\
         Return a JSON object: {{\"fixes\": [{{\"file\": \"...\", \"issue\": \"...\", \
         \"suggestion\": \"...\"}}], \"notes\": \"...\"}}\n\n\
         Files:\n{}",
        summaries.join("\n\n")
    )
}

/// Build a test-generation prompt for a single artifact.
pub fn build_test_prompt(artifact: &Artifact, crystals: &[&Crystal]) -> String {
    let scenarios: Vec<String> = crystals.iter()
        .flat_map(|c| c.implementation.test_scenarios.iter())
        .cloned()
        .collect();

    let scenario_hint = if scenarios.is_empty() {
        String::new()
    } else {
        format!("\nKnown test scenarios:\n{}", scenarios.join("\n"))
    };

    format!(
        "Generate Rust integration tests for the following file.\n\
         Cover: happy path, validation errors, not-found cases, authorization failures, idempotency.\n\
         Return complete, compilable Rust test code.\n\
         {scenario_hint}\n\n\
         File: {path}\n\
         ```rust\n{content}\n```",
        scenario_hint = scenario_hint,
        path = artifact.rel_path,
        content = artifact.content,
    )
}

/// Build a polish prompt for documentation and naming improvements.
pub fn build_polish_prompt(artifact: &Artifact) -> String {
    format!(
        "Improve the following Rust source file: add doc comments to public items, \
         improve error messages to be user-friendly, add structured log statements at key points, \
         fix any inconsistent naming.\n\
         Return the complete improved file.\n\n\
         File: {path}\n\
         ```rust\n{content}\n```",
        path = artifact.rel_path,
        content = artifact.content,
    )
}

fn truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

// ─── CrystalLevel label helper ────────────────────────────────────────────────

impl crate::crystal::CrystalLevel {
    pub(crate) fn label(&self) -> &str {
        match self {
            crate::crystal::CrystalLevel::Universal => "universal",
            crate::crystal::CrystalLevel::Architectural => "architectural",
            crate::crystal::CrystalLevel::Structural => "structural",
            crate::crystal::CrystalLevel::DomainSpecific => "domain-specific",
        }
    }
}
