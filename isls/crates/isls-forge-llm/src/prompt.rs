// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Prompt construction for ISLS v3.1 LLM file generation.
//!
//! Every prompt follows the universal template from §4.1 of the spec:
//! application context → growing type context → norm requirements → file to
//! generate → critical rules → domain-specific requirements.

use isls_renderloop::type_context::TypeContext;

use crate::{AppSpec, EntityDef, FileSpec, ForgePlan};

/// Build the full LLM prompt for generating `file_spec`.
///
/// Includes all types generated so far via `type_ctx.to_prompt_string_full()`.
pub fn build_prompt(file_spec: &FileSpec, type_ctx: &TypeContext, plan: &ForgePlan) -> String {
    let spec = &plan.spec;
    let type_ctx_str = type_ctx.to_prompt_string_full();

    let norm_context = norm_context_for_file(file_spec, plan);
    let entity_context = entity_specific_context(file_spec, plan);
    let module_map = build_module_map(&plan.spec);

    let type_section = if type_ctx_str.trim().is_empty()
        || type_ctx_str.contains("=== TYPE CONTEXT")
            && !type_ctx_str.contains("AppError")
            && !type_ctx_str.contains("struct ")
    {
        "(no types generated yet — this is one of the first files)".to_string()
    } else {
        type_ctx_str
    };

    format!(
        r#"You are generating a production-quality Rust application.

## Application
Name: {app_name}
Description: {description}
Domain: {domain}

## ALREADY GENERATED FILES — USE THESE EXACT TYPES
{type_section}

{module_map}

## Norm Requirements for This File
{norm_context}

## File to Generate
Path: {path}
Purpose: {purpose}

## Rules (CRITICAL — violations cause compile failures)
- Use ONLY types shown in the type context above (no invented field names)
- Use `tracing::info!`, `tracing::warn!`, `tracing::error!` — NEVER `log::info!`, `log::warn!`, `log::error!`
  The project uses tracing-subscriber, NOT env_logger.
- CRITICAL: Use `sqlx::query_as::<_, Type>("SQL").bind(x).fetch_one(pool).await` for typed queries
  NEVER use `sqlx::query_as!()`, `sqlx::query!()`, or `sqlx::query_scalar!()`
  These are compile-time macros that require DATABASE_URL at build time and WILL fail.
  For untyped queries use `sqlx::query("SQL").bind(x).execute(pool).await`
- Do NOT use `sqlx::migrate!()` macro — migrations are loaded at runtime via `include_str!`
- All errors via `AppError` (exact variants listed in type context)
- Never use `unwrap()` — use `?` or `.map_err(...)`
- When updating nullable fields (Option<T>):
  `if let Some(v) = payload.field {{ current.field = Some(v); }}`
  Do NOT write `current.field = v` — wrap in `Some()`.
- For `FromRequest` implementations, use `std::future::Ready` and `std::future::ready`
  Do NOT import `Ready`/`ready` from the `futures` crate.
- Do not import any new external crates not in Cargo.toml
- Return the COMPLETE file, no markdown fences, no triple backticks
- No explanation — just the complete source code

## Domain-Specific Requirements
{entity_context}
"#,
        app_name = spec.app_name,
        description = spec.description,
        domain = spec.domain_name,
        type_section = type_section,
        module_map = module_map,
        norm_context = norm_context,
        path = file_spec.path,
        purpose = file_spec.purpose,
        entity_context = entity_context,
    )
}

/// Build a fix-prompt that appends the compile error to the original prompt.
///
/// Used on retry: the LLM receives the original prompt plus the exact error
/// message from `cargo check`, and must return the corrected complete file.
pub fn build_fix_prompt(original_prompt: &str, compile_error: &str) -> String {
    format!(
        r#"{original_prompt}

## COMPILATION ERROR (attempt failed)
The code you returned produced this compilation error:
```
{compile_error}
```

Fix ALL errors listed above. Return the COMPLETE corrected file.
- Do not change function signatures or types from the context above
- Do not add new external dependencies
- Ensure curly braces are balanced
- No markdown fences — pure source code only
"#,
        original_prompt = original_prompt,
        compile_error = compile_error
    )
}

// ─── Module map ──────────────────────────────────────────────────────────────

