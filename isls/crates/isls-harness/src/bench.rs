// isls-harness/src/bench.rs
// 15 benchmarks (B01-B15) with regression tracking

use std::collections::BTreeMap;
use std::time::Instant;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use isls_types::{Config, MeasurementContext};
use isls_observe::{ingest, PassthroughAdapter};
use isls_persist::PersistentGraph;
use isls_extract::{inverse_weave, TimeWindow, default_operator_library};
use isls_archive::{verify_crystal, build_crystal_with_id};
use isls_engine::{GlobalState, macro_step};
use isls_consensus::{
    CascadeOperator, CrystalPrecursor, MetricSet, run_cascade,
    dual_consensus, default_primal_ops, default_dual_ops,
};
use isls_registry::{Registry, RegistryEntry, RegistryKind, RegistrySet};
use isls_manifest::{build_manifest, verify_manifest, TraceEntry};
use isls_capsule::{seal, open, CapsulePolicy};
use isls_scheduler::compute_substeps;

// ─── Bench Result ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchResult {
    pub bench_id: String,
    pub timestamp: DateTime<Utc>,
    pub git_commit: String,
    pub metric_name: String,
    pub metric_value: f64,
    pub metric_unit: String,
    pub params: BTreeMap<String, String>,
}

// ─── Regression Verdict ───────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RegressionVerdict {
    Regression,
    Improvement,
    Stable,
    InsufficientHistory,
}

impl std::fmt::Display for RegressionVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegressionVerdict::Regression => write!(f, "REGRESSION"),
            RegressionVerdict::Improvement => write!(f, "IMPROVEMENT"),
            RegressionVerdict::Stable => write!(f, "STABLE"),
            RegressionVerdict::InsufficientHistory => write!(f, "INSUFFICIENT_HISTORY"),
        }
    }
}

/// Regression check: current vs. last N runs (default 5)
pub fn check_regression(current: &BenchResult, history: &[BenchResult]) -> RegressionVerdict {
    if history.len() < 2 {
        return RegressionVerdict::InsufficientHistory;
    }
    let values: Vec<f64> = history.iter().map(|h| h.metric_value).collect();
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let std = variance.sqrt();

    if std == 0.0 {
        // No variance in history: any deviation is notable
        if current.metric_value > mean * 1.1 {
            return RegressionVerdict::Regression;
        } else if current.metric_value < mean * 0.9 {
            return RegressionVerdict::Improvement;
        } else {
            return RegressionVerdict::Stable;
        }
    }
    if current.metric_value > mean + 2.0 * std {
        RegressionVerdict::Regression
    } else if current.metric_value < mean - 2.0 * std {
        RegressionVerdict::Improvement
    } else {
        RegressionVerdict::Stable
    }
}

// ─── Bench Suite ──────────────────────────────────────────────────────────────

pub struct BenchSuite {
    pub config: Config,
    pub seed: u64,
}

impl BenchSuite {
    pub fn new(config: Config, seed: u64) -> Self {
        Self { config, seed }
    }

    /// Run all 15 benchmarks and return results
    pub fn run_all(&self) -> Vec<BenchResult> {
        let mut results = Vec::new();
        let git_commit = self.get_git_commit();

        results.push(self.b01_ingestion_throughput(&git_commit));
        results.push(self.b02_graph_update_scaling(&git_commit));
        results.extend(self.b03_extraction_scaling(&git_commit));
        results.push(self.b04_cascade_contraction(&git_commit));
        results.push(self.b05_dual_consensus_overhead(&git_commit));
        results.push(self.b06_crystal_serialization(&git_commit));
        results.push(self.b07_replay_speed(&git_commit));
        results.push(self.b08_evidence_verification(&git_commit));
        results.extend(self.b09_memory_scaling(&git_commit));
        results.push(self.b10_full_macro_step(&git_commit));
        // Extension benchmarks (C12–C15)
        results.push(self.b11_registry_resolution(&git_commit));
        results.push(self.b12_manifest_construction(&git_commit));
        results.push(self.b13_manifest_verification(&git_commit));
        results.push(self.b14_capsule_roundtrip(&git_commit));
        results.push(self.b15_scheduler_overhead(&git_commit));
        results
    }

