// isls-carrier: Tubus, helix, mandorla, phase-ladder (C6)
// depends on isls-types, isls-consensus

use isls_types::{
    CarrierConfig, CarrierInstance, MandorlaState, PhaseLadder, ThresholdConfig, TubusCoord,
};
use isls_consensus::MetricSet;

// ─── Helix Pair ───────────────────────────────────────────────────────────────

/// Helix pair with pi-phase coupling (ISLS Def 7.2, Inv I15)
/// Inv I15: enforces pi offset between helix_a and helix_b
pub fn helix_pair(tau: f64, phi: f64, r: f64) -> (TubusCoord, TubusCoord) {
    let alpha = TubusCoord { tau, phi, r };
    let beta = TubusCoord {
        tau,
        phi: (phi + std::f64::consts::PI) % (2.0 * std::f64::consts::PI),
        r,
    };
    (alpha, beta)
}

// ─── Mandorla Formation ───────────────────────────────────────────────────────

/// Mandorla formation (ISLS Def 7.3, OI-07 resolved)
/// kappa(t) = exp(-lambda * delta_phi(t)) * exp(-mu_r * r(t)^2) in [0,1]
pub fn mandorla(
    alpha: &TubusCoord,
    beta: &TubusCoord,
    lambda: f64,
    mu_r: f64,
) -> MandorlaState {
    let raw_diff = (alpha.phi - beta.phi).abs();
    let delta_phi = raw_diff.min(2.0 * std::f64::consts::PI - raw_diff);
    let kappa = (-lambda * delta_phi).exp() * (-mu_r * alpha.r * alpha.r).exp();
    MandorlaState {
        tau: alpha.tau,
        r: alpha.r,
        delta_phi,
        kappa,
    }
}

// ─── Carrier Migration ────────────────────────────────────────────────────────

/// Carrier migration admissibility (ISLS Def 8.3)
pub fn migration_admissible(
    metrics: &MetricSet,
    target: &CarrierInstance,
    thresholds: &ThresholdConfig,
    config: &CarrierConfig,
) -> bool {
    let friction_or_shock = metrics.f_friction >= thresholds.f_friction
        || metrics.s_shock >= thresholds.s_shock;
    let readiness = config.lambda_q * target.resonance
        + config.lambda_r * 0.5 // target resonance proxy
        + config.lambda_m * target.mandorla.kappa;
    friction_or_shock && readiness >= thresholds.l_migration
}

// ─── Phase Ladder ─────────────────────────────────────────────────────────────

/// Build a phase ladder with `n` evenly spaced carrier instances
pub fn build_phase_ladder(n: usize, tau: f64, r: f64) -> PhaseLadder {
    assert!(n > 0, "phase ladder must have at least 1 carrier");
    let step = 2.0 * std::f64::consts::PI / n as f64;
    (0..n)
        .map(|i| {
            let offset = i as f64 * step;
            let phi = offset;
            let (ha, hb) = helix_pair(tau, phi, r);
            let m = mandorla(&ha, &hb, 1.0, 1.0);
            CarrierInstance {
                helix_a: ha,
                helix_b: hb,
                mandorla: m,
                resonance: 0.0,
                offset,
            }
        })
        .collect()
}

/// Advance the phase ladder by one tick (tau += delta_tau)
pub fn advance_phase_ladder(ladder: &mut PhaseLadder, delta_tau: f64) {
    for carrier in ladder.iter_mut() {
        carrier.helix_a.tau += delta_tau;
        carrier.helix_b.tau += delta_tau;
        carrier.mandorla.tau += delta_tau;
        // Inv I7: phase monotonicity enforced by only advancing forward
        assert!(delta_tau >= 0.0, "phase monotonicity violated: delta_tau < 0");
    }
}

/// Update mandorla state for a carrier instance
pub fn update_carrier_mandorla(carrier: &mut CarrierInstance, lambda: f64, mu_r: f64) {
    carrier.mandorla = mandorla(&carrier.helix_a, &carrier.helix_b, lambda, mu_r);
}

/// Restore carrier to neutral phase (reset for symmetry restoration, AT-20)
pub fn restore_neutrality(carrier: &mut CarrierInstance) {
    let tau = carrier.helix_a.tau;
    let r = carrier.helix_a.r;
    let phi = carrier.offset;
    let (ha, hb) = helix_pair(tau, phi, r);
    let m = mandorla(&ha, &hb, 1.0, 1.0);
    carrier.helix_a = ha;
    carrier.helix_b = hb;
    carrier.mandorla = m;
    carrier.resonance = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helix_pair_pi_offset() {
        let (a, b) = helix_pair(0.0, 0.0, 1.0);
        // b.phi should be pi offset from a.phi
        let diff = (b.phi - a.phi).abs();
        let diff_mod = diff.min(2.0 * std::f64::consts::PI - diff);
        assert!((diff_mod - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn helix_pair_pi_offset_nonzero_phi() {
        let (a, b) = helix_pair(0.0, std::f64::consts::PI, 1.0);
        // b.phi = (pi + pi) % 2pi = 0
        assert!((b.phi - 0.0).abs() < 1e-10, "b.phi = {}", b.phi);
        // diff = pi
        let diff = (a.phi - b.phi).abs();
        let diff_mod = diff.min(2.0 * std::f64::consts::PI - diff);
        assert!((diff_mod - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn mandorla_kappa_at_zero_phase_diff() {
        // For a helix pair with pi offset, delta_phi = pi
        let (a, b) = helix_pair(0.0, 0.0, 0.0); // r=0 so radial part = 1
        let m = mandorla(&a, &b, 1.0, 1.0);
        // kappa = exp(-1 * pi) * exp(-1 * 0) = exp(-pi)
        let expected = (-std::f64::consts::PI).exp();
        assert!((m.kappa - expected).abs() < 1e-10);
    }

    #[test]
    fn mandorla_kappa_in_unit_interval() {
        let (a, b) = helix_pair(0.0, 0.0, 0.5);
        let m = mandorla(&a, &b, 0.5, 0.5);
        assert!(m.kappa >= 0.0 && m.kappa <= 1.0);
    }

    #[test]
    fn build_phase_ladder_size() {
        let ladder = build_phase_ladder(4, 0.0, 1.0);
        assert_eq!(ladder.len(), 4);
    }

    #[test]
    fn advance_phase_ladder_monotonic() {
        let mut ladder = build_phase_ladder(4, 0.0, 1.0);
        advance_phase_ladder(&mut ladder, 0.1);
        for carrier in &ladder {
            assert!((carrier.helix_a.tau - 0.1).abs() < 1e-10);
        }
    }

    #[test]
    fn restore_neutrality_resets_resonance() {
        let mut ladder = build_phase_ladder(1, 0.0, 1.0);
        ladder[0].resonance = 0.9;
        restore_neutrality(&mut ladder[0]);
        assert_eq!(ladder[0].resonance, 0.0);
    }
}
