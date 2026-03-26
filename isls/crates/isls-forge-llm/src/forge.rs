// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Core LLM forge engine for ISLS v3.1.
//!
//! `LlmForge` orchestrates the sequential, layer-by-layer generation of a
//! complete full-stack application.  Each file's prompt includes ALL types
//! generated in previous files (the growing `TypeContext`), eliminating
//! hallucinated field names.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use isls_renderloop::{estimate_tokens, Oracle};
use isls_renderloop::type_context::TypeContext;
use regex::Regex;
use thiserror::Error;

use crate::{
    AppSpec, EntityDef, FileSpec, ForgePlan, ForgeStats, GeneratedFile, GenerationMethod,
    mock, order, prompt, static_files,
};

// ─── Error ────────────────────────────────────────────────────────────────────

/// Errors produced by the forge engine.
#[derive(Debug, Error)]
pub enum ForgeLlmError {
    /// Oracle call failed.
    #[error("oracle error: {0}")]
    Oracle(String),
    /// IO error reading or writing a file.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Final compilation check failed.
    #[error("final compile check failed: {0}")]
    CompileCheck(String),
    /// Generic forge failure.
    #[error("forge failed: {0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, ForgeLlmError>;

// ─── LlmForge ────────────────────────────────────────────────────────────────

/// The ISLS v3.1 LLM-driven code generation engine.
///
/// Generates a complete full-stack application file by file, in strict
/// dependency order (Layers 0-9).  After each successful Rust file the
/// `TypeContext` is updated so subsequent prompts include the exact types.
pub struct LlmForge {
    /// LLM oracle (OpenAI in production, MockOracle for offline runs).
    oracle: Box<dyn Oracle>,
    /// Growing type context — updated after every generated file.
    type_context: TypeContext,
    /// The forge plan (AppSpec + norm IDs).
    plan: ForgePlan,
    /// Root output directory.
    output_dir: PathBuf,
    /// All files generated so far.
    generated_files: Vec<GeneratedFile>,
    /// Accumulated statistics.
    pub stats: ForgeStats,
    /// Use mock generators instead of LLM calls.
    mock_mode: bool,
}

impl LlmForge {
    /// Create a new forge engine.
    pub fn new(
        oracle: Box<dyn Oracle>,
        plan: ForgePlan,
        output_dir: PathBuf,
        mock_mode: bool,
    ) -> Self {
        LlmForge {
            oracle,
            type_context: TypeContext::default(),
            plan,
            output_dir,
            generated_files: Vec::new(),
            stats: ForgeStats::default(),
            mock_mode,
        }
    }

    /// Generate the entire application, file by file, in dependency order.
    ///
    /// Returns the list of all generated files on success.
    pub fn generate(&mut self) -> Result<Vec<GeneratedFile>> {
        let start = Instant::now();
        tracing::info!(
            app = %self.plan.spec.app_name,
            mock = self.mock_mode,
            "forge v3.1 generation starting"
        );

        // Layer 0: static files (no LLM, no TypeContext update)
        self.generate_static_files()?;

        // Layers 1-9: LLM or mock generation
        let file_specs = order::generation_order(&self.plan);
        let total = file_specs.len();

        for (i, spec) in file_specs.iter().enumerate() {
            tracing::info!(
                layer = spec.layer,
                path = %spec.path,
                progress = format!("{}/{}", i + 1, total),
                "generating file"
            );

            let generated = if self.mock_mode {
                self.generate_mock(&spec)?
            } else if is_structural_file(&spec.path) {
                // Structural files (mod.rs, main.rs) are deterministic —
                // generate statically even in LLM mode to eliminate an
                // entire class of module-visibility and naming errors.
                self.generate_static_structural(&spec)?
            } else {
                self.generate_llm(&spec)?
            };

            // Update TypeContext after each Rust file
            if spec.is_rust {
                self.type_context.add_file(&spec.path, &generated.content);
            }

            self.stats.files_generated += 1;
            self.stats.total_tokens += generated.tokens_used;
            self.generated_files.push(generated);
        }

        // Final compile check (LLM mode only).
        // Mock mode skips cargo check — mock generators are deterministic and
        // validated by unit tests.  LLM mode runs a single cargo check after
        // ALL files exist so the full module tree is available.
        if !self.mock_mode {
            self.final_check_and_fix(&file_specs)?;
        }

        self.stats.total_time_secs = start.elapsed().as_secs_f64();
        tracing::info!(
            files = self.stats.files_generated,
            tokens = self.stats.total_tokens,
            secs = self.stats.total_time_secs,
            "forge v3.1 generation complete"
        );

        Ok(self.generated_files.clone())
    }

