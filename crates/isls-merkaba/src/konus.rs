// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Konus Lens — D_total from geometric mean of all D_k.
//!
//! ```text
//! D_total = (∏ max(D_k, ε))^(1/n) · Ω
//! ```
//!
//! where ε = 10⁻⁶ prevents log-domain collapse, and Ω is the mean pairwise
//! similarity (global coherence). Soft unanimity: if any D_k → 0, D_total → 0.

/// Epsilon to prevent log-domain collapse.
const EPSILON: f64 = 1e-6;

/// Default resonance threshold Θ.
pub const DEFAULT_THRESHOLD: f64 = 0.20;

/// Compute D_total from per-candidate resonance products D_k and mean
/// pairwise similarity Omega.
///
/// Returns D_total = geometric_mean(max(D_k, ε)) * Omega.
///
/// # Properties (from SRI Paper §6.3)
///
/// - **Soft unanimity:** If any D_k → 0, D_total → 0.
/// - **Noise suppression:** Variance scales as O(1/n).
/// - **False positive rate** → 0 as n → ∞.
pub fn compute_dtotal(dk: &[f64], omega: f64) -> f64 {
    if dk.is_empty() {
        return 0.0;
    }

    // Geometric mean via log-domain: exp(mean(ln(max(D_k, ε))))
    let log_sum: f64 = dk
        .iter()
        .map(|d| d.max(EPSILON).ln())
        .sum::<f64>();

    let geo_mean = (log_sum / dk.len() as f64).exp();

    geo_mean * omega
}

/// Compute Omega (mean pairwise similarity) from the upper-triangle
/// pairwise similarity values.
///
/// Ω = (2 / n(n-1)) · Σ_{i<j} sim(c_i, c_j)
pub fn compute_omega(pairwise_sims: &[f64], n: usize) -> f64 {
    if n < 2 || pairwise_sims.is_empty() {
        return 0.0;
    }

    let expected_pairs = n * (n - 1) / 2;
    let sum: f64 = pairwise_sims.iter().sum();

    sum / expected_pairs as f64
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_high_dk_gives_high_dtotal() {
        let dk = vec![0.7, 0.7, 0.7, 0.7];
        let omega = 0.8;
        let dtotal = compute_dtotal(&dk, omega);
        assert!(dtotal > 0.4, "uniform D_k=0.7, Ω=0.8 should give D_total > 0.4, got {}", dtotal);
    }

    #[test]
    fn one_zero_dk_collapses_dtotal() {
        let dk = vec![0.7, 0.7, 0.0, 0.7];
        let omega = 0.8;
        let dtotal = compute_dtotal(&dk, omega);
        assert!(dtotal < 0.05, "one D_k=0.0 should collapse D_total, got {}", dtotal);
    }

    #[test]
    fn all_zero_dk_gives_near_zero() {
        let dk = vec![0.0, 0.0, 0.0, 0.0];
        let dtotal = compute_dtotal(&dk, 0.5);
        assert!(dtotal < 0.001, "all D_k=0.0 should give near-zero D_total, got {}", dtotal);
    }

    #[test]
    fn empty_dk_gives_zero() {
        assert_eq!(compute_dtotal(&[], 1.0), 0.0);
    }

    #[test]
    fn single_dk() {
        let dtotal = compute_dtotal(&[0.6], 1.0);
        assert!((dtotal - 0.6).abs() < 0.01, "single D_k=0.6, Ω=1.0 should give ~0.6, got {}", dtotal);
    }

    #[test]
    fn omega_scales_dtotal() {
        let dk = vec![0.5, 0.5, 0.5, 0.5];
        let dt_high = compute_dtotal(&dk, 0.9);
        let dt_low = compute_dtotal(&dk, 0.3);
        assert!(dt_high > dt_low, "higher Ω should give higher D_total");
    }

    #[test]
    fn compute_omega_uniform() {
        let pairwise = vec![0.8, 0.8, 0.8, 0.8, 0.8, 0.8]; // n=4, 6 pairs
        let omega = compute_omega(&pairwise, 4);
        assert!((omega - 0.8).abs() < 0.001, "uniform pairwise=0.8 should give Ω=0.8, got {}", omega);
    }

    #[test]
    fn compute_omega_empty() {
        assert_eq!(compute_omega(&[], 0), 0.0);
        assert_eq!(compute_omega(&[], 1), 0.0);
    }

    #[test]
    fn compute_omega_two_candidates() {
        let pairwise = vec![0.6]; // n=2, 1 pair
        let omega = compute_omega(&pairwise, 2);
        assert!((omega - 0.6).abs() < 0.001, "n=2 single pair=0.6 should give Ω=0.6, got {}", omega);
    }

    #[test]
    fn dtotal_exceeds_default_threshold_on_good_swarm() {
        let dk = vec![0.6, 0.5, 0.55, 0.58];
        let omega = 0.7;
        let dtotal = compute_dtotal(&dk, omega);
        assert!(
            dtotal >= DEFAULT_THRESHOLD,
            "good swarm should exceed threshold {}, got {}",
            DEFAULT_THRESHOLD, dtotal
        );
    }
}
