// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
// isls-cli: Single-binary operator interface (C11)
// D1 Clean Architecture — forge-v2 + serve + help

use std::path::Path;

// ─── Command Enum ────────────────────────────────────────────────────────────

enum Command {
    /// HDAG code generation pipeline (v3.4 PCR Staged Closure).
    ForgeV2 {
        requirements: String,
        output: String,
        mock_oracle: bool,
        api_key: Option<String>,
        model: String,
    },
    /// D3: Chat-to-App — natural language to compiled application.
    ForgeChat {
        message: String,
        output: String,
        api_key: Option<String>,
        model: String,
    },
    /// Start the Gateway / Studio web interface.
    Serve { port: u16, api_key: Option<String> },
    /// Print help.
    Help,
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
            Command::ForgeV2 { requirements, output, mock_oracle, api_key, model }
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
            Command::ForgeChat { message, output, api_key, model }
        }
        "serve" => {
            let port = args.iter().position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(8420);
            let api_key = args.iter().position(|a| a == "--api-key")
                .and_then(|i| args.get(i + 1))
                .cloned();
            Command::Serve { port, api_key }
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
) {
    use isls_hypercube::{
        DimState, DimValue,
        domain::DomainRegistry,
    };
    use isls_forge_llm::{ForgePlan, forge::LlmForge};
    use isls_forge_llm::oracle::{MockOracle, OpenAiOracle};

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

    let use_mock = mock_oracle
        || (api_key.is_none() && std::env::var("OPENAI_API_KEY").is_err());

    if use_mock {
        println!("[Mode] Mock oracle — no LLM calls (compilable output)");
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

    // 5. Create oracle
    let oracle: Box<dyn isls_forge_llm::oracle::Oracle> = if use_mock {
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

    // 6. Create output dir and run LlmForge
    if let Err(e) = std::fs::create_dir_all(output_dir) {
        eprintln!("[ERROR] Cannot create output dir: {}", e);
        std::process::exit(1);
    }
    let mut forge = LlmForge::new(oracle, plan, output_dir.to_path_buf(), use_mock);

    let start = std::time::Instant::now();
    match forge.generate() {
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

// ─── forge-chat: D3 Chat-to-App ──────────────────────────────────────────────

fn cmd_forge_chat(
    message: &str,
    output: &str,
    api_key: Option<String>,
    model: &str,
) {
    use isls_forge_llm::oracle::OpenAiOracle;

    println!("╔══════════════════════════════════════════════════════╗");
    println!("║     ISLS D3 — Chat to App                            ║");
    println!("║     Natural Language → TOML → HDAG → App              ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    // Load .env if present (best-effort)
    let _ = dotenv::dotenv();

    let resolved_key = api_key.clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());

    if resolved_key.is_none() {
        eprintln!("[ERROR] --api-key or OPENAI_API_KEY required for forge-chat");
        std::process::exit(1);
    }

    // 1. Build extraction prompt
    let prompt = isls_chat::build_extraction_prompt(message);
    println!("[Chat] Extracting entities from: \"{}\"", message);

    // 2. Call LLM (single call)
    let oracle = match OpenAiOracle::new(resolved_key, Some(model.to_string())) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[ERROR] Oracle init failed: {}", e);
            std::process::exit(1);
        }
    };

    let json_str = match isls_forge_llm::Oracle::call(&oracle, &prompt, 4096) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[ERROR] LLM extraction failed: {}", e);
            std::process::exit(1);
        }
    };

    // 3. Parse JSON
    let json: serde_json::Value = match serde_json::from_str(json_str.trim()) {
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
    );
}

// ─── serve: Gateway / Studio ─────────────────────────────────────────────────

fn cmd_serve(port: u16, api_key: Option<String>) {
    // Set API key in environment if provided via --api-key flag
    if let Some(ref key) = api_key {
        std::env::set_var("OPENAI_API_KEY", key);
    }

    let has_key = api_key.is_some()
        || std::env::var("OPENAI_API_KEY").is_ok()
        || std::env::var("ANTHROPIC_API_KEY").is_ok();

    println!("ISLS Gateway v3.4 starting on port {}...", port);
    println!("Mode: {}", if has_key { "LLM generation (API key provided)" } else { "Mock mode (no API key)" });
    println!("Studio available at http://localhost:{}/studio", port);
    println!("API available at http://localhost:{}/", port);
    println!("WebSocket events at ws://localhost:{}/events", port);
    println!();

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let state = isls_gateway::AppState::new();
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        if let Err(e) = isls_gateway::serve(state, addr).await {
            eprintln!("Gateway error: {}", e);
        }
    });
}

// ─── help ────────────────────────────────────────────────────────────────────

fn print_help() {
    println!("ISLS — D3 Chat to App Architecture");
    println!();
    println!("Usage: isls <command> [options]");
    println!();
    println!("Commands:");
    println!("  forge-v2    HDAG code generation pipeline (Staged Closure)");
    println!("  forge-chat  D3: Natural language to compiled application");
    println!("  serve       Start the Gateway / Studio web interface");
    println!("  help        Print this message");
    println!();
    println!("forge-v2 options:");
    println!("  --requirements <path>  TOML requirements file (default: examples/warehouse.toml)");
    println!("  --output <path>        Output directory (default: ./output-v2)");
    println!("  --mock-oracle          Use mock oracle (no LLM calls, compilable output)");
    println!("  --api-key <key>        OpenAI API key (or set OPENAI_API_KEY env var)");
    println!("  --model <model>        LLM model name (default: gpt-4o)");
    println!();
    println!("forge-chat options:");
    println!("  --message / -m <text>  Application description in natural language (required)");
    println!("  --output <path>        Output directory (default: ./output)");
    println!("  --api-key <key>        OpenAI API key (or set OPENAI_API_KEY env var)");
    println!("  --model <model>        LLM model name (default: gpt-4o)");
    println!();
    println!("serve options:");
    println!("  --port <port>          Port number (default: 8420)");
    println!("  --api-key <key>        API key for LLM generation");
    println!();
    println!("Pipeline: forge-chat -> TOML -> forge-v2 -> cargo build -> docker-compose up");
    println!("One sentence. One app. Zero manual steps.");
}

// ─── main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = parse_args(&args);

    match cmd {
        Command::ForgeV2 { requirements, output, mock_oracle, api_key, model } => {
            cmd_forge_v2(&requirements, &output, mock_oracle, api_key, &model);
        }
        Command::ForgeChat { message, output, api_key, model } => {
            cmd_forge_chat(&message, &output, api_key, &model);
        }
        Command::Serve { port, api_key } => cmd_serve(port, api_key),
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
            Command::ForgeV2 { requirements, output, mock_oracle, model, .. } => {
                assert_eq!(requirements, "examples/warehouse.toml");
                assert_eq!(output, "./output-v2");
                assert!(!mock_oracle);
                assert_eq!(model, "gpt-4o");
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
            Command::ForgeChat { message, output, model, .. } => {
                assert_eq!(message, "Restaurant with reservations");
                assert_eq!(output, "/tmp/restaurant");
                assert_eq!(model, "gpt-4o");
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
}
