// isls-agent: accumulation.rs — C30 Accumulation Metrics (M35)
//
// Tracks how often the Agent used pattern memory vs. the Oracle, compile
// success rates, and estimated cost savings.  Displayed in the Studio sidebar.

use std::path::Path;

use serde::{Deserialize, Serialize};

// ─── AccumulationMetrics ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AccumulationMetrics {
    /// Total chat/apply requests processed
    pub total_requests: u64,
    /// Requests served from pattern memory (no Oracle call)
    pub memory_served: u64,
    /// Requests that required an Oracle call
    pub oracle_served: u64,
    /// Files written that compiled on the first attempt
    pub compile_first_try: u64,
    /// Files written that compiled after at least one fix attempt
    pub compile_with_fix: u64,
    /// Files where compile failed after all fix attempts
    pub compile_failed: u64,
    /// memory_served / total_requests, cached on update
    pub autonomy_ratio: f64,
    /// Total Oracle spend in USD
    pub total_cost_usd: f64,
    /// Average Oracle cost per request (oracle_served > 0)
    pub avg_cost_per_request: f64,
    /// Current number of patterns in memory
    pub patterns_in_memory: usize,
    /// Estimated savings: memory_served * avg Oracle cost per request
    pub money_saved_usd: f64,
}

impl AccumulationMetrics {
    /// Format a one-line dashboard label.
    /// Example: "Auto: 34% | Mem: 23 | $0.02 | Saved: $0.47"
    pub fn dashboard_label(&self) -> String {
        format!(
            "Auto: {:.0}% | Mem: {} | ${:.4} | Saved: ${:.2}",
            self.autonomy_ratio * 100.0,
            self.patterns_in_memory,
            self.total_cost_usd,
            self.money_saved_usd,
        )
    }

    // ─── Recording Methods ───────────────────────────────────────────────────

    pub fn record_memory_hit(&mut self) {
        self.total_requests += 1;
        self.memory_served += 1;
        self.recompute();
    }

    pub fn record_oracle_call(&mut self, cost_usd: f64) {
        self.total_requests += 1;
        self.oracle_served += 1;
        self.total_cost_usd += cost_usd;
        self.recompute();
    }

    pub fn record_compile_first_try(&mut self) {
        self.compile_first_try += 1;
    }

    pub fn record_compile_with_fix(&mut self) {
        self.compile_with_fix += 1;
    }

    pub fn record_compile_failed(&mut self) {
        self.compile_failed += 1;
    }

    pub fn set_patterns_in_memory(&mut self, count: usize) {
        self.patterns_in_memory = count;
        self.recompute();
    }

    // ─── Internal ────────────────────────────────────────────────────────────

    fn recompute(&mut self) {
        self.autonomy_ratio = if self.total_requests == 0 {
            0.0
        } else {
            self.memory_served as f64 / self.total_requests as f64
        };

        self.avg_cost_per_request = if self.oracle_served == 0 {
            0.0
        } else {
            self.total_cost_usd / self.oracle_served as f64
        };

        self.money_saved_usd = self.memory_served as f64 * self.avg_cost_per_request;
    }

    // ─── Persistence ─────────────────────────────────────────────────────────

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir: {}", e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(path, json).map_err(|e| format!("write: {}", e))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| format!("read: {}", e))?;
        serde_json::from_str(&json).map_err(|e| format!("deserialize: {}", e))
    }

    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_default()
    }
}

// ─── Tests (AT-AG21) ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // AT-AG21: 3 oracle + 2 memory → autonomy_ratio = 40%
    #[test]
    fn at_ag21_accumulation_metrics_autonomy() {
        let mut m = AccumulationMetrics::default();

        m.record_oracle_call(0.01);
        m.record_oracle_call(0.01);
        m.record_oracle_call(0.01);
        m.record_memory_hit();
        m.record_memory_hit();

        assert_eq!(m.total_requests, 5, "5 total requests");
        assert_eq!(m.oracle_served, 3);
        assert_eq!(m.memory_served, 2);
        assert!(
            (m.autonomy_ratio - 0.4).abs() < 1e-9,
            "autonomy_ratio should be 0.40, got {}",
            m.autonomy_ratio
        );
        assert!(
            (m.total_cost_usd - 0.03).abs() < 1e-9,
            "total cost should be $0.03, got {}",
            m.total_cost_usd
        );
        let avg = m.avg_cost_per_request;
        assert!((avg - 0.01).abs() < 1e-9, "avg cost per oracle call: {}", avg);
        // money saved = 2 memory hits * $0.01 avg = $0.02
        assert!((m.money_saved_usd - 0.02).abs() < 1e-9, "saved: {}", m.money_saved_usd);
    }

    #[test]
    fn at_ag21b_dashboard_label_format() {
        let mut m = AccumulationMetrics::default();
        m.record_oracle_call(0.02);
        m.record_memory_hit();
        m.set_patterns_in_memory(7);
        let label = m.dashboard_label();
        assert!(label.contains("Auto:"), "label has Auto:");
        assert!(label.contains("Mem:"), "label has Mem:");
        assert!(label.contains("Saved:"), "label has Saved:");
    }

    #[test]
    fn at_ag21c_persistence_round_trip() {
        let path = std::env::temp_dir().join(format!(
            "isls_accum_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        ));
        let mut m = AccumulationMetrics::default();
        m.record_oracle_call(0.05);
        m.set_patterns_in_memory(3);
        m.save(&path).expect("save");

        let loaded = AccumulationMetrics::load(&path).expect("load");
        assert_eq!(loaded.oracle_served, 1);
        assert_eq!(loaded.patterns_in_memory, 3);
        assert!((loaded.total_cost_usd - 0.05).abs() < 1e-9);
        let _ = std::fs::remove_file(&path);
    }
}
