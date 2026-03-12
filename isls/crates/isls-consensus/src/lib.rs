// isls-consensus: Consensus, PoR gate, proof engine (Layer L3)
// C5 — depends on isls-types, isls-persist

use std::collections::BTreeMap;
use isls_types::{
    ConsensusConfig, ConsensusResult, FiveDState, GateSnapshot, NormalizationConfig,
    PoRTrace, ThresholdConfig,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("consensus failed: {0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, ConsensusError>;

// ─── Normalization Functions (ISLS Sec 10) ───────────────────────────────────

/// Saturation normalization: N(u; mu) = u / (u + mu) (Def 10.1)
pub fn norm_saturate(u: f64, mu: f64) -> f64 {
    u / (u + mu)
}

/// Exponential normalization: N_exp(d; lambda) = exp(-lambda * d) (Def 10.2)
pub fn norm_exp(d: f64, lambda: f64) -> f64 {
    (-lambda * d).exp()
}

// ─── Metric Set ───────────────────────────────────────────────────────────────

/// All 11 metrics from ISLS Sec 10
#[derive(Clone, Debug, Default)]
pub struct MetricSet {
    pub d_deformation: f64,  // Def 10.3: N(D_raw; mu_D)
    pub q_coherence: f64,    // Def 10.4: N(Q_raw; mu_Q)
    pub r_resonance: f64,    // Def 10.5: exp(-d_R(H, s_ref))
    pub g_readiness: f64,    // Def 10.6: gamma_D*D + gamma_Q*Q + gamma_R*R
    pub j_doublekick: f64,   // Def 10.7: N(J_raw; mu_J)
    pub p_projection: f64,   // Def 10.8: exp(-diam(P) * lambda_P)
    pub n_seam: f64,         // Def 10.9: exp(-d_seam(L,R))
    pub k_crystal: f64,      // Def 10.10: lambda_C*C + lambda_E*E
    pub f_friction: f64,     // Def 10.11: N(F_raw; mu_F)
    pub s_shock: f64,        // Def 10.12: N(S_raw; mu_S)
    pub l_migration: f64,    // from carrier readiness
}

impl MetricSet {
    pub fn gate_snapshot(&self, thresholds: &ThresholdConfig) -> GateSnapshot {
        GateSnapshot {
            d: self.d_deformation,
            q: self.q_coherence,
            r: self.r_resonance,
            g: self.g_readiness,
            j: self.j_doublekick,
            p: self.p_projection,
            n: self.n_seam,
            k: self.k_crystal,
            kairos: self.d_deformation >= thresholds.d
                && self.q_coherence >= thresholds.q
                && self.r_resonance >= thresholds.r
                && self.g_readiness >= thresholds.g
                && self.j_doublekick >= thresholds.j
                && self.p_projection >= thresholds.p
                && self.n_seam >= thresholds.n
                && self.k_crystal >= thresholds.k,
        }
    }

    /// Compute g_readiness from components
    pub fn compute_readiness(&mut self, norm: &NormalizationConfig) {
        self.g_readiness = norm.gamma_d * self.d_deformation
            + norm.gamma_q * self.q_coherence
            + norm.gamma_r * self.r_resonance;
    }

    /// Compute k_crystal from components
    pub fn compute_k_crystal(&mut self, coherence: f64, entropy: f64, norm: &NormalizationConfig) {
        self.k_crystal = norm.lambda_c * coherence + norm.lambda_e * (1.0 - entropy.min(1.0));
    }
}

// ─── PoR State Machine ───────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum PoRState {
    Search,
    Lock,
    Verify,
    Commit,
}

pub struct PoRFsm {
    pub state: PoRState,
    pub stability_history: Vec<f64>,
    pub lock_tick: Option<f64>,
    pub verify_tick: Option<f64>,
    pub trace: PoRTrace,
}

impl Default for PoRFsm {
    fn default() -> Self {
        Self {
            state: PoRState::Search,
            stability_history: Vec::new(),
            lock_tick: None,
            verify_tick: None,
            trace: PoRTrace { search_enter: 0.0, ..Default::default() },
        }
    }
}

impl PoRFsm {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, kappa: f64, t2: f64, config: &ConsensusConfig) -> PoRState {
        match self.state {
            PoRState::Search => {
                if kappa >= config.por_kappa_bar {
                    self.stability_history.push(kappa);
                    if self.stability_history.len() >= config.por_t_min {
                        self.state = PoRState::Lock;
                        self.lock_tick = Some(t2);
                        self.trace.lock_enter = Some(t2);
                    }
                } else {
                    self.stability_history.clear();
                }
            }
            PoRState::Lock => {
                let last = self.stability_history.last().copied().unwrap_or(0.0);
                let delta = (kappa - last).abs();
                if delta <= config.por_epsilon {
                    self.stability_history.push(kappa);
                    if self.stability_history.len()
                        >= config.por_t_min + config.por_t_stable
                    {
                        self.state = PoRState::Verify;
                        self.verify_tick = Some(t2);
                        self.trace.verify_enter = Some(t2);
                    }
                } else {
                    self.reset(t2);
                }
            }
            PoRState::Verify => {
                // Check policy constraints and latch time
                // For now, immediately commit if kappa still >= threshold
                if kappa >= config.por_kappa_bar {
                    self.state = PoRState::Commit;
                    self.trace.commit_enter = Some(t2);
                } else {
                    self.reset(t2);
                }
            }
            PoRState::Commit => {
                // Terminal state until reset
            }
        }
        self.state.clone()
    }

    pub fn reset(&mut self, t2: f64) {
        self.state = PoRState::Search;
        self.stability_history.clear();
        self.lock_tick = None;
        self.verify_tick = None;
        self.trace = PoRTrace {
            search_enter: t2,
            lock_enter: None,
            verify_enter: None,
            commit_enter: None,
        };
    }

    pub fn get_trace(&self) -> &PoRTrace {
        &self.trace
    }
}

