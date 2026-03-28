// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! PCR Staged Closure — ISLS v3.4 HDAG Codegen Pipeline.
//!
//! Implements the S0–S7 pipeline from the PCR §6.1 specification:
//!
//! ```text
//! S0 Ingest → S1 Canon → S2 Expand → S4 Solve → S5 Gate → S7 Emit
//!                                         ↑              ↓
//!                                     S6 Coagula ←──── fail
//! ```
//!
//! The key invariants:
//! - Layer-0 / Structural nodes are written deterministically (no LLM).
//! - LLM nodes receive EXACT symbols from predecessor HDAG edges.
//! - `cargo check` runs **once** after all files are generated (S5 Gate).
//! - S6 Coagula is the anomaly path — logged as warning if triggered.
//! - After 3 Coagula cycles: `Err(CompileCheck)` is returned (no emission).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::oracle::{estimate_tokens, Oracle};
use crate::type_context::TypeContext;
use regex::Regex;

use crate::{
    AppSpec, ForgePlan, ForgeStats, GeneratedFile,
    hdag::{CodegenHdag, HdagNode, NodeType, ProvidedSymbol, SymbolKind},
    static_files,
    structural,
};
use crate::forge::{ForgeLlmError, Result};

// ─── StagedClosure ────────────────────────────────────────────────────────────

/// PCR Staged Closure executor for ISLS v3.4.
///
/// Owns the generation state (TypeContext, generated files, statistics) and
/// drives the S0–S7 pipeline.  The oracle is passed by reference to `execute`
/// so that `LlmForge` retains ownership.
pub struct StagedClosure {
    /// The forge plan (AppSpec + norm IDs).
    pub plan: ForgePlan,
    /// Root output directory.
    pub output_dir: PathBuf,
    /// Growing type context — updated after every Rust LLM file.
    pub type_context: TypeContext,
    /// All files generated so far.
    pub generated_files: Vec<GeneratedFile>,
    /// Accumulated statistics.
    pub stats: ForgeStats,
}

impl StagedClosure {
    /// Create a new staged closure executor.
    pub fn new(plan: ForgePlan, output_dir: PathBuf) -> Self {
        StagedClosure {
            plan,
            output_dir,
            type_context: TypeContext::default(),
            generated_files: Vec::new(),
            stats: ForgeStats::default(),
        }
    }

