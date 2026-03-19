// isls-swarm: policy — SwarmPolicy and ConsensusMode

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use isls_agent::AgentStep;

use crate::{report::ConsensusVote, SwarmMember};

// ─── ConsensusMode ────────────────────────────────────────────────────────────

/// Strategy used to evaluate whether a round's results constitute consensus.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsensusMode {
    /// Consensus when >50 % of active members produced a step (any score > 0).
    Majority,
    /// Consensus when the mean score of steps produced this round ≥ threshold.
    WeightedResonance,
    /// Consensus only when *every* active member produced a step with score > 0.
    Unanimous,
}

impl ConsensusMode {
    /// Evaluate the round results and return a ConsensusVote.
    ///
    /// A step is considered "successful" if its `score > 0.0`.
    pub fn vote(
        &self,
        _members: &[SwarmMember],
        step_results: &BTreeMap<usize, Option<AgentStep>>,
        threshold: f64,
    ) -> ConsensusVote {
        // Active members: those that were not already complete (their entry exists)
        let active_count = step_results.len();
        if active_count == 0 {
            return ConsensusVote {
                reached: false,
                confidence: 0.0,
                participating_agents: 0,
                successful_agents: 0,
            };
        }

        // A step is "successful" when the member was not yet done (Some) and score > 0
        let scores: Vec<f64> = step_results
            .values()
            .filter_map(|opt| opt.as_ref())
            .map(|s: &AgentStep| s.score)
            .filter(|&sc| sc > 0.0)
            .collect();

        let successful_count = scores.len();
        let mean_score = if successful_count == 0 {
            0.0
        } else {
            (scores.iter().sum::<f64>() / successful_count as f64).clamp(0.0, 1.0)
        };

        let reached = match self {
            ConsensusMode::Majority => successful_count * 2 > active_count,
            ConsensusMode::WeightedResonance => mean_score >= threshold,
            ConsensusMode::Unanimous => successful_count == active_count,
        };

        ConsensusVote {
            reached,
            confidence: mean_score,
            participating_agents: active_count,
            successful_agents: successful_count,
        }
    }
}

// ─── SwarmPolicy ──────────────────────────────────────────────────────────────

/// Configuration for a Swarm run.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SwarmPolicy {
    /// Number of agent members.
    pub size: usize,
    /// Base seed; member i gets seed `base_seed + i`.
    pub base_seed: u64,
    /// Maximum number of rounds before the Swarm stops.
    pub max_rounds: usize,
    /// How consensus is determined.
    pub consensus_mode: ConsensusMode,
    /// Threshold used by WeightedResonance consensus mode (0–1).
    pub consensus_threshold: f64,
}

impl Default for SwarmPolicy {
    fn default() -> Self {
        Self {
            size: 4,
            base_seed: 0,
            max_rounds: 16,
            consensus_mode: ConsensusMode::WeightedResonance,
            consensus_threshold: 0.6,
        }
    }
}
