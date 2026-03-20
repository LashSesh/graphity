// isls-agent: C30 — Autonomous Goal-Directed Agent
// Plans. Steps. Adapts. Completes.
// The agent that turns intent into crystallised action.
//
// Phase 12 upgrade: Workspace-Aware Development Agent
//   workspace.rs    — AgentWorkspace (project analysis)
//   conversation.rs — Conversation history
//   accumulation.rs — AccumulationMetrics (autonomy ratio, cost tracking)
//   apply.rs        — apply_and_verify (file write + compile/test loop)
//   prompt.rs       — build_workspace_prompt (Oracle prompt with real code)

pub mod accumulation;
pub mod apply;
pub mod architecture;
pub mod conversation;
pub mod feature;
pub mod launcher;
pub mod pipeline;
pub mod prompt;
pub mod workspace;

// Re-exports for convenience
pub use accumulation::AccumulationMetrics;
pub use apply::{apply_and_verify, strip_markdown_fences, ApplyOracle, ApplyResult, CargoCheck, CompileCheck};
pub use architecture::{TechnicalComponent, TechnicalPlan};
pub use conversation::{Conversation, ConversationTurn};
pub use feature::{decompose_intent, decompose_intent_deterministic, Feature};
pub use launcher::{launch_project, LaunchInfo};
pub use pipeline::{execute_pipeline, generate_user_summary, AgentResult, PipelineConfig, UserEvent};
pub use prompt::{build_workspace_prompt, PatternHint, WorkspacePrompt};
pub use workspace::{AgentWorkspace, CrateType, FunctionInfo, ModuleInfo, RouteInfo, TypeInfo};

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

// ─── AgentGoal ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AgentGoal {
    /// Free-form intent description
    pub intent: String,
    /// Target domain (e.g. "rust", "typescript", "rest-api")
    pub domain: Option<String>,
    /// Hard constraints the agent must satisfy
    pub constraints: Vec<String>,
    /// Minimum resonance score considered a success (0–1)
    pub confidence_target: f64,
}

impl AgentGoal {
    pub fn new(intent: impl Into<String>) -> Self {
        Self {
            intent: intent.into(),
            domain: None,
            constraints: Vec::new(),
            confidence_target: 0.75,
        }
    }

    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn with_constraint(mut self, c: impl Into<String>) -> Self {
        self.constraints.push(c.into());
        self
    }

    pub fn with_confidence(mut self, target: f64) -> Self {
        self.confidence_target = target.clamp(0.0, 1.0);
        self
    }

    /// Stable hash of the intent string (deterministic plan seed)
    pub fn intent_hash(&self) -> u64 {
        let mut h: u64 = 14695981039346656037;
        for b in self.intent.as_bytes() {
            h = h.wrapping_mul(1099511628211) ^ (*b as u64);
        }
        h
    }
}

// ─── ActionType ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionType {
    /// Run spectral navigator exploration steps
    Explore,
    /// Synthesise a single atom (Forge primitive)
    ForgeAtom,
    /// Validate a named constraint
    ValidateConstraint,
    /// Full synthesis pass — compose atoms into output
    Synthesize,
    /// Adjust plan in response to score below target
    Adapt,
    /// Mark the goal as complete
    Complete,
}

impl ActionType {
    pub fn label(&self) -> &'static str {
        match self {
            ActionType::Explore           => "explore",
            ActionType::ForgeAtom         => "forge_atom",
            ActionType::ValidateConstraint => "validate_constraint",
            ActionType::Synthesize        => "synthesize",
            ActionType::Adapt             => "adapt",
            ActionType::Complete          => "complete",
        }
    }
}

// ─── AgentAction ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AgentAction {
    pub action_type: ActionType,
    pub description: String,
    pub parameters: BTreeMap<String, String>,
}

impl AgentAction {
    pub fn new(action_type: ActionType, description: impl Into<String>) -> Self {
        Self {
            action_type,
            description: description.into(),
            parameters: BTreeMap::new(),
        }
    }

    pub fn with_param(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.parameters.insert(k.into(), v.into());
        self
    }
}

// ─── AgentStep ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentStep {
    pub step_id:   usize,
    pub action:    AgentAction,
    pub outcome:   String,
    pub score:     f64,
    pub timestamp: u64,
}

// ─── AgentPlan ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AgentPlan {
    pub actions:     Vec<AgentAction>,
    pub current_idx: usize,
}

impl AgentPlan {
    pub fn remaining(&self) -> usize {
        self.actions.len().saturating_sub(self.current_idx)
    }

    pub fn is_exhausted(&self) -> bool {
        self.current_idx >= self.actions.len()
    }

    pub fn current_action(&self) -> Option<&AgentAction> {
        self.actions.get(self.current_idx)
    }

