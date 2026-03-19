// isls-swarm: C31 — Multi-Agent Swarm Coordinator
// Many voices. One resonance.
//
// The Swarm spawns N autonomous agents (C30), assigns each a distinct
// exploration seed, and runs them through configurable rounds.  After
// every round the Swarm collects each agent's latest step result and
// applies a consensus policy to decide whether the collective goal has
// been reached.  When `SwarmPolicy.drill_config` is set, the Swarm also
// runs a PMHD DrillEngine (C21) each round to adversarially test whether
// the agents' collective hypothesis survives formal scrutiny.
//
// Design invariants
// -----------------
//   SI-1  Every agent gets a unique, deterministic seed (base_seed + agent_id).
//   SI-2  A round is complete when *all* active agents have produced a step.
//   SI-3  ConsensusVote confidence = mean score of steps produced this round,
//         clamped to [0, 1].  For DrillBacked mode: mean PMHD quality.
//   SI-4  The Swarm terminates when consensus is reached OR max_rounds is hit
//         OR all members have exhausted their plans.
//   SI-5  SwarmReport is serialisable and deterministic for the same inputs.
//   SI-6  PMHD drills are seeded by (base_seed XOR round_id * constant) so
//         each round receives a distinct, deterministic drill seed.

use std::collections::BTreeMap;

use isls_agent::{Agent, AgentConfig, AgentGoal, AgentStep};

// ─── Re-exports ───────────────────────────────────────────────────────────────

pub use policy::{ConsensusMode, SwarmPolicy};
pub use report::{ConsensusVote, DrillSummary, SwarmReport, SwarmRound};

// ─── Modules ──────────────────────────────────────────────────────────────────

pub mod policy;
pub mod report;

// ─── SwarmMember ──────────────────────────────────────────────────────────────

/// One agent inside the Swarm, with its assigned id.
pub struct SwarmMember {
    pub member_id: usize,
    pub agent: Agent,
}

impl SwarmMember {
    fn new(member_id: usize, config: AgentConfig, goal: AgentGoal) -> Self {
        Self { member_id, agent: Agent::new(config, goal) }
    }

    fn step(&mut self) -> Option<AgentStep> {
        self.agent.step()
    }

    fn is_complete(&self) -> bool {
        self.agent.is_complete()
    }

    /// Current best score (resonance proxy).
    fn best_score(&self) -> f64 {
        self.agent.best_score()
    }
}

// ─── Swarm ────────────────────────────────────────────────────────────────────

/// Multi-agent coordinator with optional PMHD adversarial drilling.
///
/// Basic usage (no PMHD):
/// ```
/// use isls_swarm::{Swarm, SwarmPolicy, ConsensusMode};
/// use isls_agent::AgentGoal;
///
/// let goal  = AgentGoal::new("discover structural invariants");
/// let policy = SwarmPolicy {
///     size: 3,
///     base_seed: 42,
///     max_rounds: 10,
///     consensus_mode: ConsensusMode::WeightedResonance,
///     consensus_threshold: 0.5,
///     drill_config: None,
/// };
/// let mut swarm = Swarm::new(policy, goal);
/// let report = swarm.run();
/// assert!(!report.rounds.is_empty());
/// ```
pub struct Swarm {
    pub policy: SwarmPolicy,
    pub goal: AgentGoal,
    members: Vec<SwarmMember>,
    pub rounds_run: usize,
    pub complete: bool,
}

impl Swarm {
    /// Create a new Swarm.  Members are initialised but not yet stepped.
    pub fn new(policy: SwarmPolicy, goal: AgentGoal) -> Self {
        let members = (0..policy.size)
            .map(|id| {
                let cfg = AgentConfig {
                    seed: policy.base_seed.wrapping_add(id as u64),
                    ..Default::default()
                };
                SwarmMember::new(id, cfg, goal.clone())
            })
            .collect();

        Self { policy, goal, members, rounds_run: 0, complete: false }
    }

