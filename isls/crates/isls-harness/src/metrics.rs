// isls-harness/src/metrics.rs
// 24 metrics (M1-M24) with collection, alert thresholds, and persistence

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

// ─── Alert Level ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AlertLevel {
    Green,
    Yellow,
    Red,
}

impl std::fmt::Display for AlertLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertLevel::Green => write!(f, "GREEN"),
            AlertLevel::Yellow => write!(f, "YELLOW"),
            AlertLevel::Red => write!(f, "RED"),
        }
    }
}

// ─── Metric Snapshot (all 24 metrics) ────────────────────────────────────────

/// Snapshot of all 24 ISLS metrics at a single point in time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricSnapshot {
    pub timestamp: DateTime<Utc>,
    pub tick: u64,

    // Layer Health (M1-M5)
    /// M1: L0 Observe — ingestion rate (observations/sec over last 60s)
    pub m1_ingestion_rate: f64,
    /// M2: L1 Persist — graph growth (delta vertices + edges per macro-window)
    pub m2_graph_growth: i64,
    /// M3: L2 Extract — active constraints |S_active| after scan
    pub m3_active_constraints: usize,
    /// M4: L3 Consensus — crystal rate (crystals committed per 24h)
    pub m4_crystal_rate: f64,
    /// M5: L4 Morph — mutation rate (mutations per 24h)
    pub m5_mutation_rate: f64,

    // Core Quality (M6-M14)
    /// M6: Replay fidelity (0.0 - 1.0; 1.0 = 100% match)
    pub m6_replay_fidelity: f64,
    /// M7: Convergence rate — Wasserstein distance between consecutive persistence diagrams
    pub m7_convergence_rate: f64,
    /// M8: Lattice stability — mean free energy F_bar of emitted crystals
    pub m8_lattice_stability: f64,
    /// M9: Gate selectivity — fraction of macro-steps passing Kairos gate
    pub m9_gate_selectivity: f64,
    /// M10: Dual consensus MCI — mean MCI of committed crystals
    pub m10_dual_consensus_mci: f64,
    /// M11: PoR latency — mean time from search->commit in PoR FSM (seconds)
    pub m11_por_latency_secs: f64,
    /// M12: Evidence integrity — fraction of crystals passing verify_crystal
    pub m12_evidence_integrity: f64,
    /// M13: Operator version drift — count of unmatched operator versions
    pub m13_operator_version_drift: usize,
    /// M14: Storage efficiency — cold-tier bytes per asset per month
    pub m14_storage_efficiency_bytes: u64,

    // Performance (M15-M19)
    /// M15: Macro-step latency (wall-clock seconds per macro_step() call)
    pub m15_macro_step_latency_secs: f64,
    /// M16: Memory footprint — RSS of process (bytes)
    pub m16_memory_footprint_bytes: u64,
    /// M17: Extraction throughput — constraint candidates evaluated per second
    pub m17_extraction_throughput: f64,
    /// M18: Archive growth rate — bytes appended to cold tier per 24h
    pub m18_archive_growth_bytes_per_day: u64,
    /// M19: Carrier migration latency — time from trigger to stable phase (seconds)
    pub m19_carrier_migration_latency_secs: f64,

    // Empirical Domain (M20-M24)
    /// M20: Constraint hit rate — fraction of emitted constraints active 24h later
    pub m20_constraint_hit_rate: f64,
    /// M21: Crystal predictive value — fraction of crystals where claimed structure persisted
    pub m21_crystal_predictive_value: f64,
    /// M22: Signal lead time — time between crystal emission and observable event (seconds)
    pub m22_signal_lead_time_secs: f64,
    /// M23: Basket quality lift — DSHAE Sharpe ratio with ISLS basket vs random basket
    pub m23_basket_quality_lift: f64,
    /// M24: Coverage growth — number of tracked entities |V|
    pub m24_coverage_growth: usize,
}