    // ── Layer 0: Static files ─────────────────────────────────────────────────

    fn generate_static_files(&mut self) -> Result<()> {
        let spec = &self.plan.spec;

        self.write_file(
            "backend/Cargo.toml",
            &static_files::generate_cargo_toml(spec),
        )?;
        self.write_file(
            "docker-compose.yml",
            &static_files::generate_docker_compose(spec),
        )?;
        self.write_file("backend/Dockerfile", &static_files::generate_dockerfile(spec))?;
        self.write_file(".env.example", &static_files::generate_env_example(spec))?;
        self.write_file(".gitignore", static_files::GITIGNORE_TEMPLATE)?;
        self.write_file("frontend/nginx.conf", static_files::NGINX_CONF)?;

        tracing::info!("layer 0 static files written");
        Ok(())
    }

    // ── LLM generation ───────────────────────────────────────────────────────

    /// Generate a file using the LLM oracle (no per-file cargo check).
    ///
    /// Compile checking happens after ALL files are generated via
    /// [`final_check_and_fix`], because per-file checks fail on early files
    /// that reference modules not yet generated (e.g. main.rs doesn't exist
    /// until Layer 7).
    fn generate_llm(&mut self, spec: &FileSpec) -> Result<GeneratedFile> {
        let prompt_text = prompt::build_prompt(spec, &self.type_context, &self.plan);

        let response = self
            .oracle
            .call(&prompt_text, 4096)
            .map_err(|e| ForgeLlmError::Oracle(e.to_string()))?;

        let code = response.trim().to_string();
        if code.is_empty() {
            return Err(ForgeLlmError::Failed(format!(
                "oracle returned empty response for {}",
                spec.path
            )));
        }

        self.write_file(&spec.path, &code)?;

        let tokens = estimate_tokens(&prompt_text) + estimate_tokens(&code);
        Ok(GeneratedFile {
            path: spec.path.clone(),
            content: code,
            generation_method: "llm".into(),
            attempts: 1,
            tokens_used: tokens,
        })
    }

    // ── Mock generation ───────────────────────────────────────────────────────

    /// Generate a file using the mock generators (no LLM call).
    fn generate_mock(&mut self, spec: &FileSpec) -> Result<GeneratedFile> {
        let content = dispatch_mock(spec, &self.plan.spec);
        self.write_file(&spec.path, &content)?;
        Ok(GeneratedFile {
            path: spec.path.clone(),
            content,
            generation_method: "mock".into(),
            attempts: 1,
            tokens_used: 0,
        })
    }

    // ── Static structural generation ──────────────────────────────────────────

    /// Generate a structural file (mod.rs, main.rs) statically — no LLM call.
    fn generate_static_structural(&mut self, spec: &FileSpec) -> Result<GeneratedFile> {
        let content = generate_structural(spec, &self.plan.spec);
        self.write_file(&spec.path, &content)?;
        tracing::info!(path = %spec.path, "generated structural file statically");
        Ok(GeneratedFile {
            path: spec.path.clone(),
            content,
            generation_method: "static".into(),
            attempts: 1,
            tokens_used: 0,
        })
    }

    // ── cargo check ───────────────────────────────────────────────────────────