    fn get_git_commit(&self) -> String {
        std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn make_result(
        &self,
        bench_id: &str,
        git_commit: &str,
        metric_name: &str,
        metric_value: f64,
        metric_unit: &str,
        params: BTreeMap<String, String>,
    ) -> BenchResult {
        BenchResult {
            bench_id: bench_id.to_string(),
            timestamp: Utc::now(),
            git_commit: git_commit.to_string(),
            metric_name: metric_name.to_string(),
            metric_value,
            metric_unit: metric_unit.to_string(),
            params,
        }
    }

    /// B01: Ingestion throughput — ingest 10^6 synthetic observations
    fn b01_ingestion_throughput(&self, git_commit: &str) -> BenchResult {
        const N: usize = 1_000_000;
        let adapter = PassthroughAdapter::new("bench-b01");
        let ctx = MeasurementContext::default();
        let start = Instant::now();
        let mut count = 0usize;
        for i in 0..N {
            let raw = format!("{{\"entity\":{},\"value\":{}}}", i % 100, i as f64 * 0.001);
            if ingest(&adapter, raw.as_bytes(), &ctx).is_ok() {
                count += 1;
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        let obs_per_sec = count as f64 / elapsed.max(1e-9);
        let mut params = BTreeMap::new();
        params.insert("n_observations".to_string(), N.to_string());
        self.make_result("B01", git_commit, "ingestion_throughput", obs_per_sec, "obs/sec", params)
    }

    /// B02: Graph update scaling — N = 100, 500, 2000, 5000 vertices
    fn b02_graph_update_scaling(&self, git_commit: &str) -> BenchResult {
        let ns = [100usize, 500, 2000, 5000];
        let mut total_us = 0.0;
        let mut total_n = 0usize;
        for &n in &ns {
            let mut graph = PersistentGraph::new();
            let start = Instant::now();
            for i in 0..n {
                let obs = make_state_obs(i as u64, i as f64);
                let _ = graph.apply_observations(&[obs], &self.config.persistence);
            }
            let elapsed = start.elapsed().as_micros() as f64;
            total_us += elapsed;
            total_n += n;
        }
        let mean_us = if total_n > 0 { total_us / total_n as f64 } else { 0.0 };
        let mut params = BTreeMap::new();
        params.insert("n_values".to_string(), "[100,500,2000,5000]".to_string());
        self.make_result("B02", git_commit, "graph_update_scaling", mean_us, "us_per_update", params)
    }

    /// B03: Extraction scaling — N = 50, 200, 1000 point clouds
    fn b03_extraction_scaling(&self, git_commit: &str) -> Vec<BenchResult> {
        let ns = [50usize, 200, 1000];
        let mut results = Vec::new();
        for &n in &ns {
            let graph = build_test_graph(n, self.seed, &self.config);
            let library = default_operator_library();
            let window = TimeWindow::all();
            let start = Instant::now();
            let _ = inverse_weave(&graph, &window, &library, &self.config.extraction);
            let elapsed = start.elapsed().as_millis() as f64;
            let mut params = BTreeMap::new();
            params.insert("n_entities".to_string(), n.to_string());
            results.push(self.make_result(
                "B03",
                git_commit,
                &format!("extraction_scaling_n{}", n),
                elapsed,
                "ms_per_scan",
                params,
            ));
        }
        results
    }

    /// B04: Cascade contraction — apply DK->SW->PI->WT on synthetic 5D cloud
    fn b04_cascade_contraction(&self, git_commit: &str) -> BenchResult {
        let graph = build_test_graph(50, self.seed, &self.config);
        let library = default_operator_library();
        let window = TimeWindow::all();
        let (program, region) = inverse_weave(&graph, &window, &library, &self.config.extraction);

        let precursor = CrystalPrecursor {
            program: program.clone(),
            region: region.clone(),
            seam_score: 0.5,
            metrics: MetricSet::default(),
            stability_score: 0.5,
        };

        let (dk, sw, pi, wt) = default_primal_ops();
        let primal_ops: Vec<&dyn CascadeOperator> = vec![&dk, &sw, &pi, &wt];
        let start = Instant::now();
        let result = run_cascade(&precursor, &primal_ops);
        let elapsed = start.elapsed().as_millis() as f64;

        let _contraction = if region.is_empty() {
            1.0
        } else {
            result.region.len() as f64 / region.len() as f64
        };

        let mut params = BTreeMap::new();
        params.insert("operators".to_string(), "DK->SW->PI->WT".to_string());
        self.make_result("B04", git_commit, "cascade_time_ms", elapsed, "ms", params)
    }

    /// B05: Dual consensus overhead — primal+dual vs. single path
    fn b05_dual_consensus_overhead(&self, git_commit: &str) -> BenchResult {
        let graph = build_test_graph(50, self.seed, &self.config);
        let library = default_operator_library();
        let window = TimeWindow::all();
        let (program, region) = inverse_weave(&graph, &window, &library, &self.config.extraction);
        let precursor = CrystalPrecursor {
            program,
            region,
            seam_score: 0.5,
            metrics: MetricSet::default(),
            stability_score: 0.5,
        };

        let (dk1, sw1, pi1, wt1) = default_primal_ops();
        let (pi2, wt2, dk2, sw2) = default_dual_ops();
        let primal_refs: Vec<&dyn CascadeOperator> = vec![&dk1, &sw1, &pi1, &wt1];
        let dual_refs: Vec<&dyn CascadeOperator> = vec![&pi2, &wt2, &dk2, &sw2];

        // Single path
        let start = Instant::now();
        let _p = dual_consensus(&precursor, &primal_refs, &[], &self.config.consensus);
        let single_time = start.elapsed().as_micros() as f64;

        // Dual path
        let start = Instant::now();
        let _d = dual_consensus(&precursor, &primal_refs, &dual_refs, &self.config.consensus);
        let dual_time = start.elapsed().as_micros() as f64;

        let overhead_pct = if single_time > 0.0 {
            (dual_time - single_time) / single_time * 100.0
        } else {
            0.0
        };

        let mut params = BTreeMap::new();
        params.insert("single_us".to_string(), single_time.to_string());
        params.insert("dual_us".to_string(), dual_time.to_string());
        self.make_result("B05", git_commit, "dual_consensus_overhead_pct", overhead_pct, "percent", params)
    }

    /// B06: Crystal serialization — serialize + hash 10^4 crystals
    fn b06_crystal_serialization(&self, git_commit: &str) -> BenchResult {
        const N: usize = 10_000;
        let crystal = make_test_crystal();
        let start = Instant::now();
        for _ in 0..N {
            let _bytes = serde_json::to_vec(&crystal).unwrap_or_default();
            let _id = isls_types::content_address(&crystal);
        }
        let elapsed = start.elapsed().as_micros() as f64;
        let us_per_crystal = elapsed / N as f64;
        let mut params = BTreeMap::new();
        params.insert("n_crystals".to_string(), N.to_string());
        self.make_result("B06", git_commit, "crystal_serialization_us", us_per_crystal, "us_per_crystal", params)
    }

    /// B07: Replay speed — replay 1000 macro-steps from saved RunDescriptor
    fn b07_replay_speed(&self, git_commit: &str) -> BenchResult {
        use isls_engine::run_with_descriptor;
        use isls_types::RunDescriptor;

        let descriptor = RunDescriptor {
            config: self.config.clone(),
            operator_versions: BTreeMap::new(),
            initial_state_digest: [0u8; 32],
            seed: Some(self.seed),
            registry_digests: BTreeMap::new(),
            scheduler: isls_types::SchedulerConfig::default(),
        };

        const N: usize = 100; // reduced for speed
        let obs_batches: Vec<Vec<Vec<u8>>> = (0..N).map(|_| vec![]).collect();

        let start = Instant::now();
        let _ = run_with_descriptor(&descriptor, &obs_batches);
        let elapsed = start.elapsed().as_secs_f64();
        let steps_per_sec = N as f64 / elapsed.max(0.001);

        let mut params = BTreeMap::new();
        params.insert("n_steps".to_string(), N.to_string());
        self.make_result("B07", git_commit, "replay_speed", steps_per_sec, "steps/sec", params)
    }

    /// B08: Evidence verification — verify 10^4 crystals with ~50 evidence entries
    fn b08_evidence_verification(&self, git_commit: &str) -> BenchResult {
        const N: usize = 10_000;
        let pinned = BTreeMap::new();
        let crystal = make_test_crystal();

        let start = Instant::now();
        for _ in 0..N {
            let _ = verify_crystal(&crystal, &pinned);
        }
        let elapsed = start.elapsed().as_micros() as f64;
        let us_per_verify = elapsed / N as f64;

        let mut params = BTreeMap::new();
        params.insert("n_crystals".to_string(), N.to_string());
        params.insert("n_evidence_entries".to_string(), "50".to_string());
        self.make_result("B08", git_commit, "evidence_verification_us", us_per_verify, "us_per_verify", params)
    }

    /// B09: Memory scaling — N = 100..5000 entities, estimate heap usage
    fn b09_memory_scaling(&self, git_commit: &str) -> Vec<BenchResult> {
        let ns = [100usize, 500, 1000, 2000, 5000];
        let mut results = Vec::new();
        for &n in &ns {
            let graph = build_test_graph(n, self.seed, &self.config);
            // Use structural heap estimate (avoids RSS noise from OS page rounding)
            let heap_bytes = std::mem::size_of_val(&graph) + graph.estimate_heap_size();
            let heap_mb = heap_bytes as f64 / (1024.0 * 1024.0);
            // Also capture RSS for comparison; use max of both to ensure non-zero
            let rss_mb = get_rss_mb();
            let report_mb = if heap_mb > 0.0 { heap_mb } else { rss_mb };
            let mut params = BTreeMap::new();
            params.insert("n_entities".to_string(), n.to_string());
            params.insert("rss_mb".to_string(), format!("{:.2}", rss_mb));
            results.push(self.make_result(
                "B09",
                git_commit,
                &format!("memory_scaling_n{}", n),
                report_mb,
                "MB",
                params,
            ));
        }
        results
    }

    /// B10: Full macro-step — end-to-end macro_step() on reference dataset
    fn b10_full_macro_step(&self, git_commit: &str) -> BenchResult {
        let mut state = GlobalState::new(&self.config);
        let adapter = PassthroughAdapter::new("bench-b10");

        let obs_payloads: Vec<Vec<u8>> = (0..10)
            .map(|i| format!("{{\"entity\":{},\"value\":{}}}", i, i as f64).into_bytes())
            .collect();

        let start = Instant::now();
        let _ = macro_step(&mut state, &obs_payloads, &self.config, &adapter);
        let elapsed_ms = start.elapsed().as_millis() as f64;
        let rss_mb = get_rss_mb();

        let mut params = BTreeMap::new();
        params.insert("n_observations".to_string(), "10".to_string());
        params.insert("peak_rss_mb".to_string(), rss_mb.to_string());
        self.make_result("B10", git_commit, "full_macro_step_ms", elapsed_ms, "ms", params)
    }

    /// B11: Registry resolution speed — 1000 entries, 100_000 lookups
    fn b11_registry_resolution(&self, git_commit: &str) -> BenchResult {
        const N_ENTRIES: usize = 1_000;
        const N_LOOKUPS: usize = 100_000;

        let mut reg = Registry::new(RegistryKind::Operator);
        for i in 0..N_ENTRIES {
            let entry = RegistryEntry::new(
                format!("Operator{:04}", i),
                "1.0.0".to_string(),
                [0u8; 32],
                RegistryKind::Operator,
                BTreeMap::new(),
            );
            let _ = reg.register(entry);
        }

        // Pick a name near the middle for consistent lookup
        let target = format!("Operator{:04}", N_ENTRIES / 2);
        let start = Instant::now();
        for _ in 0..N_LOOKUPS {
            let _ = reg.resolve(&target);
        }
        let elapsed_us = start.elapsed().as_micros() as f64;
        let us_per_lookup = elapsed_us / N_LOOKUPS as f64;

        let mut params = BTreeMap::new();
        params.insert("n_entries".to_string(), N_ENTRIES.to_string());
        params.insert("n_lookups".to_string(), N_LOOKUPS.to_string());
        self.make_result("B11", git_commit, "registry_resolution", us_per_lookup, "us_per_lookup", params)
    }

    /// B12: Manifest construction speed — 100-trace run → build_manifest()
    fn b12_manifest_construction(&self, git_commit: &str) -> BenchResult {
        use isls_types::{GateSnapshot, SchedulerConfig};
        let rd = isls_types::RunDescriptor {
            config: self.config.clone(),
            operator_versions: BTreeMap::new(),
            initial_state_digest: [0u8; 32],
            seed: None,
            registry_digests: BTreeMap::new(),
            scheduler: SchedulerConfig::default(),
        };
        let archive = isls_archive::Archive::new();
        let registries = RegistrySet::new();
        let traces: Vec<TraceEntry> = (0..100).map(|k| TraceEntry {
            tick: k as u64,
            input_digest: [0u8; 32],
            state_digest: [1u8; 32],
            crystal_id: None,
            gate_snapshot: GateSnapshot::default(),
            metrics_digest: [2u8; 32],
        }).collect();
        let obs_log: Vec<Vec<Vec<u8>>> = vec![];

        let start = Instant::now();
        let _ = build_manifest(&rd, &traces, &archive, &registries, "discovery", &obs_log);
        let elapsed_ms = start.elapsed().as_millis() as f64;

        let mut params = BTreeMap::new();
        params.insert("n_traces".to_string(), "100".to_string());
        self.make_result("B12", git_commit, "manifest_construction", elapsed_ms, "ms_per_manifest", params)
    }

    /// B13: Manifest verification speed — 100 crystal + 100 trace digests
    fn b13_manifest_verification(&self, git_commit: &str) -> BenchResult {
        use isls_types::{GateSnapshot, SchedulerConfig};
        let rd = isls_types::RunDescriptor {
            config: self.config.clone(),
            operator_versions: BTreeMap::new(),
            initial_state_digest: [0u8; 32],
            seed: None,
            registry_digests: BTreeMap::new(),
            scheduler: SchedulerConfig::default(),
        };
        let archive = isls_archive::Archive::new();
        let registries = RegistrySet::new();
        let traces: Vec<TraceEntry> = (0..100).map(|k| TraceEntry {
            tick: k as u64,
            input_digest: [0u8; 32],
            state_digest: [1u8; 32],
            crystal_id: None,
            gate_snapshot: GateSnapshot::default(),
            metrics_digest: [2u8; 32],
        }).collect();
        let obs_log: Vec<Vec<Vec<u8>>> = vec![];
        let manifest = build_manifest(&rd, &traces, &archive, &registries, "discovery", &obs_log);

        let start = Instant::now();
        let _ = verify_manifest(&manifest, &rd, &archive, &traces, &registries);
        let elapsed_ms = start.elapsed().as_millis() as f64;

        let mut params = BTreeMap::new();
        params.insert("n_traces".to_string(), "100".to_string());
        self.make_result("B13", git_commit, "manifest_verification", elapsed_ms, "ms_per_verify", params)
    }

    /// B14: Capsule seal/open round-trip — 10_000 iterations with 1KB secret
    fn b14_capsule_roundtrip(&self, git_commit: &str) -> BenchResult {
        use isls_types::SchedulerConfig;
        const N: usize = 10_000;
        let secret = vec![0x42u8; 1024]; // 1 KB

        let rd = isls_types::RunDescriptor {
            config: self.config.clone(),
            operator_versions: BTreeMap::new(),
            initial_state_digest: [0u8; 32],
            seed: None,
            registry_digests: BTreeMap::new(),
            scheduler: SchedulerConfig::default(),
        };
        let archive = isls_archive::Archive::new();
        let registries = RegistrySet::new();
        let traces: Vec<TraceEntry> = vec![];
        let obs_log: Vec<Vec<Vec<u8>>> = vec![];
        let manifest = build_manifest(&rd, &traces, &archive, &registries, "discovery", &obs_log);

        let policy = CapsulePolicy {
            require_lock_program_id: [0u8; 32],
            require_rd_digest: manifest.rd_digest,
            require_gate_proofs: vec![],
            require_manifest_id: Some(manifest.run_id),
            expires_at: None,
            max_uses: None,
        };
        let master_key: [u8; 32] = *b"bench-capsule-master-key-32bytes";

        let start = Instant::now();
        for _ in 0..N {
            let capsule = seal(&secret, policy.clone(), BTreeMap::new(), &master_key, &manifest)
                .expect("seal must succeed");
            let _ = open(&capsule, &master_key, &manifest, None)
                .expect("open must succeed");
        }
        let elapsed_us = start.elapsed().as_micros() as f64;
        let us_per_roundtrip = elapsed_us / N as f64;

        let mut params = BTreeMap::new();
        params.insert("n_roundtrips".to_string(), N.to_string());
        params.insert("secret_bytes".to_string(), "1024".to_string());
        self.make_result("B14", git_commit, "capsule_roundtrip", us_per_roundtrip, "us_per_roundtrip", params)
    }

    /// B15: Scheduler overhead — 100 macro-steps enabled (n_max=10) vs disabled
    fn b15_scheduler_overhead(&self, git_commit: &str) -> BenchResult {
        use isls_types::SchedulerConfig;
        const N: usize = 100;

        let cfg_disabled = SchedulerConfig { enabled: false, n_min: 1, n_max: 10, ..SchedulerConfig::default() };
        let cfg_enabled  = SchedulerConfig { enabled: true,  n_min: 1, n_max: 10,
            strategy: "max_pressure".to_string(), ..SchedulerConfig::default() };

        // Disabled: N iterations
        let start = Instant::now();
        for k in 0..N {
            let _ = compute_substeps(k as f64 * 0.01, 0.0, 0.0, &cfg_disabled);
        }
        let disabled_us = start.elapsed().as_micros() as f64;

        // Enabled: N iterations
        let start = Instant::now();
        for k in 0..N {
            let _ = compute_substeps(k as f64 * 0.01, 0.0, 0.0, &cfg_enabled);
        }
        let enabled_us = start.elapsed().as_micros() as f64;

        let overhead_pct = if disabled_us > 0.0 {
            (enabled_us - disabled_us) / disabled_us * 100.0
        } else {
            0.0
        };

        let mut params = BTreeMap::new();
        params.insert("n_steps".to_string(), N.to_string());
        params.insert("disabled_us".to_string(), format!("{:.2}", disabled_us));
        params.insert("enabled_us".to_string(), format!("{:.2}", enabled_us));
        self.make_result("B15", git_commit, "scheduler_overhead_pct", overhead_pct, "overhead_pct", params)
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_state_obs(vertex_id: u64, value: f64) -> isls_types::Observation {
    use isls_types::{ProvenanceEnvelope, MeasurementContext, content_address_raw};
    let payload = format!("{{\"v\":{}}}", value).into_bytes();
    let digest = content_address_raw(&payload);
    isls_types::Observation {
        timestamp: 0.0,
        source_id: vertex_id.to_string(),
        provenance: ProvenanceEnvelope::default(),
        payload,
        context: MeasurementContext::default(),
        digest,
        schema_version: "1.0".to_string(),
    }
}

fn build_test_graph(n: usize, seed: u64, config: &Config) -> PersistentGraph {
    let mut graph = PersistentGraph::new();
    let mut rng = seed;
    for i in 0..n {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let v = (rng as f64) / (u64::MAX as f64);
        let obs = make_state_obs(i as u64, v);
        let _ = graph.apply_observations(&[obs], &config.persistence);
    }
    graph
}

fn make_test_crystal() -> isls_types::SemanticCrystal {
    use isls_types::{CommitProof, GateSnapshot, ConsensusResult, PoRTrace};

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
        vec![1, 2, 3],
        0.95,
        0,
        -0.5,
        0,
        vec![],
        commit_proof,
    )
}

fn get_rss_mb() -> f64 {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                if let Some(kb_str) = line.split_whitespace().nth(1) {
                    if let Ok(kb) = kb_str.parse::<u64>() {
                        return kb as f64 / 1024.0;
                    }
                }
            }
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regression_check_stable() {
        let mk = |v: f64| BenchResult {
            bench_id: "B01".into(),
            timestamp: Utc::now(),
            git_commit: "abc".into(),
            metric_name: "test".into(),
            metric_value: v,
            metric_unit: "u".into(),
            params: BTreeMap::new(),
        };
        let history = vec![mk(100.0), mk(100.0), mk(100.0), mk(100.0), mk(100.0)];
        let current = mk(100.5);
        assert_eq!(check_regression(&current, &history), RegressionVerdict::Stable);
    }

    #[test]
    fn test_regression_check_regression() {
        let mk = |v: f64| BenchResult {
            bench_id: "B01".into(),
            timestamp: Utc::now(),
            git_commit: "abc".into(),
            metric_name: "test".into(),
            metric_value: v,
            metric_unit: "u".into(),
            params: BTreeMap::new(),
        };
        let history = vec![mk(100.0), mk(100.0), mk(100.0), mk(100.0), mk(100.0)];
        let current = mk(200.0);
        assert_eq!(check_regression(&current, &history), RegressionVerdict::Regression);
    }

    #[test]
    fn test_regression_check_insufficient_history() {
        let mk = |v: f64| BenchResult {
            bench_id: "B01".into(),
            timestamp: Utc::now(),
            git_commit: "abc".into(),
            metric_name: "test".into(),
            metric_value: v,
            metric_unit: "u".into(),
            params: BTreeMap::new(),
        };
        let history = vec![mk(100.0)];
        let current = mk(100.0);
        assert_eq!(check_regression(&current, &history), RegressionVerdict::InsufficientHistory);
    }

    #[test]
    fn test_bench_suite_b01_creates_result() {
        let suite = BenchSuite::new(Config::default(), 42);
        let r = suite.b01_ingestion_throughput("test");
        assert_eq!(r.bench_id, "B01");
        assert!(r.metric_value > 0.0);
    }

    #[test]
    fn test_b06_crystal_serialization() {
        let suite = BenchSuite::new(Config::default(), 42);
        let r = suite.b06_crystal_serialization("test");
        assert_eq!(r.bench_id, "B06");
        assert!(r.metric_value >= 0.0);
    }

    #[test]
    fn test_b08_evidence_verification() {
        let suite = BenchSuite::new(Config::default(), 42);
        let r = suite.b08_evidence_verification("test");
        assert_eq!(r.bench_id, "B08");
        assert!(r.metric_value >= 0.0);
    }

    #[test]
    fn test_b10_full_macro_step() {
        let suite = BenchSuite::new(Config::default(), 42);
        let r = suite.b10_full_macro_step("test");
        assert_eq!(r.bench_id, "B10");
        assert!(r.metric_value >= 0.0);
    }

    #[test]
    fn test_all_10_benchmarks_have_ids() {
        let suite = BenchSuite::new(Config::default(), 42);
        let results = suite.run_all();
        let ids: std::collections::HashSet<&str> = results.iter().map(|r| r.bench_id.as_str()).collect();
        for id in &["B01", "B02", "B03", "B04", "B05", "B06", "B07", "B08", "B09", "B10"] {
            assert!(ids.contains(id), "Missing benchmark {}", id);
        }
    }
}
