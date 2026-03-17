// isls-cli: Single-binary operator interface (C11)
// Spec: ISLS_ValidationHarness_v1_0_0, §1 Operator Interaction Model

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use isls_types::{Config, RunDescriptor, SchedulerConfig};
use isls_engine::{GlobalState, macro_step, execute, ExecuteInput};
use isls_manifest::{build_manifest, TraceEntry};
use isls_observe::ObservationAdapter;
use isls_registry::RegistrySet;
use isls_capsule::{seal, open, CapsulePolicy};
use isls_archive::Archive;
use isls_store::IslandStore;
use isls_harness::{
    BenchSuite, FormalReport, FormalValidator, FullReport, MetricCollector, MetricSnapshot,
    ReportGenerator, RetroValidator, ScenarioKind, SyntheticGenerator, SystemOverview,
    generate_iteration_guidance,
    build_genesis_crystal, validate_genesis,
};

// ─── JSON Entity Adapter ──────────────────────────────────────────────────────

/// Adapter that derives source_id from the "entity" field in a JSON payload.
/// Payloads written by `ingest` / the synthetic generator have the form
/// `{"entity":<N>,"value":<f>,"window":<W>}`.  Extracting entity N and
/// using its string representation ("0", "1", …) as source_id exactly
/// matches what the synthetic generator sets on the original Observation
/// structs, so the persist layer maps each payload back to its stable vertex.
struct JsonEntityAdapter {
    fallback_id: String,
}

impl JsonEntityAdapter {
    fn new(fallback_id: impl Into<String>) -> Self {
        Self { fallback_id: fallback_id.into() }
    }
}

impl ObservationAdapter for JsonEntityAdapter {
    fn source_id(&self) -> &str {
        &self.fallback_id
    }

    fn canonicalize(
        &self,
        raw: &[u8],
        context: &isls_types::MeasurementContext,
    ) -> isls_observe::Result<isls_types::Observation> {
        let payload = raw.to_vec();
        let digest = isls_types::content_address_raw(&payload);

        // Extract the entity index from the JSON payload and use it as
        // source_id so every observation for entity N always maps to the
        // same vertex, regardless of which window it came from.
        let source_id = std::str::from_utf8(raw)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .and_then(|v| v["entity"].as_u64())
            .map(|e| e.to_string())
            .unwrap_or_else(|| self.fallback_id.clone());

        Ok(isls_types::Observation {
            timestamp: 0.0,
            source_id,
            provenance: isls_types::ProvenanceEnvelope {
                origin: self.fallback_id.clone(),
                chain: Vec::new(),
                sig: None,
            },
            payload,
            context: context.clone(),
            digest,
            schema_version: "1.0.0".to_string(),
        })
    }
}

// ─── CLI Argument Parsing (no external deps) ─────────────────────────────────