    /// Run `cargo check` in the generated backend directory.
    ///
    /// Returns `Ok(())` if compilation succeeds, or `Err(error_lines)` with
    /// the relevant stderr output for LLM retry feedback.
    fn cargo_check(&self) -> std::result::Result<(), String> {
        let backend_dir = self.output_dir.join("backend");
        if !backend_dir.exists() {
            return Err("backend directory does not exist".into());
        }

        let output = std::process::Command::new("cargo")
            .args(["check", "--message-format=short"])
            .current_dir(&backend_dir)
            .output()
            .map_err(|e| format!("cargo check failed to run: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Trim to the most relevant error lines (max 50 lines)
            let relevant: Vec<&str> = stderr
                .lines()
                .filter(|l| {
                    l.contains("error") || l.contains("^") || l.contains("note:")
                })
                .take(50)
                .collect();
            Err(if relevant.is_empty() {
                stderr.trim().chars().take(2000).collect()
            } else {
                relevant.join("\n")
            })
        }
    }

    /// Run a final `cargo check` on the complete generated backend.
    #[allow(dead_code)]
    fn final_compile_check(&self) -> Result<()> {
        self.stats_note("running final compile check");
        self.cargo_check().map_err(ForgeLlmError::CompileCheck)
    }

    /// Run cargo check on the complete project.  On failure, identify files
    /// with errors, regenerate them with the error context, and retry.
    /// Up to 3 rounds.
    ///
    /// The fix prompt includes:
    /// - The actual file content read from disk
    /// - Per-file compiler errors (not the entire cargo output)
    /// - The real module map extracted from generated mod.rs files
    /// - The full type context
    fn final_check_and_fix(&mut self, file_specs: &[FileSpec]) -> Result<()> {
        for round in 1u32..=3 {
            self.stats.compile_checks += 1;
            match self.cargo_check() {
                Ok(()) => {
                    tracing::info!(round, "final compile check passed");
                    return Ok(());
                }
                Err(errors) => {
                    self.stats.compile_failures += 1;
                    tracing::warn!(
                        round,
                        error_lines = errors.lines().count(),
                        "compile errors in final check"
                    );
                    if round == 3 {
                        tracing::error!(
                            "final compile check failed after 3 rounds — keeping output"
                        );
                        return Ok(());
                    }

                    // Parse errors grouped by file
                    let errors_by_file = parse_errors_by_file(&errors);
                    if errors_by_file.is_empty() {
                        tracing::error!(
                            "could not identify error files from cargo output"
                        );
                        return Ok(());
                    }

                    // Read the actual module map from disk
                    let module_map = self.read_module_map();
                    let type_ctx_str = self.type_context.to_prompt_string_full();

                    tracing::info!(
                        round,
                        files = errors_by_file.len(),
                        "regenerating files with compile errors"
                    );

                    for (error_path, file_errors) in &errors_by_file {
                        // Skip structural files — they are deterministic
                        let full_error_path = format!("backend/{}", error_path);
                        if is_structural_file(&full_error_path) {
                            tracing::debug!(
                                path = %full_error_path,
                                "skipping structural file in fix loop"
                            );
                            continue;
                        }

                        if let Some(spec) =
                            file_specs.iter().find(|s| s.path == full_error_path)
                        {
                            self.stats.retries += 1;

                            // Read current file content from disk
                            let disk_path =
                                self.output_dir.join(&spec.path);
                            let current_content =
                                std::fs::read_to_string(&disk_path)
                                    .unwrap_or_default();

                            // Format per-file errors
                            let errors_text: String = file_errors
                                .iter()
                                .map(|e| {
                                    format!("Line {}: {}", e.line, e.message)
                                })
                                .collect::<Vec<_>>()
                                .join("\n");

                            let fix_prompt =
                                prompt::build_context_fix_prompt(
                                    error_path,
                                    &current_content,
                                    &errors_text,
                                    &module_map,
                                    &type_ctx_str,
                                );

                            match self.oracle.call(&fix_prompt, 4096) {
                                Ok(response) => {
                                    let code = response.trim().to_string();
                                    if !code.is_empty() {
                                        let _ =
                                            self.write_file(&spec.path, &code);
                                        if spec.is_rust {
                                            self.type_context.add_file(
                                                &spec.path, &code,
                                            );
                                        }
                                        tracing::info!(
                                            path = %spec.path,
                                            round,
                                            errors = file_errors.len(),
                                            "regenerated file with context fix"
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        path = %spec.path,
                                        error = %e,
                                        "failed to regenerate file"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Read the actual module map from generated files on disk.
    ///
    /// After all files are generated, the mod.rs and main.rs files exist on
    /// disk with the real module declarations. This reads them to build a
    /// ground-truth module map for the fix prompt.
    fn read_module_map(&self) -> String {
        let backend_src = self.output_dir.join("backend/src");
        let mut map = String::new();

        // Read main.rs for top-level mod declarations
        if let Ok(main_rs) = std::fs::read_to_string(backend_src.join("main.rs")) {
            map.push_str("## main.rs declares:\n");
            for line in main_rs.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("mod ") || trimmed.starts_with("pub mod ") {
                    map.push_str(&format!("  {}\n", trimmed));
                }
            }
        }

        // Read each mod.rs for submodule declarations
        for dir in &["models", "database", "api", "services"] {
            let mod_rs = backend_src.join(format!("{}/mod.rs", dir));
            if let Ok(content) = std::fs::read_to_string(&mod_rs) {
                map.push_str(&format!("\n## {}/mod.rs declares:\n", dir));
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.contains("mod ") || trimmed.contains("pub use") {
                        map.push_str(&format!("  {}\n", trimmed));
                    }
                }
            }
        }

        map.push_str("\n## IMPORT RULES:\n");
        map.push_str("  - AppError is in crate::errors::AppError\n");
        map.push_str("  - AuthUser is in crate::auth::AuthUser\n");
        map.push_str("  - PaginationParams is in crate::pagination::PaginationParams\n");
        map.push_str("  - PaginatedResponse is in crate::pagination::PaginatedResponse\n");
        map.push_str("  - AppConfig is in crate::config::AppConfig\n");
        map.push_str("  - For models: use crate::models::{entity}::{Type}\n");
        map.push_str("  - For queries: use crate::database::{entity}_queries\n");
        map.push_str("  - For services: use crate::services::{entity}\n");

        map
    }

    fn stats_note(&self, msg: &str) {
        tracing::info!("{}", msg);
    }

    // ── File I/O ──────────────────────────────────────────────────────────────

    /// Write content to a path relative to `output_dir`, creating dirs as needed.
    fn write_file(&self, rel_path: &str, content: &str) -> Result<()> {
        let full_path = self.output_dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full_path, content)?;
        tracing::debug!(path = %rel_path, bytes = content.len(), "wrote file");
        Ok(())
    }
}

// ─── Mock dispatch ────────────────────────────────────────────────────────────

/// Dispatch mock generation based on the file spec path.
fn dispatch_mock(spec: &FileSpec, app_spec: &AppSpec) -> String {
    let path = spec.path.as_str();
    let entity_name = spec.entity.as_deref().unwrap_or("");

    if path.ends_with("errors.rs") {
        return mock::mock_generate_errors();
    }
    if path.ends_with("config.rs") {
        return mock::mock_generate_config(&app_spec.app_name);
    }
    if path.ends_with("pagination.rs") {
        return mock::mock_generate_pagination();
    }
    if path.ends_with("auth.rs") && !path.contains("auth_routes") {
        return mock::mock_generate_auth();
    }
    if path.ends_with("auth_routes.rs") {
        return mock::mock_generate_auth_routes();
    }
    if path.ends_with("pool.rs") {
        return mock::mock_generate_pool();
    }
    if path.ends_with("main.rs") {
        return mock::mock_generate_main(app_spec);
    }
    if path.ends_with("001_initial.sql") {
        let non_user: Vec<EntityDef> = app_spec
            .entities
            .iter()
            .filter(|e| e.name != "User")
            .cloned()
            .collect();
        return mock::mock_generate_migrations(&non_user);
    }
    if path.ends_with("index.html") {
        return mock::mock_generate_frontend_index(app_spec);
    }
    if path.ends_with("style.css") {
        return mock::mock_generate_style_css();
    }
    if path.ends_with("client.js") {
        return mock::mock_generate_api_client();
    }
    if path.ends_with("api_tests.rs") {
        return mock::mock_generate_integration_tests(app_spec);
    }

    // mod.rs files — generate based on path
    if path.ends_with("mod.rs") {
        return generate_mod_rs(path, app_spec);
    }

    // Entity-specific files
    if let Some(entity) = app_spec.entities.iter().find(|e| e.name == entity_name) {
        if path.contains("models/") && entity.name == "User" {
            return mock::mock_generate_user_model();
        }
        if path.contains("models/") {
            return mock::mock_generate_model(entity);
        }
        if path.contains("_queries.rs") && entity.name == "User" {
            return mock::mock_generate_user_queries();
        }
        if path.contains("_queries.rs") {
            return mock::mock_generate_queries(entity);
        }
        if path.contains("services/") {
            return mock::mock_generate_service(entity);
        }
        if path.contains("api/") && !path.ends_with("mod.rs") {
            return mock::mock_generate_api(entity);
        }
        if path.contains("pages/") {
            return mock::mock_generate_entity_page(entity);
        }
    }

    // Fallback: empty placeholder
    tracing::warn!(path = %spec.path, "no mock generator matched — using placeholder");
    format!("// ISLS v3.1 mock generated — placeholder for {}\n", spec.path)
}

/// Generate a `mod.rs` file listing the appropriate submodules.
fn generate_mod_rs(path: &str, spec: &AppSpec) -> String {
    let non_user: Vec<&EntityDef> = spec.entities.iter().filter(|e| e.name != "User").collect();
    let entity_snakes: Vec<&str> = non_user.iter().map(|e| e.snake_name.as_str()).collect();
    let all_snakes: Vec<&str> = spec.entities.iter().map(|e| e.snake_name.as_str()).collect();

    if path.contains("models/mod.rs") {
        let mut code = "// ISLS v3.1 mock generated\n".to_string();
        for snake in &all_snakes {
            code.push_str(&format!("pub mod {};\n", snake));
        }
        code.push('\n');
        for snake in &all_snakes {
            code.push_str(&format!(
                "pub use {}::*;\n",
                snake
            ));
        }
        return code;
    }

    if path.contains("database/mod.rs") {
        let mut code = "// ISLS v3.1 mock generated\npub mod pool;\n".to_string();
        for snake in &all_snakes {
            code.push_str(&format!("pub mod {}_queries;\n", snake));
        }
        code.push_str("\npub use pool::create_pool;\n");
        return code;
    }

    if path.contains("services/mod.rs") {
        let mut code = "// ISLS v3.1 mock generated\n".to_string();
        for snake in &all_snakes {
            code.push_str(&format!("pub mod {};\n", snake));
        }
        return code;
    }

    if path.contains("api/mod.rs") {
        let mut code =
            "// ISLS v3.1 mock generated\npub mod auth_routes;\n".to_string();
        for snake in &entity_snakes {
            code.push_str(&format!("pub mod {};\n", snake));
        }
        code.push_str(
            r#"
use actix_web::web;

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    auth_routes::auth_routes(cfg);
"#,
        );
        for snake in &entity_snakes {
            code.push_str(&format!("    {}::{}routes(cfg);\n", snake, snake.to_string() + "_"));
        }
        code.push_str("}\n");
        return code;
    }

    "// ISLS v3.1 mock generated\n".into()
}

/// Generate a static `main.rs` from the entity list (deterministic, no LLM needed).
fn generate_main_rs(spec: &AppSpec) -> String {
    let mut s = String::new();
    s.push_str("mod api;\nmod auth;\nmod config;\nmod database;\n");
    s.push_str("mod errors;\nmod models;\nmod pagination;\nmod services;\n\n");
    s.push_str("use actix_web::{web, App, HttpServer};\n");
    s.push_str("use actix_cors::Cors;\n");
    s.push_str("use tracing_subscriber::EnvFilter;\n\n");
    s.push_str("#[actix_web::main]\n");
    s.push_str("async fn main() -> std::io::Result<()> {\n");
    s.push_str("    dotenvy::dotenv().ok();\n\n");
    s.push_str("    tracing_subscriber::fmt()\n");
    s.push_str("        .with_env_filter(EnvFilter::from_default_env())\n");
    s.push_str("        .init();\n\n");
    s.push_str("    let pool = database::pool::create_pool()\n");
    s.push_str("        .await\n");
    s.push_str("        .expect(\"failed to connect to database\");\n\n");
    s.push_str("    // Run migrations (idempotent — uses IF NOT EXISTS)\n");
    s.push_str("    let migration_sql = include_str!(\"../migrations/001_initial.sql\");\n");
    s.push_str("    sqlx::raw_sql(migration_sql)\n");
    s.push_str("        .execute(&pool)\n");
    s.push_str("        .await\n");
    s.push_str("        .expect(\"failed to run database migrations\");\n");
    s.push_str("    tracing::info!(\"database migrations applied\");\n\n");
    s.push_str(&format!(
        "    let port = std::env::var(\"PORT\").unwrap_or_else(|_| \"8080\".into());\n"
    ));
    s.push_str(&format!(
        "    tracing::info!(\"starting {} on port {{}}\", port);\n\n",
        spec.app_name
    ));
    s.push_str("    HttpServer::new(move || {\n");
    s.push_str("        App::new()\n");
    s.push_str("            .wrap(Cors::permissive())\n");
    s.push_str("            .wrap(actix_web::middleware::Logger::default())\n");
    s.push_str("            .app_data(web::Data::new(pool.clone()))\n");
    s.push_str("            .configure(api::configure_routes)\n");
    s.push_str("    })\n");
    s.push_str("    .bind(format!(\"0.0.0.0:{}\", port))?\n");
    s.push_str("    .run()\n");
    s.push_str("    .await\n");
    s.push_str("}\n");
    s
}

/// Check if a file spec is a structural file that should be generated statically.
fn is_structural_file(path: &str) -> bool {
    path.ends_with("mod.rs") || path.ends_with("main.rs")
}

/// Generate a structural file statically (mod.rs or main.rs).
fn generate_structural(spec: &FileSpec, app_spec: &AppSpec) -> String {
    let path = spec.path.as_str();
    if path.ends_with("main.rs") {
        return generate_main_rs(app_spec);
    }
    if path.ends_with("mod.rs") {
        return generate_mod_rs(path, app_spec);
    }
    unreachable!("is_structural_file should guard this")
}

// ─── Error parsing ───────────────────────────────────────────────────────────

/// A single compiler error with file location and message.
#[derive(Clone, Debug)]
struct CompilerError {
    line: usize,
    message: String,
}

/// Parse cargo check error output into structured errors grouped by file.
///
/// Handles both `--message-format=short` output and standard cargo error format.
/// Returns a map from relative file path (e.g. `"src/database/pool.rs"`) to
/// the list of errors in that file.
fn parse_errors_by_file(errors: &str) -> HashMap<String, Vec<CompilerError>> {
    let mut map: HashMap<String, Vec<CompilerError>> = HashMap::new();

    // Pattern for --message-format=short: "src/path.rs:line:col: error[E0432]: message"
    let re_short =
        Regex::new(r"^(src/[^\s:]+\.rs):(\d+):\d+:\s*error(?:\[E\d+\])?:\s*(.+)")
            .expect("regex compiles");

    // Pattern for standard cargo output: " --> src/path.rs:line:col"
    let re_location =
        Regex::new(r"-->\s+(src/[^\s:]+\.rs):(\d+):\d+")
            .expect("regex compiles");

    // Pattern for error message lines: "error[E0432]: unresolved import..."
    let re_error =
        Regex::new(r"^error(?:\[E\d+\])?:\s*(.+)")
            .expect("regex compiles");

    // First pass: short format
    for line in errors.lines() {
        if let Some(caps) = re_short.captures(line) {
            let file = caps[1].to_string();
            let line_num: usize = caps[2].parse().unwrap_or(0);
            let message = caps[3].to_string();
            map.entry(file).or_default().push(CompilerError {
                line: line_num,
                message,
            });
        }
    }

    // Second pass: standard format (error line followed by --> location)
    let mut current_error: Option<String> = None;
    for line in errors.lines() {
        if let Some(caps) = re_error.captures(line) {
            current_error = Some(caps[1].to_string());
        }
        if let Some(caps) = re_location.captures(line) {
            if let Some(ref err_msg) = current_error {
                let file = caps[1].to_string();
                let line_num: usize = caps[2].parse().unwrap_or(0);
                // Avoid duplicates from the short-format pass
                let already = map.get(&file).map_or(false, |errs| {
                    errs.iter().any(|e| e.line == line_num && e.message == *err_msg)
                });
                if !already {
                    map.entry(file).or_default().push(CompilerError {
                        line: line_num,
                        message: err_msg.clone(),
                    });
                }
                current_error = None;
            }
        }
    }

    map
}

