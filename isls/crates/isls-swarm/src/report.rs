// isls-swarm: report — SwarmRound, ConsensusVote, SwarmReport

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use isls_agent::AgentStep;

// ─── ConsensusVote ────────────────────────────────────────────────────────────

/// The outcome of one round's consensus evaluation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConsensusVote {
    /// Whether consensus was reached this round.
    pub reached: bool,
    /// Mean score of successful steps, clamped to [0, 1].
    pub confidence: f64,
    /// Number of members that participated (had a pending step).
    pub participating_agents: usize,
    /// Number of members whose step had score > 0.
    pub successful_agents: usize,
}

// ─── SwarmRound ───────────────────────────────────────────────────────────────

/// Record of a single Swarm round.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwarmRound {
    pub round_id: usize,
    /// Map from member_id → step result (None if member was already complete).
    pub member_steps: BTreeMap<usize, Option<AgentStep>>,
    pub consensus: ConsensusVote,
}

// ─── SwarmReport ──────────────────────────────────────────────────────────────

/// Aggregated report emitted after a full Swarm run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwarmReport {
    pub goal_intent: String,
    pub swarm_size: usize,
    pub rounds_run: usize,
    pub consensus_reached: bool,
    /// Mean best_score across all members at run end, clamped to [0, 1].
    pub final_resonance: f64,
    pub rounds: Vec<SwarmRound>,
}
