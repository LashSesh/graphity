// isls-harness/src/validate.rs
// V-Formal, V-Retro, V-Live validation levels

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

use isls_types::SemanticCrystal;
use isls_archive::{Archive, verify_crystal};
use isls_persist::PersistentGraph;

// ─── Formal Report ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub check_id: String,
    pub passed: bool,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FormalCrystalResult {
    pub crystal_id: String,
    pub checks: Vec<CheckResult>,
    pub all_passed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FormalReport {
    pub crystal_results: Vec<FormalCrystalResult>,
    pub total_crystals: usize,
    pub passed_crystals: usize,
    pub failed_crystals: usize,
    pub check_counts: BTreeMap<String, (usize, usize)>, // (passed, total)
}

impl FormalReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check(&mut self, crystal_id: &str, check_id: &str, passed: bool) {
        // Find or create crystal result
        if let Some(cr) = self.crystal_results.iter_mut().find(|r| r.crystal_id == crystal_id) {
            cr.checks.push(CheckResult { check_id: check_id.to_string(), passed, detail: None });
            if !passed {
                cr.all_passed = false;
            }
        } else {
            self.crystal_results.push(FormalCrystalResult {
                crystal_id: crystal_id.to_string(),
                checks: vec![CheckResult { check_id: check_id.to_string(), passed, detail: None }],
                all_passed: passed,
            });
        }
        // Update check counts
        let entry = self.check_counts.entry(check_id.to_string()).or_insert((0, 0));
        if passed { entry.0 += 1; }
        entry.1 += 1;
    }

    pub fn finalize(&mut self) {
        self.total_crystals = self.crystal_results.len();
        self.passed_crystals = self.crystal_results.iter().filter(|r| r.all_passed).count();
        self.failed_crystals = self.total_crystals - self.passed_crystals;
    }

    pub fn pass_rate(&self) -> f64 {
        if self.total_crystals == 0 { return 1.0; }
        self.passed_crystals as f64 / self.total_crystals as f64
    }
}

// ─── Retro Entry / Report ─────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetroEntry {
    pub crystal_id: String,
    pub constraint_id: String,
    pub predicted_coverage: f64,
    pub actual_coverage: f64,
    pub still_active: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RetroReport {
    pub entries: Vec<RetroEntry>,
    pub hit_rate: f64,
    pub mean_coverage_drift: f64,
    pub false_positive_rate: f64,
    pub total_constraints_evaluated: usize,
    pub active_constraints: usize,
}

impl RetroReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, entry: RetroEntry) {
        self.entries.push(entry);
    }

    pub fn compute_aggregates(&mut self) {
        self.total_constraints_evaluated = self.entries.len();
        if self.entries.is_empty() {
            return;
        }
        self.active_constraints = self.entries.iter().filter(|e| e.still_active).count();
        self.hit_rate = self.active_constraints as f64 / self.total_constraints_evaluated as f64;

        self.mean_coverage_drift = self.entries.iter()
            .map(|e| (e.actual_coverage - e.predicted_coverage).abs())
            .sum::<f64>() / self.entries.len() as f64;

        // False positive: predicted active but actually inactive
        let predicted_active = self.entries.iter()
            .filter(|e| e.predicted_coverage >= 0.5)
            .count();
        let actually_inactive = self.entries.iter()
            .filter(|e| e.predicted_coverage >= 0.5 && !e.still_active)
            .count();
        self.false_positive_rate = if predicted_active == 0 {
            0.0
        } else {
            actually_inactive as f64 / predicted_active as f64
        };
    }
}

// ─── Formal Validator ─────────────────────────────────────────────────────────

pub struct FormalValidator;

impl FormalValidator {
    /// V-Formal: Iterate over every crystal in the archive and run 8 invariant checks
    pub fn validate(archive: &Archive, pinned: &BTreeMap<String, String>) -> FormalReport {
        let mut report = FormalReport::new();

        for crystal in archive.crystals() {
            let cid = &crystal.crystal_id;
            let cid_str = hex::encode(cid);

            // V-F1: Content address integrity — verify via archive's verify_crystal
            let content_ok = verify_crystal(crystal, pinned).is_ok();
            report.check(&cid_str, "content_address", content_ok);

            // V-F2: Evidence chain integrity
            let chain_ok = Self::check_evidence_chain(crystal);
            report.check(&cid_str, "evidence_chain", chain_ok);

            // V-F3: Operator version match
            let ver_ok = Self::check_operator_versions(crystal, pinned);
            report.check(&cid_str, "operator_versions", ver_ok);

            // V-F4: Gate values above thresholds (Kairos must be true)
            let gate_ok = crystal.commit_proof.gate_values.kairos;
            report.check(&cid_str, "gate_kairos", gate_ok);

            // V-F5: Dual consensus — primal, dual, and MCI all meet thresholds
            let cr = &crystal.commit_proof.consensus_result;
            let dual_ok = cr.primal_score >= cr.threshold
                && cr.dual_score >= cr.threshold
                && cr.mci >= 0.80;
            report.check(&cid_str, "dual_consensus", dual_ok);

            // V-F6: PoR trace monotonicity
            let por_ok = Self::check_por_trace(crystal);
            report.check(&cid_str, "por_trace", por_ok);

            // V-F7: Free energy < 0
            let fe_ok = crystal.free_energy < 0.0;
            report.check(&cid_str, "free_energy", fe_ok);

            // V-F8: Append-only — (Archive itself enforces this; we verify id uniqueness)
            let unique_ok = Self::check_uniqueness(crystal, archive);
            report.check(&cid_str, "immutability", unique_ok);
        }

        report.finalize();
        report
    }