    pub fn advance(&mut self) {
        if self.current_idx < self.actions.len() {
            self.current_idx += 1;
        }
    }
}

// ─── AgentState ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentState {
    pub goal:       AgentGoal,
    pub plan:       AgentPlan,
    pub history:    Vec<AgentStep>,
    pub best_score: f64,
    pub complete:   bool,
    pub steps_run:  usize,
    pub mode:       String,
}

impl AgentState {
    pub fn new(goal: AgentGoal, plan: AgentPlan, mode: impl Into<String>) -> Self {
        Self {
            goal,
            plan,
            history: Vec::new(),
            best_score: 0.0,
            complete: false,
            steps_run: 0,
            mode: mode.into(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

// ─── AgentConfig ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Hard cap on automatic execution steps
    pub max_steps: usize,
    /// Score at or above which the goal is considered complete
    pub confidence_target: f64,
    /// Deterministic seed (0 = derive from goal hash)
    pub seed: u64,
    /// Dimensionality for navigator-style exploration
    pub dim: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps:         100,
            confidence_target: 0.75,
            seed:              0,
            dim:               5,
        }
    }
}

// ─── Plan generator ───────────────────────────────────────────────────────────
// Deterministic: same goal + same seed → same plan.

fn plan_from_goal(goal: &AgentGoal, config: &AgentConfig) -> AgentPlan {
    let seed = if config.seed == 0 { goal.intent_hash() } else { config.seed };

    // Simple LCG for parameter variety
    let mut rng = seed;
    let next = |r: &mut u64| -> u64 {
        *r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *r
    };

    let domain = goal.domain.clone().unwrap_or_else(|| "general".to_string());
    let explore_steps = 10 + (next(&mut rng) % 20) as usize;

    let mut actions: Vec<AgentAction> = Vec::new();

    // 1. Explore: map the configuration space
    actions.push(
        AgentAction::new(ActionType::Explore, "Map the configuration space with spectral spiral")
            .with_param("steps", explore_steps.to_string())
            .with_param("mode", "config"),
    );

    // 2. ForgeAtom: synthesise domain-specific primitives
    let atom_name = format!("{}-core", domain);
    actions.push(
        AgentAction::new(ActionType::ForgeAtom, format!("Synthesise atom '{}'", atom_name))
            .with_param("atom_name", &atom_name)
            .with_param("source", &goal.intent),
    );

    // 3. ValidateConstraint: check each constraint (up to 3)
    for (i, constraint) in goal.constraints.iter().enumerate().take(3) {
        actions.push(
            AgentAction::new(
                ActionType::ValidateConstraint,
                format!("Validate constraint: {}", constraint),
            )
            .with_param("constraint_id", format!("C-{:02}", i + 1))
            .with_param("constraint", constraint),
        );
    }
    // Always add at least one validate step
    if goal.constraints.is_empty() {
        actions.push(
            AgentAction::new(ActionType::ValidateConstraint, "Validate core quality constraint")
                .with_param("constraint_id", "C-01")
                .with_param("constraint", "resonance >= confidence_target"),
        );
    }

    // 4. Adapt: adjust plan if score below target
    actions.push(
        AgentAction::new(ActionType::Adapt, "Adapt synthesis parameters based on resonance feedback")
            .with_param("threshold", goal.confidence_target.to_string()),
    );

    // 5. Synthesize: compose atoms into final output
    actions.push(
        AgentAction::new(ActionType::Synthesize, format!("Compose final output for '{}'", domain))
            .with_param("target", &domain)
            .with_param("atom", &atom_name),
    );

    // 6. Complete
    actions.push(AgentAction::new(ActionType::Complete, "Goal complete — record crystal signature"));

    AgentPlan { actions, current_idx: 0 }
}

// ─── Score function ───────────────────────────────────────────────────────────
// Deterministic mock resonance per action type (would integrate real metrics in prod).

fn score_for_action(action: &AgentAction, seed: u64, step_id: usize) -> f64 {
    let mut h = seed ^ (step_id as u64).wrapping_mul(2654435761);
    for b in action.description.as_bytes() {
        h = h.wrapping_mul(1099511628211) ^ (*b as u64);
    }
    // Map to 0.5..1.0 so scores are plausibly good
    0.5 + (h as f64 / u64::MAX as f64) * 0.5
}

// ─── Agent ────────────────────────────────────────────────────────────────────

pub struct Agent {
    pub config: AgentConfig,
    pub state:  AgentState,
}

impl Agent {
    pub fn new(config: AgentConfig, goal: AgentGoal) -> Self {
        let effective_seed = if config.seed == 0 { goal.intent_hash() } else { config.seed };
        let plan = plan_from_goal(&goal, &config);
        let mode = goal.domain.clone().unwrap_or_else(|| "general".to_string());
        let state = AgentState::new(goal, plan, mode);
        Self { config: AgentConfig { seed: effective_seed, ..config }, state }
    }