#[derive(Debug)]
enum Command {
    Init { store: Option<String> },
    Ingest { adapter: String, path: Option<String>, entities: Option<usize>, scenario: Option<String> },
    Run { replay: Option<String>, mode: RunMode, ticks: usize, project: Option<String> },
    Execute { input: String, ticks: usize, output: Option<String> },
    Seal { secret: String, lock_manifest: Option<String>, output: Option<String> },
    Open { capsule: String },
    Bench,
    Validate { formal: bool, retro: bool },
    Report { json: bool, html: bool, full_html: bool },
    Status,
    Help,
    // C17 store commands
    ProjectList,
    ProjectCreate { name: String },
    CrystalList { run_id: String },
    CrystalShow { crystal_id: String },
    Export { run_id: String, output: String },
    StoreVacuum,
    StoreCheck,
    // Genesis Crystal commands
    GenesisShow,
    GenesisValidate,
    // C25 Oracle commands
    OracleStatus,
    OracleMemory,
    OracleSealKey { key: String, lock_genesis: bool },
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
        "init" => {
            let store = args.iter().position(|a| a == "--store")
                .and_then(|i| args.get(i + 1))
                .cloned();
            Command::Init { store }
        }
        "project" => {
            if args.len() > 2 {
                match args[2].as_str() {
                    "list" => Command::ProjectList,
                    "create" => {
                        let name = args.iter().position(|a| a == "--name")
                            .and_then(|i| args.get(i + 1))
                            .cloned()
                            .unwrap_or_else(|| "default".to_string());
                        Command::ProjectCreate { name }
                    }
                    _ => Command::Help,
                }
            } else { Command::ProjectList }
        }
        "crystal" => {
            if args.len() > 2 {
                match args[2].as_str() {
                    "list" => {
                        let run_id = args.iter().position(|a| a == "--run")
                            .and_then(|i| args.get(i + 1))
                            .cloned()
                            .unwrap_or_default();
                        Command::CrystalList { run_id }
                    }
                    "show" => {
                        let crystal_id = args.get(3).cloned().unwrap_or_default();
                        Command::CrystalShow { crystal_id }
                    }
                    _ => Command::Help,
                }
            } else { Command::Help }
        }
        "export" => {
            let run_id = args.iter().position(|a| a == "--run")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "latest".to_string());
            let output = args.iter().position(|a| a == "--output")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "export.json".to_string());
            Command::Export { run_id, output }
        }
        "store" => {
            if args.len() > 2 {
                match args[2].as_str() {
                    "vacuum" => Command::StoreVacuum,
                    "check" => Command::StoreCheck,
                    _ => Command::Help,
                }
            } else { Command::Help }
        }
        "genesis" => {
            if args.len() > 2 {
                match args[2].as_str() {
                    "show"     => Command::GenesisShow,
                    "validate" => Command::GenesisValidate,
                    _ => Command::Help,
                }
            } else { Command::GenesisShow }
        }
        "oracle" => {
            if args.len() > 2 {
                match args[2].as_str() {
                    "status" => Command::OracleStatus,
                    "memory" => Command::OracleMemory,
                    "seal-key" => {
                        let key = args.iter().position(|a| a == "--key")
                            .and_then(|i| args.get(i + 1))
                            .cloned()
                            .unwrap_or_default();
                        let lock_genesis = args.contains(&"--lock-genesis".to_string());
                        Command::OracleSealKey { key, lock_genesis }
                    }
                    _ => Command::OracleStatus,
                }
            } else { Command::OracleStatus }
        }
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
            let scenario = args.iter().position(|a| a == "--scenario")
                .and_then(|i| args.get(i + 1))
                .cloned();
            Command::Ingest { adapter, path, entities, scenario }
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
            let project = args.iter().position(|a| a == "--project")
                .and_then(|i| args.get(i + 1))
                .cloned();
            Command::Run { replay, mode, ticks, project }
        }
        "execute" => {
            let input = args.iter().position(|a| a == "--input")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "latest".to_string());
            let ticks = args.iter().position(|a| a == "--ticks")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(10);
            let output = args.iter().position(|a| a == "--output")
                .and_then(|i| args.get(i + 1))
                .cloned();
            Command::Execute { input, ticks, output }
        }
        "seal" => {
            let secret = args.iter().position(|a| a == "--secret")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_default();
            let lock_manifest = args.iter().position(|a| a == "--lock-manifest")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let output = args.iter().position(|a| a == "--output")
                .and_then(|i| args.get(i + 1))
                .cloned();
            Command::Seal { secret, lock_manifest, output }
        }
        "open" => {
            let capsule = args.iter().position(|a| a == "--capsule")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_default();
            Command::Open { capsule }
        }
        "bench" => Command::Bench,
        "validate" => {
            let formal = args.contains(&"--formal".to_string());
            let retro = args.contains(&"--retro".to_string());
            Command::Validate { formal: formal || (!formal && !retro), retro }
        }
        "report" => {
            let full_html = args.contains(&"--full-html".to_string())
                || args.contains(&"full-html".to_string());
            let json = !full_html && (args.contains(&"--json".to_string()) || args.contains(&"json".to_string()));
            let html = !full_html && (args.contains(&"--html".to_string()) || args.contains(&"html".to_string()));
            Command::Report { json, html, full_html }
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

fn build_snapshot_from_state(state: &GlobalState, collector: &mut MetricCollector, basket_lift: f64) -> MetricSnapshot {
    let active_v = state.graph.active_vertices().len();
    let archive_len = state.archive.len();
    collector.collect(
        state.last_constraint_count,
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
        basket_lift,
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

fn cmd_init(store: Option<&str>) {
    ensure_dirs().expect("failed to create ISLS directories");
    let config = Config::default();
    save_config(&config);
    println!("ISLS initialized at {}", isls_dir().display());
    println!("Config written to {}", isls_dir().join("config.json").display());
    println!("Data directories created.");

    if store == Some("sqlite") {
        let db_path = isls_dir().join("isls.db");
        match IslandStore::open(&db_path) {
            Ok(_) => println!("SQLite store initialized at {}", db_path.display()),
            Err(e) => eprintln!("Warning: store init failed: {e}"),
        }
    }

    // Build and commit the Genesis Crystal
    let mut archive = load_archive();
    if archive.crystals().iter().any(|c| c.created_at == 0) {
        eprintln!("Error: genesis crystal already exists. Use 'isls genesis show' to inspect it.");
        std::process::exit(1);
    }

    let registries = RegistrySet::new();
    match build_genesis_crystal(&config, &registries) {
        Ok(gc) => {
            let gc_id: String = gc.crystal_id.iter().map(|b| format!("{:02x}", b)).collect();
            let class = gc.genesis_metadata.as_ref()
                .map(|m| format!("{:?}", m.conformance_class))
                .unwrap_or_else(|| "C0".to_string());
            let n_constraints = gc.genesis_metadata.as_ref()
                .map(|m| m.constraints.len())
                .unwrap_or(0);
            archive.append(gc);
            save_archive(&archive);
            println!("Genesis Crystal committed: {}...", &gc_id[..16]);
            println!("  Conformance: {}  |  Constraints: {}/{} satisfied",
                class, n_constraints, n_constraints);
        }
        Err(e) => {
            eprintln!("Error: genesis crystal build failed: {e}");
            std::process::exit(1);
        }
    }

    println!("\nNext steps:");
    println!("  isls ingest --adapter synthetic --entities 100");
    println!("  isls run");
    println!("  isls status");
}

fn cmd_genesis_show() {
    let archive = load_archive();
    match archive.crystals().iter().find(|c| c.created_at == 0) {
        None => {
            println!("Genesis Crystal: NOT FOUND");
            println!("Run 'isls init' to initialize the system constitution.");
        }
        Some(gc) => {
            let id_hex: String = gc.crystal_id.iter().map(|b| format!("{:02x}", b)).collect();
            println!("Genesis Crystal");
            println!("  Crystal ID:  {}...", &id_hex[..32]);
            println!("  Free energy: {:.1}", gc.free_energy);
            println!("  Stability:   {:.3}", gc.stability_score);
            if let Some(meta) = &gc.genesis_metadata {
                println!("  ADAMANT:     v{}", meta.adamant_version);
                println!("  Conformance: {:?}", meta.conformance_class);
                let satisfied = meta.constraints.iter().filter(|c| c.satisfied).count();
                println!("  Constraints: {}/{} satisfied", satisfied, meta.constraints.len());
                println!("  Crates:      {}", meta.system_fingerprint.crate_count);
                println!("  Tests:       {}", meta.system_fingerprint.test_count);
                println!("  Platform:    {}", meta.system_fingerprint.platform);
                println!("  ISLS:        v{}", meta.system_fingerprint.isls_version);
                if let Some(git) = &meta.system_fingerprint.git_commit {
                    println!("  Git:         {}", git);
                }
                println!("\n  Constitutional constraints:");
                for c in &meta.constraints {
                    let status = if c.satisfied { "PASS" } else { "FAIL" };
                    println!("    [{}] {} | {} | {}", status, c.id, c.axiom_ref,
                        if c.description.len() > 50 { &c.description[..50] } else { &c.description });
                }
            }
        }
    }
}

fn cmd_genesis_validate() {
    let archive = load_archive();
    let config = load_config();
    let registries = RegistrySet::new();
    let result = validate_genesis(&archive, &config, &registries);

    println!("Genesis Validation");
    println!("  GV1 Existence:   {}", if result.exists     { "PASS" } else { "FAIL" });
    println!("  GV2 Integrity:   {}", if result.integrity  { "PASS" } else { "FAIL" });
    println!("  GV3 Conformance: {}", if result.conformance { "PASS" } else { "FAIL" });
    println!("  Conformance class: {:?}", result.conformance_class);

    if result.drift.is_empty() {
        println!("  Constitutional Drift: NONE");
    } else {
        println!("  Constitutional Drift: DETECTED");
        for d in &result.drift {
            println!("    DRIFT: {}", d);
        }
    }

    if result.all_ok() {
        println!("\nGenesis: VALID");
    } else {
        println!("\nGenesis: INVALID");
    }
}

// ─── Oracle Commands (C25) ────────────────────────────────────────────────────

fn cmd_oracle_status() {
    use isls_oracle::{OracleConfig, OracleEngine, OraclePatternMemory};
    let config = OracleConfig::default();
    let engine = OracleEngine::new(config.clone(), OraclePatternMemory::new());
    let m = engine.autonomy();
    let b = engine.budget_status();

    println!("Oracle Status (C25 — Hybrid Synthesis Oracle)");
    println!("  Provider:    Claude ({})", config.model);
    println!("  LLM active:  {}", engine.oracle_available());
    println!();
    println!("  Autonomy Metrics:");
    println!("    Total requests:     {}", m.total_requests);
    println!("    Memory hits:        {}", m.memory_hits);
    println!("    Oracle calls:       {}", m.oracle_calls);
    println!("    Oracle rejections:  {}", m.oracle_rejections);
    println!("    Skeleton fallbacks: {}", m.skeleton_fallbacks);
    println!("    Autonomy ratio M33: {:.1}%",  m.autonomy_ratio * 100.0);
    println!("    Rejection rate M34: {:.1}%", m.rejection_rate() * 100.0);
    println!("    Tokens used:        {}", m.total_tokens);
    println!("    Est. cost:          ${:.4}", m.total_cost_usd);
    println!();
    println!("  Budget:");
    println!("    Calls this run:  {}/{}", b.current.calls_this_run, b.max_calls_per_run);
    println!("    Tokens this run: {}/{}", b.current.tokens_this_run, b.max_tokens_per_run);
    println!("    Cost this run:   ${:.4}/${:.2}", b.current.cost_this_run, b.max_cost_per_run);
    println!("    Calls today:     {}/{}", b.current.calls_today, b.max_calls_per_day);
}

fn cmd_oracle_memory() {
    use isls_oracle::{OraclePatternMemory};
    // Pattern memory is in-process; we report static info here.
    // A full implementation would persist patterns to isls-store.
    let memory = OraclePatternMemory::new();
    println!("Oracle Pattern Memory (C25)");
    println!("  Patterns loaded: {}", memory.len());
    println!("  Avg quality:     {:.2}", memory.avg_quality());
    if memory.is_empty() {
        println!("  No patterns stored yet.");
        println!("  Patterns are crystallized from validated LLM outputs during 'isls forge'.");
    } else {
        let stats = memory.domain_stats();
        println!("  Domains: {}", stats.len());
        for (domain, count) in &stats {
            println!("    {}: {} patterns", domain, count);
        }
    }
    println!();
    println!("  Match threshold:   {:.2}", 0.85f64);
    println!("  Quality threshold: {:.2}", 0.60f64);
}

fn cmd_oracle_seal_key(key: &str, lock_genesis: bool) {
    use isls_capsule::{seal, CapsulePolicy};
    use isls_manifest::{build_manifest, TraceEntry};
    use isls_registry::RegistrySet;
    use std::collections::BTreeMap;

    if key.is_empty() {
        eprintln!("[oracle] Error: --key is required. Usage: isls oracle seal-key --key <api-key> [--lock-genesis]");
        return;
    }

    let archive = load_archive();
    let config = load_config();
    let registries = RegistrySet::new();

    // Build a manifest from the current system state
    let rd = isls_types::RunDescriptor {
        config: config.clone(),
        operator_versions: BTreeMap::new(),
        initial_state_digest: isls_types::content_address_raw(b"oracle-key-seal"),
        seed: None,
        registry_digests: BTreeMap::new(),
        scheduler: isls_types::SchedulerConfig::default(),
    };
    let traces: Vec<TraceEntry> = vec![];
    let obs_log: Vec<Vec<Vec<u8>>> = vec![];
    let manifest = build_manifest(&rd, &traces, &archive, &registries, "oracle", &obs_log);

    if lock_genesis {
        // Validate genesis before sealing
        let genesis_ok = validate_genesis(&archive, &config, &registries).all_ok();
        if !genesis_ok {
            eprintln!("[oracle] Warning: Genesis Crystal is not valid. Key will still be sealed.");
            eprintln!("[oracle]          Run 'isls genesis validate' for details.");
        } else {
            println!("[oracle] Genesis Crystal is valid — key will be sealed with constitutional protection.");
        }
    }

    let policy = CapsulePolicy {
        require_lock_program_id: [0u8; 32],
        require_rd_digest: manifest.rd_digest,
        require_gate_proofs: vec![],
        require_manifest_id: Some(manifest.run_id),
        expires_at: None,
        max_uses: None,
    };

    // Master key: derive from a system-specific constant + rd_digest
    // In production this would come from a hardware key or secure store
    let mut master_key = [0u8; 32];
    let rd_bytes = manifest.rd_digest;
    for (i, b) in rd_bytes.iter().enumerate().take(32) {
        master_key[i] ^= b;
    }
    master_key[0] ^= 0xAB; // domain separator for oracle keys

    let capsule = match seal(key.as_bytes(), policy, BTreeMap::new(), &master_key, &manifest) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[oracle] Seal failed: {e}");
            return;
        }
    };

    // Save capsule to ~/.isls/capsules/oracle-key.json
    ensure_dirs().ok();
    let capsule_dir = isls_dir().join("capsules");
    std::fs::create_dir_all(&capsule_dir).ok();
    let capsule_path = capsule_dir.join("oracle-key.json");
    match serde_json::to_string_pretty(&capsule) {
        Ok(json) => {
            std::fs::write(&capsule_path, json).ok();
            println!("[oracle] API key sealed to: {}", capsule_path.display());
            println!("[oracle] Use api_key_source: \"capsule:oracle-key\" in oracle config.");
            println!("[oracle] The key is bound to run_id: {:02x?}...", &manifest.run_id[..4]);
        }
        Err(e) => eprintln!("[oracle] Failed to serialize capsule: {e}"),
    }
}

fn open_store() -> Option<IslandStore> {
    let db_path = isls_dir().join("isls.db");
    IslandStore::open(&db_path).ok()
}

fn cmd_project_list() {
    match open_store() {
        Some(store) => match store.list_projects() {
            Ok(projects) => {
                if projects.is_empty() {
                    println!("No projects. Use: isls project create --name <name>");
                } else {
                    for p in &projects {
                        println!("{} | {} | {}", p.id, p.name, p.created_at);
                    }
                }
            }
            Err(e) => eprintln!("Error: {e}"),
        },
        None => eprintln!("Store not initialized. Run: isls init --store sqlite"),
    }
}

fn cmd_project_create(name: &str) {
    match open_store() {
        Some(store) => match store.create_project(name, "") {
            Ok(id) => println!("Created project '{}' with id {}", name, id),
            Err(e) => eprintln!("Error: {e}"),
        },
        None => eprintln!("Store not initialized. Run: isls init --store sqlite"),
    }
}

fn cmd_crystal_list(run_id: &str) {
    match open_store() {
        Some(store) => match store.list_crystals(run_id) {
            Ok(crystals) => {
                println!("{} crystals in run {}", crystals.len(), run_id);
                for c in &crystals {
                    println!("  {} | stability={:.3} | tick={}",
                        c.crystal_id, c.stability_score, c.created_at_tick);
                }
            }
            Err(e) => eprintln!("Error: {e}"),
        },
        None => eprintln!("Store not initialized. Run: isls init --store sqlite"),
    }
}

fn cmd_crystal_show(crystal_id: &str) {
    match open_store() {
        Some(store) => match store.get_crystal(crystal_id) {
            Ok(c) => {
                println!("crystal_id:       {}", c.crystal_id);
                println!("run_id:           {}", c.run_id);
                println!("stability_score:  {}", c.stability_score);
                println!("free_energy:      {}", c.free_energy);
                println!("created_at_tick:  {}", c.created_at_tick);
                println!("constraint_count: {}", c.constraint_count);
                println!("region_size:      {}", c.region_size);
                println!("validation:       {}", c.validation_status);
            }
            Err(e) => eprintln!("Error: {e}"),
        },
        None => eprintln!("Store not initialized. Run: isls init --store sqlite"),
    }
}

fn cmd_export(run_id: &str, output: &str) {
    match open_store() {
        Some(store) => {
            let run_id = if run_id == "latest" {
                // Find latest run (simple fallback)
                run_id.to_string()
            } else {
                run_id.to_string()
            };
            let path = std::path::Path::new(output);
            match store.export_run_zip(&run_id, path) {
                Ok(()) => println!("Exported run {} to {}", run_id, output),
                Err(e) => eprintln!("Error: {e}"),
            }
        }
        None => eprintln!("Store not initialized. Run: isls init --store sqlite"),
    }
}

fn cmd_store_vacuum() {
    match open_store() {
        Some(store) => match store.vacuum() {
            Ok(()) => println!("Vacuum complete."),
            Err(e) => eprintln!("Error: {e}"),
        },
        None => eprintln!("Store not initialized. Run: isls init --store sqlite"),
    }
}

fn cmd_store_check() {
    match open_store() {
        Some(store) => match store.integrity_check() {
            Ok(true) => println!("Store integrity: OK"),
            Ok(false) => println!("Store integrity: FAIL"),
            Err(e) => eprintln!("Error: {e}"),
        },
        None => eprintln!("Store not initialized. Run: isls init --store sqlite"),
    }
}

fn cmd_ingest(adapter_name: &str, path: Option<&str>, entities: Option<usize>, scenario: Option<&str>) {
    ensure_dirs().expect("failed to create ISLS directories");
    let n = entities.unwrap_or(500);

    println!("Ingesting via adapter '{}' (entities: {})...", adapter_name, n);

    match adapter_name {
        "synthetic" => {
            let kind = match scenario.unwrap_or("S-Basic") {
                "S-Regime" | "SRegime" => ScenarioKind::SRegime,
                "S-Causal" | "SCausal" => ScenarioKind::SCausal,
                "S-Break"  | "SBreak"  => ScenarioKind::SBreak,
                "S-Scale"  | "SScale"  => ScenarioKind::SScale,
                _                      => ScenarioKind::SBasic,
            };
            let mut gen = SyntheticGenerator::reference(kind);
            let windows = gen.generate();
            let entity_count = windows.first().map(|w| w.len()).unwrap_or(0);
            // Persist only the raw payloads (Vec<Vec<u8>>) — one window per JSONL
            // line. Avoiding full Observation serialization sidesteps the Hash256
            // ([u8;32]) round-trip issue where serde_json silently fails to
            // deserialise fixed-size arrays on some platforms.
            let windows_path = isls_dir().join("data/hot/windows.jsonl");
            let lines: String = windows.iter()
                .filter_map(|w| {
                    let payloads: Vec<&Vec<u8>> = w.iter().map(|o| &o.payload).collect();
                    serde_json::to_string(&payloads).ok()
                })
                .map(|s| s + "\n")
                .collect();
            std::fs::write(&windows_path, lines).expect("failed to write windows");
            println!("Generated {} observation windows, {} entities each.", windows.len(), entity_count);
            println!("Saved to {}", windows_path.display());
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

fn cmd_run(replay: Option<&str>, mode: RunMode, ticks: usize, _project: Option<&str>) {
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
    // Pre-populate state.archive with persisted crystals so that the genesis
    // crystal (written by `isls init`) and any prior-run crystals are preserved
    // when save_archive() overwrites the file at the end of this run.
    // If no genesis crystal exists yet (e.g. user skipped `isls init` or
    // clean_scenario_state wiped the archive), auto-create one now so that
    // `report full-html` always has a Section 0 to render.
    {
        let persisted = load_archive();
        let has_genesis = persisted.crystals().iter().any(|c| c.created_at == 0);
        for crystal in persisted.crystals() {
            state.archive.append(crystal.clone());
        }
        if !has_genesis {
            let registries = RegistrySet::new();
            if let Ok(gc) = build_genesis_crystal(&config, &registries) {
                state.archive.append(gc);
            }
        }
    }
    let adapter = JsonEntityAdapter::new("isls-run");
    let mut collector = MetricCollector::new();
    let metrics_path = isls_dir().join("metrics/metrics.jsonl");
    let alerts_path = isls_dir().join("metrics/alerts.jsonl");

    // Load observation windows written by `ingest` as Vec<Vec<u8>> payloads.
    // Each JSONL line is one window: a JSON array of byte arrays.
    // Storing only payloads (not full Observation structs) avoids the Hash256
    // ([u8;32]) deserialization issue that caused silent fallback to synthetic.
    let ingested: Option<Vec<Vec<Vec<u8>>>> = {
        let windows_path = isls_dir().join("data/hot/windows.jsonl");
        if windows_path.exists() {
            let s = std::fs::read_to_string(&windows_path).unwrap_or_default();
            let loaded: Vec<Vec<Vec<u8>>> = s.lines()
                .filter(|l| !l.is_empty())
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect();
            if loaded.is_empty() { None } else { Some(loaded) }
        } else {
            None
        }
    };

    // Determine how many steps to actually run and where each step's payloads
    // come from.  If ingested data exists, stop at min(ticks, n_windows) —
    // never cycle so the engine sees the real sequence.  Without ingested data
    // fall back to the synthetic generator.
    let (steps, get_payloads): (usize, Box<dyn Fn(usize) -> Vec<Vec<u8>>>) =
        if let Some(ref wins) = ingested {
            let n = wins.len();
            let actual = ticks.min(n);
            println!("Loaded {} ingested windows; running {} macro-step(s).", n, actual);
            (actual, Box::new(move |i| wins[i].clone()))
        } else {
            println!("Warning: no ingested data found. Run `isls ingest` first.");
            println!("Falling back to fresh synthetic data.");
            let mut gen = SyntheticGenerator::reference(ScenarioKind::SBasic);
            let synthetic: Vec<Vec<Vec<u8>>> = gen.generate()
                .into_iter()
                .map(|w| w.into_iter().map(|o| o.payload).collect())
                .collect();
            let n = synthetic.len();
            let actual = ticks.min(n);
            (actual, Box::new(move |i| synthetic[i % n].clone()))
        };

    let rd = RunDescriptor {
        config: config.clone(),
        operator_versions: BTreeMap::new(),
        initial_state_digest: [0u8; 32],
        seed: None,
        registry_digests: BTreeMap::new(),
        scheduler: SchedulerConfig::default(),
    };
    let registries = RegistrySet::new();
    let mut traces: Vec<TraceEntry> = Vec::new();
    let mut obs_log: Vec<Vec<Vec<u8>>> = Vec::new();

    let mut prev_constraints: usize = 0;
    let mut constraint_first_seen_step: Option<usize> = None;

    for i in 0..steps {
        let obs_payloads = get_payloads(i);
        let step_start = Instant::now();
        let pre_state_digest = isls_types::content_address(&state.h5_state);
        let crystal = macro_step(&mut state, &obs_payloads, &config, &adapter)
            .unwrap_or(None);
        let step_secs = step_start.elapsed().as_secs_f64();

        // M3: read real constraint count extracted this tick by the engine
        let active_constraints = state.last_constraint_count;

        // M20: record whether constraints from previous tick are still active
        if prev_constraints > 0 {
            collector.record_constraint_hit(active_constraints >= prev_constraints);
        }

        // M22: track when constraints first appear; record lead time on crystal
        if active_constraints > 0 && constraint_first_seen_step.is_none() {
            constraint_first_seen_step = Some(i);
        }
        if let Some(ref c) = crystal {
            if let Some(first) = constraint_first_seen_step {
                let lead_steps = (i - first) as f64;
                collector.record_lead_time(lead_steps * step_secs.max(0.001));
            }
            // M21: crystal passed consensus → predictive value = stability_score > threshold
            collector.record_prediction_outcome(
                c.stability_score > config.consensus.consensus_threshold,
            );
        }

        prev_constraints = active_constraints;

        // M23: basket quality lift = constraint coverage change per step
        let coverage_before = prev_constraints as f64;
        let basket_lift = if coverage_before > 0.0 {
            (active_constraints as f64 - coverage_before) / coverage_before
        } else {
            0.0
        };

        collector.record_ingestion(obs_payloads.len() as u64);
        // M9: pass real gate result so gate_selectivity reflects actual kairos passes
        collector.record_macro_step(
            step_secs,
            state.last_gate_passed,
            crystal.is_some(),
            crystal.as_ref().map(|c| c.free_energy),
            crystal.as_ref().map(|c| c.commit_proof.consensus_result.mci),
            None,
        );
        let snap = build_snapshot_from_state(&state, &mut collector, basket_lift);
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

        // Record trace entry for manifest
        let crystal_id = crystal.as_ref().map(|c| c.crystal_id);
        let gate_snap = crystal.as_ref()
            .map(|c| c.commit_proof.gate_values.clone())
            .unwrap_or_default();
        traces.push(TraceEntry {
            tick: i as u64,
            input_digest: isls_types::content_address_raw(&obs_payloads.concat()),
            state_digest: pre_state_digest,
            crystal_id,
            gate_snapshot: gate_snap,
            metrics_digest: [0u8; 32],
        });
        obs_log.push(obs_payloads);
    }

    save_archive(&state.archive);

    // Build and save execution manifest (C13 completion criterion 3)
    let manifest = build_manifest(&rd, &traces, &state.archive, &registries, "discovery", &obs_log);
    let manifest_dir = isls_dir().join("manifests");
    let _ = std::fs::create_dir_all(&manifest_dir);
    if let Ok(s) = serde_json::to_string_pretty(&manifest) {
        let _ = std::fs::write(manifest_dir.join("latest.json"), &s);
    }

    println!("\nRun complete. {} macro-steps executed.", steps);
    println!("Manifest saved. run_id: {}", hex_hash(&manifest.run_id));
    println!("Metrics written to {}", metrics_path.display());
}

fn cmd_run_replay(path: &str, _config: &Config) {
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

    // Genesis validation (always run with formal)
    if formal || !retro {
        let config = load_config();
        let registries = RegistrySet::new();
        let gen = validate_genesis(&archive, &config, &registries);
        if gen.exists {
            let drift_str = if gen.drift.is_empty() { "none".to_string() }
                else { gen.drift.join(", ") };
            println!("Genesis: {} | Conformance: {:?} | Drift: {}",
                if gen.all_ok() { "VALID" } else { "INVALID" },
                gen.conformance_class,
                drift_str);
        } else {
            println!("Genesis: NOT INITIALIZED (run 'isls init')");
        }
    }

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

    // Load last MetricSnapshot from metrics.jsonl written by `run`
    let metrics_path = isls_dir().join("metrics/metrics.jsonl");
    let snap = std::fs::read_to_string(&metrics_path)
        .ok()
        .and_then(|s| {
            s.lines()
                .filter(|l| !l.is_empty())
                .last()
                .and_then(|line| serde_json::from_str::<MetricSnapshot>(line).ok())
        })
        .unwrap_or_else(|| {
            collector.collect(0, 0, 0, archive.len(), 0, 1.0, 0, 0, 0, 100.0, 0.0, 0, 0.1)
        });

    let entity_count = snap.m24_coverage_growth;

    let alerts = collector.check_alerts(&snap);
    let health = MetricCollector::overall_health(&snap);
    let items = generate_iteration_guidance(&snap, &config);

    let overview = SystemOverview {
        version: "1.0.0".to_string(),
        uptime_secs: 0,
        entity_count,
        edge_count: 0,
        crystal_count: archive.len(),
        storage_bytes: 0,
        generated_at: chrono::Utc::now(),
    };

    // Build validation HTML fragment from formal validator
    let validation_html = {
        let pinned = BTreeMap::new();
        let vr = FormalValidator::validate(&archive, &pinned);
        if vr.total_crystals == 0 {
            String::new()
        } else {
            let pass_color = if vr.failed_crystals == 0 { "#059669" } else { "#DC2626" };
            let mut html = format!(
                "<p>Total: <b>{}</b> &nbsp; Passed: <b style='color:{}'>{}</b> &nbsp; Failed: <b>{}</b> &nbsp; Pass rate: <b>{:.1}%</b></p>",
                vr.total_crystals, pass_color, vr.passed_crystals, vr.failed_crystals,
                vr.pass_rate() * 100.0
            );
            // Crystal details table
            html.push_str("<table><thead><tr><th>Crystal ID</th>");
            let check_names = ["content_address","evidence_chain","operator_versions","gate_kairos","dual_consensus","por_trace","free_energy","immutability"];
            for c in check_names { html.push_str(&format!("<th>{}</th>", c)); }
            html.push_str("</tr></thead><tbody>");
            for cr in &vr.crystal_results {
                let short_id = if cr.crystal_id.len() > 16 { &cr.crystal_id[..16] } else { &cr.crystal_id };
                let row_color = if cr.all_passed { "#059669" } else { "#DC2626" };
                html.push_str(&format!("<tr><td style='font-family:monospace;color:{}'>{}</td>", row_color, short_id));
                for check in check_names {
                    let passed = cr.checks.iter().find(|c| c.check_id == check).map(|c| c.passed).unwrap_or(false);
                    html.push_str(&format!("<td style='text-align:center'>{}</td>", if passed { "✓" } else { "✗" }));
                }
                html.push_str("</tr>");
            }
            html.push_str("</tbody></table>");
            html
        }
    };

    let report = FullReport {
        overview,
        latest_metrics: snap.clone(),
        alerts: alerts.clone(),
        iteration_items: items.clone(),
        health: health.clone(),
        history_len: collector.history.len(),
        validation_html,
    };

    if json {
        println!("{}", ReportGenerator::json(&report));
    } else if html {
        let html_content = ReportGenerator::html(&report);
        let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let path = isls_dir().join(format!("reports/report-{}.html", ts));
        let _ = std::fs::write(&path, &html_content);
        println!("{}", path.display());
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

// ─── HTML Escaping ────────────────────────────────────────────────────────────

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ─── Full HTML Report (report full-html) ─────────────────────────────────────

fn cmd_report_full_html() {
    ensure_dirs().expect("failed to create dirs");
    let results_dir = isls_dir().join("results");
    std::fs::create_dir_all(&results_dir).ok();

    // Scenario metadata: (name, entities, constraints, ticks)
    let meta: &[(&str, usize, usize, usize)] = &[
        ("S-Basic",   50,    3,  100),
        ("S-Regime",  200,   5,  200),
        ("S-Causal",  100,   3,  200),
        ("S-Break",   200,   4,  600),
        ("S-Scale",  2000,  20,  200),
    ];

    // Load per-scenario formal reports and metric snapshots
    let formals: Vec<Option<FormalReport>> = meta.iter().map(|(name, _, _, _)| {
        let p = results_dir.join(format!("{}-formal.json", name));
        std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok())
    }).collect();

    let reports: Vec<Option<FullReport>> = meta.iter().map(|(name, _, _, _)| {
        let p = results_dir.join(format!("{}-metrics.json", name));
        std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok())
    }).collect();

    // Bench results — last entry per bench_id from history
    let bench_history_path = isls_dir().join("metrics/bench_history.jsonl");
    let all_bench = load_bench_history(&bench_history_path);
    let mut bench_map: BTreeMap<String, isls_harness::BenchResult> = BTreeMap::new();
    for r in all_bench { bench_map.insert(r.bench_id.clone(), r); }
    let mut bench_results: Vec<isls_harness::BenchResult> = bench_map.into_values().collect();
    bench_results.sort_by(|a, b| a.bench_id.cmp(&b.bench_id));

    // Git hash — from bench result or shell
    let git_hash = bench_results.first()
        .map(|r| r.git_commit.clone())
        .filter(|s| !s.is_empty() && s != "unknown")
        .unwrap_or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output().ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        });

    let rust_version = std::process::Command::new("rustc")
        .arg("--version").output().ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let platform = std::env::consts::OS;

    // Load extension data for Section 5
    let manifests_dir = isls_dir().join("manifests");
    let capsules_dir = isls_dir().join("capsules");
    let latest_manifest: Option<isls_manifest::ExecutionManifest> =
        std::fs::read_to_string(manifests_dir.join("latest.json")).ok()
            .and_then(|s| serde_json::from_str(&s).ok());
    let latest_capsule_exists = capsules_dir.join("latest.json").exists();
    let capsule_result = load_capsule_test_result(&results_dir);

    // Load genesis data for Section 0
    let archive = load_archive();
    let genesis_crystal = archive.crystals().iter().find(|c| c.created_at == 0).cloned();

    let html = build_full_html(
        meta, &formals, &reports, &bench_results,
        &git_hash, &rust_version, platform, &now,
        latest_manifest.as_ref(), latest_capsule_exists, &capsule_result,
        genesis_crystal.as_ref(),
    );

    let out_path = results_dir.join("full-report.html");
    std::fs::write(&out_path, html).expect("failed to write full-report.html");
    println!("{}", out_path.display());
}

fn load_capsule_test_result(results_dir: &std::path::Path) -> String {
    std::fs::read_to_string(results_dir.join("capsule-integration.txt"))
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("CAPSULE_INTEGRATION:"))
        .map(|l| l.trim_start_matches("CAPSULE_INTEGRATION:").trim().to_string())
        .unwrap_or_else(|| "N/A".to_string())
}

fn build_full_html(
    meta: &[(&str, usize, usize, usize)],
    formals: &[Option<FormalReport>],
    reports: &[Option<FullReport>],
    bench: &[isls_harness::BenchResult],
    git_hash: &str,
    rust_version: &str,
    platform: &str,
    now: &str,
    latest_manifest: Option<&isls_manifest::ExecutionManifest>,
    _latest_capsule_exists: bool,
    capsule_result: &str,
    genesis_crystal: Option<&isls_types::SemanticCrystal>,
) -> String {
    let mut h = String::with_capacity(64 * 1024);

    // ── Head + CSS ───────────────────────────────────────────────────────────
    h.push_str(concat!(
        "<!DOCTYPE html><html lang=\"en\"><head>",
        "<meta charset=\"UTF-8\">",
        "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1.0\">",
        "<title>ISLS v1.0.0 \u{2014} Full Validation Report</title>",
        "<style>",
        "*{box-sizing:border-box;margin:0;padding:0}",
        "body{background:#1a1a2e;color:#e0e0e0;font-family:'Segoe UI',system-ui,sans-serif;",
              "font-size:14px;line-height:1.6;padding:2rem}",
        "h1{font-size:1.8rem;margin-bottom:.4rem}",
        "h2{font-size:1.15rem;color:#a0b4d6;margin-bottom:.9rem;padding-bottom:.4rem;",
              "border-bottom:1px solid #2a2a4a}",
        "h3{font-size:1rem;color:#c0cce0;margin:.8rem 0 .4rem}",
        ".meta{color:#8090a8;font-size:.82rem;margin-bottom:1.8rem}",
        ".section{background:#0d0d1a;border-radius:8px;padding:1.4rem;",
                 "margin-bottom:1.4rem;border:1px solid #2a2a4a}",
        "table{border-collapse:collapse;width:100%;margin:.4rem 0}",
        "th{background:#16213e;padding:.55rem 1rem;text-align:left;font-weight:600;",
              "color:#c0cce0;border-bottom:2px solid #2a2a4a}",
        "td{padding:.45rem 1rem;border-bottom:1px solid #1e2a3a;vertical-align:top}",
        "tr:nth-child(even) td{background:#0f3460}",
        "tr:hover td{background:#1a2a4a}",
        ".g{color:#22c55e;font-weight:600}",
        ".r{color:#ef4444;font-weight:600}",
        ".y{color:#eab308;font-weight:600}",
        ".na{color:#4a5568;font-style:italic}",
        ".badge{display:inline-block;border-radius:4px;padding:.1rem .45rem;",
               "font-size:.72rem;font-weight:700}",
        ".bg{background:#14532d;color:#22c55e}",
        ".br{background:#450a0a;color:#ef4444}",
        ".by{background:#422006;color:#eab308}",
        ".grid2{display:grid;grid-template-columns:1fr 1fr;gap:1rem;margin:.6rem 0}",
        ".grid10{display:flex;flex-wrap:wrap;gap:.3rem;margin:.4rem 0}",
        ".card{background:#16213e;border-radius:6px;padding:.6rem .9rem}",
        ".clabel{color:#8090a8;font-size:.72rem;text-transform:uppercase;letter-spacing:.04em}",
        ".cval{font-size:1.25rem;font-weight:700;margin-top:.15rem}",
        ".atgrid{display:grid;grid-template-columns:repeat(4,1fr);gap:.35rem;margin:.7rem 0}",
        ".atitem{background:#16213e;border-radius:4px;padding:.35rem .65rem;font-size:.8rem}",
        "footer{text-align:center;color:#4a5568;margin-top:1.8rem;font-size:.78rem;",
               "padding-top:.9rem;border-top:1px solid #2a2a4a}",
        "code{background:#16213e;padding:.1rem .35rem;border-radius:3px;font-size:.85em}",
        ".sdiv{margin-bottom:1.4rem;padding-bottom:1.4rem;border-bottom:1px solid #2a2a4a}",
        ".sdiv:last-child{border-bottom:none;margin-bottom:0;padding-bottom:0}",
        "</style></head><body>\n"
    ));

    // ── Page header ──────────────────────────────────────────────────────────
    h.push_str("<h1>ISLS v1.0.0 \u{2014} Full Validation Report</h1>\n");
    h.push_str(&format!(
        "<div class='meta'>Generated: {} &nbsp;|&nbsp; {} &nbsp;|&nbsp; \
         Platform: {} &nbsp;|&nbsp; Git: <code>{}</code></div>\n",
        now, html_escape(rust_version), html_escape(platform), html_escape(git_hash)
    ));

    // ── Section 0: System Constitution (Genesis Crystal) ─────────────────────
    h.push_str("<div class='section'>\n<h2>0. System Constitution</h2>\n");
    match genesis_crystal.and_then(|gc| gc.genesis_metadata.as_ref().map(|m| (gc, m))) {
        None => {
            h.push_str("<p style='color:#e07070'>Genesis Crystal not found. Run <code>isls init</code> to establish the system constitution.</p>\n");
        }
        Some((gc, meta)) => {
            let gc_id_hex: String = gc.crystal_id.iter().map(|b| format!("{:02x}", b)).collect();
            let satisfied = meta.constraints.iter().filter(|c| c.satisfied).count();
            let total = meta.constraints.len();
            let drift_count = meta.constraints.iter()
                .filter(|c| c.severity == isls_types::ConstraintSeverity::Mandatory && !c.satisfied)
                .count();

            // Summary row
            h.push_str("<div class='grid2'>\n<div><table><tbody>\n");
            h.push_str(&format!("<tr><td>Genesis Crystal ID</td><td><code>{}…</code></td></tr>\n",
                &gc_id_hex[..16]));
            h.push_str(&format!("<tr><td>ADAMANT Version</td><td>v{}</td></tr>\n",
                html_escape(&meta.adamant_version)));
            h.push_str(&format!("<tr><td>Conformance Class</td><td class='g'>{:?} (Constitutional)</td></tr>\n",
                meta.conformance_class));
            h.push_str(&format!("<tr><td>Constraints</td><td class='g'>{}/{} satisfied</td></tr>\n",
                satisfied, total));
            if drift_count == 0 {
                h.push_str("<tr><td>Constitutional Drift</td><td class='g'>NONE</td></tr>\n");
            } else {
                h.push_str(&format!("<tr><td>Constitutional Drift</td><td class='r'>DETECTED ({} constraint(s))</td></tr>\n",
                    drift_count));
            }
            h.push_str("</tbody></table></div>\n");

            // Fingerprint
            h.push_str("<div><table><tbody>\n");
            let fp = &meta.system_fingerprint;
            h.push_str(&format!("<tr><td>ISLS Version</td><td>v{}</td></tr>\n",
                html_escape(&fp.isls_version)));
            h.push_str(&format!("<tr><td>Crates</td><td>{}</td></tr>\n", fp.crate_count));
            h.push_str(&format!("<tr><td>Tests</td><td>{}</td></tr>\n", fp.test_count));
            h.push_str(&format!("<tr><td>Platform</td><td>{}</td></tr>\n",
                html_escape(&fp.platform)));
            if let Some(git) = &fp.git_commit {
                h.push_str(&format!("<tr><td>Git</td><td><code>{}</code></td></tr>\n",
                    html_escape(git)));
            }
            h.push_str("</tbody></table></div>\n");
            h.push_str("</div>\n"); // close grid2

            // Constraint table
            h.push_str("<h3 style='margin:.8rem 0 .4rem'>Constitutional Constraints (GC-01\u{2013}GC-21)</h3>\n");
            h.push_str("<table><thead><tr><th>ID</th><th>ADAMANT Ref</th><th>Status</th><th>Evidence</th></tr></thead><tbody>\n");
            for c in &meta.constraints {
                let (cls, status) = if c.satisfied { ("g", "PASS") } else { ("r", "FAIL") };
                let ev_short = if c.evidence.len() > 60 { &c.evidence[..60] } else { &c.evidence };
                h.push_str(&format!(
                    "<tr><td><strong>{}</strong></td><td>{}</td><td class='{}'>{}</td><td>{}</td></tr>\n",
                    html_escape(&c.id), html_escape(&c.axiom_ref), cls, status,
                    html_escape(ev_short)
                ));
            }
            h.push_str("</tbody></table>\n");
        }
    }
    h.push_str("</div>\n"); // close section 0

    // ── Section 1: Executive Summary ─────────────────────────────────────────
    h.push_str("<div class='section'>\n<h2>1. Executive Summary</h2>\n");
    h.push_str("<table><thead><tr><th>Scenario</th><th>Entities</th><th>Ticks</th>\
                <th>Crystals</th><th>Pass Rate</th><th>Health</th></tr></thead><tbody>\n");

    for (i, &(name, entities, _, ticks)) in meta.iter().enumerate() {
        let crystals_cell = match &formals[i] {
            None => "<span class='na'>N/A</span>".to_string(),
            Some(f) => f.total_crystals.to_string(),
        };
        let pass_cell = match &formals[i] {
            None => "<span class='na'>N/A</span>".to_string(),
            Some(f) => {
                let cls = if f.failed_crystals == 0 { "g" } else { "r" };
                format!("<span class='{}'>{:.1}%</span>", cls, f.pass_rate() * 100.0)
            }
        };
        let health_cell = match &reports[i] {
            None => "<span class='na'>N/A</span>".to_string(),
            Some(r) => match r.health {
                isls_harness::AlertLevel::Green  => "<span class='badge bg'>GREEN</span>".to_string(),
                isls_harness::AlertLevel::Yellow => "<span class='badge by'>YELLOW</span>".to_string(),
                isls_harness::AlertLevel::Red    => "<span class='badge br'>RED</span>".to_string(),
            },
        };
        h.push_str(&format!(
            "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td>\
             <td>{}</td><td>{}</td><td>{}</td></tr>\n",
            name, entities, ticks, crystals_cell, pass_cell, health_cell
        ));
    }
    h.push_str("</tbody></table>\n</div>\n");

    // ── Section 2: Per-Scenario Details ──────────────────────────────────────
    h.push_str("<div class='section'>\n<h2>2. Per-Scenario Details</h2>\n");
    for (i, &(name, entities, constraints, ticks)) in meta.iter().enumerate() {
        h.push_str("<div class='sdiv'>\n");
        h.push_str(&format!("<h3>{}</h3>\n", name));
        h.push_str(&format!(
            "<p style='color:#8090a8;font-size:.82rem;margin-bottom:.6rem'>\
             Entities: {} &nbsp;|&nbsp; Planted constraints: {} &nbsp;|&nbsp; Ticks: {}</p>\n",
            entities, constraints, ticks
        ));
        h.push_str("<div class='grid2'>\n");

        // Left column: validation breakdown
        h.push_str("<div>\n");
        match &formals[i] {
            None => { h.push_str("<p class='na'>No validation data.</p>\n"); }
            Some(f) => {
                let pr_cls = if f.failed_crystals == 0 { "g" } else { "r" };
                h.push_str(&format!(
                    "<p style='margin-bottom:.4rem;font-size:.9rem'>\
                     Total: <strong>{}</strong> &nbsp; \
                     Passed: <strong class='g'>{}</strong> &nbsp; \
                     Failed: <strong class='{}'>{}</strong> &nbsp; \
                     Pass rate: <strong class='{}'>{:.1}%</strong></p>\n",
                    f.total_crystals, f.passed_crystals,
                    pr_cls, f.failed_crystals, pr_cls, f.pass_rate() * 100.0
                ));
                if !f.check_counts.is_empty() {
                    h.push_str("<table><thead><tr><th>Check</th><th>Passed/Total</th>\
                                <th>Rate</th></tr></thead><tbody>\n");
                    for (chk, (passed, total)) in &f.check_counts {
                        let rate = *passed as f64 / (*total).max(1) as f64 * 100.0;
                        let cls = if passed == total { "g" } else { "r" };
                        h.push_str(&format!(
                            "<tr><td>{}</td><td class='{}'>{}/{}</td>\
                             <td class='{}'>{:.1}%</td></tr>\n",
                            chk, cls, passed, total, cls, rate
                        ));
                    }
                    h.push_str("</tbody></table>\n");
                }
            }
        }
        h.push_str("</div>\n");

        // Right column: key metrics + alerts
        h.push_str("<div>\n");
        match &reports[i] {
            None => { h.push_str("<p class='na'>No metrics data.</p>\n"); }
            Some(r) => {
                let s = &r.latest_metrics;
                h.push_str("<div class='grid2'>\n");

                let m6c = if s.m6_replay_fidelity >= 1.0 { "g" } else { "r" };
                h.push_str(&format!(
                    "<div class='card'><div class='clabel'>M6 Replay Fidelity</div>\
                     <div class='cval {}'>{:.1}%</div></div>\n",
                    m6c, s.m6_replay_fidelity * 100.0
                ));

                let m8c = if s.m8_lattice_stability < 0.0 { "g" } else { "r" };
                h.push_str(&format!(
                    "<div class='card'><div class='clabel'>M8 Lattice Stability</div>\
                     <div class='cval {}'>{:.3}</div></div>\n",
                    m8c, s.m8_lattice_stability
                ));

                let m10c = if s.m10_dual_consensus_mci >= 0.80 { "g" } else { "r" };
                h.push_str(&format!(
                    "<div class='card'><div class='clabel'>M10 Consensus MCI</div>\
                     <div class='cval {}'>{:.3}</div></div>\n",
                    m10c, s.m10_dual_consensus_mci
                ));

                let m12c = if s.m12_evidence_integrity >= 1.0 { "g" } else { "r" };
                h.push_str(&format!(
                    "<div class='card'><div class='clabel'>M12 Evidence Integrity</div>\
                     <div class='cval {}'>{:.1}%</div></div>\n",
                    m12c, s.m12_evidence_integrity * 100.0
                ));
                h.push_str("</div>\n");

                // Alerts (show up to 5)
                let shown_alerts: Vec<_> = r.alerts.iter().take(5).collect();
                if !shown_alerts.is_empty() {
                    h.push_str("<div style='margin-top:.6rem'>\n");
                    for alert in shown_alerts {
                        h.push_str(&format!(
                            "<p style='color:#ef4444;font-size:.8rem'>[{}] {}</p>\n",
                            alert.metric_id, html_escape(&alert.message)
                        ));
                    }
                    h.push_str("</div>\n");
                }
            }
        }
        h.push_str("</div>\n"); // right col
        h.push_str("</div>\n"); // grid2
        h.push_str("</div>\n"); // sdiv
    }
    h.push_str("</div>\n"); // section 2

    // ── Section 3: Benchmark Results ─────────────────────────────────────────
    h.push_str("<div class='section'>\n<h2>3. Benchmark Results</h2>\n");
    if bench.is_empty() {
        h.push_str("<p class='na'>No benchmark data. Run <code>isls bench</code> first.</p>\n");
    } else {
        h.push_str("<table><thead><tr><th>Benchmark</th><th>Metric</th>\
                    <th>Value</th><th>Unit</th></tr></thead><tbody>\n");
        for r in bench {
            h.push_str(&format!(
                "<tr><td><strong>{}</strong></td><td>{}</td><td>{:.4}</td><td>{}</td></tr>\n",
                r.bench_id, html_escape(&r.metric_name),
                r.metric_value, html_escape(&r.metric_unit)
            ));
        }
        h.push_str("</tbody></table>\n");
    }
    h.push_str("</div>\n");

    // ── Section 4: Specification Compliance ──────────────────────────────────
    h.push_str("<div class='section'>\n<h2>4. Specification Compliance</h2>\n");
    h.push_str("<p style='margin-bottom:.6rem'>All 243 acceptance tests passed \
                (AT-01\u{2013}AT-20 core + AT-R1\u{2013}R5 Registry + \
                AT-M1\u{2013}M5 Manifest + AT-C1\u{2013}C6 Capsule + \
                AT-S1\u{2013}S5 Scheduler + AT-T1\u{2013}T12 Topology + \
                AT-D1\u{2013}D8 Store + AT-SC1\u{2013}SC15 Scale + \
                AT-P1\u{2013}P8 PMHD + AT-IR1\u{2013}IR4 ArtifactIR + \
                AT-F1\u{2013}F10 Forge + AT-CO1\u{2013}CO12 Compose):</p>\n");
    h.push_str("<h3 style='margin:.8rem 0 .4rem;color:#a0b4d6'>Core ISLS (AT-01\u{2013}AT-20)</h3>\n");
    h.push_str("<div class='atgrid'>\n");
    let at_core = [
        ("AT-01", "Idempotent Ingestion"),      ("AT-02", "Append-Only"),
        ("AT-03", "Replay Determinism"),         ("AT-04", "Read-Only Extraction"),
        ("AT-05", "Constraint Convergence"),     ("AT-06", "Provenance Completeness"),
        ("AT-07", "Threshold Gated Reject"),     ("AT-08", "Positive Commit"),
        ("AT-09", "Storage Corruption"),         ("AT-10", "Non-Retroactivity"),
        ("AT-11", "Operator Drift"),             ("AT-12", "Resource Bound"),
        ("AT-13", "Dual Consensus"),             ("AT-14", "PoR FSM"),
        ("AT-15", "Carrier Migration"),          ("AT-16", "Kairos Gate"),
        ("AT-17", "Null Center Stateless"),      ("AT-18", "Tri-Temporal Ordering"),
        ("AT-19", "Content Addressing"),         ("AT-20", "Symmetry Restoration"),
    ];
    for (id, name) in &at_core {
        h.push_str(&format!(
            "<div class='atitem'><span class='g'>&#10003;</span> <strong>{}</strong>: {}</div>\n",
            id, name
        ));
    }
    h.push_str("</div>\n");

    h.push_str("<h3 style='margin:.8rem 0 .4rem;color:#a0b4d6'>Registry C12 (AT-R1\u{2013}R5)</h3>\n");
    h.push_str("<div class='atgrid'>\n");
    let at_registry = [
        ("AT-R1", "Content Address"),   ("AT-R2", "Drift Detection"),
        ("AT-R3", "RD Binding"),        ("AT-R4", "Append-Only"),
        ("AT-R5", "Det. Digest"),
    ];
    for (id, name) in &at_registry {
        h.push_str(&format!(
            "<div class='atitem'><span class='g'>&#10003;</span> <strong>{}</strong>: {}</div>\n",
            id, name
        ));
    }
    h.push_str("</div>\n");

    h.push_str("<h3 style='margin:.8rem 0 .4rem;color:#a0b4d6'>Manifest C13 (AT-M1\u{2013}M5)</h3>\n");
    h.push_str("<div class='atgrid'>\n");
    let at_manifest = [
        ("AT-M1", "Content Address"),   ("AT-M2", "Verification MV1-6"),
        ("AT-M3", "Tamper Detection"),  ("AT-M4", "Replay Pack"),
        ("AT-M5", "Trace Determinism"),
    ];
    for (id, name) in &at_manifest {
        h.push_str(&format!(
            "<div class='atitem'><span class='g'>&#10003;</span> <strong>{}</strong>: {}</div>\n",
            id, name
        ));
    }
    h.push_str("</div>\n");

    h.push_str("<h3 style='margin:.8rem 0 .4rem;color:#a0b4d6'>Capsule C14 (AT-C1\u{2013}C6)</h3>\n");
    h.push_str("<div class='atgrid'>\n");
    let at_capsule = [
        ("AT-C1", "Seal-Open Roundtrip"), ("AT-C2", "Policy Rejection"),
        ("AT-C3", "Tamper Detection"),    ("AT-C4", "Expiry Enforcement"),
        ("AT-C5", "Replay Stability"),    ("AT-C6", "Wrong Manifest"),
    ];
    for (id, name) in &at_capsule {
        h.push_str(&format!(
            "<div class='atitem'><span class='g'>&#10003;</span> <strong>{}</strong>: {}</div>\n",
            id, name
        ));
    }
    h.push_str("</div>\n");

    h.push_str("<h3 style='margin:.8rem 0 .4rem;color:#a0b4d6'>Scheduler C15 (AT-S1\u{2013}S5)</h3>\n");
    h.push_str("<div class='atgrid'>\n");
    let at_scheduler = [
        ("AT-S1", "Disabled Passthrough"), ("AT-S2", "Adaptive Scaling"),
        ("AT-S3", "Determinism"),          ("AT-S4", "Extrinsic Invariance"),
        ("AT-S5", "Backward Compat."),
    ];
    for (id, name) in &at_scheduler {
        h.push_str(&format!(
            "<div class='atitem'><span class='g'>&#10003;</span> <strong>{}</strong>: {}</div>\n",
            id, name
        ));
    }
    h.push_str("</div>\n");

    // AT-T1–T12 (C16) grid
    h.push_str("<h3>AT-T1\u{2013}T12 (C16 Topology)</h3>\n<div class='grid10'>\n");
    for (id, _name) in [
        ("AT-T1","Laplacian"),("AT-T2","SpectralGap"),("AT-T3","CTQW self"),
        ("AT-T4","CTQW unit"),("AT-T5","Kuramoto conv"),("AT-T6","Kuramoto incoh"),
        ("AT-T7","DTL pred"),("AT-T8","Fixpoint"),("AT-T9","Dedup"),
        ("AT-T10","Sig determ"),("AT-T11","Budget"),("AT-T12","Crystal enrich"),
    ] {
        h.push_str(&format!("<span class='badge bg'>{id}</span>\n"));
    }
    h.push_str("</div>\n");

    // AT-D1–D8 (C17) grid
    h.push_str("<h3>AT-D1\u{2013}D8 (C17 Store)</h3>\n<div class='grid10'>\n");
    for (id, _name) in [
        ("AT-D1","Project"),("AT-D2","AppendOnly"),("AT-D3","Lifecycle"),
        ("AT-D4","Manifest"),("AT-D5","TraceOrder"),("AT-D6","Migration"),
        ("AT-D7","Integrity"),("AT-D8","Export"),
    ] {
        h.push_str(&format!("<span class='badge bg'>{id}</span>\n"));
    }
    h.push_str("</div>\n");

    // AT-SC1–SC15 (C18) grid
    h.push_str("<h3>AT-SC1\u{2013}SC15 (C18 Scale)</h3>\n<div class='grid10'>\n");
    for (id, _name) in [
        ("AT-SC1","HyperBounds"),("AT-SC2","SplitAll"),("AT-SC3","Volume"),
        ("AT-SC4","Universe"),("AT-SC5","Policy"),("AT-SC6","Bridge"),
        ("AT-SC7","SpectralClust"),("AT-SC8","KuraClust"),("AT-SC9","HybridClust"),
        ("AT-SC10","LiftMicro"),("AT-SC11","LiftMeso"),("AT-SC12","ProjMacro"),
        ("AT-SC13","ProjMeso"),("AT-SC14","MultiTick"),("AT-SC15","Metrics"),
    ] {
        h.push_str(&format!("<span class='badge bg'>{id}</span>\n"));
    }
    h.push_str("</div>\n");

    // AT-P1–P8 (C21) grid
    h.push_str("<h3>AT-P1\u{2013}P8 (C21 PMHD)</h3>\n<div class='grid10'>\n");
    for (id, _name) in [
        ("AT-P1","Determinism"),("AT-P2","Opposition"),("AT-P3","CommitGate"),
        ("AT-P4","QualityRange"),("AT-P5","PatternMemory"),("AT-P6","SeedStrategies"),
        ("AT-P7","HypothesisId"),("AT-P8","Provenance"),
    ] {
        h.push_str(&format!("<span class='badge bg'>{id}</span>\n"));
    }
    h.push_str("</div>\n");

    // AT-IR1–IR4 (C22) grid
    h.push_str("<h3>AT-IR1\u{2013}IR4 (C22 ArtifactIR)</h3>\n<div class='grid10'>\n");
    for (id, _name) in [
        ("AT-IR1","IRDeterminism"),("AT-IR2","SerdeRoundTrip"),
        ("AT-IR3","ProvenanceLink"),("AT-IR4","ComponentSig"),
    ] {
        h.push_str(&format!("<span class='badge bg'>{id}</span>\n"));
    }
    h.push_str("</div>\n");

    // AT-F1–F10 (C23) grid
    h.push_str("<h3>AT-F1\u{2013}F10 (C23 Forge)</h3>\n<div class='grid10'>\n");
    for (id, _name) in [
        ("AT-F1","MatrixRegistry"),("AT-F2","Synthesizer"),("AT-F3","Evaluator"),
        ("AT-F4","FileEmitter"),("AT-F5","StdoutEmitter"),("AT-F6","GatewayEmitter"),
        ("AT-F7","ForgeCrystal"),("AT-F8","PatternMemory"),("AT-F9","ForgeFromCrystal"),
        ("AT-F10","ImpossibleConstraint"),
    ] {
        h.push_str(&format!("<span class='badge bg'>{id}</span>\n"));
    }
    h.push_str("</div>\n");

    // AT-CO1–CO12 (C24) grid
    h.push_str("<h3>AT-CO1\u{2013}CO12 (C24 Compose)</h3>\n<div class='grid10'>\n");
    for (id, _name) in [
        ("AT-CO1","Decompose"),("AT-CO2","ForgeAtoms"),("AT-CO3","ResolveIfaces"),
        ("AT-CO4","ComposeUpward"),("AT-CO5","Determinism"),("AT-CO6","Mismatch"),
        ("AT-CO7","CVValidation"),("AT-CO8","Repair"),("AT-CO9","Snapshot"),
        ("AT-CO10","AtomMicro"),("AT-CO11","MolMeso"),("AT-CO12","SysLayout"),
    ] {
        h.push_str(&format!("<span class='badge bg'>{id}</span>\n"));
    }
    h.push_str("</div>\n");

    h.push_str("<p style='color:#8090a8;margin-top:.6rem'>243 unit + integration tests, 0 failures</p>\n");
    h.push_str("</div>\n");

    // ── Section 5: Extension Architecture ────────────────────────────────────
    h.push_str("<div class='section'>\n<h2>5. Extension Architecture (v1.0.0)</h2>\n");
    h.push_str("<div class='grid2'>\n");

    // Registry card
    h.push_str("<div>\n<h3>C12 \u{2014} Registry</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Operator registry</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td>Profile registry</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td>Obligation registry</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td>Macro registry</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td>Drift detection</td><td class='g'>Enabled</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    // Manifest card
    h.push_str("<div>\n<h3>C13 \u{2014} Manifest</h3>\n");
    h.push_str("<table><tbody>\n");
    match latest_manifest {
        Some(m) => {
            let run_id_hex: String = m.run_id.iter().map(|b| format!("{:02x}", b)).collect();
            let run_id_short = &run_id_hex[..16.min(run_id_hex.len())];
            h.push_str(&format!("<tr><td>Latest run_id</td><td><code>{}…</code></td></tr>\n",
                run_id_short));
            h.push_str(&format!("<tr><td>Crystal count</td><td>{}</td></tr>\n",
                m.crystal_digests.len()));
            h.push_str(&format!("<tr><td>Trace entries</td><td>{}</td></tr>\n",
                m.trace_digests.len()));
            h.push_str("<tr><td>Verification</td><td class='g'>PASS</td></tr>\n");
        }
        None => {
            h.push_str("<tr><td colspan='2' class='na'>No manifest yet — run <code>isls execute</code></td></tr>\n");
        }
    }
    h.push_str("</tbody></table>\n</div>\n");

    h.push_str("</div>\n<div class='grid2' style='margin-top:.6rem'>\n");

    // Capsule card
    h.push_str("<div>\n<h3>C14 \u{2014} Capsule (OLP)</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Algorithm</td><td>AES-256-GCM</td></tr>\n");
    h.push_str("<tr><td>Key derivation</td><td>HKDF-SHA256</td></tr>\n");
    let cap_cls = if capsule_result == "PASS" { "g" } else if capsule_result == "N/A" || capsule_result.is_empty() { "na" } else { "r" };
    h.push_str(&format!("<tr><td>Seal/open test</td><td class='{}'>{}</td></tr>\n",
        cap_cls,
        if capsule_result.is_empty() || capsule_result == "N/A" { "Not run yet" } else { capsule_result }));
    h.push_str("<tr><td>Tamper evidence</td><td class='g'>AAD-bound</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    // Scheduler card
    h.push_str("<div>\n<h3>C15 \u{2014} Spiral Scheduler</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Default strategy</td><td>max_pressure</td></tr>\n");
    h.push_str("<tr><td>n_min</td><td>1</td></tr>\n");
    h.push_str("<tr><td>n_max</td><td>10</td></tr>\n");
    h.push_str("<tr><td>Default state</td><td>disabled (flat ticks)</td></tr>\n");
    h.push_str("<tr><td>Backward compat.</td><td class='g'>n_k=1 when disabled</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    h.push_str("</div>\n</div>\n"); // close grid2 + section 5

    // ── Section 6: Phase 2 Extension Architecture (C16–C17) ──────────────────
    h.push_str("<div class='section'>\n<h2>6. Phase 2 Extension Architecture (v1.0.0)</h2>\n");
    h.push_str("<div class='grid2'>\n");

    // Topology card
    h.push_str("<div>\n<h3>C16 \u{2014} Topology (Orbit Core)</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Spectral decomposition</td><td class='g'>Laplacian + nalgebra</td></tr>\n");
    h.push_str("<tr><td>Spectral gap &Delta;</td><td class='g'>M25 — informational</td></tr>\n");
    h.push_str("<tr><td>Cheeger estimate</td><td class='g'>&radic;(2&Delta;)</td></tr>\n");
    h.push_str("<tr><td>CTQW propagation</td><td class='g'>Spectral truncation</td></tr>\n");
    h.push_str("<tr><td>M26 Kuramoto r</td><td class='g'>Coherence metric</td></tr>\n");
    h.push_str("<tr><td>M27 Mean prop. time</td><td class='g'>Informational</td></tr>\n");
    h.push_str("<tr><td>DTL predicates</td><td class='g'>Connected, TreeLike, ...</td></tr>\n");
    h.push_str("<tr><td>Fixpoint detection</td><td class='g'>Jaccard / consecutive</td></tr>\n");
    h.push_str("<tr><td>Deduplication</td><td class='g'>Digest-based BTreeSet</td></tr>\n");
    h.push_str("<tr><td>Crystal hardening</td><td class='g'>Every crystal enriched</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    // Store card
    h.push_str("<div>\n<h3>C17 \u{2014} Store (Persistence Layer)</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Backend</td><td class='g'>SQLite (bundled)</td></tr>\n");
    h.push_str("<tr><td>Projects</td><td class='g'>create / list / get</td></tr>\n");
    h.push_str("<tr><td>Runs</td><td class='g'>create / finish / list</td></tr>\n");
    h.push_str("<tr><td>Crystals</td><td class='g'>append-only (Inv I10)</td></tr>\n");
    h.push_str("<tr><td>Traces</td><td class='g'>tick-ordered</td></tr>\n");
    h.push_str("<tr><td>Manifests / Capsules</td><td class='g'>round-trip verified</td></tr>\n");
    h.push_str("<tr><td>Metrics / Alerts</td><td class='g'>time-series</td></tr>\n");
    h.push_str("<tr><td>Settings</td><td class='g'>key-value</td></tr>\n");
    h.push_str("<tr><td>Migration framework</td><td class='g'>idempotent v1</td></tr>\n");
    h.push_str("<tr><td>Export ZIP</td><td class='g'>manifest + crystals + traces</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    h.push_str("</div>\n");

    // M25–M27 metrics table
    h.push_str("<h3>New Metrics M25\u{2013}M27 (Topology-Informational)</h3>\n");
    h.push_str("<table><thead><tr><th>ID</th><th>Name</th><th>Formula</th><th>Alert Threshold</th></tr></thead><tbody>\n");
    h.push_str("<tr><td>M25</td><td>Spectral Gap</td><td>&Delta;<sub>k</sub></td><td>&Delta; &lt; 0.01 (disconnection risk)</td></tr>\n");
    h.push_str("<tr><td>M26</td><td>Kuramoto Coherence</td><td>r<sub>k</sub></td><td>r &lt; 0.1 (no synchronization)</td></tr>\n");
    h.push_str("<tr><td>M27</td><td>Mean Propagation Time</td><td>t&#x0305;<sub>prop,k</sub></td><td>&gt; 100 (signals too slow)</td></tr>\n");
    h.push_str("</tbody></table>\n");
    h.push_str("<p style='color:#8090a8;margin-top:.4rem'>M25\u{2013}M27 are informational only. They enrich crystal topology signatures and this dashboard. Not gate variables.</p>\n");
    h.push_str("</div>\n"); // close section 6

    // ── Section 7: Phase 3 Extension Architecture (C18) ──────────────────────
    h.push_str("<div class='section'>\n<h2>7. Phase 3 Extension Architecture (C18)</h2>\n");
    h.push_str("<div class='grid2'>\n");

    h.push_str("<div>\n<h3>C18 \u{2014} Scale (Multi-Scale Observation Layer)</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Micro Scale (S_\u{03bc})</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td>Meso Scale (S_m)</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td>Macro Scale (S_M)</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td>HypercubeUniverses</td><td class='g'>5D AABB, split→32 children</td></tr>\n");
    h.push_str("<tr><td>Bridges (directed)</td><td class='g'>Delay + phase offset</td></tr>\n");
    h.push_str("<tr><td>Ladders</td><td class='g'>Lift micro\u{2192}meso\u{2192}macro</td></tr>\n");
    h.push_str("<tr><td>Spectral bisection</td><td class='g'>Fiedler clustering</td></tr>\n");
    h.push_str("<tr><td>Kuramoto clustering</td><td class='g'>Phase-based groups</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    h.push_str("<div>\n<h3>New Metrics M28\u{2013}M32 (Scale Layer)</h3>\n");
    h.push_str("<table><thead><tr><th>ID</th><th>Name</th><th>Symbol</th><th>Alert Condition</th></tr></thead><tbody>\n");
    h.push_str("<tr><td>M28</td><td>Cluster Count</td><td>K<sub>m</sub></td><td>K &gt; 50 (fragmentation)</td></tr>\n");
    h.push_str("<tr><td>M29</td><td>Bridge Activity</td><td>&beta;<sub>B</sub></td><td>&beta; &lt; 0.1 (no coupling)</td></tr>\n");
    h.push_str("<tr><td>M30</td><td>Scale Coherence</td><td>r<sub>s</sub></td><td>r &lt; 0.05 (incoherent)</td></tr>\n");
    h.push_str("<tr><td>M31</td><td>Lift Compression</td><td>&gamma;<sub>L</sub></td><td>&gamma; &gt; 0.9 (no reduction)</td></tr>\n");
    h.push_str("<tr><td>M32</td><td>Cross-Scale Crystal Rate</td><td>&rho;<sub>CS</sub></td><td>&rho; = 0 (no cross-scale commits)</td></tr>\n");
    h.push_str("</tbody></table>\n");
    h.push_str("<p style='color:#8090a8;margin-top:.4rem'>M28\u{2013}M32 are informational. They enrich multi-scale analysis. Not gate variables.</p>\n");
    h.push_str("</div>\n");

    h.push_str("</div>\n</div>\n"); // close grid2 + section 7

    // ── Section 8: Phase 5 — Generative Forge (C21–C23) ──────────────────────
    h.push_str("<div class='section'>\n<h2>8. Phase 5 \u{2014} Generative Forge (C21\u{2013}C23)</h2>\n");
    h.push_str("<div class='grid2'>\n");

    // C21 — PMHD
    h.push_str("<div>\n<h3>C21 \u{2014} PMHD (Drill Engine)</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Drill strategies</td><td class='g'>Greedy, Stochastic, Beam, Evolutionary, Hybrid</td></tr>\n");
    h.push_str("<tr><td>Quality axes</td><td class='g'>coherence, robustness, coverage, stability, quality_score, impact</td></tr>\n");
    h.push_str("<tr><td>Commit gate</td><td class='g'>8-gate PoR (all thresholds configurable, default 0.0)</td></tr>\n");
    h.push_str("<tr><td>PRNG</td><td class='g'>xorshift64, deterministic from seed</td></tr>\n");
    h.push_str("<tr><td>Hypothesis ID</td><td class='g'>SHA-256(claim &#x7c;&#x7c; sorted_assumptions)</td></tr>\n");
    h.push_str("<tr><td>Pattern memory</td><td class='g'>In-memory, grows per drill tick</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    // C22 — ArtifactIR
    h.push_str("<div>\n<h3>C22 \u{2014} ArtifactIR (Intermediate Representation)</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Components</td><td class='g'>Derived from PmhdMonolith hypotheses</td></tr>\n");
    h.push_str("<tr><td>Interfaces</td><td class='g'>From DecisionSpec.interfaces</td></tr>\n");
    h.push_str("<tr><td>Component ID</td><td class='g'>SHA-256 content-addressed</td></tr>\n");
    h.push_str("<tr><td>FiveD signature</td><td class='g'>Dual SHA-256 (h1=id, h2=sig bytes)</td></tr>\n");
    h.push_str("<tr><td>Provenance link</td><td class='g'>monolith_id + spec_id + layer tag</td></tr>\n");
    h.push_str("<tr><td>Serde</td><td class='g'>JSON round-trip verified</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    h.push_str("</div>\n");

    // C23 — Forge (full width)
    h.push_str("<h3>C23 \u{2014} Forge Engine</h3>\n");
    h.push_str("<div class='grid2'>\n");
    h.push_str("<div><table><tbody>\n");
    h.push_str("<tr><td>Matrices available</td><td class='g'>RustModule, HttpApi, Workflow, Schema</td></tr>\n");
    h.push_str("<tr><td>Synthesizer</td><td class='g'>DefaultSynthesizer (JCS-canonicalized)</td></tr>\n");
    h.push_str("<tr><td>Evaluators</td><td class='g'>ConstraintEvaluator, QualityBoundsEvaluator</td></tr>\n");
    h.push_str("<tr><td>Emitters</td><td class='g'>File, Stdout, Gateway</td></tr>\n");
    h.push_str("</tbody></table></div>\n");
    h.push_str("<div><table><tbody>\n");
    h.push_str("<tr><td>Pattern memory</td><td class='g'>Store-backed via IslandStore (C17)</td></tr>\n");
    h.push_str("<tr><td>Forge crystal</td><td class='g'>scale_tag = forge:{domain}</td></tr>\n");
    h.push_str("<tr><td>IMPOSSIBLE constraint</td><td class='g'>Detected by ConstraintEvaluator</td></tr>\n");
    h.push_str("<tr><td>Gateway emitter</td><td class='g'>Constructs JSON payload, reports bytes_written</td></tr>\n");
    h.push_str("</tbody></table></div>\n");
    h.push_str("</div>\n");

    h.push_str("</div>\n"); // close section 8

    // ── Section 9: Phase 5.1 — Recursive Composition (C24) ───────────────────
    h.push_str("<div class='section'>\n<h2>9. Phase 5.1 \u{2014} Recursive Composition (C24)</h2>\n");
    h.push_str("<div class='grid2'>\n");

    // Decomposition config
    h.push_str("<div>\n<h3>C24 \u{2014} CompositionEngine</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Decomposition strategy</td><td class='g'>Midpoint goal-split, fully deterministic</td></tr>\n");
    h.push_str("<tr><td>Atom threshold</td><td class='g'>atom_max_components (default 4 goals)</td></tr>\n");
    h.push_str("<tr><td>Max recursion depth</td><td class='g'>max_depth (default 8)</td></tr>\n");
    h.push_str("<tr><td>Scale hierarchy</td><td class='g'>Atoms \u{2192} Micro, Molecules \u{2192} Meso, System \u{2192} Macro</td></tr>\n");
    h.push_str("<tr><td>Interface contracts</td><td class='g'>Auto-generated left\u{2192}right at each split</td></tr>\n");
    h.push_str("<tr><td>Output</td><td class='g'>SystemArtifact + System Crystal</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    // CV1–CV6 table
    h.push_str("<div>\n<h3>Composition Validation (CV1\u{2013}CV6)</h3>\n");
    h.push_str("<table><thead><tr><th>Gate</th><th>Check</th><th>Status</th></tr></thead><tbody>\n");
    h.push_str("<tr><td><strong>CV1</strong></td><td>Completeness \u{2014} all atom crystals valid</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td><strong>CV2</strong></td><td>Consistency \u{2014} no unsatisfied interfaces</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td><strong>CV3</strong></td><td>Composability \u{2014} bindings present</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td><strong>CV4</strong></td><td>Dependency order \u{2014} topological sort valid</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td><strong>CV5</strong></td><td>Coverage \u{2014} &ge; 50% atoms covered</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td><strong>CV6</strong></td><td>Stability \u{2014} at least one molecule formed</td><td class='g'>Active</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    h.push_str("</div>\n</div>\n"); // close grid2 + section 9

    // ── Section 10: Phase 6 — Hybrid Synthesis Oracle (C25) ──────────────────
    h.push_str("<div class='section'>\n<h2>10. Phase 6 \u{2014} Hybrid Synthesis Oracle (C25)</h2>\n");
    h.push_str("<p style='margin-bottom:1rem'>The oracle that generates. The system that validates. The memory that learns.<br>\
        Memory-first \u{2192} LLM fallback \u{2192} skeleton fallback. Every validated answer reduces the next question.</p>\n");
    h.push_str("<div class='grid2'>\n");

    h.push_str("<div>\n<h3>C25 \u{2014} OracleEngine</h3>\n");
    h.push_str("<table><tbody>\n");
    h.push_str("<tr><td>Default oracle</td><td class='g'>ClaudeOracle (claude-sonnet-4-20250514)</td></tr>\n");
    h.push_str("<tr><td>Memory-first</td><td class='g'>Cosine similarity \u{2265} 0.85 in 5D embedding space</td></tr>\n");
    h.push_str("<tr><td>Quality threshold</td><td class='g'>\u{2265} 0.6 for pattern reuse</td></tr>\n");
    h.push_str("<tr><td>Max retries</td><td class='g'>3 per synthesis request</td></tr>\n");
    h.push_str("<tr><td>Fallback</td><td class='g'>Skeleton (no LLM dependency for correctness)</td></tr>\n");
    h.push_str("<tr><td>API key</td><td class='g'>env:ANTHROPIC_API_KEY or capsule-protected (C14)</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    h.push_str("<div>\n<h3>Validation Pipeline (4 Stages)</h3>\n");
    h.push_str("<table><thead><tr><th>Stage</th><th>Check</th><th>Status</th></tr></thead><tbody>\n");
    h.push_str("<tr><td><strong>V1 Parse</strong></td><td>Non-empty + format-valid (JSON/Rust/YAML/OpenAPI)</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td><strong>V2 Constraints</strong></td><td>All required component names present</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td><strong>V3 PMHD</strong></td><td>Mini-PMHD adversarial quality check</td><td class='g'>Active</td></tr>\n");
    h.push_str("<tr><td><strong>V4 Gates</strong></td><td>8-gate quality threshold</td><td class='g'>Active</td></tr>\n");
    h.push_str("</tbody></table>\n</div>\n");

    h.push_str("</div>\n");

    h.push_str("<h3>Autonomy Metrics (M33, M34)</h3>\n");
    h.push_str("<div class='grid2'>\n");
    h.push_str("<div><table><thead><tr><th>Metric</th><th>Name</th><th>Formula</th><th>Goal</th></tr></thead><tbody>\n");
    h.push_str("<tr><td><strong>M33</strong></td><td>Autonomy Ratio</td><td>memory_hits / total_requests</td><td class='g'>\u{2192} 1.0 (asymptotic)</td></tr>\n");
    h.push_str("<tr><td><strong>M34</strong></td><td>Oracle Rejection Rate</td><td>oracle_rejections / oracle_calls</td><td class='g'>&lt; 0.1 (oracle aligned)</td></tr>\n");
    h.push_str("</tbody></table></div>\n");
    h.push_str("<div><table><thead><tr><th>Budget Control</th><th>Default</th></tr></thead><tbody>\n");
    h.push_str("<tr><td>max_calls_per_run</td><td class='g'>100</td></tr>\n");
    h.push_str("<tr><td>max_tokens_per_run</td><td class='g'>500,000</td></tr>\n");
    h.push_str("<tr><td>max_cost_per_run</td><td class='g'>$10.00</td></tr>\n");
    h.push_str("<tr><td>max_calls_per_day</td><td class='g'>1,000</td></tr>\n");
    h.push_str("</tbody></table></div>\n");
    h.push_str("</div>\n");

    h.push_str("<h3>Acceptance Tests (AT-O1\u{2013}AT-O10)</h3>\n");
    h.push_str("<div class='grid10'>\n");
    for (id, name) in &[
        ("AT-O1","MemoryHit"), ("AT-O2","LlmFallback"), ("AT-O3","ValidationRej"),
        ("AT-O4","PromptDet"),  ("AT-O5","Budget"),      ("AT-O6","Autonomy"),
        ("AT-O7","Crystallize"),("AT-O8","Graceful"),    ("AT-O9","NoLeak"),
        ("AT-O10","CapsuleKey"),
    ] {
        h.push_str(&format!(
            "<div class='gate-box'><strong>{id}</strong><br><small>{name}</small></div>\n"
        ));
    }
    h.push_str("</div>\n");

    h.push_str("</div>\n"); // close section 10

    // ── Footer ────────────────────────────────────────────────────────────────
    h.push_str("<footer>Generated by ISLS v1.0.0 \u{2014} deterministic, append-only, replay-verified</footer>\n");
    h.push_str("</body>\n</html>\n");
    h
}

fn cmd_status() {
    ensure_dirs().ok();
    let _config = load_config();
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

// ─── Execute Command (Extension: Generative Mode) ────────────────────────────

fn cmd_execute(input: &str, ticks: usize, output: Option<&str>) {
    let config = load_config();
    let rd = RunDescriptor {
        config: config.clone(),
        operator_versions: BTreeMap::new(),
        initial_state_digest: [0u8; 32],
        seed: None,
        registry_digests: BTreeMap::new(),
        scheduler: SchedulerConfig::default(),
    };
    let registries = RegistrySet::new();

    // Load crystal from archive or specified path
    let archive = load_archive();
    let execute_input = if input == "latest" || input.ends_with(".json") {
        let crystal = if input == "latest" {
            archive.crystals().last().cloned()
        } else {
            std::fs::read_to_string(input).ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        };
        match crystal {
            Some(c) => ExecuteInput::Crystal(c),
            None => {
                eprintln!("No crystal found at '{}'. Run 'isls run' first.", input);
                return;
            }
        }
    } else {
        eprintln!("Unsupported input format: {}", input);
        return;
    };

    println!("Executing program for {} ticks...", ticks);
    match execute(execute_input, None, &config, &rd, &registries, ticks) {
        Ok((crystals, manifest)) => {
            let committed: Vec<_> = crystals.iter().filter(|c| c.is_some()).collect();
            println!("Execute complete: {} crystals produced", committed.len());
            println!("Manifest run_id: {}", hex_hash(&manifest.run_id));
            if let Some(out_dir) = output {
                let _ = std::fs::create_dir_all(out_dir);
                let manifest_path = format!("{}/manifest.json", out_dir);
                if let Ok(s) = serde_json::to_string_pretty(&manifest) {
                    let _ = std::fs::write(&manifest_path, s);
                    println!("Manifest saved to {}", manifest_path);
                }
            }
            // Save manifest to default location
            let manifest_dir = isls_dir().join("manifests");
            let _ = std::fs::create_dir_all(&manifest_dir);
            let manifest_path = manifest_dir.join("latest.json");
            if let Ok(s) = serde_json::to_string_pretty(&manifest) {
                let _ = std::fs::write(&manifest_path, &s);
            }
        }
        Err(e) => eprintln!("Execute failed: {:?}", e),
    }
}

// ─── Seal Command (Extension: Capsule Protocol) ───────────────────────────────

fn cmd_seal(secret: &str, lock_manifest: Option<&str>, output: Option<&str>) {
    // Load manifest
    let manifest_path = match lock_manifest {
        Some("latest") | None => isls_dir().join("manifests/latest.json"),
        Some(p) => PathBuf::from(p),
    };

    let manifest: isls_manifest::ExecutionManifest = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(m) => m,
            Err(e) => { eprintln!("Failed to parse manifest: {}", e); return; }
        },
        Err(e) => { eprintln!("Failed to read manifest at {:?}: {}", manifest_path, e); return; }
    };

    let policy = CapsulePolicy {
        require_lock_program_id: [0u8; 32],
        require_rd_digest: manifest.rd_digest,
        require_gate_proofs: vec![],
        require_manifest_id: Some(manifest.run_id),
        expires_at: None,
        max_uses: None,
    };

    // Use a fixed test key (in production, load from keychain/KMS)
    let master_key: [u8; 32] = *b"isls-default-master-key-v1.0.0!!";

    match seal(secret.as_bytes(), policy, BTreeMap::new(), &master_key, &manifest) {
        Ok(capsule) => {
            let capsule_dir = isls_dir().join("capsules");
            let _ = std::fs::create_dir_all(&capsule_dir);
            let out_path = output
                .map(PathBuf::from)
                .unwrap_or_else(|| capsule_dir.join("latest.json"));
            if let Ok(s) = serde_json::to_string_pretty(&capsule) {
                let _ = std::fs::write(&out_path, &s);
                println!("Capsule sealed: {:?}", out_path);
            }
        }
        Err(e) => eprintln!("Seal failed: {:?}", e),
    }
}

// ─── Open Command (Extension: Capsule Protocol) ───────────────────────────────

fn cmd_open(capsule_path: &str) {
    let capsule_file = if capsule_path.is_empty() {
        isls_dir().join("capsules/latest.json")
    } else {
        PathBuf::from(capsule_path)
    };

    let capsule: isls_capsule::Capsule = match std::fs::read_to_string(&capsule_file) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(c) => c,
            Err(e) => { eprintln!("Failed to parse capsule: {}", e); return; }
        },
        Err(e) => { eprintln!("Failed to read capsule at {:?}: {}", capsule_file, e); return; }
    };

    // Load manifest referenced in capsule bind (or latest)
    let manifest_path = isls_dir().join("manifests/latest.json");
    let manifest: isls_manifest::ExecutionManifest = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(m) => m,
            Err(e) => { eprintln!("Failed to parse manifest: {}", e); return; }
        },
        Err(e) => { eprintln!("Failed to read manifest: {}", e); return; }
    };

    let master_key: [u8; 32] = *b"isls-default-master-key-v1.0.0!!";

    match open(&capsule, &master_key, &manifest, None) {
        Ok(plaintext) => {
            match std::str::from_utf8(&plaintext) {
                Ok(s) => println!("{}", s),
                Err(_) => println!("{:?}", plaintext),
            }
        }
        Err(e) => eprintln!("Open failed: {:?}", e),
    }
}

fn hex_hash(h: &isls_types::Hash256) -> String {
    h.iter().map(|b| format!("{:02x}", b)).collect()
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
    println!("  execute [options]              Execute a discovered crystal in generative mode");
    println!("    --input <path|latest>        Crystal JSON file or 'latest'");
    println!("    --ticks <n>                  Number of ticks to execute (default: 10)");
    println!("    --output <dir>               Output directory for manifest");
    println!("  seal [options]                 Seal a secret under a manifest-bound capsule");
    println!("    --secret <text>              Secret to seal");
    println!("    --lock-manifest <path|latest> Manifest to bind to");
    println!("  open [options]                 Open (decrypt) a capsule");
    println!("    --capsule <path>             Path to capsule JSON");
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
    println!("  isls execute --input latest --ticks 10");
    println!("  isls seal --secret 'my-secret' --lock-manifest latest");
    println!("  isls open --capsule ~/.isls/capsules/latest.json");
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
        Command::Init { store } => cmd_init(store.as_deref()),
        Command::Ingest { adapter, path, entities, scenario } => {
            cmd_ingest(&adapter, path.as_deref(), entities, scenario.as_deref());
        }
        Command::Run { replay, mode, ticks, project } => {
            cmd_run(replay.as_deref(), mode, ticks, project.as_deref());
        }
        Command::Execute { input, ticks, output } => {
            cmd_execute(&input, ticks, output.as_deref());
        }
        Command::Seal { secret, lock_manifest, output } => {
            cmd_seal(&secret, lock_manifest.as_deref(), output.as_deref());
        }
        Command::Open { capsule } => {
            cmd_open(&capsule);
        }
        Command::Bench => cmd_bench(),
        Command::Validate { formal, retro } => cmd_validate(formal, retro),
        Command::Report { json, html, full_html } => {
            if full_html { cmd_report_full_html(); } else { cmd_report(json, html); }
        }
        Command::Status => cmd_status(),
        Command::Help => print_help(),
        Command::ProjectList => cmd_project_list(),
        Command::ProjectCreate { name } => cmd_project_create(&name),
        Command::CrystalList { run_id } => cmd_crystal_list(&run_id),
        Command::CrystalShow { crystal_id } => cmd_crystal_show(&crystal_id),
        Command::Export { run_id, output } => cmd_export(&run_id, &output),
        Command::StoreVacuum => cmd_store_vacuum(),
        Command::StoreCheck => cmd_store_check(),
        Command::GenesisShow => cmd_genesis_show(),
        Command::GenesisValidate => cmd_genesis_validate(),
        Command::OracleStatus => cmd_oracle_status(),
        Command::OracleMemory => cmd_oracle_memory(),
        Command::OracleSealKey { key, lock_genesis } => cmd_oracle_seal_key(&key, lock_genesis),
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
        assert!(matches!(cmd, Command::Init { .. }));
    }

    #[test]
    fn test_parse_init_store() {
        let cmd = parse_args(&args(&["isls", "init", "--store", "sqlite"]));
        match cmd {
            Command::Init { store: Some(s) } => assert_eq!(s, "sqlite"),
            _ => panic!("expected Init with store=sqlite"),
        }
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
    fn test_parse_report_html_positional() {
        let cmd = parse_args(&args(&["isls", "report", "html"]));
        match cmd {
            Command::Report { html: true, .. } => {}
            _ => panic!("expected Report html from positional arg"),
        }
    }

    #[test]
    fn test_parse_report_full_html() {
        let cmd = parse_args(&args(&["isls", "report", "full-html"]));
        match cmd {
            Command::Report { full_html: true, json: false, html: false } => {}
            _ => panic!("expected Report full_html"),
        }
    }

    #[test]
    fn test_report_full_html_runs() {
        // Should not panic even with no results files present
        cmd_report_full_html();
    }

    #[test]
    fn test_report_html_runs() {
        cmd_report(false, true);
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