/// Build a module-structure map showing exact Rust import paths.
///
/// Injected into every prompt so the LLM never guesses import paths.
fn build_module_map(spec: &AppSpec) -> String {
    let mut m = String::new();
    m.push_str("## MODULE STRUCTURE (use these EXACT import paths)\n\n");
    m.push_str("src/main.rs declares these modules:\n");
    m.push_str("  mod api;\n  mod auth;\n  mod config;\n  mod database;\n");
    m.push_str("  mod errors;\n  mod models;\n  mod pagination;\n  mod services;\n\n");

    m.push_str("### Correct import paths:\n");
    m.push_str("  use crate::errors::AppError;\n");
    m.push_str("  use crate::pagination::{PaginationParams, PaginatedResponse};\n");
    m.push_str("  use crate::auth::{AuthUser, Claims, encode_jwt, decode_jwt};\n");
    m.push_str("  use crate::config::AppConfig;\n");

    // Models
    for e in &spec.entities {
        m.push_str(&format!(
            "  use crate::models::{}::{{{}, Create{}Payload, Update{}Payload}};\n",
            e.snake_name, e.name, e.name, e.name
        ));
    }

    // Database queries
    m.push('\n');
    for e in &spec.entities {
        m.push_str(&format!(
            "  use crate::database::{}_queries;\n",
            e.snake_name
        ));
    }

    // Services
    m.push('\n');
    for e in &spec.entities {
        m.push_str(&format!(
            "  use crate::services::{} as {}_service;\n",
            e.snake_name, e.snake_name
        ));
    }

    m.push_str("\n### WRONG paths (these do NOT exist — will cause compile errors):\n");
    m.push_str(
        "  use crate::AppError;              // WRONG: must be crate::errors::AppError\n",
    );
    m.push_str(
        "  use crate::User;                  // WRONG: must be crate::models::user::User\n",
    );
    m.push_str(
        "  use crate::api::AppError;         // WRONG: AppError is in crate::errors\n",
    );
    m.push_str("  use crate::database::AppError;    // WRONG\n");
    m.push_str("  use crate::models::AppError;      // WRONG\n");
    m.push_str(
        "  use crate::api::ResponseError;    // WRONG: use actix_web::ResponseError\n",
    );
    m.push_str("  use crate::domain::*;             // WRONG: no domain module exists\n");

    m
}

// ─── Norm context ─────────────────────────────────────────────────────────────

/// Build the norm-requirements section for the given file spec.
///
/// Describes what the file must implement in terms of layer-specific norms.
fn norm_context_for_file(spec: &FileSpec, plan: &ForgePlan) -> String {
    let path = spec.path.as_str();

    if path.ends_with("errors.rs") {
        return r#"NORM: ErrorSystem (ISLS-NORM-0103)
Required AppError variants:
- NotFound(String) — HTTP 404
- ValidationError(Vec<String>) — HTTP 422
- Unauthorized — HTTP 401
- Forbidden — HTTP 403
- InternalError(String) — HTTP 500
- Conflict(String) — HTTP 409
Implement actix_web::ResponseError returning correct status codes and JSON body.
"#
        .into();
    }

    if path.ends_with("pagination.rs") {
        return r#"NORM: Pagination (ISLS-NORM-0096)
Required types:
- PaginationParams { page: u64, per_page: u64 } (defaults: page=1, per_page=20)
- PaginatedResponse<T> { items: Vec<T>, total: i64, page: u64, per_page: u64 }
- Helper: fn offset(params: &PaginationParams) -> i64 { ((params.page - 1) * params.per_page) as i64 }
Derive Serialize, Deserialize, Debug, Clone. Use serde defaults for page/per_page.
"#
        .into();
    }

    if path.ends_with("auth.rs") && !path.contains("auth_routes") {
        return r#"NORM: JWT-Auth (ISLS-NORM-0088)
Required:
- Claims { sub: i64, email: String, role: String, exp: usize }
- AuthUser { id: i64, email: String, role: String } — actix FromRequest extractor
- fn encode_jwt(claims: &Claims, secret: &str) -> Result<String, AppError>
- fn decode_jwt(token: &str, secret: &str) -> Result<Claims, AppError>
- fn require_role(user: &AuthUser, min_role: &str) -> Result<(), AppError>
  (roles: "admin" > "operator" > "viewer")

For the AuthUser FromRequest implementation:
- Use std::future::Ready and std::future::ready (from std::future)
- Do NOT import Ready/ready from the futures crate
- Extract Bearer token from the Authorization header
- Decode JWT using decode_jwt with JWT_SECRET read from std::env::var("JWT_SECRET")
"#
        .into();
    }

    if path.ends_with("pool.rs") {
        return r#"NORM: Database pool setup
- pub async fn create_pool() -> Result<sqlx::PgPool, AppError>
- Read DATABASE_URL from environment via std::env::var
- Set max_connections(5)
- Do NOT load or run migrations here — main.rs handles migration loading
- Do NOT use sqlx::migrate!() macro
- Import AppError as: use crate::errors::AppError;
"#
        .into();
    }

    if path.ends_with("001_initial.sql") {
        return format_migration_norm(plan);
    }

    if let Some(entity_name) = &spec.entity {
        let entity = plan.spec.entities.iter().find(|e| &e.name == entity_name);
        if let Some(e) = entity {
            if path.contains("models/") {
                return format_model_norm(e);
            }
            if path.contains("_queries.rs") {
                return format_queries_norm(e);
            }
            if path.contains("services/") {
                return format_service_norm(e);
            }
            if path.contains("api/") {
                return format_api_norm(e);
            }
        }
    }

    if path.ends_with("auth_routes.rs") {
        return r#"NORM: Auth routes
Endpoints:
- POST /api/auth/register — body: {email, password, role?} → 201 {user}
- POST /api/auth/login    — body: {email, password} → 200 {token: String}
- GET  /api/auth/me       — Bearer auth → 200 {user}
- GET  /api/health        — no auth → 200 {"status": "ok", "database": "connected"}
Hash passwords with bcrypt::hash(password, 12). Verify with bcrypt::verify.
Return AppError::Unauthorized on bad credentials.

IMPORTANT: The User entity has these exact fields from the TypeContext above.
Use ONLY the field names shown in the User struct. Do not assume 'name',
'last_login_at', or any other field unless it appears in the User struct.
Use sqlx::query_as::<_, User>("SQL") — NEVER sqlx::query_as!() macro.
"#
        .into();
    }

    if path.ends_with("main.rs") {
        return format_main_norm(&plan.spec);
    }

    String::new()
}

