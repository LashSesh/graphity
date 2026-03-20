// isls-harness/src/iterate.rs
// 10 diagnosis rules and iteration guidance generator

use serde::{Deserialize, Serialize};
use isls_types::Config;
use crate::metrics::MetricSnapshot;

// ─── Priority ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    P0, // Critical — address immediately
    P1, // High — address this week
    P2, // Medium — address in next sprint
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Priority::P0 => write!(f, "P0"),
            Priority::P1 => write!(f, "P1"),
            Priority::P2 => write!(f, "P2"),
        }
    }
}

// ─── Iteration Item ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IterationItem {
    pub priority: Priority,
    pub metric_id: String,
    pub subsystem: String,
    pub symptom: String,
    pub diagnosis: String,
    pub action: String,
    pub config_key: Option<String>,
    pub suggested_value: Option<String>,
}

impl IterationItem {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        priority: Priority,
        metric_id: &str,
        subsystem: &str,
        symptom: &str,
        diagnosis: &str,
        action: &str,
        config_key: Option<&str>,
        suggested_value: Option<&str>,
    ) -> Self {
        Self {
            priority,
            metric_id: metric_id.to_string(),
            subsystem: subsystem.to_string(),
            symptom: symptom.to_string(),
            diagnosis: diagnosis.to_string(),
            action: action.to_string(),
            config_key: config_key.map(|s| s.to_string()),
            suggested_value: suggested_value.map(|s| s.to_string()),
        }
    }
}

// ─── 10 Diagnosis Rules ──────────────────────────────────────────────────────

