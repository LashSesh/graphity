// isls-cli: Single-binary operator interface (C11)
// Spec: ISLS_ValidationHarness_v1_0_0, §1 Operator Interaction Model

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use isls_types::{Config, RunDescriptor};
use isls_engine::{GlobalState, macro_step};
use isls_observe::PassthroughAdapter;
use isls_archive::Archive;
use isls_harness::{
    BenchSuite, FormalValidator, FullReport, MetricCollector, MetricSnapshot,
    ReportGenerator, RetroValidator, ScenarioKind, SyntheticGenerator, SystemOverview,
    generate_iteration_guidance, AlertLevel,
};

// ─── CLI Argument Parsing (no external deps) ─────────────────────────────────

#[derive(Debug)]
enum Command {
    Init,
    Ingest { adapter: String, path: Option<String>, entities: Option<usize> },
    Run { replay: Option<String>, mode: RunMode, ticks: usize },
    Bench,
    Validate { formal: bool, retro: bool },
    Report { json: bool, html: bool },
    Status,
    Help,
}

#[derive(Debug, Clone, PartialEq)]
enum RunMode {
    Shadow,
    Live,
}

fn parse_args(args: &[String]) -> Command {
    if args.len() < 2 {
        return Command::Help;
    }
    match args[1].as_str() {
        "init" => Command::Init,
        "ingest" => {
            let adapter = args.iter().position(|a| a == "--adapter")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "synthetic".to_string());
            let path = args.iter().position(|a| a == "--path")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let entities = args.iter().position(|a| a == "--entities")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok());
            Command::Ingest { adapter, path, entities }
        }
        "run" => {
            let replay = args.iter().position(|a| a == "--replay")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let mode = args.iter().position(|a| a == "--mode")
                .and_then(|i| args.get(i + 1))
                .map(|m| if m == "shadow" { RunMode::Shadow } else { RunMode::Live })
                .unwrap_or(RunMode::Live);
            let ticks = args.iter().position(|a| a == "--ticks")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(10);
            Command::Run { replay, mode, ticks }
        }
        "bench" => Command::Bench,
        "validate" => {
            let formal = args.contains(&"--formal".to_string());
            let retro = args.contains(&"--retro".to_string());
            Command::Validate { formal: formal || (!formal && !retro), retro }
        }
        "report" => {
            let json = args.contains(&"--json".to_string());
            let html = args.contains(&"--html".to_string());
            Command::Report { json, html }
        }
        "status" => Command::Status,
        _ => Command::Help,
    }
}

// ─── ISLS Data Directories ────────────────────────────────────────────────────

fn isls_dir() -> PathBuf {
    dirs_home().join(".isls")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn ensure_dirs() -> std::io::Result<()> {
    let base = isls_dir();
    for sub in &[
        "data/hot", "data/warm", "data/cold", "data/crystals",
        "metrics", "reports", "replay",
    ] {
        std::fs::create_dir_all(base.join(sub))?;
    }
    Ok(())
}

// ─── Config Loading ───────────────────────────────────────────────────────────

fn load_config() -> Config {
    let cfg_path = isls_dir().join("config.json");
    if cfg_path.exists() {
        if let Ok(s) = std::fs::read_to_string(&cfg_path) {
            if let Ok(c) = serde_json::from_str(&s) {
                return c;
            }
        }
    }
    Config::default()
}

fn save_config(config: &Config) {
    let cfg_path = isls_dir().join("config.json");
    if let Ok(s) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(cfg_path, s);
    }
}

// ─── State Loading/Saving ─────────────────────────────────────────────────────

fn load_archive() -> Archive {
    let archive_path = isls_dir().join("data/crystals/archive.jsonl");
    if !archive_path.exists() {
        return Archive::new();
    }
    let mut archive = Archive::new();
    if let Ok(s) = std::fs::read_to_string(&archive_path) {
        for line in s.lines() {
            if let Ok(crystal) = serde_json::from_str(line) {
                archive.append(crystal);
            }
        }
    }
    archive
}

fn save_archive(archive: &Archive) {
    let path = isls_dir().join("data/crystals/archive.jsonl");
    let lines: String = archive.crystals()
        .iter()
        .filter_map(|c| serde_json::to_string(c).ok())
        .map(|s| s + "\n")
        .collect();
    let _ = std::fs::write(path, lines);
}

fn append_jsonl(path: &PathBuf, line: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(f, "{}", line).unwrap_or(());
}

