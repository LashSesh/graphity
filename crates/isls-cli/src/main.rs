// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
// isls-cli: Single-binary operator interface (C11)
// D1 Clean Architecture — forge-v2 + serve + help

use std::path::Path;

mod cmd_evolve;
mod cmd_metrics;
mod cmd_norms;
mod cmd_scrape;
mod cmd_spectroscopy;

// ─── Command Enum ────────────────────────────────────────────────────────────

enum Command {
    /// HDAG code generation pipeline (v3.4 PCR Staged Closure).
    ForgeV2 {
        requirements: String,
        output: String,
        mock_oracle: bool,
        api_key: Option<String>,
        model: String,
        ollama: bool,
        ollama_url: String,
        swarm: bool,
        swarm_size: usize,
        swarm_threshold: f64,
    },
    /// D3: Chat-to-App — natural language to compiled application.
    ForgeChat {
        message: String,
        output: String,
        api_key: Option<String>,
        model: String,
        ollama: bool,
        ollama_url: String,
        swarm: bool,
        swarm_size: usize,
        swarm_threshold: f64,
    },
    /// D4: Norm catalog inspection and management.
    Norms { subcmd: NormsSubcmd },
    /// D5: Repository scraping — topology to norms.
    Scrape {
        path: Option<String>,
        url: Option<String>,
        manifest: Option<String>,
        domain: Option<String>,
        max_size_mb: u64,
        timeout_secs: u64,
    },
    /// D6: Generate ISLS Studio — the generator generating itself.
    ForgeSelf {
        output: String,
        mock_oracle: bool,
        api_key: Option<String>,
        model: String,
        ollama: bool,
        ollama_url: String,
        swarm: bool,
        swarm_size: usize,
        swarm_threshold: f64,
    },
    /// D7: Generation metrics inspection.
    Metrics { compare: bool, last: Option<usize> },
    /// I3/W1: Constraint Spectroscopy — analyse a target system.
    Spectroscopy {
        path: Option<String>,
        scrape: bool,
    },
    /// I3/W3: Solve-Coagula orchestration.
    Evolve {
        from: String,
        delta: String,
        output: String,
        scrape_gaps: bool,
        api_key: Option<String>,
        model: String,
        ollama: bool,
        ollama_url: String,
    },
    /// Start the Gateway / Studio web interface.
    Serve {
        port: u16,
        api_key: Option<String>,
        ollama: bool,
        ollama_url: String,
        ollama_model: String,
    },
    /// Print help.
    Help,
}

/// Subcommands for `isls norms`.
enum NormsSubcmd {
    /// List all norms.
    List { auto_only: bool, source: Option<String> },
    /// Inspect a specific norm by ID.
    Inspect { norm_id: String },
    /// List candidate pool.
    Candidates,
    /// Summary statistics.
    Stats,
    /// Norm fitness report.
    Fitness,
    /// I2/W2: Gen-Clustering (gene detection from metrics.jsonl).
    Genome,
    /// Reset auto-discovered norms.
    Reset,
    /// I3/W2: Inject a norm blueprint from JSON file.
    Inject { file: String },
    /// I3/W2: Remove an injected norm by id.
    Remove { id: String },
}

// ─── Argument Parsing ────────────────────────────────────────────────────────

