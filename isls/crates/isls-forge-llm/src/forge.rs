// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Core LLM forge engine for ISLS v3.1.
//!
//! `LlmForge` orchestrates the sequential, layer-by-layer generation of a
//! complete full-stack application.  Each file's prompt includes ALL types
//! generated in previous files (the growing `TypeContext`), eliminating
//! hallucinated field names.

use std::path::PathBuf;
use std::time::Instant;

use isls_renderloop::{estimate_tokens, Oracle};
use isls_renderloop::type_context::TypeContext;
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
                    // Parse error file paths and regenerate those files
                    let error_files = parse_error_files(&errors);
                    if error_files.is_empty() {
                        tracing::error!(
                            "could not identify error files from cargo output"
                        );
                        return Ok(());
                    }
                    tracing::info!(
                        round,
                        files = error_files.len(),
                        "regenerating files with compile errors"
                    );
                    for error_path in &error_files {
                        // Find the matching FileSpec (path in error is relative
                        // to backend/, e.g. "src/database/pool.rs")
                        let full_error_path = format!("backend/{}", error_path);
                        if let Some(spec) =
                            file_specs.iter().find(|s| s.path == full_error_path)
                        {
                            self.stats.retries += 1;
                            let base_prompt = prompt::build_prompt(
                                spec,
                                &self.type_context,
                                &self.plan,
                            );
                            let fix_prompt =
                                prompt::build_fix_prompt(&base_prompt, &errors);
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
                                            "regenerated file"
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

// ─── Error file parser ───────────────────────────────────────────────────────

/// Parse cargo check error output to identify source files with errors.
///
/// Looks for patterns like `src/database/pool.rs:5:1: error[E0432]` in
/// `--message-format=short` output.
fn parse_error_files(errors: &str) -> Vec<String> {
    let mut files = std::collections::HashSet::new();
    for line in errors.lines() {
        // cargo --message-format=short outputs: "path:line:col: error..."
        if let Some(colon_pos) = line.find(':') {
            let path = &line[..colon_pos];
            if path.ends_with(".rs") && !path.contains(' ') {
                files.insert(path.to_string());
            }
        }
    }
    files.into_iter().collect()
}