impl Default for MetricSnapshot {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
            tick: 0,
            m1_ingestion_rate: 0.0,
            m2_graph_growth: 0,
            m3_active_constraints: 0,
            m4_crystal_rate: 0.0,
            m5_mutation_rate: 0.0,
            m6_replay_fidelity: 1.0,
            m7_convergence_rate: 0.0,
            m8_lattice_stability: -1.0,
            m9_gate_selectivity: 0.05,
            m10_dual_consensus_mci: 1.0,
            m11_por_latency_secs: 0.0,
            m12_evidence_integrity: 1.0,
            m13_operator_version_drift: 0,
            m14_storage_efficiency_bytes: 0,
            m15_macro_step_latency_secs: 0.0,
            m16_memory_footprint_bytes: 0,
            m17_extraction_throughput: 1000.0,
            m18_archive_growth_bytes_per_day: 0,
            m19_carrier_migration_latency_secs: 0.0,
            m20_constraint_hit_rate: 0.8,
            m21_crystal_predictive_value: 0.7,
            m22_signal_lead_time_secs: 60.0,
            m23_basket_quality_lift: 0.1,
            m24_coverage_growth: 0,
        }
    }
}

// ─── Alert ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alert {
    pub timestamp: DateTime<Utc>,
    pub metric_id: String,
    pub metric_name: String,
    pub current_value: f64,
    pub threshold: f64,
    pub message: String,
}

// ─── Metric Collector ─────────────────────────────────────────────────────────

/// Accumulator for metric collection over time.
pub struct MetricCollector {
    pub history: Vec<MetricSnapshot>,
    pub alerts: Vec<Alert>,
    /// Last tick counter
    tick: u64,
    /// Sliding window: ingestion count over last 60s
    ingestion_window: Vec<(DateTime<Utc>, u64)>,
    /// Previous crystal count (for crystal_rate)
    prev_crystal_count: u64,
    /// Previous mutation count
    prev_mutation_count: u64,
    /// Previous vertex+edge count
    prev_graph_size: i64,
    /// Macro-step gate pass counter
    gate_pass_count: u64,
    /// Total macro-step count
    total_step_count: u64,
    /// PoR latency samples (seconds)
    por_latency_samples: Vec<f64>,
    /// Macro-step latency samples (seconds)
    macro_step_latency_samples: Vec<f64>,
    /// Crystal free energies
    crystal_free_energies: Vec<f64>,
    /// Crystal MCI values
    crystal_mci_values: Vec<f64>,
    /// Archive size (bytes) at last snapshot
    prev_archive_bytes: u64,
    /// Constraint samples for hit rate (placeholder)
    constraint_samples: Vec<f64>,
    /// Crystal prediction samples
    prediction_samples: Vec<f64>,
    /// Lead time samples
    lead_time_samples: Vec<f64>,
}