fn parse_args(args: &[String]) -> Command {
    if args.len() < 2 {
        return Command::Help;
    }
    match args[1].as_str() {
        "forge-v2" => {
            let requirements = args.iter().position(|a| a == "--requirements")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "examples/warehouse.toml".to_string());
            let output = args.iter().position(|a| a == "--output")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "./output-v2".to_string());
            let mock_oracle = args.contains(&"--mock-oracle".to_string());
            let api_key = args.iter().position(|a| a == "--api-key")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok());
            let model = args.iter().position(|a| a == "--model")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "gpt-4o".to_string());
            let ollama = args.contains(&"--ollama".to_string());
            let ollama_url = args.iter().position(|a| a == "--ollama-url")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let swarm = args.contains(&"--swarm".to_string());
            let swarm_size = args.iter().position(|a| a == "--swarm-size")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(4);
            let swarm_threshold = args.iter().position(|a| a == "--swarm-threshold")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.20);
            Command::ForgeV2 { requirements, output, mock_oracle, api_key, model, ollama, ollama_url, swarm, swarm_size, swarm_threshold }
        }
        "forge-chat" => {
            let message = args.iter().position(|a| a == "--message" || a == "-m")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| {
                    eprintln!("[ERROR] --message / -m is required");
                    std::process::exit(1);
                });
            let output = args.iter().position(|a| a == "--output")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "./output".to_string());
            let api_key = args.iter().position(|a| a == "--api-key")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok());
            let model = args.iter().position(|a| a == "--model")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "gpt-4o".to_string());
            let ollama = args.contains(&"--ollama".to_string());
            let ollama_url = args.iter().position(|a| a == "--ollama-url")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let swarm = args.contains(&"--swarm".to_string());
            let swarm_size = args.iter().position(|a| a == "--swarm-size")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(4);
            let swarm_threshold = args.iter().position(|a| a == "--swarm-threshold")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.20);
            Command::ForgeChat { message, output, api_key, model, ollama, ollama_url, swarm, swarm_size, swarm_threshold }
        }
        "norms" => {
            let subcmd = args.get(2).map(|s| s.as_str()).unwrap_or("list");
            let subcmd = match subcmd {
                "list" => {
                    let auto_only = args.contains(&"--auto-only".to_string());
                    let source = args.iter().position(|a| a == "--source")
                        .and_then(|i| args.get(i + 1))
                        .cloned();
                    NormsSubcmd::List { auto_only, source }
                }
                "inject" => {
                    let file = args.get(3).cloned().unwrap_or_else(|| {
                        eprintln!("[ERROR] isls norms inject requires a blueprint file path");
                        std::process::exit(1);
                    });
                    NormsSubcmd::Inject { file }
                }
                "remove" => {
                    let id = args.get(3).cloned().unwrap_or_else(|| {
                        eprintln!("[ERROR] isls norms remove requires a norm ID");
                        std::process::exit(1);
                    });
                    NormsSubcmd::Remove { id }
                }
                "inspect" => {
                    let norm_id = args.get(3).cloned().unwrap_or_else(|| {
                        eprintln!("[ERROR] isls norms inspect requires a norm ID");
                        std::process::exit(1);
                    });
                    NormsSubcmd::Inspect { norm_id }
                }
                "candidates" => NormsSubcmd::Candidates,
                "stats" => NormsSubcmd::Stats,
                "fitness" => NormsSubcmd::Fitness,
                "genome" => NormsSubcmd::Genome,
                "reset" => NormsSubcmd::Reset,
                _ => {
                    eprintln!("[ERROR] Unknown norms subcommand: {}", subcmd);
                    eprintln!("Available: list, inspect, candidates, stats, fitness, genome, reset, inject, remove");
                    std::process::exit(1);
                }
            };
            Command::Norms { subcmd }
        }
        "scrape" => {
            let path = args.iter().position(|a| a == "--path")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let url = args.iter().position(|a| a == "--url")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let manifest = args.iter().position(|a| a == "--manifest")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let domain = args.iter().position(|a| a == "--domain")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let max_size_mb = args.iter().position(|a| a == "--max-size-mb")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(200);
            let timeout_secs = args.iter().position(|a| a == "--timeout")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(120);
            Command::Scrape { path, url, manifest, domain, max_size_mb, timeout_secs }
        }
        "forge-self" => {
            let output = args.iter().position(|a| a == "--output")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "./output/isls-studio".to_string());
            let mock_oracle = args.contains(&"--mock-oracle".to_string());
            let api_key = args.iter().position(|a| a == "--api-key")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok());
            let model = args.iter().position(|a| a == "--model")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "gpt-4o".to_string());
            let ollama = args.contains(&"--ollama".to_string());
            let ollama_url = args.iter().position(|a| a == "--ollama-url")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let swarm = args.contains(&"--swarm".to_string());
            let swarm_size = args.iter().position(|a| a == "--swarm-size")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(4);
            let swarm_threshold = args.iter().position(|a| a == "--swarm-threshold")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.20);
            Command::ForgeSelf { output, mock_oracle, api_key, model, ollama, ollama_url, swarm, swarm_size, swarm_threshold }
        }
        "metrics" => {
            let compare = args.contains(&"--compare".to_string());
            let last = args.iter().position(|a| a == "--last")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok());
            Command::Metrics { compare, last }
        }
        "spectroscopy" => {
            let path = args.iter().position(|a| a == "--path")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let scrape = args.contains(&"--scrape".to_string());
            Command::Spectroscopy { path, scrape }
        }
        "evolve" => {
            let from = args.iter().position(|a| a == "--from")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| {
                    eprintln!("[ERROR] --from <path> is required for evolve");
                    std::process::exit(1);
                });
            let delta = args.iter().position(|a| a == "--delta")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| {
                    eprintln!("[ERROR] --delta <description> is required for evolve");
                    std::process::exit(1);
                });
            let output = args.iter().position(|a| a == "--output")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "./evolved".to_string());
            let scrape_gaps = args.contains(&"--scrape-gaps".to_string());
            let api_key = args.iter().position(|a| a == "--api-key")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok());
            let model = args.iter().position(|a| a == "--model")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "gpt-4o".to_string());
            let ollama = args.contains(&"--ollama".to_string());
            let ollama_url = args.iter().position(|a| a == "--ollama-url")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            Command::Evolve { from, delta, output, scrape_gaps, api_key, model, ollama, ollama_url }
        }
        "serve" => {
            let port = args.iter().position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(8420);
            let api_key = args.iter().position(|a| a == "--api-key")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let ollama = args.contains(&"--ollama".to_string());
            let ollama_url = args.iter().position(|a| a == "--ollama-url")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            let ollama_model = args.iter().position(|a| a == "--ollama-model")
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_else(|| "qwen2.5-coder:32b".to_string());
            Command::Serve { port, api_key, ollama, ollama_url, ollama_model }
        }
        "--help" | "-h" | "help" => Command::Help,
        _ => Command::Help,
    }
}

// ─── forge-v2: HDAG Pipeline ─────────────────────────────────────────────────