    /// Execute one round:
    ///   1. Step every active member.
    ///   2. Optionally run a PMHD drill.
    ///   3. Build a ConsensusVote.
    pub fn round(&mut self) -> SwarmRound {
        let round_id = self.rounds_run;
        self.rounds_run += 1;

        // Step all non-exhausted members
        let mut member_steps: BTreeMap<usize, Option<AgentStep>> = BTreeMap::new();
        for m in &mut self.members {
            if !m.is_complete() {
                member_steps.insert(m.member_id, m.step());
            } else {
                member_steps.insert(m.member_id, None);
            }
        }

        // Optional PMHD drill
        let drill_summary = self.policy.drill_config.as_ref().map(|cfg| {
            policy::drill_round(
                &self.goal.intent,
                self.goal.domain.as_deref(),
                &self.goal.constraints,
                self.goal.confidence_target,
                round_id,
                cfg,
            )
        });

        // Consensus evaluation
        let vote = self.policy.consensus_mode.vote(
            &self.members,
            &member_steps,
            self.policy.consensus_threshold,
            drill_summary.as_ref(),
        );

        if vote.reached {
            self.complete = true;
        }

        // Also terminate when all members have exhausted their plans
        if self.members.iter().all(|m| m.is_complete()) {
            self.complete = true;
        }

        SwarmRound { round_id, member_steps, consensus: vote, drill_summary }
    }

    /// Run until consensus is reached or `max_rounds` is exhausted.
    pub fn run(&mut self) -> SwarmReport {
        let mut rounds = Vec::new();
        while !self.complete && self.rounds_run < self.policy.max_rounds {
            rounds.push(self.round());
        }
        self.build_report(rounds)
    }

    fn build_report(&self, rounds: Vec<SwarmRound>) -> SwarmReport {
        let final_resonance = if self.members.is_empty() {
            0.0
        } else {
            let sum: f64 = self.members.iter().map(|m| m.best_score()).sum();
            (sum / self.members.len() as f64).clamp(0.0, 1.0)
        };

        let consensus_reached = rounds.iter().any(|r| r.consensus.reached);

        SwarmReport {
            goal_intent: self.goal.intent.clone(),
            swarm_size: self.members.len(),
            rounds_run: self.rounds_run,
            consensus_reached,
            final_resonance,
            rounds,
        }
    }

    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_agent::AgentGoal;
    use isls_pmhd::PmhdConfig;

    fn default_policy() -> SwarmPolicy {
        SwarmPolicy {
            size: 3,
            base_seed: 7,
            max_rounds: 20,
            consensus_mode: ConsensusMode::WeightedResonance,
            consensus_threshold: 0.4,
            drill_config: None,
        }
    }

    fn default_goal() -> AgentGoal {
        AgentGoal::new("test swarm coordination")
    }

    fn drill_config() -> PmhdConfig {
        PmhdConfig {
            ticks: 5,
            pool_size: 4,
            commit_budget: 3,
            seed: 99,
            ..PmhdConfig::default()
        }
    }

    // AT-SW1: Swarm creates the correct number of members
    #[test]
    fn at_sw1_member_count() {
        let swarm = Swarm::new(default_policy(), default_goal());
        assert_eq!(swarm.member_count(), 3);
    }

    // AT-SW2: Each member gets a unique member_id
    #[test]
    fn at_sw2_unique_member_ids() {
        let swarm = Swarm::new(default_policy(), default_goal());
        let ids: Vec<usize> = swarm.members.iter().map(|m| m.member_id).collect();
        assert_eq!(ids, vec![0, 1, 2]);
    }

    // AT-SW3: Swarm is not complete before running
    #[test]
    fn at_sw3_initial_not_complete() {
        let swarm = Swarm::new(default_policy(), default_goal());
        assert!(!swarm.is_complete());
        assert_eq!(swarm.rounds_run, 0);
    }

    // AT-SW4: A single round increments rounds_run by 1
    #[test]
    fn at_sw4_round_increments_counter() {
        let mut swarm = Swarm::new(default_policy(), default_goal());
        swarm.round();
        assert_eq!(swarm.rounds_run, 1);
    }

    // AT-SW5: Round result has the correct round_id
    #[test]
    fn at_sw5_round_id_correct() {
        let mut swarm = Swarm::new(default_policy(), default_goal());
        let r0 = swarm.round();
        let r1 = swarm.round();
        assert_eq!(r0.round_id, 0);
        assert_eq!(r1.round_id, 1);
    }

    // AT-SW6: member_steps contains an entry for each member
    #[test]
    fn at_sw6_member_steps_populated() {
        let mut swarm = Swarm::new(default_policy(), default_goal());
        let round = swarm.round();
        assert_eq!(round.member_steps.len(), 3);
    }

    // AT-SW7: run() respects max_rounds ceiling
    #[test]
    fn at_sw7_max_rounds_respected() {
        let policy = SwarmPolicy {
            size: 2,
            base_seed: 0,
            max_rounds: 5,
            consensus_mode: ConsensusMode::Majority,
            consensus_threshold: 0.99,
            drill_config: None,
        };
        let mut swarm = Swarm::new(policy, default_goal());
        let report = swarm.run();
        assert!(report.rounds_run <= 5);
    }