    /// Execute the full S0–S7 pipeline.
    ///
    /// # Errors
    /// Returns `Err(CompileCheck)` if S6 Coagula exhausts 3 cycles without
    /// passing `cargo check`.  In that case **no emission** occurs.
    pub fn execute(&mut self, oracle: &dyn Oracle) -> Result<Vec<GeneratedFile>> {
        let start = Instant::now();
        let spec = self.plan.spec.clone();

        // S0: Ingest — AppSpec is already available
        eprintln!("[HDAG S0] Ingest: AppSpec ready (app={})", spec.app_name);
        tracing::info!(app = %spec.app_name, "S0 Ingest: AppSpec ready");

        // S1: Canon — entity names already canonicalized by lib.rs::to_snake_case
        eprintln!("[HDAG S1] Canon: entity names canonicalized");
        tracing::info!("S1 Canon: entity names canonicalized");

        // S2: Expand — build Codegen-HDAG deterministically from AppSpec
        let hdag = CodegenHdag::build(&spec);
        eprintln!(
            "[HDAG S2] Expand: HDAG built ({} nodes, {} edges)",
            hdag.nodes.len(),
            hdag.edges.len()
        );
        tracing::info!(
            nodes = hdag.nodes.len(),
            edges = hdag.edges.len(),
            "S2 Expand: HDAG built"
        );

        // S4: Solve — topological traversal
        eprintln!("[HDAG S4] Solve: topological traversal beginning");
        tracing::info!("S4 Solve: topological traversal beginning");
        let order = hdag.topological_sort();
        let total = order.len();

        for (i, node_idx) in order.iter().enumerate() {
            let node = hdag.nodes[*node_idx].clone();
            tracing::info!(
                layer = node.layer,
                path = %node.path,
                kind = ?node.node_type,
                progress = format!("{}/{}", i + 1, total),
                "S4 Solve: processing node"
            );

            match node.node_type {
                NodeType::Structural => {
                    eprintln!("[HDAG S4]   structural  [{}/{}] {}", i + 1, total, node.path);
                    let content = self.generate_structural_content(&node, &spec);
                    self.write_file(&node.path, &content)?;
                    self.generated_files.push(GeneratedFile {
                        path: node.path.clone(),
                        content,
                        generation_method: "structural".into(),
                        attempts: 1,
                        tokens_used: 0,
                    });
                    self.stats.files_generated += 1;
                }
                NodeType::Llm => {
                    eprintln!("[HDAG S4]   llm         [{}/{}] {}", i + 1, total, node.path);
                    let provided = hdag.provided_symbols(*node_idx);
                    let prompt = build_hdag_prompt(&node, &provided, &self.type_context, &self.plan);
                    let tokens_in = estimate_tokens(&prompt);

                    let response = oracle
                        .call(&prompt, 4096)
                        .map_err(|e| ForgeLlmError::Oracle(e.to_string()))?;
                    let code = response.trim().to_string();

                    if code.is_empty() {
                        return Err(ForgeLlmError::Failed(format!(
                            "oracle returned empty response for {}",
                            node.path
                        )));
                    }

                    let tokens_out = estimate_tokens(&code);
                    self.write_file(&node.path, &code)?;

                    if node.is_rust {
                        self.type_context.add_file(&node.path, &code);
                    }

                    self.generated_files.push(GeneratedFile {
                        path: node.path.clone(),
                        content: code,
                        generation_method: "llm".into(),
                        attempts: 1,
                        tokens_used: tokens_in + tokens_out,
                    });
                    self.stats.files_generated += 1;
                    self.stats.total_tokens += tokens_in + tokens_out;
                }
            }
        }

        eprintln!("[HDAG S4] Solve: all {} nodes traversed", self.stats.files_generated);
        tracing::info!(
            files = self.stats.files_generated,
            "S4 Solve: all nodes traversed"
        );

        // S5: Gate — single cargo check after complete generation
        eprintln!("[HDAG S5] Gate: running cargo check on complete project");
        tracing::info!("S5 Gate: running cargo check on complete project");
        self.stats.compile_checks += 1;

        match self.cargo_check() {
            Ok(()) => {
                eprintln!("[HDAG S5] Gate: passed — proceeding to S7 Emit");
                tracing::info!("S5 Gate: passed — proceeding to S7 Emit");
            }
            Err(errors) => {
                // S6: Coagula — anomaly path (MUST be logged)
                eprintln!(
                    "[HDAG S6] Coagula triggered — anomaly path ({} error lines)",
                    errors.lines().count()
                );
                tracing::warn!(
                    error_lines = errors.lines().count(),
                    "S6 Coagula triggered — anomaly path"
                );
                self.stats.compile_failures += 1;
                self.coagula(errors, &hdag, oracle)?;
            }
        }

        // S7: Emit — project directory is the output
        self.stats.total_time_secs = start.elapsed().as_secs_f64();
        eprintln!(
            "[HDAG S7] Emit: project complete ({} files, {} tokens, {:.2}s)",
            self.stats.files_generated,
            self.stats.total_tokens,
            self.stats.total_time_secs
        );
        tracing::info!(
            files = self.stats.files_generated,
            tokens = self.stats.total_tokens,
            secs = self.stats.total_time_secs,
            "S7 Emit: project complete"
        );

        Ok(self.generated_files.clone())
    }

    // ── S6 Coagula ────────────────────────────────────────────────────────────