fn cmd_forge_v2(
    requirements_path: &str,
    output: &str,
    mock_oracle: bool,
    api_key: Option<String>,
    model: &str,
    ollama: bool,
    ollama_url: &str,
    swarm: bool,
    swarm_size: usize,
    swarm_threshold: f64,
) {
    use isls_hypercube::{
        DimState, DimValue,
        domain::DomainRegistry,
    };
    use isls_forge_llm::{ForgePlan, forge::LlmForge};
    use isls_forge_llm::oracle::{MockOracle, OllamaOracle, OpenAiOracle};
    use isls_merkaba::SwarmOracle;

    println!("╔══════════════════════════════════════════════════════╗");
    println!("║     ISLS v3.4 HDAG Staged Closure Pipeline           ║");
    println!("║     Structural + LLM + Compile Verification           ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    let req_path = Path::new(requirements_path);
    if !req_path.exists() {
        eprintln!("[ERROR] Requirements file not found: {}", requirements_path);
        std::process::exit(1);
    }
    let output_dir = Path::new(output);

    // Load .env if present (best-effort)
    let _ = dotenv::dotenv();

    // D8: Oracle selection priority: mock > ollama > openai > mock fallback
    let use_mock = mock_oracle
        || (!ollama && api_key.is_none() && std::env::var("OPENAI_API_KEY").is_err());

    if use_mock && !swarm {
        println!("[Mode] Mock oracle — no LLM calls (compilable output)");
    } else if swarm {
        let inner_desc = if use_mock { "mock".to_string() }
            else if ollama { format!("ollama/{}", model) }
            else { format!("openai/{}", model) };
        println!("[Mode] MERKABA Swarm(n={}, inner={}, Θ={:.2})", swarm_size, inner_desc, swarm_threshold);
    } else if ollama {
        println!("[Mode] Ollama oracle — {} at {}", model, ollama_url);
    } else {
        println!("[Mode] LLM oracle — {}", model);
    }

    // 1. Parse TOML → HyperCube
    let cube = match isls_hypercube::parser::parse_toml_to_cube(req_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[ERROR] Failed to parse requirements: {}", e);
            std::process::exit(1);
        }
    };

    // 2. Extract app name from cube
    let app_name = cube.dimensions.iter()
        .find(|d| d.name == "arch.app_name")
        .and_then(|d| match &d.state {
            DimState::Fixed(DimValue::Choice(s)) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "app".into());

    // 3. Build ForgePlan — D2 generic path or D1 domain-template path
    let (plan, domain_name) = if cube.entities_from_toml {
        // D2: entities parsed from TOML [[entities]] arrays
        let toml_entities = cube.extract_entities();
        let domain_name = app_name.replace('-', " ")
            .split_whitespace()
            .next()
            .unwrap_or("app")
            .to_string();
        let description = format!(
            "A {} application generated by ISLS v3.4.",
            domain_name
        );
        println!("[D2] Parsed {} entities from TOML", toml_entities.len());
        let plan = ForgePlan::from_toml_entities(&app_name, &description, &domain_name, &toml_entities);
        (plan, domain_name)
    } else {
        // D1: detect domain from module descriptions
        let registry = DomainRegistry::new();
        let toml_content = std::fs::read_to_string(req_path).unwrap_or_default();
        let domain = registry
            .detect(&toml_content)
            .cloned()
            .unwrap_or_else(|| {
                isls_hypercube::domain::DomainTemplate {
                    name: "warehouse".into(),
                    keywords: vec!["warehouse".into()],
                    entities: vec![],
                    relationships: vec![],
                    business_rules: vec![],
                    api_features: isls_hypercube::domain::ApiFeatures {
                        pagination: true,
                        filtering: vec![],
                        sorting: vec!["created_at".into()],
                        search_fields: vec!["name".into()],
                        export_formats: vec!["json".into()],
                    },
                }
            });
        let domain_name = domain.name.clone();
        let description = format!(
            "A {} application generated by ISLS v3.4.",
            domain_name
        );
        let plan = ForgePlan::from_domain(&app_name, &description, &domain);
        (plan, domain_name)
    };

    // D6: Derive InfraBlueprint from description via infrastructure norm matching
    let mut plan = plan;
    plan.blueprint = isls_forge_llm::blueprint::derive_blueprint_from_description(&plan.spec.description);

    // 5. Create inner oracle (D8: mock > ollama > openai > mock fallback)
    let inner_oracle: Box<dyn isls_forge_llm::oracle::Oracle> = if use_mock && !swarm {
        Box::new(MockOracle)
    } else if ollama {
        match OllamaOracle::check_availability(ollama_url) {
            Ok(()) => {
                if let Err(e) = OllamaOracle::check_model(ollama_url, model) {
                    eprintln!("[WARN] {e}");
                }
                Box::new(OllamaOracle::new(model, ollama_url))
            }
            Err(e) => {
                eprintln!("[WARN] {e}; falling back to mock oracle");
                Box::new(MockOracle)
            }
        }
    } else if swarm && use_mock {
        // --swarm alone → SwarmOracle(MockOracle) for testing consensus
        Box::new(MockOracle)
    } else {
        match OpenAiOracle::new(api_key, Some(model.to_string())) {
            Ok(o) => Box::new(o),
            Err(e) => {
                eprintln!("[WARN] Oracle init failed: {e}; falling back to mock");
                Box::new(MockOracle)
            }
        }
    };

    // M1: Optionally wrap with SwarmOracle for Ophanim consensus
    let oracle: Box<dyn isls_forge_llm::oracle::Oracle> = if swarm {
        Box::new(
            SwarmOracle::new(inner_oracle, swarm_size)
                .with_threshold(swarm_threshold)
        )
    } else {
        inner_oracle
    };

    // 6. Create output dir and run LlmForge
    if let Err(e) = std::fs::create_dir_all(output_dir) {
        eprintln!("[ERROR] Cannot create output dir: {}", e);
        std::process::exit(1);
    }
    // Snapshot data needed for metrics BEFORE plan is moved into forge
    let metrics_description = plan.spec.description.clone();
    let metrics_entity_count = plan.spec.entities.len();
    let metrics_norm_ids = plan.norm_ids.clone();

    let mut forge = LlmForge::new(oracle, plan, output_dir.to_path_buf(), use_mock);

    let start = std::time::Instant::now();
    let forge_result = forge.generate();

    // Always write a metrics entry, success or failure.
    write_generation_metrics(
        &forge.stats,
        &metrics_description,
        metrics_entity_count,
        &metrics_norm_ids,
        forge_result.is_ok(),
        start.elapsed().as_secs_f64(),
        forge_result.as_ref().map(|files| files.len()).unwrap_or(0),
        forge_result.as_ref().map(|files| {
            files.iter().filter(|f| f.generation_method == "structural").count()
        }).unwrap_or(0),
        forge_result.as_ref().map(|files| {
            files.iter().filter(|f| f.generation_method == "llm").count()
        }).unwrap_or(0),
    );

    match forge_result {
        Ok(generated_files) => {
            let stats = &forge.stats;
            let time_secs = start.elapsed().as_secs_f64();

            println!();
            println!("╔══════════════════════════════════════════════════════╗");
            println!("║              V3.4 GENERATION COMPLETE                ║");
            println!("╚══════════════════════════════════════════════════════╝");
            println!();
            println!("  App:              {}", app_name);
            println!("  Files generated:  {}", generated_files.len());
            println!("  Total LOC:        {}", generated_files.iter().map(|f| f.content.lines().count()).sum::<usize>());
            println!("  Domain:           {}", domain_name);
            println!("  Total tokens:     {}", stats.total_tokens);
            println!("  Total time:       {:.2}s", time_secs);
            println!();
            println!("  Output:           {}", output);
            println!("  Backend:          {}/backend/", output);
            println!("  Frontend:         {}/frontend/", output);
            println!();
            println!("Next steps:");
            println!("  cd {} && docker-compose up -d", output);
            println!("  # or: cd {}/backend && cargo build", output);
        }
        Err(e) => {
            eprintln!();
            eprintln!("[ERROR] V3.4 pipeline failed: {}", e);
            std::process::exit(1);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn write_generation_metrics(
    stats: &isls_forge_llm::ForgeStats,
    description: &str,
    entity_count: usize,
    norm_ids: &[String],
    compile_success: bool,
    duration_secs: f64,
    file_count: usize,
    structural_files: usize,
    llm_files: usize,
) {
    use isls_forge_llm::metrics::{append_metrics, GenerationMetrics, GenerationSource};
    let id = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut hasher);
        format!("gen-{:08x}", hasher.finish() as u32)
    };
    let metrics = GenerationMetrics {
        id,
        timestamp: chrono::Utc::now().to_rfc3339(),
        source: GenerationSource::Cli,
        description: description.to_string(),
        entity_count,
        file_count,
        structural_files,
        llm_files,
        total_tokens: stats.total_tokens,
        compile_success,
        coagula_cycles: stats.compile_failures as u32,
        duration_secs,
        norms_activated: norm_ids.to_vec(),
        conversation_turns: 1,
        contraction_ratios: stats.i1_contraction_ratios.clone(),
        was_contractive: stats.i1_was_contractive,
        mikro_gate_pass_rate: stats.i1_mikro_gate_pass_rate,
        meso_gate_pass_rate: stats.i1_meso_gate_pass_rate,
        // I2/W4: persist average Codematrix resonance so
        // `isls norms genome` and fitness analytics can read it later.
        codematrix_avg: stats.i2_codematrix_avg,
    };
    if let Err(e) = append_metrics(&metrics) {
        eprintln!("[WARN] Could not write metrics.jsonl: {}", e);
    } else {
        eprintln!("[Metrics] Entry written to ~/.isls/metrics.jsonl");
    }
}

// ─── forge-chat: D3 Chat-to-App ──────────────────────────────────────────────

fn cmd_forge_chat(
    message: &str,
    output: &str,
    api_key: Option<String>,
    model: &str,
    ollama: bool,
    ollama_url: &str,
    swarm: bool,
    swarm_size: usize,
    swarm_threshold: f64,
) {
    use isls_forge_llm::oracle::{OllamaOracle, OpenAiOracle};

    println!("╔══════════════════════════════════════════════════════╗");
    println!("║     ISLS D3 — Chat to App                            ║");
    println!("║     Natural Language → TOML → HDAG → App              ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    // Load .env if present (best-effort)
    let _ = dotenv::dotenv();

    let resolved_key = api_key.clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());

    if !ollama && !swarm && resolved_key.is_none() {
        eprintln!("[ERROR] --api-key, OPENAI_API_KEY, --ollama, or --swarm required for forge-chat");
        std::process::exit(1);
    }

    // 1. Build extraction prompt
    let prompt = isls_chat::build_extraction_prompt(message);
    println!("[Chat] Extracting entities from: \"{}\"", message);

    // 2. Call LLM (single call) — D8: support Ollama oracle
    let extraction_oracle: Box<dyn isls_forge_llm::Oracle> = if ollama {
        match OllamaOracle::check_availability(ollama_url) {
            Ok(()) => Box::new(OllamaOracle::new(model, ollama_url)),
            Err(e) => {
                eprintln!("[ERROR] {e}");
                std::process::exit(1);
            }
        }
    } else {
        match OpenAiOracle::new(resolved_key.clone(), Some(model.to_string())) {
            Ok(o) => Box::new(o),
            Err(e) => {
                eprintln!("[ERROR] Oracle init failed: {}", e);
                std::process::exit(1);
            }
        }
    };

    let json_str = match isls_forge_llm::Oracle::call(extraction_oracle.as_ref(), &prompt, 4096) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[ERROR] LLM extraction failed: {}", e);
            std::process::exit(1);
        }
    };

    // 3. Parse JSON
    let mut json: serde_json::Value = match serde_json::from_str(json_str.trim()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[ERROR] LLM returned invalid JSON: {}", e);
            eprintln!("Raw response:\n{}", json_str);
            std::process::exit(1);
        }
    };

    // 4. Validate
    if let Err(e) = isls_chat::validate_extracted_spec(&json) {
        eprintln!("[ERROR] Validation failed: {}", e);
        eprintln!("Try rephrasing your description or adding more detail.");
        std::process::exit(1);
    }

    let entity_count = json["entities"].as_array().map_or(0, |e| e.len());
    println!("[Chat] Extracted {} entities", entity_count);

    // 4.1 D4: Norm-guided enrichment (additive only, fallback to D3 on failure)
    isls_chat::norm_enrichment::enrich_with_norms(message, &mut json);

    // 5. Convert to TOML
    let toml_content = match isls_chat::json_to_toml(&json) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[ERROR] TOML conversion failed: {}", e);
            std::process::exit(1);
        }
    };

    // 6. Save TOML
    let output_dir = Path::new(output);
    if let Err(e) = std::fs::create_dir_all(output_dir) {
        eprintln!("[ERROR] Cannot create output dir: {}", e);
        std::process::exit(1);
    }
    let toml_path = output_dir.join("spec.toml");
    if let Err(e) = std::fs::write(&toml_path, &toml_content) {
        eprintln!("[ERROR] Cannot write spec.toml: {}", e);
        std::process::exit(1);
    }

    // 7. Print TOML for user review
    println!();
    println!("--- Extracted Specification ---");
    println!("{}", toml_content);
    println!("--- End Specification ---");
    println!();
    println!("[Chat] TOML saved to {}", toml_path.display());
    println!("[Chat] Starting forge pipeline...");
    println!();

    // 8. Run the proven D2 pipeline
    cmd_forge_v2(
        toml_path.to_str().unwrap_or("spec.toml"),
        output,
        false,
        api_key.or_else(|| std::env::var("OPENAI_API_KEY").ok()),
        model,
        ollama,
        ollama_url,
        swarm,
        swarm_size,
        swarm_threshold,
    );
}