// ─── Entity-specific context ──────────────────────────────────────────────────

/// Build the entity-specific requirements section.
fn entity_specific_context(spec: &FileSpec, plan: &ForgePlan) -> String {
    if let Some(entity_name) = &spec.entity {
        if let Some(entity) = plan.spec.entities.iter().find(|e| &e.name == entity_name) {
            let mut parts = Vec::new();

            // List all fields with types
            parts.push(format!("Entity: {} ({} fields)", entity.name, entity.fields.len()));
            parts.push("Fields:".into());
            for f in &entity.fields {
                let nullable = if f.nullable { " (nullable)" } else { "" };
                parts.push(format!(
                    "  - {}: {} [SQL: {}]{}",
                    f.name, f.rust_type, f.sql_type, nullable
                ));
            }

            // Validations
            if !entity.validations.is_empty() {
                parts.push("Validations:".into());
                for v in &entity.validations {
                    parts.push(format!("  - if !({}) → \"{}\"", v.condition, v.message));
                }
            }

            // Business rules
            if !entity.business_rules.is_empty() {
                parts.push("Business rules:".into());
                for r in &entity.business_rules {
                    parts.push(format!("  - {}", r));
                }
            }

            // Cross-entity context
            if !entity.relationships.is_empty() {
                parts.push("Relationships:".into());
                for r in &entity.relationships {
                    parts.push(format!("  - {}", r));
                }
            }

            return parts.join("\n");
        }
    }

    // Global business rules for non-entity files
    if !plan.spec.business_rules.is_empty() {
        let mut parts = vec!["Business rules:".to_string()];
        for r in &plan.spec.business_rules {
            parts.push(format!("  - {}", r));
        }
        return parts.join("\n");
    }

    String::new()
}

// ─── Norm formatters ──────────────────────────────────────────────────────────