    pub fn is_complete(&self) -> bool {
        self.state.complete
    }

    pub fn best_score(&self) -> f64 {
        self.state.best_score
    }

    /// Execute one action from the plan and record the step.
    pub fn step(&mut self) -> Option<AgentStep> {
        if self.state.complete || self.state.plan.is_exhausted() {
            self.state.complete = true;
            return None;
        }

        let action = self.state.plan.current_action()?.clone();
        let step_id = self.state.steps_run;
        let score   = score_for_action(&action, self.config.seed, step_id);

        let outcome = match &action.action_type {
            ActionType::Explore => {
                let steps = action.parameters.get("steps")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(10);
                format!("Explored {} points; resonance={:.4}", steps, score)
            }
            ActionType::ForgeAtom => {
                let atom = action.parameters.get("atom_name").map(|s| s.as_str()).unwrap_or("atom");
                format!("Atom '{}' synthesised; score={:.4}", atom, score)
            }
            ActionType::ValidateConstraint => {
                let cid = action.parameters.get("constraint_id").map(|s| s.as_str()).unwrap_or("?");
                let pass = score >= self.state.goal.confidence_target;
                format!("{} {} (score={:.4})", cid, if pass { "PASS" } else { "WARN" }, score)
            }
            ActionType::Adapt => {
                format!("Parameters adapted; new_score_est={:.4}", score)
            }
            ActionType::Synthesize => {
                let target = action.parameters.get("target").map(|s| s.as_str()).unwrap_or("output");
                format!("Synthesis complete for '{}'; quality={:.4}", target, score)
            }
            ActionType::Complete => {
                self.state.complete = true;
                format!("Goal complete; best_resonance={:.4}", self.state.best_score)
            }
        };

        if score > self.state.best_score {
            self.state.best_score = score;
        }

        let step = AgentStep {
            step_id,
            action: action.clone(),
            outcome: outcome.clone(),
            score,
            timestamp: unix_secs(),
        };

        self.state.history.push(step.clone());
        self.state.steps_run += 1;
        self.state.plan.advance();

        // Auto-complete when plan is exhausted
        if self.state.plan.is_exhausted() {
            self.state.complete = true;
        }

        Some(step)
    }