// ─── forge-self: D6 Möbius ───────────────────────────────────────────────────

fn cmd_forge_self(
    output: &str,
    mock_oracle: bool,
    api_key: Option<String>,
    model: &str,
    ollama: bool,
    ollama_url: &str,
    swarm: bool,
    swarm_size: usize,
    swarm_threshold: f64,
) {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║  D6 Möbius — Generating ISLS Studio                   ║");
    println!("║  The generator generating itself.                     ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    let requirements = "examples/isls_studio.toml";
    cmd_forge_v2(requirements, output, mock_oracle, api_key.clone(), model, ollama, ollama_url, swarm, swarm_size, swarm_threshold);

    // D6: Self-observation — feed generated artifacts into D4 norm learning
    let output_dir = Path::new(output);
    match (|| -> std::result::Result<(), Box<dyn std::error::Error>> {
        let collector = isls_forge_llm::artifact_collector::ArtifactCollector::new(output_dir);
        let observed = collector.collect();
        let domain = "isls-studio";
        let run_id = format!(
            "{}_{}",
            domain,
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        );
        let mut registry = isls_norms::NormRegistry::new();
        registry.observe_and_learn(&observed, domain, &run_id);
        println!("[D6] Self-observation: {} artifacts fed to norm learning (domain: {})", observed.len(), domain);
        Ok(())
    })() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("[D6] Self-observation failed (non-blocking): {}", e);
        }
    }
}

