// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! 11-stage full-stack generation pipeline orchestrator for ISLS.
//!
//! Drives the complete pipeline from AppSpec to a fully generated, evidence-chained
//! full-stack application:
//!
//! - Stage 0:  DESCRIBE   — TOML → AppSpec
//! - Stage 1:  PLAN       — AppSpec → Architecture
//! - Stage 2:  MODEL      — Architecture → Rust structs + SQL schema
//! - Stage 3:  PERSIST    — Models → SQL migrations + database access layer
//! - Stage 4:  LOGIC      — Services layer (template CRUD or oracle for complex logic)
//! - Stage 5:  API        — Services → Actix-web endpoint handlers
//! - Stage 6:  FRONTEND   — API → Single-page application (vanilla JS)
//! - Stage 7:  TEST       — All layers → Integration tests
//! - Stage 8:  DEPLOY     — All → Docker, configs, README
//! - Stage 9:  VERIFY     — Full system → compilation + topology check
//! - Stage 10: LEARN      — Results → crystallised blueprints

pub mod stages;
pub mod templates;
pub mod evidence;

use std::path::{Path, PathBuf};
use std::time::Instant;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use isls_blueprint::BlueprintRegistry;
use isls_planner::{AppSpec, Architecture};
use isls_learner::PatternLibrary;

pub use stages::{Stage, StageResult};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("planner error: {0}")]
    Planner(#[from] isls_planner::PlannerError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("template error: {0}")]
    Template(String),
    #[error("stage {stage:?} failed: {reason}")]
    StageFailed { stage: Stage, reason: String },
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, OrchestratorError>;

// ─── Config ──────────────────────────────────────────────────────────────────

/// Configuration for the full-stack generation orchestrator.
#[derive(Clone, Debug)]
pub struct OrchestratorConfig {
    /// When true, use template-based mock generation instead of calling an LLM.
    pub mock_oracle: bool,
    /// Maximum retry attempts for oracle-assisted generation.
    pub max_oracle_attempts: usize,
    /// Path to load/save the blueprint registry.
    pub blueprint_path: Option<PathBuf>,
    /// Run `cargo build` on the generated backend during Stage 9.
    pub verify_compilation: bool,
    /// Similarity threshold for topology verification (0.88 = Barbara hardened).
    pub topo_similarity_threshold: f64,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            mock_oracle: true,
            max_oracle_attempts: 3,
            blueprint_path: None,
            verify_compilation: false,
            topo_similarity_threshold: 0.88,
        }
    }
}

// ─── RunReport ───────────────────────────────────────────────────────────────

/// Full run report produced after the 11-stage pipeline completes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunReport {
    pub app_name: String,
    pub stages_completed: Vec<StageResult>,
    pub total_files_generated: usize,
    pub total_loc: usize,
    pub oracle_calls: usize,
    pub blueprint_hits: usize,
    pub blueprint_hit_rate: f64,
    pub compile_success: bool,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub crystals_created: usize,
    pub total_time_secs: f64,
    pub evidence_chain_valid: bool,
    pub output_dir: PathBuf,
}

// ─── GenContext ───────────────────────────────────────────────────────────────

/// Mutable context threaded through all pipeline stages.
pub struct GenContext {
    pub spec: AppSpec,
    pub architecture: Option<Architecture>,
    pub output_dir: PathBuf,
    pub files_written: Vec<PathBuf>,
    pub oracle_calls: usize,
    pub blueprint_hits: usize,
    pub evidence: evidence::EvidenceChain,
}

impl GenContext {
    fn new(spec: AppSpec, output_dir: PathBuf) -> Self {
        Self {
            spec,
            architecture: None,
            output_dir,
            files_written: Vec::new(),
            oracle_calls: 0,
            blueprint_hits: 0,
            evidence: evidence::EvidenceChain::new(),
        }
    }
}

// ─── Orchestrator ─────────────────────────────────────────────────────────────