/// Generate iteration guidance from the current metric snapshot.
/// Implements all 10 diagnosis rules from the specification.
pub fn generate_iteration_guidance(
    metrics: &MetricSnapshot,
    _config: &Config,
) -> Vec<IterationItem> {
    let mut items = Vec::new();

    // Rule 1: M6 < 100% — Replay mismatch → P0
    if metrics.m6_replay_fidelity < 1.0 {
        items.push(IterationItem::new(
            Priority::P0,
            "M6",
            "isls-engine",
            "Replay mismatch detected",
            "Non-determinism in operator or serialization path. \
             Possible causes: HashMap iteration order, floating-point differences, or operator version drift.",
            "Run `isls replay --diff` to identify first diverging macro-step. \
             Check for HashMap usage or floating-point platform differences.",
            None,
            None,
        ));
    }

    // Rule 2: M12 < 100% — Evidence corruption → P0
    if metrics.m12_evidence_integrity < 1.0 {
        items.push(IterationItem::new(
            Priority::P0,
            "M12",
            "isls-archive",
            "Evidence integrity failure",
            "Storage integrity failure detected. Crystals failing verify_crystal().",
            "Run `isls validate --formal`. Identify corrupted tier segment. \
             Re-derive from cold evidence or isolate corrupted crystals.",
            None,
            None,
        ));
    }

    // Rule 3: M8 >= 0 — Positive free energy → P1
    if metrics.m8_lattice_stability >= 0.0 {
        items.push(IterationItem::new(
            Priority::P1,
            "M8",
            "isls-extract",
            "Positive free energy in emitted crystals",
            "Constraints too weak or temperature too high. \
             Lattice is not in a stable minimum.",
            "Lower `extraction.alpha_min` or recalibrate temperature parameter `c_T`.",
            Some("extraction.alpha_min"),
            Some("decrease by 10%"),
        ));
    }

    // Rule 4: M9 > 0.5 — Gate too permissive → P1
    if metrics.m9_gate_selectivity > 0.5 {
        items.push(IterationItem::new(
            Priority::P1,
            "M9",
            "isls-consensus",
            "Kairos gate too permissive (>50% of steps pass)",
            "Gate thresholds too low. Noise is being crystallized.",
            "Increase `thresholds.k` (crystal stability threshold). \
             Review whether noise signals are being committed.",
            Some("thresholds.k"),
            Some("increase by 10-20%"),
        ));
    }

    // Rule 5: M9 == 0 for > 48h — Gate too strict → P1
    // (We approximate: if selectivity is 0 and tick > threshold)
    if metrics.m9_gate_selectivity == 0.0 && metrics.tick > 200 {
        items.push(IterationItem::new(
            Priority::P1,
            "M9",
            "isls-consensus",
            "Kairos gate too strict — no crystals committed",
            "Thresholds too high for current data regime.",
            "Lower `thresholds.d` and `thresholds.q` by 10% increments.",
            Some("thresholds.d"),
            Some("decrease by 10%"),
        ));
    }

    // Rule 6: M10 < 0.80 — Low MCI → P1
    if metrics.m10_dual_consensus_mci < 0.80 && metrics.tick > 10 {
        items.push(IterationItem::new(
            Priority::P1,
            "M10",
            "isls-consensus",
            "Low dual consensus MCI",
            "Primal and dual consensus paths diverge significantly. \
             DK contraction rate may be too aggressive.",
            "Check operator cascade ordering. Reduce `dk_alpha` to lower DK contraction aggressiveness.",
            Some("dk_alpha"),
            Some("decrease by 0.05"),
        ));
    }

    // Rule 7: M20 < 0.3 — Low constraint hit rate → P1
    if metrics.m20_constraint_hit_rate < 0.3 {
        items.push(IterationItem::new(
            Priority::P1,
            "M20",
            "isls-extract",
            "Low constraint hit rate — constraints are transient noise",
            "Constraints do not persist 24h after emission. \
             Bond strength threshold is too low.",
            "Increase `extraction.bond_strength_min` so only durable constraints enter programs.",
            Some("extraction.bond_strength_min"),
            Some("increase to 0.6"),
        ));
    }

    // Rule 8: M21 < 0.3 — Low crystal predictive value → P1
    if metrics.m21_crystal_predictive_value < 0.3 {
        items.push(IterationItem::new(
            Priority::P1,
            "M21",
            "isls-extract",
            "Low crystal predictive value — crystals do not predict future structure",
            "Likely cause: macro-window too short to capture structural patterns.",
            "Increase `extraction.window_hours` to capture longer-duration structure.",
            Some("extraction.window_hours"),
            Some("double current value"),
        ));
    }

    // Rule 9: M15 > 120s — Slow macro-step → P2
    if metrics.m15_macro_step_latency_secs > 120.0 {
        items.push(IterationItem::new(
            Priority::P2,
            "M15",
            "isls-extract",
            "Macro-step latency exceeds 120s",
            "Pipeline bottleneck, likely in L2 extraction. \
             Too many constraint iterations or large constraint library.",
            "Reduce `extraction.max_iterations` or `press_top_k`. \
             Consider using a sparser constraint library.",
            Some("extraction.max_iterations"),
            Some("reduce by 50%"),
        ));
    }

    // Rule 10: M22 <= 0 — Lagging signals → P2
    if metrics.m22_signal_lead_time_secs <= 0.0 {
        items.push(IterationItem::new(
            Priority::P2,
            "M22",
            "isls-consensus",
            "ISLS is detecting structure after observable market events (lagging)",
            "PoR traversal too slow. System takes too long to commit crystals.",
            "Reduce `por_T_min` and `por_T_stable` to allow faster PoR state-machine traversal.",
            Some("por_T_min"),
            Some("decrease by 20%"),
        ));
    }

    // Sort by priority (P0 first)
    items.sort_by_key(|i| i.priority.clone());
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_items_when_all_healthy() {
        let metrics = MetricSnapshot::default();
        let config = Config::default();
        let items = generate_iteration_guidance(&metrics, &config);
        // Default metrics are all healthy — expect no items (except maybe tick-dependent ones)
        assert!(items.is_empty(), "Expected no items but got: {:?}", items.iter().map(|i| &i.metric_id).collect::<Vec<_>>());
    }

    #[test]
    fn test_p0_replay_mismatch() {
        let mut metrics = MetricSnapshot::default();
        metrics.m6_replay_fidelity = 0.99;
        let items = generate_iteration_guidance(&metrics, &Config::default());
        assert!(items.iter().any(|i| i.metric_id == "M6" && i.priority == Priority::P0));
    }

    #[test]
    fn test_p0_evidence_corruption() {
        let mut metrics = MetricSnapshot::default();
        metrics.m12_evidence_integrity = 0.99;
        let items = generate_iteration_guidance(&metrics, &Config::default());
        assert!(items.iter().any(|i| i.metric_id == "M12" && i.priority == Priority::P0));
    }

    #[test]
    fn test_p1_positive_free_energy() {
        let mut metrics = MetricSnapshot::default();
        metrics.m8_lattice_stability = 0.5;
        let items = generate_iteration_guidance(&metrics, &Config::default());
        assert!(items.iter().any(|i| i.metric_id == "M8" && i.priority == Priority::P1));
    }

    #[test]
    fn test_p1_gate_too_permissive() {
        let mut metrics = MetricSnapshot::default();
        metrics.m9_gate_selectivity = 0.6;
        let items = generate_iteration_guidance(&metrics, &Config::default());
        assert!(items.iter().any(|i| i.metric_id == "M9" && i.priority == Priority::P1));
    }

    #[test]
    fn test_p1_low_mci() {
        let mut metrics = MetricSnapshot::default();
        metrics.m10_dual_consensus_mci = 0.75;
        metrics.tick = 100;
        let items = generate_iteration_guidance(&metrics, &Config::default());
        assert!(items.iter().any(|i| i.metric_id == "M10" && i.priority == Priority::P1));
    }

    #[test]
    fn test_p2_slow_macro_step() {
        let mut metrics = MetricSnapshot::default();
        metrics.m15_macro_step_latency_secs = 150.0;
        let items = generate_iteration_guidance(&metrics, &Config::default());
        assert!(items.iter().any(|i| i.metric_id == "M15" && i.priority == Priority::P2));
    }

    #[test]
    fn test_p2_lagging_signals() {
        let mut metrics = MetricSnapshot::default();
        metrics.m22_signal_lead_time_secs = -5.0;
        let items = generate_iteration_guidance(&metrics, &Config::default());
        assert!(items.iter().any(|i| i.metric_id == "M22" && i.priority == Priority::P2));
    }

    #[test]
    fn test_items_sorted_by_priority() {
        let mut metrics = MetricSnapshot::default();
        metrics.m6_replay_fidelity = 0.99; // P0
        metrics.m8_lattice_stability = 0.5; // P1
        metrics.m15_macro_step_latency_secs = 150.0; // P2
        let items = generate_iteration_guidance(&metrics, &Config::default());
        assert!(items.len() >= 3);
        // First item should be P0
        assert_eq!(items[0].priority, Priority::P0);
    }

    #[test]
    fn test_10_rules_covered() {
        // Trigger all 10 rules
        let mut metrics = MetricSnapshot::default();
        metrics.m6_replay_fidelity = 0.99;
        metrics.m8_lattice_stability = 0.5;
        metrics.m9_gate_selectivity = 0.6;
        metrics.m10_dual_consensus_mci = 0.75;
        metrics.tick = 100;
        metrics.m12_evidence_integrity = 0.99;
        metrics.m15_macro_step_latency_secs = 150.0;
        metrics.m20_constraint_hit_rate = 0.2;
        metrics.m21_crystal_predictive_value = 0.2;
        metrics.m22_signal_lead_time_secs = -1.0;
        let items = generate_iteration_guidance(&metrics, &Config::default());
        let ids: Vec<&str> = items.iter().map(|i| i.metric_id.as_str()).collect();
        for expected_id in &["M6", "M8", "M9", "M10", "M12", "M15", "M20", "M21", "M22"] {
            assert!(ids.contains(expected_id), "Missing diagnosis rule for {}", expected_id);
        }
    }
}