// ─── serve: Gateway / Studio ─────────────────────────────────────────────────

fn cmd_serve(
    port: u16,
    api_key: Option<String>,
    ollama: bool,
    ollama_url: String,
    ollama_model: String,
) {
    // Set API key in environment if provided via --api-key flag
    if let Some(ref key) = api_key {
        std::env::set_var("OPENAI_API_KEY", key);
    }

    let resolved_key = api_key.clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());
    let has_key = resolved_key.is_some();

    let mode = if has_key {
        "LLM generation (OpenAI API key)"
    } else if ollama {
        "LLM generation (Ollama local)"
    } else {
        "Mock mode (no LLM configured)"
    };

    println!("ISLS Gateway v3.4 starting on port {}...", port);
    println!("Mode: {}", mode);
    if ollama {
        println!("Ollama: {} ({})", ollama_model, ollama_url);
    }
    println!("Studio available at http://localhost:{}/studio", port);
    println!("API available at http://localhost:{}/", port);
    println!("WebSocket events at ws://localhost:{}/events", port);
    println!();

    let oracle_config = isls_gateway::OracleConfig {
        api_key: resolved_key,
        use_ollama: ollama,
        ollama_url,
        ollama_model,
        openai_model: "gpt-4o".to_string(),
    };

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let state = isls_gateway::AppState::new().with_oracle_config(oracle_config);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        if let Err(e) = isls_gateway::serve(state, addr).await {
            eprintln!("Gateway error: {}", e);
        }
    });
}

// ─── help ────────────────────────────────────────────────────────────────────