fn format_model_norm(entity: &EntityDef) -> String {
    let mut lines = vec![format!(
        "NORM: CRUD-Entity model for {} (ISLS-NORM-0042)",
        entity.name
    )];
    lines.push(format!("Generate these structs for {}:", entity.name));

    // Main struct fields
    lines.push(format!("1. {} {{", entity.name));
    for f in &entity.fields {
        lines.push(format!("   pub {}: {},", f.name, f.rust_type));
    }
    lines.push("}".into());

    // Create payload (non-system fields)
    let user_fields: Vec<_> = entity
        .fields
        .iter()
        .filter(|f| !matches!(f.name.as_str(), "id" | "created_at" | "updated_at"))
        .collect();
    lines.push(format!("2. Create{}Payload {{", entity.name));
    for f in &user_fields {
        lines.push(format!("   pub {}: {},", f.name, f.rust_type));
    }
    lines.push("}".into());

    // Update payload (all as Option)
    lines.push(format!("3. Update{}Payload {{", entity.name));
    for f in &user_fields {
        let opt_type = if f.rust_type.starts_with("Option<") {
            f.rust_type.clone()
        } else {
            format!("Option<{}>", f.rust_type)
        };
        lines.push(format!("   pub {}: {},", f.name, opt_type));
    }
    lines.push("}".into());

    // Validations
    if !entity.validations.is_empty() {
        lines.push(format!("4. impl {} {{ fn validate(&self) -> Vec<String> }}", entity.name));
    }

    lines.join("\n")
}

fn format_queries_norm(entity: &EntityDef) -> String {
    let sn = &entity.snake_name;
    let n = &entity.name;
    format!(
        r#"NORM: CRUD queries for {n} (ISLS-NORM-0042)
Required async functions (all take &PgPool, return Result<_, AppError>):
- pub async fn get_{sn}(pool: &PgPool, id: i64) -> Result<{n}, AppError>
- pub async fn list_{sn}s(pool: &PgPool, params: &PaginationParams) -> Result<PaginatedResponse<{n}>, AppError>
- pub async fn create_{sn}(pool: &PgPool, payload: Create{n}Payload) -> Result<{n}, AppError>
- pub async fn update_{sn}(pool: &PgPool, id: i64, payload: Update{n}Payload) -> Result<{n}, AppError>
- pub async fn delete_{sn}(pool: &PgPool, id: i64) -> Result<(), AppError>

Use sqlx::query_as::<_, {n}>("SQL").bind(x) — NEVER use sqlx::query_as!() macro.
Use exact field names from the {n} struct above in SELECT columns.
On "no rows" from SELECT, return AppError::NotFound("{n} {{id}} not found".into()).
For list: SELECT COUNT(*) first (via sqlx::query_scalar), then SELECT with LIMIT/OFFSET from PaginationParams.
For count query: sqlx::query_scalar::<_, i64>("SELECT COUNT(*) ...") is NOT allowed.
Instead use: sqlx::query("SELECT COUNT(*)...").fetch_one(pool) and extract the count manually,
or use sqlx::query_as::<_, (i64,)>("SELECT COUNT(*)...").fetch_one(pool).await?.0
"#,
        n = n,
        sn = sn
    )
}

fn format_service_norm(entity: &EntityDef) -> String {
    let sn = &entity.snake_name;
    let n = &entity.name;
    let rules = if entity.business_rules.is_empty() {
        "No special business rules — thin wrapper around database queries.".to_string()
    } else {
        entity.business_rules.join("\n")
    };
    format!(
        r#"NORM: Service layer for {n} (ISLS-NORM-0042)
Thin wrappers calling {sn}_queries functions, adding validation and tracing:
- pub async fn get_{sn}(pool: &PgPool, id: i64) -> Result<{n}, AppError>
- pub async fn list_{sn}s(pool: &PgPool, params: &PaginationParams) -> Result<PaginatedResponse<{n}>, AppError>
- pub async fn create_{sn}(pool: &PgPool, payload: Create{n}Payload) -> Result<{n}, AppError>
  (validate payload.validate() before inserting)
- pub async fn update_{sn}(pool: &PgPool, id: i64, payload: Update{n}Payload) -> Result<{n}, AppError>
- pub async fn delete_{sn}(pool: &PgPool, id: i64) -> Result<(), AppError>

Business rules:
{rules}

Log operations: tracing::info!("creating {sn}"); tracing::info!("{sn} {{id}} deleted");
"#,
        n = n,
        sn = sn,
        rules = rules
    )
}

