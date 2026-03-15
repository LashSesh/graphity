// isls-harness/src/synthetic.rs
// Deterministic ground-truth generator with 5 reference scenarios

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use isls_types::{ConstraintTemplate, ConstraintProgram, Observation,
                  MeasurementContext, ProvenanceEnvelope, content_address_raw};

// ─── Regime Type ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum RegimeType {
    Calm,
    Normal,
    Volatile,
}

// ─── Planted Constraint ───────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlantedConstraint {
    pub id: String,
    pub template: ConstraintTemplate,
    pub parameters: BTreeMap<String, f64>,
    pub active_from: usize,   // window number
    pub active_until: usize,  // window number (inclusive)
    pub strength: f64,        // how strongly constraint is enforced in [0,1]
}

// ─── Synthetic Scenario ───────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyntheticScenario {
    pub name: String,
    pub entities: usize,
    pub windows: usize,
    pub seed: u64,
    pub planted_constraints: Vec<PlantedConstraint>,
    pub regime_switches: Vec<(usize, RegimeType)>, // (window, new_regime)
}

/// Scenario kind enum for selecting reference scenarios
#[derive(Clone, Debug, PartialEq)]
pub enum ScenarioKind {
    SBasic,
    SRegime,
    SCausal,
    SBreak,
    SScale,
}

// ─── Recovery Score ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryScore {
    /// Fraction of discovered constraints matching a planted one
    pub precision: f64,
    /// Fraction of planted constraints that were discovered
    pub recall: f64,
    /// Harmonic mean of precision and recall
    pub f1: f64,
    /// Mean |param_discovered - param_planted| for matched constraints
    pub param_accuracy: f64,
}

impl RecoveryScore {
    pub fn perfect() -> Self {
        Self { precision: 1.0, recall: 1.0, f1: 1.0, param_accuracy: 0.0 }
    }

    pub fn zero() -> Self {
        Self { precision: 0.0, recall: 0.0, f1: 0.0, param_accuracy: f64::MAX }
    }
}

// ─── Synthetic Generator ──────────────────────────────────────────────────────

pub struct SyntheticGenerator {
    scenario: SyntheticScenario,
    rng: u64,
}

impl SyntheticGenerator {
    pub fn new(scenario: SyntheticScenario) -> Self {
        Self { scenario, rng: 0 }
    }

    /// Create a reference scenario by kind
    pub fn reference(kind: ScenarioKind) -> Self {
        let scenario = match kind {
            ScenarioKind::SBasic => Self::s_basic(),
            ScenarioKind::SRegime => Self::s_regime(),
            ScenarioKind::SCausal => Self::s_causal(),
            ScenarioKind::SBreak => Self::s_break(),
            ScenarioKind::SScale => Self::s_scale(),
        };
        Self::new(scenario)
    }

    /// S-Basic: 50 entities, 3 planted Band constraints, no regime switches
    fn s_basic() -> SyntheticScenario {
        SyntheticScenario {
            name: "S-Basic".to_string(),
            entities: 50,
            windows: 100,
            seed: 42,
            planted_constraints: vec![
                Self::make_band_constraint("pc1", 0, 100, 0.9, 0.01, 0.005),
                Self::make_band_constraint("pc2", 0, 100, 0.9, 0.05, 0.003),
                Self::make_band_constraint("pc3", 0, 100, 0.9, 0.1, 0.004),
            ],
            regime_switches: vec![],
        }
    }

    /// S-Regime: 200 entities, 5 constraints, 2 regime switches
    fn s_regime() -> SyntheticScenario {
        SyntheticScenario {
            name: "S-Regime".to_string(),
            entities: 200,
            windows: 300,
            seed: 123,
            planted_constraints: vec![
                Self::make_band_constraint("pr1", 0, 300, 0.85, 0.02, 0.004),
                Self::make_corr_constraint("pr2", 0, 300, 0.80, 0.7),
                Self::make_band_constraint("pr3", 100, 200, 0.75, 0.03, 0.005),
                Self::make_corr_constraint("pr4", 50, 250, 0.85, 0.6),
                Self::make_band_constraint("pr5", 0, 300, 0.90, 0.015, 0.003),
            ],
            regime_switches: vec![
                (100, RegimeType::Volatile),
                (200, RegimeType::Calm),
            ],
        }
    }