fn print_help() {
    println!("ISLS — D5 Repository Scraping Architecture");
    println!();
    println!("Usage: isls <command> [options]");
    println!();
    println!("Commands:");
    println!("  forge-v2    HDAG code generation pipeline (Staged Closure)");
    println!("  forge-self  D6: Generate ISLS Studio — the generator generating itself");
    println!("  forge-chat  D3: Natural language to compiled application");
    println!("  norms       D4: Inspect norm catalog, candidates, and auto-discovered norms");
    println!("  scrape      D5: Scrape repositories — extract topology into norms");
    println!("  spectroscopy I3/W1: Constraint Spectroscopy (target → gaps + keywords)");
    println!("  evolve      I3/W3: Solve-Coagula cycle (from → delta → next-gen)");
    println!("  metrics     D7: Generation metrics (CLI vs Cockpit comparison)");
    println!("  serve       Start the Gateway / Studio web interface");
    println!("  help        Print this message");
    println!();
    println!("forge-v2 options:");
    println!("  --requirements <path>  TOML requirements file (default: examples/warehouse.toml)");
    println!("  --output <path>        Output directory (default: ./output-v2)");
    println!("  --mock-oracle          Use mock oracle (no LLM calls, compilable output)");
    println!("  --api-key <key>        OpenAI API key (or set OPENAI_API_KEY env var)");
    println!("  --model <model>        LLM model name (default: gpt-4o)");
    println!("  --ollama               Use local Ollama instance instead of OpenAI");
    println!("  --ollama-url <url>     Ollama API URL (default: http://localhost:11434)");
    println!("  --swarm                M1: Use MERKABA Ophanim Swarm (n oracle calls + consensus)");
    println!("  --swarm-size <n>       Number of Thronengel per call (default: 4)");
    println!("  --swarm-threshold <f>  Resonance threshold Theta (default: 0.20)");
    println!();
    println!("forge-self options:");
    println!("  --output <path>        Output directory (default: ./output/isls-studio)");
    println!("  --mock-oracle          Use mock oracle (no LLM calls)");
    println!("  --api-key <key>        OpenAI API key (or set OPENAI_API_KEY env var)");
    println!("  --model <model>        LLM model name (default: gpt-4o)");
    println!("  --ollama               Use local Ollama instance instead of OpenAI");
    println!("  --ollama-url <url>     Ollama API URL (default: http://localhost:11434)");
    println!("  --swarm                M1: Use MERKABA Ophanim Swarm");
    println!("  --swarm-size <n>       Number of Thronengel (default: 4)");
    println!("  --swarm-threshold <f>  Resonance threshold (default: 0.20)");
    println!();
    println!("forge-chat options:");
    println!("  --message / -m <text>  Application description in natural language (required)");
    println!("  --output <path>        Output directory (default: ./output)");
    println!("  --api-key <key>        OpenAI API key (or set OPENAI_API_KEY env var)");
    println!("  --model <model>        LLM model name (default: gpt-4o)");
    println!("  --ollama               Use local Ollama instance instead of OpenAI");
    println!("  --ollama-url <url>     Ollama API URL (default: http://localhost:11434)");
    println!("  --swarm                M1: Use MERKABA Ophanim Swarm");
    println!("  --swarm-size <n>       Number of Thronengel (default: 4)");
    println!("  --swarm-threshold <f>  Resonance threshold (default: 0.20)");
    println!();
    println!("norms subcommands:");
    println!("  list [--auto-only] [--source builtin|auto|injected]");
    println!("                          List norms (filtered by origin)");
    println!("  inspect <norm-id>      Show full norm details");
    println!("  candidates             List candidate pool");
    println!("  stats                  Summary statistics");
    println!("  inject <file>          I3/W2: Register a norm blueprint JSON");
    println!("  remove <id>            I3/W2: Remove an injected norm (ISLS-NORM-INJECT-*)");
    println!("  reset                  Delete ~/.isls/norms.json (with confirm)");
    println!();
    println!("spectroscopy options:");
    println!("  --path <dir>           Target project (default: .)");
    println!("  --scrape               Emit scrape instructions for detected gaps");
    println!();
    println!("evolve options:");
    println!("  --from <path>          Existing project to evolve (required)");
    println!("  --delta <text>         Natural-language description of the delta (required)");
    println!("  --output <path>        Output directory (default: ./evolved)");
    println!("  --scrape-gaps          Emit targeted scrape campaigns for each gap");
    println!("  --api-key <key>        OpenAI API key (or set OPENAI_API_KEY env var)");
    println!("  --model <model>        LLM model name (default: gpt-4o)");
    println!("  --ollama               Use local Ollama instance instead of OpenAI");
    println!("  --ollama-url <url>     Ollama API URL (default: http://localhost:11434)");
    println!();
    println!("scrape options:");
    println!("  --path <dir>           Local directory to scrape");
    println!("  --url <url>            Git repository URL to clone and scrape");
    println!("  --manifest <file>      TOML manifest with multiple repos");
    println!("  --domain <name>        Override inferred domain name");
    println!("  --max-size-mb <mb>     Max clone size in MB (default: 200)");
    println!("  --timeout <secs>       Clone timeout in seconds (default: 120)");
    println!();
    println!("metrics options:");
    println!("  --compare              CLI vs Cockpit comparison table");
    println!("  --last <N>             Show last N generation entries");
    println!();
    println!("serve options:");
    println!("  --port <port>          Port number (default: 8420)");
    println!("  --api-key <key>        OpenAI API key for LLM generation");
    println!("  --ollama               Use local Ollama instead of OpenAI");
    println!("  --ollama-url <url>     Ollama base URL (default: http://localhost:11434)");
    println!("  --ollama-model <name>  Ollama model (default: qwen2.5-coder:32b)");
    println!();
    println!("Pipeline: forge-chat -> TOML -> forge-v2 -> cargo build -> docker-compose up");
    println!("One sentence. One app. Zero manual steps.");
}