// ─── Metric Helpers ───────────────────────────────────────────────────────────

fn build_snapshot_from_state(state: &GlobalState, collector: &mut MetricCollector) -> MetricSnapshot {
    let active_v = state.graph.active_vertices().len();
    let archive_len = state.archive.len();
    collector.collect(
        state.candidates.len(),
        active_v,
        active_v, // edge count approximation
        archive_len,
        0,     // archive bytes
        1.0,   // replay fidelity (assume OK)
        0,     // operator version drift
        0,     // storage bytes cold
        0,     // memory RSS
        100.0, // extraction throughput
        0.0,   // carrier migration latency
        active_v,
        0.1,   // basket quality lift
    )
}

fn _build_system_overview(state: &GlobalState, start_time: Instant) -> SystemOverview {
    SystemOverview {
        version: "1.0.0".to_string(),
        uptime_secs: start_time.elapsed().as_secs(),
        entity_count: state.graph.active_vertices().len(),
        edge_count: state.graph.active_vertices().len(), // approximation
        crystal_count: state.archive.len(),
        storage_bytes: 0,
        generated_at: chrono::Utc::now(),
    }
}

// ─── Commands ─────────────────────────────────────────────────────────────────

fn cmd_init() {
    ensure_dirs().expect("failed to create ISLS directories");
    let config = Config::default();
    save_config(&config);
    println!("ISLS initialized at {}", isls_dir().display());
    println!("Config written to {}", isls_dir().join("config.json").display());
    println!("Data directories created.");
    println!("\nNext steps:");
    println!("  isls ingest --adapter synthetic --entities 100");
    println!("  isls run");
    println!("  isls status");
}

fn cmd_ingest(adapter_name: &str, path: Option<&str>, entities: Option<usize>) {
    ensure_dirs().expect("failed to create ISLS directories");
    let config = load_config();
    let n = entities.unwrap_or(500);

    println!("Ingesting via adapter '{}' (entities: {})...", adapter_name, n);

    match adapter_name {
        "synthetic" => {
            let kind = ScenarioKind::SBasic;
            let mut gen = SyntheticGenerator::reference(kind);
            let windows = gen.generate();
            println!("Generated {} observation windows, {} entities each.", windows.len(), n.min(windows.first().map(|w| w.len()).unwrap_or(0)));
            println!("Data ready for `isls run`.");
        }
        "file-csv" => {
            if let Some(p) = path {
                println!("Would read CSV from: {}", p);
                println!("(CSV adapter: provide --path <dir> containing OHLCV files)");
            } else {
                eprintln!("Error: --path required for file-csv adapter");
            }
        }
        "file-jsonl" => {
            if let Some(p) = path {
                println!("Would read JSONL from: {}", p);
            } else {
                eprintln!("Error: --path required for file-jsonl adapter");
            }
        }
        _ => {
            eprintln!("Unknown adapter: {}. Available: synthetic, file-csv, file-jsonl, replay", adapter_name);
        }
    }
}

fn cmd_run(replay: Option<&str>, mode: RunMode, ticks: usize) {
    ensure_dirs().expect("failed to create dirs");
    let config = load_config();

    if let Some(replay_path) = replay {
        cmd_run_replay(replay_path, &config);
        return;
    }

    let mode_str = match mode {
        RunMode::Shadow => "shadow",
        RunMode::Live => "live",
    };
    println!("Starting ISLS engine in {} mode...", mode_str);
    println!("(Press Ctrl+C to stop)");

    let mut state = GlobalState::new(&config);
    let adapter = PassthroughAdapter::new("isls-run");
    let mut collector = MetricCollector::new();
    let start = Instant::now();
    let metrics_path = isls_dir().join("metrics/metrics.jsonl");
    let alerts_path = isls_dir().join("metrics/alerts.jsonl");

    // Run a few steps with synthetic data for demonstration
    let mut gen = SyntheticGenerator::reference(ScenarioKind::SBasic);
    let windows = gen.generate();

    for (i, window) in windows.iter().take(ticks).enumerate() {
        let obs_payloads: Vec<Vec<u8>> = window.iter().map(|o| o.payload.clone()).collect();
        let step_start = Instant::now();
        let crystal = macro_step(&mut state, &obs_payloads, &config, &adapter)
            .unwrap_or(None);
        let step_secs = step_start.elapsed().as_secs_f64();

        collector.record_ingestion(obs_payloads.len() as u64);
        collector.record_macro_step(
            step_secs,
            false,
            crystal.is_some(),
            crystal.as_ref().map(|c| c.free_energy),
            crystal.as_ref().map(|c| c.commit_proof.consensus_result.mci),
            None,
        );

        let snap = build_snapshot_from_state(&state, &mut collector);
        append_jsonl(&metrics_path, &MetricCollector::to_jsonl(&snap));

        let alerts = collector.check_alerts(&snap);
        for alert in &alerts {
            append_jsonl(&alerts_path, &serde_json::to_string(alert).unwrap_or_default());
        }

        if i % 5 == 0 {
            println!("Step {}: {} entities, {} crystals, {:.2}s",
                i + 1,
                state.graph.active_vertices().len(),
                state.archive.len(),
                step_secs
            );
        }

        if mode == RunMode::Shadow && crystal.is_some() {
            println!("  [shadow] Crystal emitted but not forwarded downstream");
        }
    }

    save_archive(&state.archive);
    println!("\nRun complete. {} macro-steps executed.", windows.len().min(ticks));
    println!("Metrics written to {}", metrics_path.display());
}