    /// S-Causal: 100 entities, 3 Granger causal chains A->B->C, planted with 2h lag
    fn s_causal() -> SyntheticScenario {
        SyntheticScenario {
            name: "S-Causal".to_string(),
            entities: 100,
            windows: 200,
            seed: 77,
            planted_constraints: vec![
                Self::make_granger_constraint("pc1", 0, 200, 0.85, 2.0),
                Self::make_granger_constraint("pc2", 0, 200, 0.80, 2.0),
                Self::make_granger_constraint("pc3", 0, 200, 0.75, 2.0),
            ],
            regime_switches: vec![],
        }
    }

    /// S-Break: 200 entities, 4 constraints active for 500 windows then breaking
    fn s_break() -> SyntheticScenario {
        SyntheticScenario {
            name: "S-Break".to_string(),
            entities: 200,
            windows: 600,
            seed: 999,
            planted_constraints: vec![
                Self::make_band_constraint("pb1", 0, 500, 0.90, 0.02, 0.004),
                Self::make_corr_constraint("pb2", 0, 500, 0.85, 0.65),
                Self::make_band_constraint("pb3", 0, 500, 0.80, 0.03, 0.005),
                Self::make_corr_constraint("pb4", 0, 500, 0.80, 0.55),
            ],
            regime_switches: vec![
                (500, RegimeType::Volatile),
            ],
        }
    }

    /// S-Scale: 2000 entities, 20 constraints, full regime complexity
    fn s_scale() -> SyntheticScenario {
        let mut constraints = Vec::new();
        for i in 0..20 {
            let start_w = (i * 50) % 500;
            let end_w = start_w + 400;
            let strength = 0.7 + (i as f64 * 0.01).min(0.25);
            if i % 3 == 0 {
                constraints.push(Self::make_granger_constraint(
                    &format!("ps{}", i), start_w, end_w, strength, 1.0 + i as f64 * 0.1,
                ));
            } else if i % 3 == 1 {
                constraints.push(Self::make_corr_constraint(
                    &format!("ps{}", i), start_w, end_w, strength, 0.5 + i as f64 * 0.02,
                ));
            } else {
                constraints.push(Self::make_band_constraint(
                    &format!("ps{}", i), start_w, end_w, strength, 0.02 + i as f64 * 0.001, 0.003,
                ));
            }
        }
        SyntheticScenario {
            name: "S-Scale".to_string(),
            entities: 2000,
            windows: 600,
            seed: 31415,
            planted_constraints: constraints,
            regime_switches: vec![
                (100, RegimeType::Volatile),
                (200, RegimeType::Calm),
                (400, RegimeType::Volatile),
                (500, RegimeType::Normal),
            ],
        }
    }

    fn make_band_constraint(id: &str, from: usize, until: usize, strength: f64, center: f64, width: f64) -> PlantedConstraint {
        let mut params = BTreeMap::new();
        params.insert("center".to_string(), center);
        params.insert("width".to_string(), width);
        PlantedConstraint { id: id.to_string(), template: ConstraintTemplate::Band, parameters: params, active_from: from, active_until: until, strength }
    }

    fn make_corr_constraint(id: &str, from: usize, until: usize, strength: f64, rho: f64) -> PlantedConstraint {
        let mut params = BTreeMap::new();
        params.insert("rho".to_string(), rho);
        PlantedConstraint { id: id.to_string(), template: ConstraintTemplate::Correlation, parameters: params, active_from: from, active_until: until, strength }
    }

    fn make_granger_constraint(id: &str, from: usize, until: usize, strength: f64, lag_h: f64) -> PlantedConstraint {
        let mut params = BTreeMap::new();
        params.insert("lag_hours".to_string(), lag_h);
        PlantedConstraint { id: id.to_string(), template: ConstraintTemplate::Granger, parameters: params, active_from: from, active_until: until, strength }
    }