    /// S6 Coagula: anomaly path.
    ///
    /// Groups compiler errors by file, regenerates only failing files with a
    /// fix prompt containing the error context and HDAG-derived predecessor
    /// symbols, then retries `cargo check`.  Up to 3 cycles maximum.
    ///
    /// After 3 cycles without a clean compile: returns `Err(CompileCheck)`.
    /// **No emission occurs on failure.**
    fn coagula(
        &mut self,
        initial_errors: String,
        hdag: &CodegenHdag,
        oracle: &dyn Oracle,
    ) -> Result<()> {
        let mut current_errors = initial_errors;

        for cycle in 1u32..=3 {
            eprintln!("[HDAG S6] Coagula: cycle {}/3", cycle);
            tracing::warn!(cycle, "S6 Coagula: cycle {}/3", cycle);

            let parsed = parse_errors_by_file(&current_errors);
            let errors_by_file = if !parsed.is_empty() {
                parsed
            } else {
                tracing::warn!(
                    cycle,
                    "S6 Coagula: could not parse errors by file — using full stderr as fallback"
                );
                // Graceful degradation: log the raw output and skip per-file regeneration.
                // The cycle will re-run cargo check, which may surface cleaner output next time.
                let mut fallback = HashMap::new();
                fallback.insert(
                    "unknown".to_string(),
                    vec![CompilerError { line: 0, message: current_errors.clone() }],
                );
                fallback
            };

            // Read module map from generated files on disk
            let module_map = self.read_module_map();
            let type_ctx_str = self.type_context.to_prompt_string_full();

            tracing::info!(
                cycle,
                files = errors_by_file.len(),
                "S6 Coagula: regenerating files with compile errors"
            );

            for (error_path, file_errors) in &errors_by_file {
                let full_path = format!("backend/{}", error_path);

                // Skip structural files — they are deterministic; errors there
                // indicate a bug in the generator, not in LLM output.
                let node = hdag.nodes.iter().find(|n| n.path == full_path);
                if let Some(n) = node {
                    if n.node_type == NodeType::Structural {
                        tracing::debug!(
                            path = %full_path,
                            "S6 Coagula: skipping structural file"
                        );
                        continue;
                    }

                    self.stats.retries += 1;

                    // Read current file content from disk
                    let disk_path = self.output_dir.join(&full_path);
                    let current_content =
                        std::fs::read_to_string(&disk_path).unwrap_or_default();

                    // Collect provided symbols for this node from HDAG
                    let provided = hdag.provided_symbols(n.index);

                    // Format per-file errors
                    let errors_text: String = file_errors
                        .iter()
                        .map(|e| format!("Line {}: {}", e.line, e.message))
                        .collect::<Vec<_>>()
                        .join("\n");

                    let fix_prompt = build_coagula_fix_prompt(
                        &full_path,
                        &current_content,
                        &errors_text,
                        &provided,
                        &module_map,
                        &type_ctx_str,
                    );

                    let tokens_in = estimate_tokens(&fix_prompt);
                    match oracle.call(&fix_prompt, 4096) {
                        Ok(response) => {
                            let code = response.trim().to_string();
                            if !code.is_empty() {
                                let _ = self.write_file(&full_path, &code);
                                if n.is_rust {
                                    self.type_context.add_file(&full_path, &code);
                                }
                                // Update generated file record
                                if let Some(gf) = self.generated_files.iter_mut().find(|f| f.path == full_path) {
                                    let tokens_out = estimate_tokens(&code);
                                    gf.content = code;
                                    gf.attempts += 1;
                                    gf.tokens_used += tokens_in + tokens_out;
                                    self.stats.total_tokens += tokens_in + tokens_out;
                                }
                                tracing::info!(
                                    path = %full_path,
                                    cycle,
                                    "S6 Coagula: regenerated file"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %full_path,
                                error = %e,
                                "S6 Coagula: oracle call failed for fix"
                            );
                        }
                    }
                }
            }

            // Retry cargo check after this coagula cycle
            self.stats.compile_checks += 1;
            match self.cargo_check() {
                Ok(()) => {
                    tracing::info!(cycle, "S6 Coagula: cargo check passed after cycle {}", cycle);
                    return Ok(());
                }
                Err(new_errors) => {
                    self.stats.compile_failures += 1;
                    tracing::warn!(
                        cycle,
                        error_lines = new_errors.lines().count(),
                        "S6 Coagula: still failing after cycle {}", cycle
                    );
                    current_errors = new_errors;
                    if cycle == 3 {
                        // Spec constraint #9: no emission after 3 cycles
                        tracing::error!(
                            "S6 Coagula: 3 cycles exhausted — no emission"
                        );
                        return Err(ForgeLlmError::CompileCheck(format!(
                            "S6 Coagula exhausted 3 cycles without passing cargo check.\nFinal errors:\n{}",
                            current_errors
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    // ── Structural content dispatch ───────────────────────────────────────────

    /// Generate content for a structural node, dispatching to the appropriate
    /// generator in `structural.rs` or `static_files.rs`.
    fn generate_structural_content(&self, node: &HdagNode, spec: &AppSpec) -> String {
        match node.path.as_str() {
            p if p.ends_with("backend/Cargo.toml") || (p.contains("Cargo.toml") && !p.contains("src")) => {
                static_files::generate_cargo_toml(spec)
            }
            p if p.ends_with("docker-compose.yml") => {
                static_files::generate_docker_compose(spec)
            }
            p if p.ends_with("Dockerfile") => {
                static_files::generate_dockerfile(spec)
            }
            p if p.ends_with(".env.example") => {
                static_files::generate_env_example(spec)
            }
            ".gitignore" => {
                static_files::GITIGNORE_TEMPLATE.to_string()
            }
            p if p.ends_with("nginx.conf") => {
                static_files::NGINX_CONF.to_string()
            }
            other => structural::generate_for_path(other, spec),
        }
    }

    // ── cargo check ──────────────────────────────────────────────────────────

    /// Run `cargo check --message-format=short` in the generated backend directory.
    fn cargo_check(&self) -> std::result::Result<(), String> {
        let backend_dir = self.output_dir.join("backend");
        if !backend_dir.exists() {
            return Err("backend directory does not exist".into());
        }

        let output = std::process::Command::new("cargo")
            .args(["check", "--message-format=short"])
            .current_dir(&backend_dir)
            .output()
            .map_err(|e| format!("cargo check failed to spawn: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let relevant: Vec<&str> = stderr
                .lines()
                .filter(|l| l.contains("error") || l.contains("^") || l.contains("note:"))
                .take(50)
                .collect();
            Err(if relevant.is_empty() {
                stderr.trim().chars().take(2000).collect()
            } else {
                relevant.join("\n")
            })
        }
    }

    // ── Module map ───────────────────────────────────────────────────────────

    /// Read the ground-truth module map from generated mod.rs files on disk.
    fn read_module_map(&self) -> String {
        let backend_src = self.output_dir.join("backend/src");
        let mut map = String::new();

        if let Ok(main_rs) = std::fs::read_to_string(backend_src.join("main.rs")) {
            map.push_str("## main.rs declares:\n");
            for line in main_rs.lines() {
                let t = line.trim();
                if t.starts_with("mod ") || t.starts_with("pub mod ") {
                    map.push_str(&format!("  {}\n", t));
                }
            }
        }

        for dir in &["models", "database", "api", "services"] {
            let mod_rs = backend_src.join(format!("{}/mod.rs", dir));
            if let Ok(content) = std::fs::read_to_string(&mod_rs) {
                map.push_str(&format!("\n## {}/mod.rs declares:\n", dir));
                for line in content.lines() {
                    let t = line.trim();
                    if t.contains("mod ") || t.contains("pub use") {
                        map.push_str(&format!("  {}\n", t));
                    }
                }
            }
        }

        map.push_str("\n## IMPORT RULES:\n");
        map.push_str("  - AppError: use crate::errors::AppError\n");
        map.push_str("  - AuthUser: use crate::auth::AuthUser\n");
        map.push_str("  - PaginationParams: use crate::pagination::PaginationParams\n");
        map.push_str("  - PaginatedResponse: use crate::pagination::PaginatedResponse\n");
        map.push_str("  - AppConfig: use crate::config::AppConfig\n");
        map.push_str("  - Models: use crate::models::{entity}::{Type}\n");
        map.push_str("  - Queries: use crate::database::{entity}_queries\n");
        map.push_str("  - Services: use crate::services::{entity}\n");
        map
    }

    // ── File I/O ─────────────────────────────────────────────────────────────

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

// ─── Prompt builders ──────────────────────────────────────────────────────────

/// Build the HDAG-aware LLM prompt for a node.
///
/// The prompt contains EXACTLY the symbols from predecessor edges — no guessing.
/// This is the core innovation of the HDAG pipeline vs the previous approach.
fn build_hdag_prompt(
    node: &HdagNode,
    provided: &[ProvidedSymbol],
    type_context: &TypeContext,
    plan: &ForgePlan,
) -> String {
    let mut p = String::new();

    p.push_str(&format!("Generate `{}`.\n\n", node.path));
    p.push_str(&format!("{}\n\n", node.purpose));
    p.push_str(&format!(
        "Application: {} — {}\n\n",
        plan.spec.app_name, plan.spec.description
    ));

    // EXACT imports from HDAG predecessor edges
    if !provided.is_empty() {
        p.push_str("## AVAILABLE IMPORTS (use ONLY these):\n\n");
        for sym in provided {
            p.push_str(&format!("use {};  // {}\n", sym.import_path, sym.kind));
        }
        p.push('\n');
    }

    // Always-available crate imports
    p.push_str("## CRATE IMPORTS (always available — include what you need):\n");
    p.push_str("use actix_web::{web, HttpRequest, HttpResponse, Responder};\n");
    p.push_str("use actix_web::FromRequest;\n");
    p.push_str("use sqlx::PgPool;\n");
    p.push_str("use sqlx::FromRow;\n");
    p.push_str("use sqlx::Row;  // for .get() on query results\n");
    p.push_str("use serde::{Serialize, Deserialize};\n");
    p.push_str("use serde_json::json;\n");
    p.push_str("use chrono::{DateTime, Utc};\n");
    p.push_str("use tracing::{info, warn, error};\n");
    p.push_str("use bcrypt::{hash, verify, DEFAULT_COST};\n");
    p.push_str("use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};\n");
    p.push_str("use std::env;\n");
    p.push('\n');

    // Full type signatures from predecessor symbols
    let sigs: Vec<&ProvidedSymbol> = provided.iter().filter(|s| !s.signature.is_empty()).collect();
    if !sigs.is_empty() {
        p.push_str("## TYPE SIGNATURES:\n\n");
        for sym in &sigs {
            p.push_str(&format!("// {}:\n{}\n\n", sym.import_path, sym.signature));
        }
    }

    // Growing TypeContext (all types generated so far)
    let ctx_str = type_context.to_prompt_string_full();
    if !ctx_str.is_empty() {
        p.push_str("## PREVIOUSLY GENERATED TYPES:\n\n");
        p.push_str(&ctx_str);
        p.push('\n');
    }

    // Universal rules
    p.push_str("## RULES:\n");
    p.push_str("- Output ONLY the complete Rust file — no markdown, no explanation\n");
    p.push_str("- Use ONLY the imports listed above\n");
    p.push_str("- sqlx: use query_as::<_, T>(), NEVER query_as!() compile-time macro\n");
    p.push_str("- sqlx: use raw_sql() for DDL, NOT migrate!()\n");
    p.push_str("- Pagination: use i64 for page/per_page (NOT u64)\n");
    p.push_str("- Nullable fields: Option<T> — clone before .bind(option.clone())\n");
    p.push_str("- Routes return Result<impl Responder, AppError>\n");
    p.push_str("- Route config fn: pub fn {entity}_routes(cfg: &mut web::ServiceConfig)\n");
    p.push_str("- Use tracing::info!, NOT log::info!\n");
    p.push_str("- No unwrap() — use ? or map_err\n");
    p.push_str("- No new external crates beyond those in Cargo.toml\n");
    p.push_str("- bcrypt: use DEFAULT_COST for password hashing\n");
    p.push_str("- JWT: use jsonwebtoken crate, secret from env JWT_SECRET\n");
    p.push_str("\n## FUNCTION VISIBILITY RULE:\n");
    p.push_str("- ALL functions listed in AVAILABLE IMPORTS MUST be declared `pub async fn` (or `pub fn` for sync)\n");
    p.push_str("- Private functions (without `pub`) cannot be called from other modules — causes E0603\n");
    p.push_str("\n## API ROUTE IMPORT PATTERN:\n");
    p.push_str("- Import service module, then call functions via module path:\n");
    p.push_str("    use crate::services::product as product_service;\n");
    p.push_str("    product_service::get_product(&pool, id).await\n");
    p.push_str("- DO NOT import individual service functions directly (causes visibility errors):\n");
    p.push_str("    use crate::services::product::{get_product};  // WRONG — use module path instead\n");

    // Role-specific rules based on file path
    if node.path.contains("/services/") {
        p.push_str("\n## SERVICE FILE RULES:\n");
        p.push_str("- You MUST import: use sqlx::PgPool;\n");
        p.push_str("- You MUST import: use tracing::{info, warn, error};  // if you use logging\n");
        p.push_str("- Service functions take `pool: &PgPool` as first parameter\n");
        p.push_str("- Service functions return `Result<T, AppError>`\n");
        p.push_str("- Functions take OWNED payloads (not references):\n");
        p.push_str("    pub async fn create_product(pool: &PgPool, payload: CreateProductPayload) -> Result<Product, AppError>\n");
        p.push_str("    NOT: create_product(pool: &PgPool, payload: &CreateProductPayload)\n");
        p.push_str("- Pass payload directly to query (already owned, no clone needed):\n");
        p.push_str("    {entity}_queries::create_{entity}(pool, payload).await\n");
    }

    if node.path.contains("/models/") {
        p.push_str("\n## MODEL FILE RULES:\n");
        p.push_str("- You MUST import: use chrono::{DateTime, Utc};  // for timestamp fields\n");
        p.push_str("- You MUST import: use serde::{Serialize, Deserialize};\n");
        p.push_str("- You MUST import: use sqlx::FromRow;\n");
        p.push_str("- The main struct derives: #[derive(Debug, Serialize, Deserialize, FromRow, Clone)]\n");
        p.push_str("- Timestamp fields: pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc>\n");
    }

    if node.path.contains("_queries.rs") {
        p.push_str("\n## QUERY FILE RULES:\n");
        p.push_str("- Do NOT use actix_web types (no web::Data, no web::Path, no HttpResponse)\n");
        p.push_str("- Function parameters use raw types: pool: &PgPool, id: i64, params: &PaginationParams\n");
        p.push_str("- NOT pool: web::Data<PgPool> — that is for API routes, not queries\n");
        p.push_str("- You MUST import: use sqlx::Row;  // required for .get() on raw query results\n");
        p.push_str("- Functions take OWNED payloads (not references):\n");
        p.push_str("    pub async fn create_product(pool: &PgPool, payload: CreateProductPayload) -> Result<Product, AppError>\n");
        p.push_str("    NOT: create_product(pool: &PgPool, payload: &CreateProductPayload)\n");
        p.push_str("- When updating, access payload fields with the correct Option<T> types:\n");
        p.push_str("    payload.name is Option<String> — bind with .bind(payload.name.as_deref())\n");
        p.push_str("    payload.quantity is Option<i32> — bind with .bind(payload.quantity)\n");
    }

    if node.path.contains("/api/") {
        p.push_str("\n## AUTH EXTRACTION (MANDATORY):\n");
        p.push_str("- AuthUser implements FromRequest. Use it as a FUNCTION PARAMETER — never extract manually:\n");
        p.push_str("    pub async fn list_products(\n");
        p.push_str("        pool: web::Data<PgPool>,\n");
        p.push_str("        params: web::Query<PaginationParams>,\n");
        p.push_str("        user: AuthUser,\n");
        p.push_str("    ) -> Result<impl Responder, AppError>\n");
        p.push_str("- NEVER use any of these — they do NOT compile:\n");
        p.push_str("    req.extensions().get::<AuthUser>()  // WRONG\n");
        p.push_str("    req.headers().get(\"Authorization\")  // WRONG\n");
        p.push_str("    HttpRequest parameter for auth purposes  // WRONG\n");
        p.push_str("- actix-web extracts AuthUser automatically from the Authorization: Bearer header\n");
        p.push_str("- Pool is accessed via pool.get_ref(): let pool = pool.get_ref();\n");
        p.push_str("- Service functions are called via module alias:\n");
        p.push_str("    use crate::services::product as product_service;\n");
        p.push_str("    product_service::create_product(pool, payload.into_inner()).await\n");
    }

    if node.path.contains("auth_routes") {
        p.push_str("\n## AUTH ROUTES PUBLIC/PROTECTED:\n");
        p.push_str("- POST /api/auth/register — PUBLIC: no AuthUser parameter\n");
        p.push_str("- POST /api/auth/login — PUBLIC: no AuthUser parameter\n");
        p.push_str("- GET /api/auth/me — PROTECTED: AuthUser as function parameter\n");
        p.push_str("- Login: call user_queries::get_user_by_email, verify bcrypt hash, return encode_jwt result\n");
        p.push_str("- Register: hash password with bcrypt, call user_queries::create_user\n");
    }

    p.push_str("\n## PASSWORD HASHING:\n");
    p.push_str("- Use bcrypt ONLY. The bcrypt crate is in Cargo.toml.\n");
    p.push_str("    use bcrypt::{hash, verify};\n");
    p.push_str("    let hashed = hash(&password, 12).map_err(|e| AppError::InternalError(e.to_string()))?;\n");
    p.push_str("    let valid = verify(&password, &stored_hash).map_err(|e| AppError::InternalError(e.to_string()))?;\n");
    p.push_str("- NEVER use argon2, scrypt, or any other hashing library.\n");

    p
}

/// Build the S6 Coagula fix prompt — includes error context and HDAG symbols.
fn build_coagula_fix_prompt(
    path: &str,
    current_content: &str,
    errors: &str,
    provided: &[ProvidedSymbol],
    module_map: &str,
    type_ctx: &str,
) -> String {
    let mut p = String::new();

    p.push_str(&format!(
        "Fix the compile errors in `{}`.\n\n",
        path
    ));

    p.push_str("## CURRENT FILE CONTENT:\n```rust\n");
    p.push_str(current_content);
    p.push_str("\n```\n\n");

    p.push_str("## COMPILE ERRORS:\n");
    p.push_str(errors);
    p.push_str("\n\n");

    if !provided.is_empty() {
        p.push_str("## CORRECT IMPORTS (use ONLY these):\n");
        for sym in provided {
            p.push_str(&format!("use {};  // {}\n", sym.import_path, sym.kind));
        }
        p.push('\n');
    }

    if !module_map.is_empty() {
        p.push_str("## MODULE MAP (ground truth from generated files):\n");
        p.push_str(module_map);
        p.push('\n');
    }

    if !type_ctx.is_empty() {
        p.push_str("## GENERATED TYPES:\n");
        p.push_str(type_ctx);
        p.push('\n');
    }

    p.push_str("## RULES:\n");
    p.push_str("- Output ONLY the complete fixed Rust file — no markdown\n");
    p.push_str("- Fix ONLY the errors listed above, keep the rest unchanged\n");
    p.push_str("- sqlx: query_as::<_, T>(), NEVER query_as!()\n");
    p.push_str("- No unwrap()\n");
    p.push_str("- No new external crates\n");

    p
}

// ─── Error parsing ────────────────────────────────────────────────────────────

/// A single compiler error with location and message.
#[derive(Clone, Debug)]
struct CompilerError {
    line: usize,
    message: String,
}

/// Parse `cargo check --message-format=short` output into errors grouped by file.
///
/// Returns a map from relative path (e.g. `"src/database/pool.rs"`) to the list
/// of errors in that file.
fn parse_errors_by_file(errors: &str) -> HashMap<String, Vec<CompilerError>> {
    let mut map: HashMap<String, Vec<CompilerError>> = HashMap::new();

    // Short format: "src/path.rs:line:col: error[E...]: message"
    // Handles both forward slashes (Linux/Mac) and backslashes (Windows).
    let re_short = Regex::new(r"^(src[/\\][^\s:]+\.rs):(\d+):\d+:\s*error(?:\[E\d+\])?:\s*(.+)")
        .expect("regex compiles");
    // Standard format: " --> src/path.rs:line:col" (also handles backslashes)
    let re_loc =
        Regex::new(r"-->\s+(src[/\\][^\s:]+\.rs):(\d+):\d+").expect("regex compiles");
    let re_err =
        Regex::new(r"^error(?:\[E\d+\])?:\s*(.+)").expect("regex compiles");

    // First pass: short format
    for line in errors.lines() {
        if let Some(caps) = re_short.captures(line) {
            // Normalize backslashes to forward slashes for consistent map keys
            let file = caps[1].replace('\\', "/");
            let line_num: usize = caps[2].parse().unwrap_or(0);
            let message = caps[3].to_string();
            map.entry(file)
                .or_default()
                .push(CompilerError { line: line_num, message });
        }
    }

    // Second pass: standard format
    let mut current_error: Option<String> = None;
    for line in errors.lines() {
        if let Some(caps) = re_err.captures(line) {
            current_error = Some(caps[1].to_string());
        }
        if let Some(caps) = re_loc.captures(line) {
            if let Some(ref err_msg) = current_error {
                // Normalize backslashes to forward slashes
                let file = caps[1].replace('\\', "/");
                let line_num: usize = caps[2].parse().unwrap_or(0);
                let already = map.get(&file).map_or(false, |errs| {
                    errs.iter().any(|e| e.line == line_num && e.message == *err_msg)
                });
                if !already {
                    map.entry(file)
                        .or_default()
                        .push(CompilerError { line: line_num, message: err_msg.clone() });
                }
                current_error = None;
            }
        }
    }

    map
}