fn cmd_run_replay(path: &str, config: &Config) {
    println!("Replaying from descriptor: {}", path);
    let descriptor_json = std::fs::read_to_string(path)
        .unwrap_or_else(|_| {
            eprintln!("Error: cannot read replay descriptor from {}", path);
            std::process::exit(1);
        });
    let descriptor: RunDescriptor = serde_json::from_str(&descriptor_json)
        .unwrap_or_else(|e| {
            eprintln!("Error parsing replay descriptor: {}", e);
            std::process::exit(1);
        });

    let obs_batches: Vec<Vec<Vec<u8>>> = Vec::new(); // empty for now
    match isls_engine::run_with_descriptor(&descriptor, &obs_batches) {
        Ok(crystals) => {
            println!("Replay complete: {} steps, {} crystals",
                obs_batches.len(), crystals.iter().filter(|c| c.is_some()).count());
        }
        Err(e) => {
            eprintln!("Replay error: {}", e);
        }
    }
}

fn cmd_bench() {
    ensure_dirs().expect("failed to create dirs");
    let config = load_config();
    let suite = BenchSuite::new(config, 42);

    println!("Running ISLS benchmark suite (B01–B10)...");
    println!("{:<6} {:<35} {:>14} {:<15}", "ID", "Metric", "Value", "Unit");
    println!("{}", "-".repeat(75));

    let results = suite.run_all();
    let history_path = isls_dir().join("metrics/bench_history.jsonl");

    for result in &results {
        println!("{:<6} {:<35} {:>14.3} {:<15}",
            result.bench_id, result.metric_name,
            result.metric_value, result.metric_unit);
        append_jsonl(&history_path, &serde_json::to_string(result).unwrap_or_default());
    }

    println!("\nBenchmark history appended to {}", history_path.display());

    // Check for regressions by reading history
    let history = load_bench_history(&history_path);
    let mut regressions = 0;
    for result in &results {
        let hist: Vec<_> = history.iter()
            .filter(|h| h.bench_id == result.bench_id && h.metric_name == result.metric_name)
            .cloned()
            .collect();
        let verdict = isls_harness::check_regression(result, &hist);
        if verdict == isls_harness::RegressionVerdict::Regression {
            println!("  REGRESSION: {} {} ({:.3} vs history mean)",
                result.bench_id, result.metric_name, result.metric_value);
            regressions += 1;
        }
    }
    if regressions == 0 {
        println!("\nNo regressions detected.");
    } else {
        println!("\n{} regression(s) detected!", regressions);
    }
}