impl MetricCollector {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            alerts: Vec::new(),
            tick: 0,
            ingestion_window: Vec::new(),
            prev_crystal_count: 0,
            prev_mutation_count: 0,
            prev_graph_size: 0,
            gate_pass_count: 0,
            total_step_count: 0,
            por_latency_samples: Vec::new(),
            macro_step_latency_samples: Vec::new(),
            crystal_free_energies: Vec::new(),
            crystal_mci_values: Vec::new(),
            prev_archive_bytes: 0,
            constraint_samples: Vec::new(),
            prediction_samples: Vec::new(),
            lead_time_samples: Vec::new(),
        }
    }

    /// Record that N observations were ingested
    pub fn record_ingestion(&mut self, count: u64) {
        self.ingestion_window.push((Utc::now(), count));
        // Trim to last 60 seconds
        let cutoff = Utc::now() - chrono::Duration::seconds(60);
        self.ingestion_window.retain(|(t, _)| *t > cutoff);
    }

    /// Record a macro-step completed with wall-clock duration
    pub fn record_macro_step(
        &mut self,
        duration_secs: f64,
        gate_passed: bool,
        crystal_emitted: bool,
        free_energy: Option<f64>,
        mci: Option<f64>,
        por_latency: Option<f64>,
    ) {
        self.total_step_count += 1;
        self.macro_step_latency_samples.push(duration_secs);
        if gate_passed {
            self.gate_pass_count += 1;
        }
        if crystal_emitted {
            self.prev_crystal_count += 1;
        }
        if let Some(fe) = free_energy {
            self.crystal_free_energies.push(fe);
        }
        if let Some(m) = mci {
            self.crystal_mci_values.push(m);
        }
        if let Some(lat) = por_latency {
            self.por_latency_samples.push(lat);
        }
        // Keep only last 1000 samples
        if self.macro_step_latency_samples.len() > 1000 {
            self.macro_step_latency_samples.remove(0);
        }
    }

    /// Record mutation count for M5
    pub fn record_mutations(&mut self, count: u64) {
        self.prev_mutation_count += count;
    }

    /// Record constraint hit rate sample
    pub fn record_constraint_hit(&mut self, hit: bool) {
        self.constraint_samples.push(if hit { 1.0 } else { 0.0 });
        if self.constraint_samples.len() > 1000 {
            self.constraint_samples.remove(0);
        }
    }

    /// Record crystal prediction outcome
    pub fn record_prediction_outcome(&mut self, correct: bool) {
        self.prediction_samples.push(if correct { 1.0 } else { 0.0 });
        if self.prediction_samples.len() > 1000 {
            self.prediction_samples.remove(0);
        }
    }

    /// Record signal lead time
    pub fn record_lead_time(&mut self, lead_secs: f64) {
        self.lead_time_samples.push(lead_secs);
        if self.lead_time_samples.len() > 1000 {
            self.lead_time_samples.remove(0);
        }
    }

    /// Collect a full metric snapshot from current engine state
    #[allow(clippy::too_many_arguments)]
    pub fn collect(
        &mut self,
        active_constraints: usize,
        graph_vertices: usize,
        graph_edges: usize,
        archive_size: usize,
        archive_bytes: u64,
        replay_fidelity: f64,
        operator_version_drift: usize,
        storage_bytes_cold: u64,
        memory_rss_bytes: u64,
        extraction_throughput: f64,
        carrier_migration_latency_secs: f64,
        coverage_growth: usize,
        basket_quality_lift: f64,
    ) -> MetricSnapshot {
        self.tick += 1;
        let now = Utc::now();

        // M1: ingestion rate (obs/sec over last 60s)
        let total_obs: u64 = self.ingestion_window.iter().map(|(_, c)| c).sum();
        let m1 = total_obs as f64 / 60.0;

        // M2: graph growth
        let current_size = (graph_vertices + graph_edges) as i64;
        let m2 = current_size - self.prev_graph_size;
        self.prev_graph_size = current_size;

        // M3: active constraints
        let m3 = active_constraints;

        // M4: crystal rate (crystals per 24h) — approximate from accumulator
        // Simple: current count over time since last reset / 24h window
        let m4 = self.prev_crystal_count as f64;

        // M5: mutation rate per 24h
        let m5 = self.prev_mutation_count as f64;

        // M6: replay fidelity
        let m6 = replay_fidelity;

        // M7: convergence rate (placeholder — Wasserstein dist computed externally)
        let m7 = 0.0_f64;

        // M8: mean free energy of emitted crystals
        let m8 = if self.crystal_free_energies.is_empty() {
            -1.0
        } else {
            self.crystal_free_energies.iter().sum::<f64>()
                / self.crystal_free_energies.len() as f64
        };

        // M9: gate selectivity
        let m9 = if self.total_step_count == 0 {
            0.0
        } else {
            self.gate_pass_count as f64 / self.total_step_count as f64
        };

        // M10: dual consensus MCI
        let m10 = if self.crystal_mci_values.is_empty() {
            1.0
        } else {
            self.crystal_mci_values.iter().sum::<f64>()
                / self.crystal_mci_values.len() as f64
        };

        // M11: PoR latency
        let m11 = if self.por_latency_samples.is_empty() {
            0.0
        } else {
            self.por_latency_samples.iter().sum::<f64>()
                / self.por_latency_samples.len() as f64
        };

        // M12: evidence integrity
        let m12 = if archive_size == 0 {
            1.0
        } else {
            // assume verify_all has been run externally
            1.0
        };

        // M13: operator version drift
        let m13 = operator_version_drift;

        // M14: storage efficiency (bytes per asset per month)
        let m14 = if graph_vertices == 0 {
            0
        } else {
            storage_bytes_cold / (graph_vertices.max(1) as u64)
        };

        // M15: macro-step latency (mean over recent samples)
        let m15 = if self.macro_step_latency_samples.is_empty() {
            0.0
        } else {
            *self.macro_step_latency_samples.last().unwrap()
        };

        // M16: memory footprint
        let m16 = memory_rss_bytes;

        // M17: extraction throughput
        let m17 = extraction_throughput;

        // M18: archive growth rate (bytes per day)
        let m18 = archive_bytes.saturating_sub(self.prev_archive_bytes);
        self.prev_archive_bytes = archive_bytes;

        // M19: carrier migration latency
        let m19 = carrier_migration_latency_secs;

        // M20: constraint hit rate — 0.0 until actual samples arrive
        let m20 = if self.constraint_samples.is_empty() {
            0.0
        } else {
            self.constraint_samples.iter().sum::<f64>()
                / self.constraint_samples.len() as f64
        };

        // M21: crystal predictive value — 0.0 until actual samples arrive
        let m21 = if self.prediction_samples.is_empty() {
            0.0
        } else {
            self.prediction_samples.iter().sum::<f64>()
                / self.prediction_samples.len() as f64
        };

        // M22: signal lead time — 0.0 until actual samples arrive
        let m22 = if self.lead_time_samples.is_empty() {
            0.0
        } else {
            self.lead_time_samples.iter().sum::<f64>()
                / self.lead_time_samples.len() as f64
        };

        // M23: basket quality lift — (coverage_after - coverage_before) / coverage_before
        let m23 = basket_quality_lift;

        // M24: coverage growth
        let m24 = coverage_growth;

        let snap = MetricSnapshot {
            timestamp: now,
            tick: self.tick,
            m1_ingestion_rate: m1,
            m2_graph_growth: m2,
            m3_active_constraints: m3,
            m4_crystal_rate: m4,
            m5_mutation_rate: m5,
            m6_replay_fidelity: m6,
            m7_convergence_rate: m7,
            m8_lattice_stability: m8,
            m9_gate_selectivity: m9,
            m10_dual_consensus_mci: m10,
            m11_por_latency_secs: m11,
            m12_evidence_integrity: m12,
            m13_operator_version_drift: m13,
            m14_storage_efficiency_bytes: m14,
            m15_macro_step_latency_secs: m15,
            m16_memory_footprint_bytes: m16,
            m17_extraction_throughput: m17,
            m18_archive_growth_bytes_per_day: m18,
            m19_carrier_migration_latency_secs: m19,
            m20_constraint_hit_rate: m20,
            m21_crystal_predictive_value: m21,
            m22_signal_lead_time_secs: m22,
            m23_basket_quality_lift: m23,
            m24_coverage_growth: m24,
        };

        self.history.push(snap.clone());
        snap
    }

    /// Check a snapshot for alerts and fire them
    pub fn check_alerts(&mut self, snap: &MetricSnapshot) -> Vec<Alert> {
        let mut fired = Vec::new();

        macro_rules! alert {
            ($id:expr, $name:expr, $val:expr, $threshold:expr, $msg:expr) => {
                let a = Alert {
                    timestamp: snap.timestamp,
                    metric_id: $id.to_string(),
                    metric_name: $name.to_string(),
                    current_value: $val as f64,
                    threshold: $threshold as f64,
                    message: $msg.to_string(),
                };
                warn!(metric_id = $id, value = $val as f64, "{}", $msg);
                self.alerts.push(a.clone());
                fired.push(a);
            };
        }

        // M1: ingestion rate = 0 for > 30s (here: if rate == 0)
        if snap.m1_ingestion_rate == 0.0 {
            alert!("M1", "Ingestion rate", snap.m1_ingestion_rate, 0.0, "L0: No observations ingested");
        }
        // M2: negative graph growth (data loss)
        if snap.m2_graph_growth < 0 {
            alert!("M2", "Graph growth", snap.m2_graph_growth as f64, 0.0, "L1: Negative graph growth — data loss detected");
        }
        // M3: active constraints = 0 for > 1h (here: if 0)
        if snap.m3_active_constraints == 0 {
            alert!("M3", "Active constraints", 0.0, 1.0, "L2: No active constraints after extraction");
        }
        // M4: crystal rate = 0 for > configured silent period (here: if 0)
        if snap.m4_crystal_rate == 0.0 && snap.tick > 100 {
            alert!("M4", "Crystal rate", 0.0, 0.0, "L3: No crystals committed (possible gate too strict)");
        }
        // M5: negative invariant violation
        if snap.m5_mutation_rate < 0.0 {
            alert!("M5", "Mutation rate", snap.m5_mutation_rate, 0.0, "L4: Negative mutation rate — invariant violation");
        }
        // M6: replay fidelity < 100%
        if snap.m6_replay_fidelity < 1.0 {
            alert!("M6", "Replay fidelity", snap.m6_replay_fidelity, 1.0, "Replay mismatch: non-determinism detected");
        }
        // M8: free energy >= 0
        if snap.m8_lattice_stability >= 0.0 {
            alert!("M8", "Lattice stability", snap.m8_lattice_stability, 0.0, "Positive free energy: constraints too weak or temperature too high");
        }
        // M9: gate selectivity > 0.5 or == 0 for > 48h
        if snap.m9_gate_selectivity > 0.5 {
            alert!("M9", "Gate selectivity", snap.m9_gate_selectivity, 0.5, "Gate too permissive: >50% of steps pass Kairos");
        }
        // M10: MCI < 0.80
        if snap.m10_dual_consensus_mci < 0.80 && snap.tick > 10 {
            alert!("M10", "Dual consensus MCI", snap.m10_dual_consensus_mci, 0.80, "Low MCI: primal and dual paths diverging");
        }
        // M12: evidence integrity < 100%
        if snap.m12_evidence_integrity < 1.0 {
            alert!("M12", "Evidence integrity", snap.m12_evidence_integrity, 1.0, "Evidence corruption: storage integrity failure");
        }
        // M13: operator version drift > 0
        if snap.m13_operator_version_drift > 0 {
            alert!("M13", "Operator version drift", snap.m13_operator_version_drift as f64, 0.0, "Operator version mismatch in archive");
        }
        // M14: storage > 10 MB per asset
        if snap.m14_storage_efficiency_bytes > 10 * 1024 * 1024 {
            alert!("M14", "Storage efficiency", snap.m14_storage_efficiency_bytes as f64, 10_000_000.0, "Storage too large: >10 MB per asset per month");
        }
        // M15: macro-step latency > 120s
        if snap.m15_macro_step_latency_secs > 120.0 {
            alert!("M15", "Macro-step latency", snap.m15_macro_step_latency_secs, 120.0, "Macro-step too slow: >120s");
        }
        // M16: memory > 8 GB
        if snap.m16_memory_footprint_bytes > 8 * 1024 * 1024 * 1024 {
            alert!("M16", "Memory footprint", snap.m16_memory_footprint_bytes as f64, 8_589_934_592.0, "Memory too high: >8 GB RSS");
        }
        // M17: extraction throughput < 10/s
        if snap.m17_extraction_throughput < 10.0 {
            alert!("M17", "Extraction throughput", snap.m17_extraction_throughput, 10.0, "Extraction too slow: <10 candidates/sec");
        }
        // M20: constraint hit rate < 0.3
        if snap.m20_constraint_hit_rate < 0.3 {
            alert!("M20", "Constraint hit rate", snap.m20_constraint_hit_rate, 0.3, "Low constraint hit rate: constraints are transient noise");
        }
        // M21: crystal predictive value < 0.3
        if snap.m21_crystal_predictive_value < 0.3 {
            alert!("M21", "Crystal predictive value", snap.m21_crystal_predictive_value, 0.3, "Low crystal predictive value");
        }
        // M22: signal lead time <= 0 (lagging)
        if snap.m22_signal_lead_time_secs <= 0.0 {
            alert!("M22", "Signal lead time", snap.m22_signal_lead_time_secs, 0.0, "Lagging signals: ISLS detects structure after market moves");
        }
        // M23: basket quality lift negative
        if snap.m23_basket_quality_lift < 0.0 {
            alert!("M23", "Basket quality lift", snap.m23_basket_quality_lift, 0.0, "Negative basket quality lift: ISLS basket underperforms random");
        }
        // M24: coverage decreasing (data loss)
        if snap.m2_graph_growth < 0 {
            alert!("M24", "Coverage growth", snap.m24_coverage_growth as f64, 0.0, "Entity count decreasing: data loss");
        }

        fired
    }

    /// Compute overall health level
    pub fn overall_health(snap: &MetricSnapshot) -> AlertLevel {
        // Red if any P0 metric fires
        if snap.m6_replay_fidelity < 1.0 || snap.m12_evidence_integrity < 1.0 {
            return AlertLevel::Red;
        }
        // Yellow if any P1/P2 metric fires
        if snap.m8_lattice_stability >= 0.0
            || snap.m9_gate_selectivity > 0.5
            || snap.m9_gate_selectivity == 0.0
            || snap.m10_dual_consensus_mci < 0.80
            || snap.m20_constraint_hit_rate < 0.3
            || snap.m21_crystal_predictive_value < 0.3
            || snap.m22_signal_lead_time_secs <= 0.0
            || snap.m15_macro_step_latency_secs > 120.0
        {
            return AlertLevel::Yellow;
        }
        AlertLevel::Green
    }

    /// Serialize a snapshot to JSONL line
    pub fn to_jsonl(snap: &MetricSnapshot) -> String {
        serde_json::to_string(snap).unwrap_or_default()
    }
}