fn format_api_norm(entity: &EntityDef) -> String {
    let sn = &entity.snake_name;
    let n = &entity.name;
    format!(
        r#"NORM: API handlers for {n} (ISLS-NORM-0042)
Actix-web handler functions:
- pub async fn list_{sn}s(pool: web::Data<PgPool>, params: web::Query<PaginationParams>, user: AuthUser) → impl Responder
- pub async fn get_{sn}(pool: web::Data<PgPool>, path: web::Path<i64>, user: AuthUser) → impl Responder
- pub async fn create_{sn}(pool: web::Data<PgPool>, body: web::Json<Create{n}Payload>, user: AuthUser) → impl Responder
- pub async fn update_{sn}(pool: web::Data<PgPool>, path: web::Path<i64>, body: web::Json<Update{n}Payload>, user: AuthUser) → impl Responder
- pub async fn delete_{sn}(pool: web::Data<PgPool>, path: web::Path<i64>, user: AuthUser) → impl Responder
  (delete requires role "admin")

pub fn {sn}_routes(cfg: &mut web::ServiceConfig) — register all routes under /api/{sn}s

Use web::HttpResponse::Ok().json(result) for success.
Map AppError to proper HTTP response via ResponseError.
"#,
        n = n,
        sn = sn
    )
}

fn format_migration_norm(plan: &ForgePlan) -> String {
    // Generate a verified bcrypt hash at code-generation time
    let admin_hash = bcrypt::hash("admin123", 12).expect("bcrypt hash failed");

    let mut lines = vec![
        "NORM: Database schema migration (all entities)".to_string(),
        "CREATE TABLE IF NOT EXISTS statements in FK dependency order.".to_string(),
        "Tables must be created in topological order: users first, then tables".to_string(),
        "with no FK deps, then tables referencing already-created tables.".to_string(),
        "Include:".to_string(),
        "- users table (id BIGSERIAL PRIMARY KEY, email VARCHAR(255) NOT NULL UNIQUE, password_hash VARCHAR(255) NOT NULL, role VARCHAR(50) NOT NULL DEFAULT 'operator', is_active BOOLEAN NOT NULL DEFAULT TRUE, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())".to_string(),
        format!("- Seed admin: INSERT INTO users (email, password_hash, role, is_active) VALUES ('admin@example.com', '{}', 'admin', true) ON CONFLICT (email) DO NOTHING;", admin_hash),
        "(that hash is bcrypt of 'admin123' — use it exactly as shown)".to_string(),
    ];

    for entity in &plan.spec.entities {
        if entity.name == "User" {
            continue;
        }
        lines.push(format!("- {} table:", entity.snake_name));
        lines.push(format!(
            "  CREATE TABLE IF NOT EXISTS {}s (",
            entity.snake_name
        ));
        for f in &entity.fields {
            let null = if f.nullable { "" } else { " NOT NULL" };
            let default = f
                .default_value
                .as_deref()
                .map(|d| format!(" DEFAULT {}", d))
                .unwrap_or_default();
            lines.push(format!(
                "    {} {}{}{},",
                f.name, f.sql_type, null, default
            ));
        }
        lines.push(");".to_string());
    }

    lines.join("\n")
}

fn format_main_norm(spec: &AppSpec) -> String {
    let entity_mods: Vec<String> = spec
        .entities
        .iter()
        .filter(|e| e.name != "User")
        .map(|e| format!("api::{}_routes(cfg)", e.snake_name))
        .collect();

    format!(
        r#"NORM: Application entry point
- Call dotenvy::dotenv().ok() to load .env file
- Init tracing_subscriber with EnvFilter from RUST_LOG env var
- Create PgPool via database::pool::create_pool().await
- Run migrations AFTER creating the pool:
  let migration_sql = include_str!("../migrations/001_initial.sql");
  sqlx::raw_sql(migration_sql).execute(&pool).await.expect("migrations failed");
  (The path "../migrations/001_initial.sql" is correct — relative to src/main.rs)
  Do NOT use sqlx::migrate!() macro — it requires DATABASE_URL at build time.
- Configure actix-web HttpServer:
  - CORS: use Cors::permissive() to allow ANY origin (frontend may be on different port)
  - actix_web::middleware::Logger::default() for request logging
  - App::new()
    .app_data(web::Data::new(pool.clone()))
    .configure(api::configure_routes)
  - Bind to 0.0.0.0:{{PORT}} (default 8080)
- The api module has a function configure_routes(cfg: &mut web::ServiceConfig)
  that registers all entity routes. Call it with .configure(api::configure_routes).
- configure_routes registers: auth_routes, {}
- Import: use errors::AppError; (in main.rs, not crate::errors since main IS the crate root)
"#,
        entity_mods.join(", ")
    )
}