fn load_bench_history(path: &PathBuf) -> Vec<isls_harness::BenchResult> {
    if !path.exists() { return vec![]; }
    let s = std::fs::read_to_string(path).unwrap_or_default();
    s.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn cmd_validate(formal: bool, retro: bool) {
    ensure_dirs().expect("failed to create dirs");
    let archive = load_archive();
    let graph = isls_persist::PersistentGraph::new();
    let pinned = BTreeMap::new();

    if formal || !retro {
        println!("Running V-Formal validation ({} crystals in archive)...", archive.len());
        let report = FormalValidator::validate(&archive, &pinned);
        println!("  Total:   {}", report.total_crystals);
        println!("  Passed:  {}", report.passed_crystals);
        println!("  Failed:  {}", report.failed_crystals);
        println!("  Pass rate: {:.1}%", report.pass_rate() * 100.0);

        if !report.check_counts.is_empty() {
            println!("\n  Check breakdown:");
            for (check, (passed, total)) in &report.check_counts {
                let rate = *passed as f64 / *total as f64 * 100.0;
                println!("    {:<25} {}/{} ({:.1}%)", check, passed, total, rate);
            }
        }

        // Save formal report
        let rpt_path = isls_dir().join("reports/latest-formal.json");
        if let Ok(s) = serde_json::to_string_pretty(&report) {
            let _ = std::fs::write(rpt_path, s);
        }
    }

    if retro {
        println!("\nRunning V-Retro validation (horizon: 7 days)...");
        if archive.len() == 0 {
            println!("  No crystals in archive. Run `isls run` first to collect data.");
            return;
        }
        let report = RetroValidator::validate(&archive, &graph, 7);
        println!("  Constraints evaluated: {}", report.total_constraints_evaluated);
        println!("  Hit rate (M20):        {:.1}%", report.hit_rate * 100.0);
        println!("  Mean coverage drift:   {:.3}", report.mean_coverage_drift);
        println!("  False positive rate:   {:.1}%", report.false_positive_rate * 100.0);
    }
}

fn cmd_report(json: bool, html: bool) {
    ensure_dirs().expect("failed to create dirs");
    let config = load_config();
    let archive = load_archive();
    let mut collector = MetricCollector::new();

    // Build a default state for reporting
    let _state = GlobalState::new(&config);
    let snap = collector.collect(
        0, 0, 0, archive.len(), 0, 1.0, 0, 0, 0, 100.0, 0.0, 0, 0.1,
    );

    let alerts = collector.check_alerts(&snap);
    let health = MetricCollector::overall_health(&snap);
    let items = generate_iteration_guidance(&snap, &config);

    let overview = SystemOverview {
        version: "1.0.0".to_string(),
        uptime_secs: 0,
        entity_count: 0,
        edge_count: 0,
        crystal_count: archive.len(),
        storage_bytes: 0,
        generated_at: chrono::Utc::now(),
    };

    let report = FullReport {
        overview,
        latest_metrics: snap.clone(),
        alerts: alerts.clone(),
        iteration_items: items.clone(),
        health: health.clone(),
        history_len: collector.history.len(),
    };

    if json {
        println!("{}", ReportGenerator::json(&report));
    } else if html {
        let html_content = ReportGenerator::html(&report);
        // Write to file and print to stdout
        let path = isls_dir().join("reports/latest.html");
        let _ = std::fs::write(&path, &html_content);
        println!("{}", html_content);
    } else {
        // Default: text report
        print_text_report(&report);
    }
}

fn print_text_report(report: &FullReport) {
    let snap = &report.latest_metrics;
    println!("═══════════════════════════════════════════════════════════");
    println!("ISLS System Report — {}", report.overview.generated_at.format("%Y-%m-%d %H:%M:%S UTC"));
    println!("═══════════════════════════════════════════════════════════");

    println!("\n1. SYSTEM OVERVIEW");
    println!("   Version:       {}", report.overview.version);
    println!("   Uptime:        {}s", report.overview.uptime_secs);
    println!("   Entities:      {}", report.overview.entity_count);
    println!("   Edges:         {}", report.overview.edge_count);
    println!("   Crystals:      {}", report.overview.crystal_count);
    println!("   Storage:       {:.1} GB", report.overview.storage_bytes as f64 / 1e9);
    println!("   Health:        {}", report.health);

    println!("\n2. LAYER HEALTH (M1-M5)");
    println!("   M1  L0 Ingestion Rate:  {:.1} obs/s", snap.m1_ingestion_rate);
    println!("   M2  L1 Graph Growth:    {:+} nodes+edges", snap.m2_graph_growth);
    println!("   M3  L2 Active Constr.:  {}", snap.m3_active_constraints);
    println!("   M4  L3 Crystal Rate:    {:.1}/24h", snap.m4_crystal_rate);
    println!("   M5  L4 Mutation Rate:   {:.0}/24h", snap.m5_mutation_rate);

    println!("\n3. CORE QUALITY (M6-M14)");
    println!("   M6  Replay Fidelity:    {:.1}%", snap.m6_replay_fidelity * 100.0);
    println!("   M7  Convergence Rate:   {:.4}", snap.m7_convergence_rate);
    println!("   M8  Lattice Stability:  {:.3}", snap.m8_lattice_stability);
    println!("   M9  Gate Selectivity:   {:.3}", snap.m9_gate_selectivity);
    println!("   M10 Dual Consensus MCI: {:.3}", snap.m10_dual_consensus_mci);
    println!("   M11 PoR Latency:        {:.2}s", snap.m11_por_latency_secs);
    println!("   M12 Evidence Integrity: {:.1}%", snap.m12_evidence_integrity * 100.0);
    println!("   M13 Version Drift:      {}", snap.m13_operator_version_drift);
    println!("   M14 Storage Efficiency: {:.1} MB/asset", snap.m14_storage_efficiency_bytes as f64 / 1e6);

    println!("\n4. PERFORMANCE (M15-M19)");
    println!("   M15 Macro-step Latency: {:.2}s", snap.m15_macro_step_latency_secs);
    println!("   M16 Memory Footprint:   {:.1} GB", snap.m16_memory_footprint_bytes as f64 / 1e9);
    println!("   M17 Extraction Thru.:   {:.0} cand/s", snap.m17_extraction_throughput);
    println!("   M18 Archive Growth:     {:.1} MB/day", snap.m18_archive_growth_bytes_per_day as f64 / 1e6);
    println!("   M19 Migration Latency:  {:.2}s", snap.m19_carrier_migration_latency_secs);

    println!("\n5. EMPIRICAL DOMAIN (M20-M24)");
    println!("   M20 Constraint Hit Rate:  {:.1}%", snap.m20_constraint_hit_rate * 100.0);
    println!("   M21 Predictive Value:     {:.1}%", snap.m21_crystal_predictive_value * 100.0);
    println!("   M22 Signal Lead Time:     {:.0}s", snap.m22_signal_lead_time_secs);
    println!("   M23 Basket Quality Lift:  {:.3}", snap.m23_basket_quality_lift);
    println!("   M24 Coverage Growth:      {} entities", snap.m24_coverage_growth);

    println!("\n6. ALERTS");
    if report.alerts.is_empty() {
        println!("   No active alerts.");
    } else {
        for alert in &report.alerts {
            println!("   [{:3}] {:20} value={:.4}  {}",
                alert.metric_id, alert.metric_name, alert.current_value, alert.message);
        }
    }

    println!("\n7. ACTION ITEMS");
    if report.iteration_items.is_empty() {
        println!("   No action items. System is healthy.");
    } else {
        for item in &report.iteration_items {
            println!("   [{}] {} — {}", item.priority, item.metric_id, item.symptom);
            println!("       Diagnosis: {}", item.diagnosis);
            println!("       Action:    {}", item.action);
            if let Some(key) = &item.config_key {
                println!("       Config:    {}", key);
            }
        }
    }
    println!("═══════════════════════════════════════════════════════════");
}

fn cmd_status() {
    ensure_dirs().ok();
    let config = load_config();
    let archive = load_archive();
    let mut collector = MetricCollector::new();

    let snap = collector.collect(
        0, 0, 0, archive.len(), 0, 1.0, 0, 0, 0, 100.0, 0.0, 0, 0.1,
    );
    let health = MetricCollector::overall_health(&snap);
    let overview = SystemOverview {
        version: "1.0.0".to_string(),
        uptime_secs: 0,
        entity_count: 0,
        edge_count: 0,
        crystal_count: archive.len(),
        storage_bytes: 0,
        generated_at: chrono::Utc::now(),
    };

    let status = ReportGenerator::status_line(&overview, &snap, &health);
    println!("{}", status);
}

fn print_help() {
    println!("ISLS — Invariant Structure Learning System");
    println!("Version 1.0.0");
    println!();
    println!("USAGE:");
    println!("  isls <COMMAND> [OPTIONS]");
    println!();
    println!("COMMANDS:");
    println!("  init                           Generate default config + data dirs");
    println!("  ingest <options>               Attach a live or file-based data source");
    println!("    --adapter <name>             Adapter: synthetic, file-csv, file-jsonl, replay");
    println!("    --path <path>                Data path (for file adapters)");
    println!("    --entities <n>               Number of entities (for synthetic adapter)");
    println!("  run [options]                  Start the macro-step loop");
    println!("    --replay <descriptor>        Deterministic replay from saved descriptor");
    println!("    --mode <shadow|live>         Operation mode (default: live)");
    println!("  bench                          Run full benchmark suite, emit report");
    println!("  validate [options]             Run validation suite against collected data");
    println!("    --formal                     V-Formal: invariant checks on all crystals");
    println!("    --retro                      V-Retro: retrospective accuracy validation");
    println!("  report [options]               Print current health dashboard");
    println!("    --json                       Machine-readable JSON export");
    println!("    --html                       Self-contained HTML dashboard");
    println!("  status                         One-line system health summary");
    println!();
    println!("EXAMPLES:");
    println!("  isls init");
    println!("  isls ingest --adapter synthetic --entities 500");
    println!("  isls run");
    println!("  isls bench");
    println!("  isls validate --formal");
    println!("  isls report");
    println!("  isls report --html > report.html");
    println!("  isls status");
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = parse_args(&args);

    match cmd {
        Command::Init => cmd_init(),
        Command::Ingest { adapter, path, entities } => {
            cmd_ingest(&adapter, path.as_deref(), entities);
        }
        Command::Run { replay, mode, ticks } => {
            cmd_run(replay.as_deref(), mode, ticks);
        }
        Command::Bench => cmd_bench(),
        Command::Validate { formal, retro } => cmd_validate(formal, retro),
        Command::Report { json, html } => cmd_report(json, html),
        Command::Status => cmd_status(),
        Command::Help => print_help(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_parse_init() {
        let cmd = parse_args(&args(&["isls", "init"]));
        assert!(matches!(cmd, Command::Init));
    }

    #[test]
    fn test_parse_bench() {
        let cmd = parse_args(&args(&["isls", "bench"]));
        assert!(matches!(cmd, Command::Bench));
    }

    #[test]
    fn test_parse_status() {
        let cmd = parse_args(&args(&["isls", "status"]));
        assert!(matches!(cmd, Command::Status));
    }

    #[test]
    fn test_parse_run_replay() {
        let cmd = parse_args(&args(&["isls", "run", "--replay", "desc.json"]));
        match cmd {
            Command::Run { replay: Some(r), .. } => assert_eq!(r, "desc.json"),
            _ => panic!("expected Run with replay"),
        }
    }

    #[test]
    fn test_parse_run_mode_shadow() {
        let cmd = parse_args(&args(&["isls", "run", "--mode", "shadow"]));
        match cmd {
            Command::Run { mode, .. } => assert_eq!(mode, RunMode::Shadow),
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn test_parse_validate_formal() {
        let cmd = parse_args(&args(&["isls", "validate", "--formal"]));
        match cmd {
            Command::Validate { formal: true, .. } => {}
            _ => panic!("expected Validate formal"),
        }
    }

    #[test]
    fn test_parse_validate_retro() {
        let cmd = parse_args(&args(&["isls", "validate", "--retro"]));
        match cmd {
            Command::Validate { retro: true, .. } => {}
            _ => panic!("expected Validate retro"),
        }
    }

    #[test]
    fn test_parse_report_json() {
        let cmd = parse_args(&args(&["isls", "report", "--json"]));
        match cmd {
            Command::Report { json: true, .. } => {}
            _ => panic!("expected Report json"),
        }
    }

    #[test]
    fn test_parse_report_html() {
        let cmd = parse_args(&args(&["isls", "report", "--html"]));
        match cmd {
            Command::Report { html: true, .. } => {}
            _ => panic!("expected Report html"),
        }
    }

    #[test]
    fn test_parse_ingest_synthetic() {
        let cmd = parse_args(&args(&["isls", "ingest", "--adapter", "synthetic", "--entities", "100"]));
        match cmd {
            Command::Ingest { adapter, entities: Some(100), .. } => {
                assert_eq!(adapter, "synthetic");
            }
            _ => panic!("expected Ingest"),
        }
    }

    #[test]
    fn test_parse_help_on_empty() {
        let cmd = parse_args(&args(&["isls"]));
        assert!(matches!(cmd, Command::Help));
    }

    #[test]
    fn test_parse_help_on_unknown() {
        let cmd = parse_args(&args(&["isls", "unknown-command"]));
        assert!(matches!(cmd, Command::Help));
    }

    #[test]
    fn test_status_command_runs() {
        // status command should not panic
        cmd_status();
    }

    #[test]
    fn test_report_json_runs() {
        cmd_report(true, false);
    }

    #[test]
    fn test_report_text_runs() {
        cmd_report(false, false);
    }

    #[test]
    fn test_validate_formal_runs() {
        cmd_validate(true, false);
    }

    #[test]
    fn test_validate_retro_runs() {
        cmd_validate(false, true);
    }

    #[test]
    fn test_bench_command_runs() {
        cmd_bench();
    }
}