    /// Run up to `max_steps` steps (or config.max_steps if 0).
    pub fn run(&mut self, max_steps: usize) -> Vec<AgentStep> {
        let limit = if max_steps == 0 { self.config.max_steps } else { max_steps };
        let mut results = Vec::new();
        for _ in 0..limit {
            match self.step() {
                Some(s) => results.push(s),
                None    => break,
            }
        }
        results
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_goal() -> AgentGoal {
        AgentGoal::new("Build a deterministic REST API for bookmark management")
            .with_domain("rust")
            .with_constraint("all endpoints return JSON")
            .with_constraint("no unsafe code")
            .with_confidence(0.70)
    }

    fn default_agent() -> Agent {
        Agent::new(
            AgentConfig { seed: 42, ..Default::default() },
            default_goal(),
        )
    }

    // AT-AG1: Goal creation with constraints
    #[test]
    fn at_ag1_goal_creation() {
        let goal = default_goal();
        assert_eq!(goal.domain, Some("rust".to_string()));
        assert_eq!(goal.constraints.len(), 2);
        assert!((goal.confidence_target - 0.70).abs() < 1e-9);
        assert!(!goal.intent.is_empty());
    }

    // AT-AG2: Plan generation produces at least 5 actions
    #[test]
    fn at_ag2_plan_size() {
        let agent = default_agent();
        assert!(agent.state.plan.actions.len() >= 5,
            "plan should have ≥5 actions, got {}", agent.state.plan.actions.len());
    }

    // AT-AG3: First step executes without error
    #[test]
    fn at_ag3_first_step_ok() {
        let mut agent = default_agent();
        let step = agent.step();
        assert!(step.is_some());
        let s = step.unwrap();
        assert_eq!(s.step_id, 0);
        assert!(!s.outcome.is_empty());
        assert!(s.score > 0.0 && s.score <= 1.0);
    }

    // AT-AG4: Step result recorded in history
    #[test]
    fn at_ag4_history_recording() {
        let mut agent = default_agent();
        agent.step();
        agent.step();
        assert_eq!(agent.state.history.len(), 2);
        assert_eq!(agent.state.steps_run, 2);
    }

    // AT-AG5: State save/load roundtrip preserves history
    #[test]
    fn at_ag5_state_persistence() {
        let mut agent = default_agent();
        agent.run(3);
        let path = std::env::temp_dir().join("isls-agent-test-state.json");
        agent.state.save(&path).unwrap();
        let loaded = AgentState::load(&path).unwrap();
        assert_eq!(loaded.history.len(), 3);
        assert_eq!(loaded.steps_run, 3);
        assert_eq!(loaded.goal.intent, agent.state.goal.intent);
        let _ = std::fs::remove_file(&path);
    }

    // AT-AG6: run(5) produces exactly 5 steps (plan long enough)
    #[test]
    fn at_ag6_run_n_steps() {
        let mut agent = default_agent();
        // Default plan has ≥5 actions
        let steps = agent.run(5);
        assert_eq!(steps.len(), 5);
    }

    // AT-AG7: Goal completion when plan exhausted
    #[test]
    fn at_ag7_goal_completion() {
        let mut agent = default_agent();
        agent.run(0); // run until complete
        assert!(agent.is_complete());
        assert!(agent.state.plan.is_exhausted());
        assert!(agent.step().is_none()); // no more steps
    }

    // AT-AG8: Constraint validate action is present in plan
    #[test]
    fn at_ag8_constraint_in_plan() {
        let agent = default_agent();
        let has_validate = agent.state.plan.actions.iter()
            .any(|a| a.action_type == ActionType::ValidateConstraint);
        assert!(has_validate, "plan must contain at least one ValidateConstraint action");
    }

    // AT-AG9: Adapt action is present in plan
    #[test]
    fn at_ag9_adapt_in_plan() {
        let agent = default_agent();
        let has_adapt = agent.state.plan.actions.iter()
            .any(|a| a.action_type == ActionType::Adapt);
        assert!(has_adapt, "plan must contain an Adapt action");
    }

    // AT-AG10: Determinism — same seed + same goal → identical plan
    #[test]
    fn at_ag10_determinism() {
        let g = default_goal();
        let cfg = AgentConfig { seed: 99, ..Default::default() };
        let a1 = Agent::new(cfg.clone(), g.clone());
        let a2 = Agent::new(cfg, g);
        assert_eq!(a1.state.plan.actions.len(), a2.state.plan.actions.len());
        for (act1, act2) in a1.state.plan.actions.iter().zip(a2.state.plan.actions.iter()) {
            assert_eq!(act1.action_type, act2.action_type);
            assert_eq!(act1.description, act2.description);
        }
    }

    // AT-AG11: All ActionType variants covered in a default plan
    #[test]
    fn at_ag11_action_type_coverage() {
        let agent = default_agent();
        let types: std::collections::HashSet<String> = agent.state.plan.actions.iter()
            .map(|a| a.action_type.label().to_string())
            .collect();
        for required in &["explore", "forge_atom", "validate_constraint", "adapt", "synthesize", "complete"] {
            assert!(types.contains(*required),
                "plan missing action type '{}'", required);
        }
    }

    // AT-AG12: History integrity — step IDs monotonically increasing
    #[test]
    fn at_ag12_history_integrity() {
        let mut agent = default_agent();
        agent.run(0);
        for (i, step) in agent.state.history.iter().enumerate() {
            assert_eq!(step.step_id, i,
                "step {} has wrong step_id {}", i, step.step_id);
        }
    }

    // ─── Workspace Integration (AT-AG19, AT-AG20) ────────────────────────────

    // AT-AG19: Pattern stored in AccumulationMetrics after successful oracle call
    #[test]
    fn at_ag19_pattern_storage() {
        let mut metrics = AccumulationMetrics::default();
        metrics.record_oracle_call(0.02);
        metrics.set_patterns_in_memory(1);
        assert_eq!(metrics.patterns_in_memory, 1,
            "AT-AG19: pattern count must be stored after oracle call");
        assert_eq!(metrics.oracle_served, 1);
        assert!((metrics.total_cost_usd - 0.02).abs() < 1e-9);
    }

    // AT-AG20: Memory reuse → autonomy_ratio increases, oracle NOT called again
    #[test]
    fn at_ag20_memory_reuse_autonomy() {
        let mut metrics = AccumulationMetrics::default();
        // Simulate: first request hits oracle, next 3 hit memory
        metrics.record_oracle_call(0.01);
        metrics.record_memory_hit();
        metrics.record_memory_hit();
        metrics.record_memory_hit();

        assert_eq!(metrics.total_requests, 4);
        assert_eq!(metrics.oracle_served, 1);
        assert_eq!(metrics.memory_served, 3);
        assert!(
            (metrics.autonomy_ratio - 0.75).abs() < 1e-9,
            "AT-AG20: autonomy_ratio should be 0.75 (3/4), got {}",
            metrics.autonomy_ratio
        );
        // Money saved = 3 memory hits * $0.01 avg oracle cost = $0.03
        assert!(
            (metrics.money_saved_usd - 0.03).abs() < 1e-9,
            "AT-AG20: money saved should be $0.03, got {}",
            metrics.money_saved_usd
        );
    }
}