    fn check_evidence_chain(crystal: &SemanticCrystal) -> bool {
        use isls_types::content_address_raw;
        for (i, entry) in crystal.evidence_chain.iter().enumerate() {
            if content_address_raw(&entry.content) != entry.digest {
                return false;
            }
            if i > 0 && entry.prev != Some(crystal.evidence_chain[i - 1].digest) {
                return false;
            }
        }
        true
    }

    fn check_operator_versions(
        crystal: &SemanticCrystal,
        pinned: &BTreeMap<String, String>,
    ) -> bool {
        for (name, ver) in &crystal.commit_proof.operator_stack {
            if let Some(pinned_ver) = pinned.get(name) {
                if pinned_ver != ver {
                    return false;
                }
            }
        }
        true
    }

    fn check_por_trace(crystal: &SemanticCrystal) -> bool {
        let t = &crystal.commit_proof.por_trace;
        // Monotonicity: search <= lock <= verify <= commit (where present)
        if let Some(lock) = t.lock_enter {
            if lock < t.search_enter { return false; }
            if let Some(verify) = t.verify_enter {
                if verify < lock { return false; }
                if let Some(commit) = t.commit_enter {
                    if commit < verify { return false; }
                }
            }
        }
        true
    }

    fn check_uniqueness(crystal: &SemanticCrystal, archive: &Archive) -> bool {
        // Count occurrences of this crystal_id in archive (immutability check)
        // Archive is append-only; duplicate IDs indicate a bug
        let count = archive.crystals()
            .iter()
            .filter(|c| c.crystal_id == crystal.crystal_id)
            .count();
        count == 1
    }
}

// ─── Retro Validator ──────────────────────────────────────────────────────────

pub struct RetroValidator;

impl RetroValidator {
    /// V-Retro: compare past crystal predictions against what actually happened
    /// Requires >= 7 days of operational data
    pub fn validate(
        archive: &Archive,
        _graph: &PersistentGraph,
        _horizon_days: u64,
    ) -> RetroReport {
        let mut report = RetroReport::new();

        // CommitIndex is u64 (sequential), not a DateTime.
        // Use a simple heuristic: skip crystals with recent commit indices.
        // "Recent" = top 10% of archive (approximate 7-day horizon).
        let total = archive.len();
        let horizon_idx = if total > 0 { total * 9 / 10 } else { 0 };

        for (idx, crystal) in archive.crystals().iter().enumerate() {
            if idx >= horizon_idx {
                continue; // Too recent (within simulated horizon)
            }
            for constraint in &crystal.constraint_program {
                let predicted_coverage = constraint.coverage;
                let actual_coverage = Self::evaluate_post_coverage(constraint, crystal);
                let still_active = actual_coverage >= 0.5;

                report.add(RetroEntry {
                    crystal_id: hex::encode(&crystal.crystal_id),
                    constraint_id: hex::encode(&constraint.id),
                    predicted_coverage,
                    actual_coverage,
                    still_active,
                });
            }
        }
        report.compute_aggregates();
        report
    }

    fn evaluate_post_coverage(
        constraint: &isls_types::ConstraintCandidate,
        _crystal: &SemanticCrystal,
    ) -> f64 {
        // Placeholder: use bond_strength as proxy for coverage persistence
        // In production, this would query the graph for post-horizon data
        let bond_decay = 0.85; // ~15% decay per horizon
        let bond_norm = (constraint.bond_strength as f64 / 100.0).min(1.0);
        constraint.coverage * bond_decay * bond_norm
    }
}

// ─── Live Validator ───────────────────────────────────────────────────────────