    // AT-SW8: run() returns a SwarmReport with rounds count matching rounds_run
    #[test]
    fn at_sw8_report_rounds_consistent() {
        let mut swarm = Swarm::new(default_policy(), default_goal());
        let report = swarm.run();
        assert_eq!(report.rounds.len(), report.rounds_run);
    }

    // AT-SW9: SwarmReport goal_intent matches the input goal
    #[test]
    fn at_sw9_report_goal_intent() {
        let goal = AgentGoal::new("invariant discovery in time-series");
        let mut swarm = Swarm::new(default_policy(), goal);
        let report = swarm.run();
        assert_eq!(report.goal_intent, "invariant discovery in time-series");
    }

    // AT-SW10: SwarmReport is serialisable and round-trips cleanly
    #[test]
    fn at_sw10_report_serialisation() {
        let mut swarm = Swarm::new(default_policy(), default_goal());
        let report = swarm.run();
        let json = serde_json::to_string(&report).expect("serialise");
        let back: SwarmReport = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back.swarm_size, report.swarm_size);
        assert_eq!(back.rounds_run, report.rounds_run);
        assert_eq!(back.goal_intent, report.goal_intent);
    }

    // AT-SW11: Determinism — identical policy + goal → identical report structure
    #[test]
    fn at_sw11_determinism() {
        let p = default_policy();
        let g = default_goal();
        let mut s1 = Swarm::new(p.clone(), g.clone());
        let mut s2 = Swarm::new(p, g);
        let r1 = s1.run();
        let r2 = s2.run();
        assert_eq!(r1.rounds_run, r2.rounds_run);
        assert_eq!(r1.consensus_reached, r2.consensus_reached);
    }

    // AT-SW12: final_resonance is in [0, 1]
    #[test]
    fn at_sw12_resonance_range() {
        let mut swarm = Swarm::new(default_policy(), default_goal());
        let report = swarm.run();
        assert!(report.final_resonance >= 0.0, "resonance below 0");
        assert!(report.final_resonance <= 1.0, "resonance above 1");
    }

    // AT-SW13: Unanimous consensus requires all members to succeed
    #[test]
    fn at_sw13_unanimous_mode() {
        let policy = SwarmPolicy {
            size: 3,
            base_seed: 42,
            max_rounds: 20,
            consensus_mode: ConsensusMode::Unanimous,
            consensus_threshold: 0.01,
            drill_config: None,
        };
        let mut swarm = Swarm::new(policy, default_goal());
        let report = swarm.run();
        assert!(report.rounds_run > 0);
    }

    // AT-SW14: SwarmPolicy serialisation round-trips
    #[test]
    fn at_sw14_policy_serialisation() {
        let p = default_policy();
        let json = serde_json::to_string(&p).expect("serialise policy");
        let back: SwarmPolicy = serde_json::from_str(&json).expect("deserialise policy");
        assert_eq!(back.size, p.size);
        assert_eq!(back.base_seed, p.base_seed);
        assert_eq!(back.max_rounds, p.max_rounds);
    }

    // AT-SW15: Zero-size swarm produces empty report without panic
    #[test]
    fn at_sw15_zero_size_swarm() {
        let policy = SwarmPolicy { size: 0, ..default_policy() };
        let mut swarm = Swarm::new(policy, default_goal());
        let report = swarm.run();
        assert_eq!(report.swarm_size, 0);
        assert_eq!(report.final_resonance, 0.0);
    }

    // AT-SW16: Single-member swarm behaves like a solo agent
    #[test]
    fn at_sw16_single_member_swarm() {
        let policy = SwarmPolicy { size: 1, ..default_policy() };
        let mut swarm = Swarm::new(policy, default_goal());
        let report = swarm.run();
        assert_eq!(report.swarm_size, 1);
        assert!(report.rounds_run > 0);
    }

    // AT-SW17: ConsensusVote participating_agents == swarm size
    #[test]
    fn at_sw17_participating_agents_count() {
        let mut swarm = Swarm::new(default_policy(), default_goal());
        let round = swarm.round();
        assert_eq!(round.consensus.participating_agents, 3);
    }

    // AT-SW18: swarm_size in report equals policy.size
    #[test]
    fn at_sw18_report_swarm_size() {
        let policy = default_policy();
        let size = policy.size;
        let mut swarm = Swarm::new(policy, default_goal());
        let report = swarm.run();
        assert_eq!(report.swarm_size, size);
    }

    // ─── PMHD Integration Tests ───────────────────────────────────────────────

    // AT-SW19: Without drill_config, drill_summary is None in every round
    #[test]
    fn at_sw19_no_drill_config_no_summary() {
        let mut swarm = Swarm::new(default_policy(), default_goal());
        let round = swarm.round();
        assert!(round.drill_summary.is_none(),
            "drill_summary should be None when drill_config is not set");
    }

    // AT-SW20: With drill_config set, drill_summary is Some in every round
    #[test]
    fn at_sw20_drill_config_produces_summary() {
        let policy = SwarmPolicy {
            drill_config: Some(drill_config()),
            ..default_policy()
        };
        let mut swarm = Swarm::new(policy, default_goal());
        let round = swarm.round();
        assert!(round.drill_summary.is_some(),
            "drill_summary should be Some when drill_config is set");
    }

    // AT-SW21: DrillSummary ticks_executed > 0
    #[test]
    fn at_sw21_drill_summary_ticks_positive() {
        let policy = SwarmPolicy {
            drill_config: Some(drill_config()),
            ..default_policy()
        };
        let mut swarm = Swarm::new(policy, default_goal());
        let round = swarm.round();
        let ds = round.drill_summary.unwrap();
        assert!(ds.ticks_executed > 0, "drill must execute at least one tick");
    }

    // AT-SW22: DrillSummary mean_quality is in [0, 1]
    #[test]
    fn at_sw22_drill_summary_quality_range() {
        let policy = SwarmPolicy {
            drill_config: Some(drill_config()),
            ..default_policy()
        };
        let mut swarm = Swarm::new(policy, default_goal());
        swarm.run(); // run all rounds
        // Check every round's drill summary
        // (re-run for inspection since run() consumes rounds internally)
        let mut swarm2 = Swarm::new(
            SwarmPolicy { drill_config: Some(drill_config()), ..default_policy() },
            default_goal(),
        );
        let report = swarm2.run();
        for r in &report.rounds {
            if let Some(ds) = &r.drill_summary {
                assert!(ds.mean_quality >= 0.0 && ds.mean_quality <= 1.0,
                    "round {}: mean_quality {} out of [0,1]", r.round_id, ds.mean_quality);
            }
        }
    }

    // AT-SW23: DrillBacked mode — consensus based on PMHD, not agent scores
    #[test]
    fn at_sw23_drill_backed_mode() {
        let policy = SwarmPolicy {
            size: 3,
            base_seed: 13,
            max_rounds: 15,
            consensus_mode: ConsensusMode::DrillBacked,
            consensus_threshold: 0.0, // any monolith committed → consensus
            drill_config: Some(PmhdConfig {
                ticks: 10,
                pool_size: 5,
                commit_budget: 5,
                seed: 77,
                ..PmhdConfig::default()
            }),
        };
        let mut swarm = Swarm::new(policy, default_goal());
        let report = swarm.run();
        // With threshold=0.0 and non-trivial drill, consensus should be reached
        assert!(report.consensus_reached,
            "DrillBacked with threshold=0.0 should reach consensus");
    }

    // AT-SW24: Drill determinism — same policy + goal → same DrillSummary each round
    #[test]
    fn at_sw24_drill_determinism() {
        let policy = SwarmPolicy {
            drill_config: Some(drill_config()),
            ..default_policy()
        };
        let g = default_goal();
        let mut s1 = Swarm::new(policy.clone(), g.clone());
        let mut s2 = Swarm::new(policy, g);
        let r1 = s1.round();
        let r2 = s2.round();
        let ds1 = r1.drill_summary.unwrap();
        let ds2 = r2.drill_summary.unwrap();
        assert_eq!(ds1, ds2, "drill summary must be deterministic for same inputs");
    }

    // AT-SW25: SwarmRound with drill serialises cleanly
    #[test]
    fn at_sw25_drill_round_serialisation() {
        let policy = SwarmPolicy {
            drill_config: Some(drill_config()),
            ..default_policy()
        };
        let mut swarm = Swarm::new(policy, default_goal());
        let round = swarm.round();
        let json = serde_json::to_string(&round).expect("serialise round");
        let back: SwarmRound = serde_json::from_str(&json).expect("deserialise round");
        assert_eq!(back.round_id, round.round_id);
        assert!(back.drill_summary.is_some());
    }
}