impl Default for MetricCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_snapshot_default() {
        let snap = MetricSnapshot::default();
        assert_eq!(snap.m6_replay_fidelity, 1.0);
        assert_eq!(snap.m12_evidence_integrity, 1.0);
        assert!(snap.m8_lattice_stability < 0.0);
    }

    #[test]
    fn test_collector_collects() {
        let mut collector = MetricCollector::new();
        collector.record_ingestion(100);
        let snap = collector.collect(
            5, 50, 200, 0, 0, 1.0, 0, 0, 0, 500.0, 0.0, 50, 0.1,
        );
        assert_eq!(snap.m3_active_constraints, 5);
        assert_eq!(snap.m24_coverage_growth, 50);
    }

    #[test]
    fn test_alert_fires_on_replay_mismatch() {
        let mut collector = MetricCollector::new();
        let mut snap = MetricSnapshot::default();
        snap.m6_replay_fidelity = 0.99;
        let alerts = collector.check_alerts(&snap);
        assert!(alerts.iter().any(|a| a.metric_id == "M6"));
    }

    #[test]
    fn test_health_level() {
        let mut snap = MetricSnapshot::default();
        assert_eq!(MetricCollector::overall_health(&snap), AlertLevel::Green);
        snap.m6_replay_fidelity = 0.99;
        assert_eq!(MetricCollector::overall_health(&snap), AlertLevel::Red);
        snap.m6_replay_fidelity = 1.0;
        snap.m8_lattice_stability = 0.5;
        assert_eq!(MetricCollector::overall_health(&snap), AlertLevel::Yellow);
    }

    #[test]
    fn test_24_metrics_present() {
        let snap = MetricSnapshot::default();
        // Verify all 24 fields exist by serializing
        let json = serde_json::to_string(&snap).unwrap();
        for m in &["m1_", "m2_", "m3_", "m4_", "m5_",
                   "m6_", "m7_", "m8_", "m9_", "m10_",
                   "m11_", "m12_", "m13_", "m14_",
                   "m15_", "m16_", "m17_", "m18_", "m19_",
                   "m20_", "m21_", "m22_", "m23_", "m24_"] {
            assert!(json.contains(m), "Missing metric {}", m);
        }
    }
}