/// V-Live: runs as background task inside `isls run`, called on every macro-step
pub struct LiveValidator {
    tick: u64,
    summary_interval: u64,
}

impl LiveValidator {
    pub fn new(summary_interval_steps: u64) -> Self {
        Self { tick: 0, summary_interval: summary_interval_steps }
    }

    /// Call on every macro-step completion
    pub fn on_step(
        &mut self,
        snap: &crate::metrics::MetricSnapshot,
        collector: &mut crate::metrics::MetricCollector,
    ) -> Vec<crate::metrics::Alert> {
        self.tick += 1;
        // 1. Check alerts
        
        // 2. Every N steps: compute rolling aggregates (handled by caller via summary_interval)
        collector.check_alerts(snap)
    }

    pub fn should_summarize(&self) -> bool {
        self.tick.is_multiple_of(self.summary_interval)
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }
}

// ─── hex helper (inline, no extra dep) ───────────────────────────────────────
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isls_archive::{Archive, build_crystal_with_id};
    use isls_types::{CommitProof, GateSnapshot, ConsensusResult, PoRTrace};

    fn make_valid_crystal() -> SemanticCrystal {
        let gate = GateSnapshot {
            d: 0.8, q: 0.8, r: 0.8, g: 0.8, j: 0.8, p: 0.8, n: 0.8, k: 0.8, kairos: true,
        };
        let commit_proof = CommitProof {
            evidence_digests: Vec::new(),
            operator_stack: Vec::new(),
            gate_values: gate,
            structural_result: true,
            consensus_result: ConsensusResult {
                primal_score: 0.9, dual_score: 0.9, mci: 0.95, threshold: 0.7,
            },
            por_trace: PoRTrace {
                search_enter: 0.0, lock_enter: Some(1.0),
                verify_enter: Some(2.0), commit_enter: Some(3.0),
            },
            carrier_id: 0,
            carrier_offset: 0.0,
        };
        build_crystal_with_id(
            vec![1], 0.95, 0, -0.5, 0, vec![], commit_proof,
        )
    }

    #[test]
    fn test_formal_validation_passes_valid_crystal() {
        let mut archive = Archive::new();
        let crystal = make_valid_crystal();
        archive.append(crystal);
        let pinned = BTreeMap::new();
        let report = FormalValidator::validate(&archive, &pinned);
        assert_eq!(report.total_crystals, 1);
        assert_eq!(report.failed_crystals, 0);
    }

    #[test]
    fn test_formal_validation_fails_positive_free_energy() {
        let mut archive = Archive::new();
        let mut crystal = make_valid_crystal();
        crystal.free_energy = 0.5; // positive — should fail V-F7
        archive.append(crystal);
        let pinned = BTreeMap::new();
        let report = FormalValidator::validate(&archive, &pinned);
        // free_energy check should fail
        let free_e_checks = report.check_counts.get("free_energy");
        assert!(free_e_checks.is_some());
        let (passed, total) = free_e_checks.unwrap();
        assert_eq!(*total, 1);
        assert_eq!(*passed, 0);
    }

    #[test]
    fn test_retro_report_compute_aggregates() {
        let mut report = RetroReport::new();
        report.add(RetroEntry {
            crystal_id: "abc".into(),
            constraint_id: "c1".into(),
            predicted_coverage: 0.8,
            actual_coverage: 0.7,
            still_active: true,
        });
        report.add(RetroEntry {
            crystal_id: "abc".into(),
            constraint_id: "c2".into(),
            predicted_coverage: 0.8,
            actual_coverage: 0.1,
            still_active: false,
        });
        report.compute_aggregates();
        assert_eq!(report.total_constraints_evaluated, 2);
        assert_eq!(report.active_constraints, 1);
        assert_eq!(report.hit_rate, 0.5);
    }

    #[test]
    fn test_formal_report_pass_rate_empty_archive() {
        let report = FormalReport::new();
        assert_eq!(report.pass_rate(), 1.0);
    }

    #[test]
    fn test_live_validator_tick() {
        let mut validator = LiveValidator::new(100);
        let snap = crate::metrics::MetricSnapshot::default();
        let mut collector = crate::metrics::MetricCollector::new();
        let _ = validator.on_step(&snap, &mut collector);
        assert_eq!(validator.tick(), 1);
    }

    #[test]
    fn test_live_validator_summarize_interval() {
        let validator = LiveValidator::new(10);
        // tick 0 is divisible by 10 -> should summarize
        assert!(validator.should_summarize());
    }

    #[test]
    fn test_three_validation_levels_exist() {
        // Just ensure all three types compile and can be instantiated
        let _ = FormalValidator;
        let _ = RetroValidator;
        let _ = LiveValidator::new(100);
    }
}