/// The main orchestrator driving all 11 pipeline stages.
pub struct Orchestrator {
    pub config: OrchestratorConfig,
    blueprints: BlueprintRegistry,
    learner: PatternLibrary,
}

impl Orchestrator {
    /// Create a new orchestrator with the given configuration.
    pub fn new(config: OrchestratorConfig) -> Self {
        // Try loading saved blueprints; fall back to built-ins
        let blueprints = config.blueprint_path.as_ref()
            .and_then(|p| BlueprintRegistry::load(p).ok())
            .unwrap_or_else(BlueprintRegistry::with_builtins);

        Self { config, blueprints, learner: PatternLibrary::new() }
    }

    /// Run the full 11-stage pipeline from an `AppSpec` to a generated system.
    pub fn run(&mut self, spec: AppSpec, output_dir: &Path) -> Result<RunReport> {
        let start = Instant::now();
        let app_name = spec.name.clone();

        std::fs::create_dir_all(output_dir)?;

        let mut ctx = GenContext::new(spec, output_dir.to_path_buf());
        let mut stage_results = Vec::new();

        let all_stages = [
            Stage::Describe, Stage::Plan, Stage::Model, Stage::Persist,
            Stage::Logic, Stage::Api, Stage::Frontend, Stage::Test,
            Stage::Deploy, Stage::Verify, Stage::Learn,
        ];

        for stage in &all_stages {
            let result = self.run_stage(*stage, &mut ctx)?;
            stage_results.push(result);
        }

        let total_loc: usize = ctx.files_written.iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .map(|s| s.lines().count())
            .sum();

        // Save updated blueprints
        if let Some(bp_path) = &self.config.blueprint_path {
            let _ = self.blueprints.save(bp_path);
        }

        let oracle_calls = ctx.oracle_calls;
        let blueprint_hits = ctx.blueprint_hits;
        let total_requests = oracle_calls + blueprint_hits;
        let hit_rate = if total_requests > 0 {
            blueprint_hits as f64 / total_requests as f64
        } else {
            0.0
        };

        Ok(RunReport {
            app_name,
            stages_completed: stage_results,
            total_files_generated: ctx.files_written.len(),
            total_loc,
            oracle_calls,
            blueprint_hits,
            blueprint_hit_rate: hit_rate,
            compile_success: false, // updated by Stage 9
            tests_passed: 0,
            tests_failed: 0,
            crystals_created: self.blueprints.len(),
            total_time_secs: start.elapsed().as_secs_f64(),
            evidence_chain_valid: ctx.evidence.is_valid(),
            output_dir: ctx.output_dir,
        })
    }

    fn run_stage(&mut self, stage: Stage, ctx: &mut GenContext) -> Result<StageResult> {
        let start = Instant::now();
        let result = match stage {
            Stage::Describe  => stages::stage0::run(ctx),
            Stage::Plan      => stages::stage1::run(ctx, &self.blueprints),
            Stage::Model     => stages::stage2::run(ctx, &self.blueprints, &mut self.learner),
            Stage::Persist   => stages::stage3::run(ctx),
            Stage::Logic     => stages::stage4::run(ctx, &self.blueprints, self.config.mock_oracle),
            Stage::Api       => stages::stage5::run(ctx, &self.blueprints),
            Stage::Frontend  => stages::stage6::run(ctx),
            Stage::Test      => stages::stage7::run(ctx, &self.blueprints),
            Stage::Deploy    => stages::stage8::run(ctx),
            Stage::Verify    => stages::stage9::run(ctx, self.config.verify_compilation),
            Stage::Learn     => stages::stage10::run(ctx, &mut self.blueprints, &mut self.learner),
        };
        let time_secs = start.elapsed().as_secs_f64();
        match result {
            Ok(mut sr) => { sr.time_secs = time_secs; Ok(sr) }
            Err(e) => Err(OrchestratorError::StageFailed {
                stage,
                reason: e.to_string(),
            }),
        }
    }
}