// ─── Crystal Precursor ───────────────────────────────────────────────────────

/// Precursor for crystal formation (before consensus)
#[derive(Clone, Debug)]
pub struct CrystalPrecursor {
    pub program: isls_types::ConstraintProgram,
    pub region: Vec<isls_types::VertexId>,
    pub seam_score: f64,
    pub metrics: MetricSet,
    pub stability_score: f64,
}

impl CrystalPrecursor {
    pub fn stability_score(&self) -> f64 {
        self.stability_score
    }

    pub fn distance(&self, other: &Self) -> f64 {
        let s1 = self.stability_score;
        let s2 = other.stability_score;
        (s1 - s2).abs()
    }
}

// ─── Cascade Operator Trait ──────────────────────────────────────────────────

/// Cascade operator for dual consensus (DK -> SW -> PI -> WT and reverse)
pub trait CascadeOperator: Send + Sync {
    fn name(&self) -> &str;
    fn apply(&self, precursor: &CrystalPrecursor) -> CrystalPrecursor;
}

// Reference cascade operators
pub struct DKOperator;
pub struct SWOperator;
pub struct PIOperator;
pub struct WTOperator;

impl CascadeOperator for DKOperator {
    fn name(&self) -> &str { "DK" }
    fn apply(&self, p: &CrystalPrecursor) -> CrystalPrecursor {
        let mut out = p.clone();
        // Double-kick: amplify stability by coherence factor
        out.stability_score = (p.stability_score * 1.1).min(1.0);
        out
    }
}

impl CascadeOperator for SWOperator {
    fn name(&self) -> &str { "SW" }
    fn apply(&self, p: &CrystalPrecursor) -> CrystalPrecursor {
        let mut out = p.clone();
        // Symmetry-weave: maintain stability
        out.stability_score = p.stability_score;
        out
    }
}

impl CascadeOperator for PIOperator {
    fn name(&self) -> &str { "PI" }
    fn apply(&self, p: &CrystalPrecursor) -> CrystalPrecursor {
        let mut out = p.clone();
        // Phase integration: smooth
        out.stability_score = (p.stability_score * 0.95 + 0.05).min(1.0);
        out
    }
}

impl CascadeOperator for WTOperator {
    fn name(&self) -> &str { "WT" }
    fn apply(&self, p: &CrystalPrecursor) -> CrystalPrecursor {
        let mut out = p.clone();
        // Wave transfer: finalize
        out.stability_score = (p.stability_score * 1.05).min(1.0);
        out
    }
}

pub fn run_cascade(
    precursor: &CrystalPrecursor,
    ops: &[&dyn CascadeOperator],
) -> CrystalPrecursor {
    let mut state = precursor.clone();
    for op in ops {
        state = op.apply(&state);
    }
    state
}

/// Run primal and dual operator paths; check MCI (OI-04/OI-05)
pub fn dual_consensus(
    precursor: &CrystalPrecursor,
    primal_ops: &[&dyn CascadeOperator], // DK -> SW -> PI -> WT
    dual_ops: &[&dyn CascadeOperator],   // PI -> WT -> DK -> SW
    config: &ConsensusConfig,
) -> ConsensusResult {
    let primal_state = run_cascade(precursor, primal_ops);
    let dual_state = run_cascade(precursor, dual_ops);
    let mci = 1.0 - primal_state.distance(&dual_state);
    ConsensusResult {
        primal_score: primal_state.stability_score(),
        dual_score: dual_state.stability_score(),
        mci,
        threshold: config.consensus_threshold,
    }
}