    fn next_rand(&mut self) -> f64 {
        self.rng = self.rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng as f64) / (u64::MAX as f64)
    }

    fn rand_normal(&mut self, mean: f64, std: f64) -> f64 {
        // Box-Muller
        let u1 = self.next_rand().max(1e-10);
        let u2 = self.next_rand();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        mean + std * z
    }

    /// Generate all observation windows
    pub fn generate(&mut self) -> Vec<Vec<Observation>> {
        self.rng = self.scenario.seed;
        let n = self.scenario.entities;
        let w = self.scenario.windows;

        eprintln!("S-Basic: planting {} constraints", self.scenario.planted_constraints.len());
        for pc in &self.scenario.planted_constraints {
            eprintln!("  planted {:?} '{}': windows {}..={}, strength={:.3}, params={:?}",
                      pc.template, pc.id, pc.active_from, pc.active_until,
                      pc.strength, pc.parameters);
        }

        let mut current_regime = RegimeType::Normal;
        let mut result = Vec::with_capacity(w);

        for window_idx in 0..w {
            // Check regime switches
            for &(sw_window, ref new_regime) in &self.scenario.regime_switches {
                if window_idx == sw_window {
                    current_regime = new_regime.clone();
                }
            }

            let noise_scale = match current_regime {
                RegimeType::Calm => 0.01,
                RegimeType::Normal => 0.05,
                RegimeType::Volatile => 0.2,
            };

            let mut obs_window = Vec::with_capacity(n);
            for entity_idx in 0..n {
                // Base value with planted constraints contributing signal
                let mut value = self.rand_normal(0.0, noise_scale);

                // Add planted constraint signal
                for pc in &self.scenario.planted_constraints {
                    if window_idx < pc.active_from || window_idx > pc.active_until {
                        continue;
                    }
                    let signal = match pc.template {
                        ConstraintTemplate::Band => {
                            let center = pc.parameters.get("center").copied().unwrap_or(0.0);
                            let width = pc.parameters.get("width").copied().unwrap_or(0.01);
                            // Constrain to band: pull value toward center
                            (center - value) * pc.strength * width
                        }
                        ConstraintTemplate::Correlation => {
                            let rho = pc.parameters.get("rho").copied().unwrap_or(0.5);
                            // Correlated across entities: use deterministic shared signal
                            // based on window index and seed (avoid borrow conflict)
                            let mut shared_rng = self.scenario.seed.wrapping_add(window_idx as u64 * 31337);
                            shared_rng = shared_rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                            let u = (shared_rng as f64) / (u64::MAX as f64) * 2.0 - 1.0;
                            u * 0.1 * rho * pc.strength
                        }
                        ConstraintTemplate::Granger => {
                            let lag = pc.parameters.get("lag_hours").copied().unwrap_or(2.0);
                            let lag_windows = (lag * 10.0) as usize;
                            if window_idx >= lag_windows {
                                // Causal: add lagged signal
                                let lag_idx = window_idx - lag_windows;
                                (lag_idx as f64 * 0.001 + entity_idx as f64 * 0.0001) * pc.strength
                            } else {
                                0.0
                            }
                        }
                        _ => 0.0,
                    };
                    value += signal;
                }

                let payload = serde_json::json!({
                    "entity": entity_idx,
                    "value": value,
                    "window": window_idx,
                }).to_string().into_bytes();
                let digest = content_address_raw(&payload);

                obs_window.push(Observation {
                    timestamp: (window_idx * 3600) as f64, // intrinsic time
                    source_id: entity_idx.to_string(),
                    provenance: ProvenanceEnvelope::default(),
                    payload,
                    context: MeasurementContext::default(),
                    digest,
                    schema_version: "1.0".to_string(),
                });
            }
            result.push(obs_window);
        }
        result
    }

    /// Score ISLS's recovery of planted constraints
    pub fn score_recovery(
        planted: &[PlantedConstraint],
        discovered: &ConstraintProgram,
    ) -> RecoveryScore {
        if planted.is_empty() {
            if discovered.is_empty() {
                return RecoveryScore::perfect();
            }
            return RecoveryScore { precision: 0.0, recall: 1.0, f1: 0.0, param_accuracy: 0.0 };
        }
        if discovered.is_empty() {
            return RecoveryScore::zero();
        }

        // Match planted to discovered by template type
        let mut matched_planted = 0usize;
        let mut matched_discovered = 0usize;
        let mut param_errors = Vec::new();

        for p in planted {
            for d in discovered.iter() {
                if d.template == p.template {
                    matched_planted += 1;
                    // Compare parameters
                    for (k, pv) in &p.parameters {
                        if let Some(&dv) = d.parameters.get(k) {
                            param_errors.push((dv - pv).abs());
                        }
                    }
                    break;
                }
            }
        }

        for d in discovered.iter() {
            if planted.iter().any(|p| p.template == d.template) {
                matched_discovered += 1;
            }
        }

        let precision = matched_discovered as f64 / discovered.len() as f64;
        let recall = matched_planted as f64 / planted.len() as f64;
        let f1 = if precision + recall > 0.0 {
            2.0 * precision * recall / (precision + recall)
        } else {
            0.0
        };
        let param_accuracy = if param_errors.is_empty() {
            0.0
        } else {
            param_errors.iter().sum::<f64>() / param_errors.len() as f64
        };

        RecoveryScore { precision, recall, f1, param_accuracy }
    }

    pub fn scenario(&self) -> &SyntheticScenario {
        &self.scenario
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isls_types::ConstraintCandidate;

    #[test]
    fn test_s_basic_generates_observations() {
        let mut gen = SyntheticGenerator::reference(ScenarioKind::SBasic);
        let windows = gen.generate();
        assert_eq!(windows.len(), 100); // 100 windows
        assert_eq!(windows[0].len(), 50); // 50 entities
    }

    #[test]
    fn test_s_regime_has_two_regime_switches() {
        let gen = SyntheticGenerator::reference(ScenarioKind::SRegime);
        assert_eq!(gen.scenario.regime_switches.len(), 2);
    }

    #[test]
    fn test_s_causal_has_granger_constraints() {
        let gen = SyntheticGenerator::reference(ScenarioKind::SCausal);
        assert!(gen.scenario.planted_constraints.iter().all(|c| c.template == ConstraintTemplate::Granger));
    }

    #[test]
    fn test_s_break_constraints_expire() {
        let gen = SyntheticGenerator::reference(ScenarioKind::SBreak);
        assert_eq!(gen.scenario.planted_constraints.len(), 4);
        assert!(gen.scenario.planted_constraints.iter().all(|c| c.active_until == 500));
    }

    #[test]
    fn test_s_scale_has_20_constraints() {
        let gen = SyntheticGenerator::reference(ScenarioKind::SScale);
        assert_eq!(gen.scenario.planted_constraints.len(), 20);
        assert_eq!(gen.scenario.entities, 2000);
    }

    #[test]
    fn test_five_scenarios_exist() {
        for kind in &[
            ScenarioKind::SBasic, ScenarioKind::SRegime, ScenarioKind::SCausal,
            ScenarioKind::SBreak, ScenarioKind::SScale,
        ] {
            let gen = SyntheticGenerator::reference(kind.clone());
            assert!(!gen.scenario.name.is_empty());
        }
    }

    #[test]
    fn test_score_recovery_perfect() {
        let planted = vec![PlantedConstraint {
            id: "p1".into(),
            template: ConstraintTemplate::Band,
            parameters: BTreeMap::new(),
            active_from: 0, active_until: 100, strength: 0.9,
        }];
        let discovered = vec![ConstraintCandidate {
            id: [0u8; 32],
            template: ConstraintTemplate::Band,
            parameters: BTreeMap::new(),
            coverage: 0.9,
            threshold: 0.5,
            formation_energy: -0.5,
            bond_strength: 10u64,
            activation_energy: 0.1,
        }];
        let score = SyntheticGenerator::score_recovery(&planted, &discovered);
        assert!(score.f1 > 0.0);
    }

    #[test]
    fn test_score_recovery_zero_on_empty_discovered() {
        let planted = vec![PlantedConstraint {
            id: "p1".into(),
            template: ConstraintTemplate::Band,
            parameters: BTreeMap::new(),
            active_from: 0, active_until: 100, strength: 0.9,
        }];
        let empty_discovered: Vec<isls_types::ConstraintCandidate> = vec![];
        let score = SyntheticGenerator::score_recovery(&planted, &empty_discovered);
        assert_eq!(score.f1, 0.0);
    }

    #[test]
    fn test_generate_is_deterministic() {
        let mut gen1 = SyntheticGenerator::reference(ScenarioKind::SBasic);
        let mut gen2 = SyntheticGenerator::reference(ScenarioKind::SBasic);
        let w1 = gen1.generate();
        let w2 = gen2.generate();
        // Compare first observation payload of first window
        assert_eq!(w1[0][0].payload, w2[0][0].payload);
    }
}