// ─── main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = parse_args(&args);

    match cmd {
        Command::ForgeV2 { requirements, output, mock_oracle, api_key, model, ollama, ollama_url, swarm, swarm_size, swarm_threshold } => {
            cmd_forge_v2(&requirements, &output, mock_oracle, api_key, &model, ollama, &ollama_url, swarm, swarm_size, swarm_threshold);
        }
        Command::ForgeChat { message, output, api_key, model, ollama, ollama_url, swarm, swarm_size, swarm_threshold } => {
            cmd_forge_chat(&message, &output, api_key, &model, ollama, &ollama_url, swarm, swarm_size, swarm_threshold);
        }
        Command::ForgeSelf { output, mock_oracle, api_key, model, ollama, ollama_url, swarm, swarm_size, swarm_threshold } => {
            cmd_forge_self(&output, mock_oracle, api_key, &model, ollama, &ollama_url, swarm, swarm_size, swarm_threshold);
        }
        Command::Scrape { path, url, manifest, domain, max_size_mb, timeout_secs } => {
            cmd_scrape::cmd_scrape(cmd_scrape::ScrapeOpts {
                path, url, manifest, domain, max_size_mb, timeout_secs,
            });
        }
        Command::Norms { subcmd } => match subcmd {
            NormsSubcmd::List { auto_only, source } => {
                cmd_norms::cmd_norms_list(auto_only, source.as_deref())
            }
            NormsSubcmd::Inspect { norm_id } => cmd_norms::cmd_norms_inspect(&norm_id),
            NormsSubcmd::Candidates => cmd_norms::cmd_norms_candidates(),
            NormsSubcmd::Stats => cmd_norms::cmd_norms_stats(),
            NormsSubcmd::Fitness => cmd_norms::cmd_norms_fitness(),
            NormsSubcmd::Genome => cmd_norms::cmd_norms_genome(),
            NormsSubcmd::Reset => cmd_norms::cmd_norms_reset(),
            NormsSubcmd::Inject { file } => cmd_norms::cmd_norms_inject(&file),
            NormsSubcmd::Remove { id } => cmd_norms::cmd_norms_remove(&id),
        },
        Command::Spectroscopy { path, scrape } => {
            cmd_spectroscopy::cmd_spectroscopy(path.as_deref(), scrape);
        }
        Command::Evolve { from, delta, output, scrape_gaps, api_key, model, ollama, ollama_url } => {
            cmd_evolve::cmd_evolve(&from, &delta, &output, scrape_gaps, api_key, &model, ollama, &ollama_url);
        }
        Command::Metrics { compare, last } => {
            if compare {
                cmd_metrics::cmd_metrics_compare();
            } else if let Some(n) = last {
                cmd_metrics::cmd_metrics_last(n);
            } else {
                cmd_metrics::cmd_metrics_summary();
            }
        }
        Command::Serve { port, api_key, ollama, ollama_url, ollama_model } => {
            cmd_serve(port, api_key, ollama, ollama_url, ollama_model)
        }
        Command::Help => print_help(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_parse_help() {
        let cmd = parse_args(&args(&["isls"]));
        assert!(matches!(cmd, Command::Help));
    }

    #[test]
    fn test_parse_forge_v2_defaults() {
        let cmd = parse_args(&args(&["isls", "forge-v2"]));
        match cmd {
            Command::ForgeV2 { requirements, output, mock_oracle, model, ollama, ollama_url, .. } => {
                assert_eq!(requirements, "examples/warehouse.toml");
                assert_eq!(output, "./output-v2");
                assert!(!mock_oracle);
                assert_eq!(model, "gpt-4o");
                assert!(!ollama);
                assert_eq!(ollama_url, "http://localhost:11434");
            }
            _ => panic!("expected ForgeV2"),
        }
    }

    #[test]
    fn test_parse_forge_v2_mock() {
        let cmd = parse_args(&args(&["isls", "forge-v2", "--mock-oracle", "--output", "/tmp/out"]));
        match cmd {
            Command::ForgeV2 { mock_oracle, output, .. } => {
                assert!(mock_oracle);
                assert_eq!(output, "/tmp/out");
            }
            _ => panic!("expected ForgeV2"),
        }
    }

    #[test]
    fn test_parse_forge_v2_ollama() {
        let cmd = parse_args(&args(&["isls", "forge-v2", "--ollama", "--model", "mistral:7b", "--ollama-url", "http://myhost:11434"]));
        match cmd {
            Command::ForgeV2 { ollama, model, ollama_url, .. } => {
                assert!(ollama);
                assert_eq!(model, "mistral:7b");
                assert_eq!(ollama_url, "http://myhost:11434");
            }
            _ => panic!("expected ForgeV2"),
        }
    }

    #[test]
    fn test_parse_serve() {
        let cmd = parse_args(&args(&["isls", "serve", "--port", "9000"]));
        match cmd {
            Command::Serve { port, .. } => assert_eq!(port, 9000),
            _ => panic!("expected Serve"),
        }
    }

    #[test]
    fn test_parse_serve_default_port() {
        let cmd = parse_args(&args(&["isls", "serve"]));
        match cmd {
            Command::Serve { port, .. } => assert_eq!(port, 8420),
            _ => panic!("expected Serve"),
        }
    }

    #[test]
    fn test_parse_forge_chat() {
        let cmd = parse_args(&args(&[
            "isls", "forge-chat",
            "--message", "Restaurant with reservations",
            "--output", "/tmp/restaurant",
        ]));
        match cmd {
            Command::ForgeChat { message, output, model, ollama, .. } => {
                assert_eq!(message, "Restaurant with reservations");
                assert_eq!(output, "/tmp/restaurant");
                assert_eq!(model, "gpt-4o");
                assert!(!ollama);
            }
            _ => panic!("expected ForgeChat"),
        }
    }

    #[test]
    fn test_parse_forge_chat_short_flag() {
        let cmd = parse_args(&args(&[
            "isls", "forge-chat",
            "-m", "Library management system",
            "--output", "/tmp/lib",
            "--model", "gpt-4o-mini",
        ]));
        match cmd {
            Command::ForgeChat { message, output, model, .. } => {
                assert_eq!(message, "Library management system");
                assert_eq!(output, "/tmp/lib");
                assert_eq!(model, "gpt-4o-mini");
            }
            _ => panic!("expected ForgeChat"),
        }
    }

    #[test]
    fn test_parse_forge_chat_ollama() {
        let cmd = parse_args(&args(&[
            "isls", "forge-chat",
            "-m", "CLI tool for CSV conversion",
            "--ollama",
            "--model", "codellama:7b",
        ]));
        match cmd {
            Command::ForgeChat { message, ollama, model, .. } => {
                assert_eq!(message, "CLI tool for CSV conversion");
                assert!(ollama);
                assert_eq!(model, "codellama:7b");
            }
            _ => panic!("expected ForgeChat"),
        }
    }

    #[test]
    fn test_parse_forge_self_defaults() {
        let cmd = parse_args(&args(&["isls", "forge-self"]));
        match cmd {
            Command::ForgeSelf { output, mock_oracle, model, ollama, .. } => {
                assert_eq!(output, "./output/isls-studio");
                assert!(!mock_oracle);
                assert_eq!(model, "gpt-4o");
                assert!(!ollama);
            }
            _ => panic!("expected ForgeSelf"),
        }
    }

    #[test]
    fn test_parse_forge_self_mock() {
        let cmd = parse_args(&args(&["isls", "forge-self", "--mock-oracle", "--output", "/tmp/studio"]));
        match cmd {
            Command::ForgeSelf { output, mock_oracle, .. } => {
                assert!(mock_oracle);
                assert_eq!(output, "/tmp/studio");
            }
            _ => panic!("expected ForgeSelf"),
        }
    }

    #[test]
    fn test_parse_norms_list() {
        let cmd = parse_args(&args(&["isls", "norms"]));
        match cmd {
            Command::Norms { subcmd: NormsSubcmd::List { auto_only, source: _ } } => {
                assert!(!auto_only);
            }
            _ => panic!("expected Norms List"),
        }
    }

    #[test]
    fn test_parse_norms_list_auto_only() {
        let cmd = parse_args(&args(&["isls", "norms", "list", "--auto-only"]));
        match cmd {
            Command::Norms { subcmd: NormsSubcmd::List { auto_only, source: _ } } => {
                assert!(auto_only);
            }
            _ => panic!("expected Norms List with auto_only"),
        }
    }

    #[test]
    fn test_parse_norms_stats() {
        let cmd = parse_args(&args(&["isls", "norms", "stats"]));
        assert!(matches!(cmd, Command::Norms { subcmd: NormsSubcmd::Stats }));
    }

    #[test]
    fn test_parse_norms_candidates() {
        let cmd = parse_args(&args(&["isls", "norms", "candidates"]));
        assert!(matches!(cmd, Command::Norms { subcmd: NormsSubcmd::Candidates }));
    }

    #[test]
    fn test_parse_norms_inspect() {
        let cmd = parse_args(&args(&["isls", "norms", "inspect", "ISLS-NORM-0042"]));
        match cmd {
            Command::Norms { subcmd: NormsSubcmd::Inspect { norm_id } } => {
                assert_eq!(norm_id, "ISLS-NORM-0042");
            }
            _ => panic!("expected Norms Inspect"),
        }
    }

    #[test]
    fn test_parse_forge_v2_swarm() {
        let cmd = parse_args(&args(&[
            "isls", "forge-v2", "--swarm", "--swarm-size", "6",
            "--swarm-threshold", "0.30", "--mock-oracle",
        ]));
        match cmd {
            Command::ForgeV2 { swarm, swarm_size, swarm_threshold, mock_oracle, .. } => {
                assert!(swarm);
                assert_eq!(swarm_size, 6);
                assert!((swarm_threshold - 0.30).abs() < 0.001);
                assert!(mock_oracle);
            }
            _ => panic!("expected ForgeV2"),
        }
    }

    #[test]
    fn test_parse_forge_v2_swarm_defaults() {
        let cmd = parse_args(&args(&["isls", "forge-v2", "--swarm"]));
        match cmd {
            Command::ForgeV2 { swarm, swarm_size, swarm_threshold, .. } => {
                assert!(swarm);
                assert_eq!(swarm_size, 4);
                assert!((swarm_threshold - 0.20).abs() < 0.001);
            }
            _ => panic!("expected ForgeV2"),
        }
    }

    #[test]
    fn test_parse_forge_chat_swarm_ollama() {
        let cmd = parse_args(&args(&[
            "isls", "forge-chat", "-m", "Pet shop", "--swarm", "--ollama",
        ]));
        match cmd {
            Command::ForgeChat { swarm, ollama, message, .. } => {
                assert!(swarm);
                assert!(ollama);
                assert_eq!(message, "Pet shop");
            }
            _ => panic!("expected ForgeChat"),
        }
    }

    #[test]
    fn test_parse_forge_v2_no_swarm_by_default() {
        let cmd = parse_args(&args(&["isls", "forge-v2"]));
        match cmd {
            Command::ForgeV2 { swarm, .. } => {
                assert!(!swarm, "swarm should be false by default");
            }
            _ => panic!("expected ForgeV2"),
        }
    }
}