/// Default primal cascade operators: DK -> SW -> PI -> WT
pub fn default_primal_ops() -> (DKOperator, SWOperator, PIOperator, WTOperator) {
    (DKOperator, SWOperator, PIOperator, WTOperator)
}

/// Default dual cascade operators: PI -> WT -> DK -> SW
pub fn default_dual_ops() -> (PIOperator, WTOperator, DKOperator, SWOperator) {
    (PIOperator, WTOperator, DKOperator, SWOperator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_saturate_basic() {
        assert!((norm_saturate(1.0, 1.0) - 0.5).abs() < 1e-10);
        assert!((norm_saturate(0.0, 1.0) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn norm_exp_basic() {
        assert!((norm_exp(0.0, 1.0) - 1.0).abs() < 1e-10);
        assert!(norm_exp(100.0, 1.0) < 1e-10);
    }

    #[test]
    fn gate_snapshot_kairos_all_high() {
        let metrics = MetricSet {
            d_deformation: 1.0,
            q_coherence: 1.0,
            r_resonance: 1.0,
            g_readiness: 1.0,
            j_doublekick: 1.0,
            p_projection: 1.0,
            n_seam: 1.0,
            k_crystal: 1.0,
            ..Default::default()
        };
        let thresholds = ThresholdConfig::default();
        let gate = metrics.gate_snapshot(&thresholds);
        assert!(gate.kairos);
    }

    #[test]
    fn gate_snapshot_kairos_one_below() {
        let mut metrics = MetricSet {
            d_deformation: 1.0,
            q_coherence: 1.0,
            r_resonance: 1.0,
            g_readiness: 1.0,
            j_doublekick: 1.0,
            p_projection: 1.0,
            n_seam: 1.0,
            k_crystal: 1.0,
            ..Default::default()
        };
        let thresholds = ThresholdConfig::default();
        // Set d below threshold
        metrics.d_deformation = 0.0;
        let gate = metrics.gate_snapshot(&thresholds);
        assert!(!gate.kairos);
    }

    #[test]
    fn por_fsm_transitions_search_to_commit() {
        let mut fsm = PoRFsm::new();
        let config = ConsensusConfig {
            por_kappa_bar: 0.5,
            por_t_min: 2,
            por_t_stable: 1,
            por_epsilon: 0.1,
            ..Default::default()
        };

        // Search -> Lock (need por_t_min=2 steps above threshold)
        assert_eq!(fsm.step(0.8, 1.0, &config), PoRState::Search);
        assert_eq!(fsm.step(0.8, 2.0, &config), PoRState::Lock);
        // Lock -> Verify (need por_t_min + por_t_stable = 3 total steps)
        assert_eq!(fsm.step(0.8, 3.0, &config), PoRState::Verify);
        // Verify -> Commit
        assert_eq!(fsm.step(0.8, 4.0, &config), PoRState::Commit);
    }

    #[test]
    fn por_fsm_resets_on_instability() {
        let mut fsm = PoRFsm::new();
        let config = ConsensusConfig {
            por_kappa_bar: 0.5,
            por_t_min: 3,
            por_t_stable: 2,
            por_epsilon: 0.05,
            ..Default::default()
        };

        // Build up to Lock
        fsm.step(0.8, 1.0, &config);
        fsm.step(0.8, 2.0, &config);
        fsm.step(0.8, 3.0, &config);
        assert_eq!(fsm.state, PoRState::Lock);

        // Large delta should reset
        fsm.step(0.2, 4.0, &config); // big drop - but wait, 0.2 < por_kappa_bar resets in Search
        // Actually in Lock, we check delta. But 0.2 < kappa_bar doesn't matter; delta = |0.2 - 0.8| = 0.6 > epsilon
        assert_eq!(fsm.state, PoRState::Search);
    }

    #[test]
    fn dual_consensus_basic() {
        let precursor = CrystalPrecursor {
            program: Vec::new(),
            region: Vec::new(),
            seam_score: 0.8,
            metrics: MetricSet::default(),
            stability_score: 0.8,
        };
        let (dk, sw, pi, wt) = default_primal_ops();
        let (pi2, wt2, dk2, sw2) = default_dual_ops();
        let primal: Vec<&dyn CascadeOperator> = vec![&dk, &sw, &pi, &wt];
        let dual: Vec<&dyn CascadeOperator> = vec![&pi2, &wt2, &dk2, &sw2];
        let config = ConsensusConfig::default();
        let result = dual_consensus(&precursor, &primal, &dual, &config);
        assert!(result.primal_score >= 0.0);
        assert!(result.dual_score >= 0.0);
        assert!(result.mci >= 0.0 && result.mci <= 1.0);
    }
}
