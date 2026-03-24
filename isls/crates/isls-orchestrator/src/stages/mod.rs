// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//! Pipeline stage definitions and stage runner modules.

use serde::{Deserialize, Serialize};

pub mod stage0;
pub mod stage1;
pub mod stage2;
pub mod stage3;
pub mod stage4;
pub mod stage5;
pub mod stage6;
pub mod stage7;
pub mod stage8;
pub mod stage9;
pub mod stage10;

/// The 11 stages of the full-stack generation pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    Describe,
    Plan,
    Model,
    Persist,
    Logic,
    Api,
    Frontend,
    Test,
    Deploy,
    Verify,
    Learn,
}

impl Stage {
    /// Human-readable stage name.
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Describe  => "DESCRIBE",
            Stage::Plan      => "PLAN",
            Stage::Model     => "MODEL",
            Stage::Persist   => "PERSIST",
            Stage::Logic     => "LOGIC",
            Stage::Api       => "API",
            Stage::Frontend  => "FRONTEND",
            Stage::Test      => "TEST",
            Stage::Deploy    => "DEPLOY",
            Stage::Verify    => "VERIFY",
            Stage::Learn     => "LEARN",
        }
    }
}

/// Result of executing a single pipeline stage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageResult {
    pub stage: Stage,
    pub components_generated: usize,
    pub oracle_calls: usize,
    pub blueprint_hits: usize,
    pub retries: usize,
    pub time_secs: f64,
    pub success: bool,
    pub notes: Vec<String>,
}

impl StageResult {
    pub fn ok(stage: Stage, components: usize, oracle: usize, hits: usize) -> Self {
        Self {
            stage,
            components_generated: components,
            oracle_calls: oracle,
            blueprint_hits: hits,
            retries: 0,
            time_secs: 0.0,
            success: true,
            notes: vec![],
        }
    }
}

// ─── Shared file writing helper ───────────────────────────────────────────────

use std::path::Path;

/// Write content to a file, creating parent directories as needed.
/// Returns the bytes written (for evidence recording).
pub(crate) fn write_file(path: &Path, content: &str) -> std::io::Result<Vec<u8>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = content.as_bytes().to_vec();
    std::fs::write(path, &bytes)?;
    Ok(bytes)
}
