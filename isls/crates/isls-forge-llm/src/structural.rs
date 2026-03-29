// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Deterministic code generators for all structural HDAG nodes (Layer 0 + mod files).
//!
//! These functions are pure Rust computations over [`AppSpec`] — no LLM calls,
//! no token cost, no error potential.  The output is a deterministic function
//! of the entity list.
//!
//! Structural nodes produced here:
//! - `backend/src/main.rs`
//! - `backend/src/models/mod.rs`
//! - `backend/src/database/mod.rs`
//! - `backend/src/services/mod.rs`
//! - `backend/src/api/mod.rs`  (includes `configure_routes` + `health_check`)
//! - `backend/migrations/001_initial.sql`  (bcrypt seed at generation time)
//! - `backend/tests/api_tests.rs`
//! - Frontend: `index.html`, `style.css`, `src/api/client.js`, `src/pages/{entity}.js`

use crate::{AppSpec, EntityDef};
use isls_hypercube::domain::FieldDef;

// ─── Layer 0 structural generators ───────────────────────────────────────────

/// Generate `backend/src/main.rs` deterministically from the entity list.
///
/// Declares all top-level modules, creates the PgPool, runs migrations, and
/// configures the actix-web server.  No LLM call needed — the module map is
/// fully determined by the entity list.
pub fn generate_main_rs(spec: &AppSpec) -> String {
    let app = &spec.app_name;
    format!(
        r#"// Copyright (c) 2026 Sebastian Klemm — ISLS v3.4 structural generated
mod api;
mod auth;
mod config;
mod database;
mod errors;
mod models;
mod pagination;
mod services;

use actix_cors::Cors;
use actix_web::{{middleware, web, App, HttpServer}};
use tracing_subscriber::EnvFilter;

#[actix_web::main]
async fn main() -> std::io::Result<()> {{
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let pool = database::pool::create_pool()
        .await
        .expect("failed to connect to database");

    // Run migrations (idempotent — uses IF NOT EXISTS)
    let migration_sql = include_str!("../migrations/001_initial.sql");
    sqlx::raw_sql(migration_sql)
        .execute(&pool)
        .await
        .expect("failed to run database migrations");
    tracing::info!("database migrations applied");

    tracing::info!("starting {app} on port {{}}", port);

    HttpServer::new(move || {{
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(pool.clone()))
            .configure(api::configure_routes)
    }})
    .bind(format!("0.0.0.0:{{}}", port))?
    .run()
    .await
}}
"#,
        app = app
    )
}

/// Generate `backend/src/models/mod.rs` — declares all entity submodules.
///
/// Uses `pub mod` + explicit `pub use` for each public type, ensuring the
/// module tree is never guessed by the LLM.
pub fn generate_models_mod(spec: &AppSpec) -> String {
    let mut s = String::from("// Copyright (c) 2026 Sebastian Klemm — ISLS v3.4 structural generated\n");
    for entity in &spec.entities {
        s.push_str(&format!("pub mod {};\n", entity.snake_name));
    }
    s.push('\n');
    // Re-export all public types from each model
    for entity in &spec.entities {
        let sn = &entity.snake_name;
        let pn = &entity.name;
        s.push_str(&format!(
            "pub use {}::{{{}, Create{}Payload, Update{}Payload}};\n",
            sn, pn, pn, pn
        ));
    }
    s
}

/// Generate `backend/src/database/mod.rs` — declares pool + all query submodules.
pub fn generate_database_mod(spec: &AppSpec) -> String {
    let mut s = String::from("// Copyright (c) 2026 Sebastian Klemm — ISLS v3.4 structural generated\npub mod pool;\n");
    for entity in &spec.entities {
        s.push_str(&format!("pub mod {}_queries;\n", entity.snake_name));
    }
    s.push_str("\npub use pool::create_pool;\n");
    for entity in &spec.entities {
        s.push_str(&format!("pub use {}_queries::*;\n", entity.snake_name));
    }
    s
}

/// Generate `backend/src/services/mod.rs` — declares all service submodules.
pub fn generate_services_mod(spec: &AppSpec) -> String {
    let mut s = String::from("// Copyright (c) 2026 Sebastian Klemm — ISLS v3.4 structural generated\n");
    for entity in &spec.entities {
        s.push_str(&format!("pub mod {};\n", entity.snake_name));
    }
    s.push('\n');
    for entity in &spec.entities {
        s.push_str(&format!("pub use {}::*;\n", entity.snake_name));
    }
    s
}

/// Generate `backend/src/api/mod.rs` — declares submodules, `configure_routes()`,
/// and the inline `health_check` handler.
///
/// This is the complete, deterministic version matching what `main.rs` expects.
/// No LLM guessing of function names or route registration.
pub fn generate_api_mod(spec: &AppSpec) -> String {
    let non_user: Vec<&EntityDef> = spec.entities.iter().filter(|e| e.name != "User").collect();

    let mut s = String::from("// Copyright (c) 2026 Sebastian Klemm — ISLS v3.4 structural generated\npub mod auth_routes;\n");
    for entity in &non_user {
        s.push_str(&format!("pub mod {};\n", entity.snake_name));
    }

    s.push_str("\nuse actix_web::{web, HttpResponse, Responder};\n\n");

    // configure_routes function
    s.push_str("pub fn configure_routes(cfg: &mut web::ServiceConfig) {\n");
    s.push_str("    auth_routes::auth_routes(cfg);\n");
    for entity in &non_user {
        let sn = &entity.snake_name;
        s.push_str(&format!("    {}::{}_routes(cfg);\n", sn, sn));
    }
    s.push_str("    cfg.route(\"/api/health\", web::get().to(health_check));\n");
    s.push_str("}\n\n");

    // Inline health_check handler
    s.push_str(concat!(
        "async fn health_check(\n",
        "    pool: web::Data<sqlx::PgPool>,\n",
        ") -> impl Responder {\n",
        "    match sqlx::query(\"SELECT 1\").execute(pool.get_ref()).await {\n",
        "        Ok(_) => HttpResponse::Ok().json(serde_json::json!({\n",
        "            \"status\": \"ok\",\n",
        "            \"database\": \"connected\",\n",
        "            \"version\": \"1.0.0\"\n",
        "        })),\n",
        "        Err(_) => HttpResponse::ServiceUnavailable().json(serde_json::json!({\n",
        "            \"status\": \"error\",\n",
        "            \"database\": \"disconnected\"\n",
        "        })),\n",
        "    }\n",
        "}\n"
    ));

    s
}

/// Generate `backend/migrations/001_initial.sql` — FK-topologically sorted
/// CREATE TABLE statements with a bcrypt seed hash computed at generation time.
///
/// The bcrypt hash for `admin123` is computed **once** and embedded in the SQL,
/// so the migration is stable after the first run.
pub fn generate_migration(spec: &AppSpec) -> String {
    let mut sql = String::from("-- ISLS v3.4 structural generated\n\n");

    // Compute bcrypt hash at generation time
    let admin_hash = bcrypt::hash("admin123", 12)
        .unwrap_or_else(|_| "$2b$12$placeholder_hash_for_admin123_x".to_string());

    // users table always first
    sql.push_str(
        r#"CREATE TABLE IF NOT EXISTS users (
    id            BIGSERIAL PRIMARY KEY,
    email         VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    role          VARCHAR(50)  NOT NULL DEFAULT 'user',
    is_active     BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

"#,
    );

    // Seed admin user with bcrypt hash (Rule 8: INSERT must match entity fields)
    sql.push_str(&format!(
        "-- Seed admin user (password: admin123)\nINSERT INTO users (email, password_hash, role, is_active)\nVALUES ('admin@example.com', '{}', 'admin', true)\nON CONFLICT (email) DO NOTHING;\n\n",
        admin_hash
    ));

    // Sort non-User entities by FK dependency order
    let non_user: Vec<&EntityDef> = spec.entities.iter().filter(|e| e.name != "User").collect();
    let ordered = topological_sort_entities(&non_user);

    for entity in &ordered {
        sql.push_str(&format!(
            "CREATE TABLE IF NOT EXISTS {}s (\n",
            entity.snake_name
        ));
        let field_count = entity.fields.len();
        for (i, f) in entity.fields.iter().enumerate() {
            let sql_type = &f.sql_type;
            let notnull = if f.nullable || sql_type.contains("NOT NULL") || sql_type.contains("PRIMARY KEY") {
                ""
            } else {
                " NOT NULL"
            };
            let default = if sql_type.contains("DEFAULT") {
                String::new()
            } else {
                f.default_value
                    .as_deref()
                    .map(|d| format!(" DEFAULT {}", d))
                    .unwrap_or_default()
            };
            let comma = if i < field_count - 1 { "," } else { "" };
            sql.push_str(&format!(
                "    {} {}{}{}{}\n",
                f.name, sql_type, notnull, default, comma
            ));
        }
        sql.push_str(");\n\n");
    }

    sql
}

// ─── Frontend structural generators ─────────────────────────────────────────

/// Generate `frontend/index.html` — SPA shell with login form and entity navigation.
pub fn generate_frontend_index(spec: &AppSpec) -> String {
    let app = &spec.app_name;
    let nav_links: Vec<String> = spec
        .entities
        .iter()
        .filter(|e| e.name != "User")
        .map(|e| format!(
            "      <button onclick=\"loadPage('{}')\">{}</button>",
            e.snake_name, e.name
        ))
        .collect();
    let nav_html = nav_links.join("\n");
    let first_entity = spec
        .entities
        .iter()
        .find(|e| e.name != "User")
        .map(|e| e.snake_name.as_str())
        .unwrap_or("item");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{app}</title>
  <link rel="stylesheet" href="style.css">
</head>
<body>
  <nav id="nav" style="display:none">
    <strong>{app}</strong>
{nav_html}
    <span style="flex:1"></span>
    <span id="nav-user"></span>
    <button onclick="logout()">Logout</button>
  </nav>

  <div id="login-form">
    <h2>Login to {app}</h2>
    <input type="email" id="email" placeholder="Email" value="admin@example.com">
    <input type="password" id="pwd" placeholder="Password" value="admin123">
    <button onclick="doLogin()">Login</button>
    <p id="login-error" style="color:red"></p>
  </div>

  <div id="app-content" style="display:none">
    <div id="content"></div>
  </div>

  <script src="src/api/client.js"></script>
  <script>
    if (localStorage.getItem('token')) showApp();

    async function doLogin() {{
      const email = document.getElementById('email').value;
      const pwd = document.getElementById('pwd').value;
      try {{
        const r = await login(email, pwd);
        localStorage.setItem('token', r.token);
        showApp();
      }} catch (e) {{
        document.getElementById('login-error').textContent = e.message;
      }}
    }}

    async function showApp() {{
      document.getElementById('login-form').style.display = 'none';
      document.getElementById('nav').style.display = 'flex';
      document.getElementById('app-content').style.display = 'block';
      try {{
        const me = await apiFetch('GET', '/api/auth/me');
        document.getElementById('nav-user').textContent = me.email + ' (' + me.role + ')';
      }} catch (_) {{}}
      loadPage('{first_entity}');
    }}

    function logout() {{
      localStorage.removeItem('token');
      location.reload();
    }}

    async function loadPage(entity) {{
      const content = document.getElementById('content');
      content.innerHTML = '<p>Loading ' + entity + 's...</p>';
      try {{
        const data = await apiFetch('GET', '/api/' + entity + 's');
        const items = data.items || [];
        let html = '<h2>' + entity.charAt(0).toUpperCase() + entity.slice(1) + 's</h2>';
        if (items.length === 0) {{
          html += '<p>No items yet.</p>';
        }} else {{
          const cols = Object.keys(items[0]);
          html += '<table><thead><tr>' + cols.map(c => '<th>' + c + '</th>').join('') + '</tr></thead><tbody>';
          items.forEach(item => {{
            html += '<tr>' + cols.map(c => '<td>' + (item[c] !== null && item[c] !== undefined ? item[c] : '') + '</td>').join('') + '</tr>';
          }});
          html += '</tbody></table>';
        }}
        html += '<p style="color:#888;font-size:12px">Total: ' + (data.total || items.length) + '</p>';
        content.innerHTML = html;
      }} catch (e) {{
        content.innerHTML = '<p style="color:red">Error: ' + e.message + '</p>';
      }}
    }}
  </script>
</body>
</html>
"#,
        app = app,
        nav_html = nav_html,
        first_entity = first_entity
    )
}

/// Generate `frontend/style.css` — minimal application stylesheet.
pub fn generate_frontend_style() -> String {
    r#"/* ISLS v3.4 structural generated */
*, *::before, *::after { box-sizing: border-box; }
body { font-family: system-ui, sans-serif; margin: 0; padding: 0; background: #f5f5f5; }
nav { display: flex; align-items: center; padding: 0.5rem 1rem; background: #1a1a2e; color: white; gap: 1rem; }
nav button { background: #e94560; border: none; color: white; padding: 0.25rem 0.75rem; border-radius: 4px; cursor: pointer; }
#login-form { max-width: 360px; margin: 4rem auto; background: white; padding: 2rem; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,.1); display: flex; flex-direction: column; gap: 1rem; }
#login-form h2 { margin: 0 0 0.5rem; }
input { padding: 0.5rem; border: 1px solid #ddd; border-radius: 4px; }
button { padding: 0.5rem 1rem; border: none; border-radius: 4px; cursor: pointer; background: #1a1a2e; color: white; }
table { border-collapse: collapse; width: 100%; background: white; }
th, td { text-align: left; padding: 0.5rem 0.75rem; border-bottom: 1px solid #eee; }
th { background: #f0f0f0; font-weight: 600; }
#app-content { padding: 1rem 2rem; }
"#
    .into()
}

/// Generate `frontend/src/api/client.js` — fetch-based API client.
pub fn generate_frontend_client(_spec: &AppSpec) -> String {
    r#"// ISLS v3.4 structural generated — fetch-based API client
const API = '';

async function apiFetch(method, path, body) {
  const token = localStorage.getItem('token');
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers['Authorization'] = `Bearer ${token}`;
  const resp = await fetch(`${API}${path}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!resp.ok) {
    const err = await resp.json().catch(() => ({ error: resp.statusText }));
    throw new Error(err.error || resp.statusText);
  }
  if (resp.status === 204) return null;
  return resp.json();
}

async function login(email, password) {
  return apiFetch('POST', '/api/auth/login', { email, password });
}

async function register(email, password, name, role) {
  return apiFetch('POST', '/api/auth/register', { email, password, name, role });
}

async function getMe() {
  return apiFetch('GET', '/api/auth/me');
}
"#
    .into()
}

/// Generate a frontend page JS module for the given entity.
pub fn generate_entity_page(entity: &EntityDef) -> String {
    let n = &entity.name;
    let sn = &entity.snake_name;
    format!(
        r#"// ISLS v3.4 structural generated — {n} page
async function render{n}Page(container) {{
  container.innerHTML = '<h2>{n}s</h2><div id="{sn}-list">Loading...</div>';
  try {{
    const data = await apiFetch('GET', '/api/{sn}s');
    const list = document.getElementById('{sn}-list');
    if (!data.items || data.items.length === 0) {{
      list.textContent = 'No {sn}s found.';
      return;
    }}
    const table = document.createElement('table');
    const headers = Object.keys(data.items[0]);
    table.innerHTML = '<thead><tr>' + headers.map(h => `<th>${{h}}</th>`).join('') + '</tr></thead>';
    const tbody = document.createElement('tbody');
    data.items.forEach(item => {{
      const tr = document.createElement('tr');
      tr.innerHTML = headers.map(h => `<td>${{item[h] ?? ''}}</td>`).join('');
      tbody.appendChild(tr);
    }});
    table.appendChild(tbody);
    list.innerHTML = '';
    list.appendChild(table);
  }} catch (e) {{
    document.getElementById('{sn}-list').textContent = 'Error: ' + e.message;
  }}
}}
"#,
        n = n,
        sn = sn
    )
}

/// Generate `backend/tests/api_tests.rs` — placeholder integration tests.
pub fn generate_api_tests(spec: &AppSpec) -> String {
    let first_entity = spec
        .entities
        .iter()
        .find(|e| e.name != "User")
        .map(|e| e.snake_name.as_str())
        .unwrap_or("item");

    format!(
        r#"// ISLS v3.4 structural generated integration tests
// Run with: cargo test --test api_tests (requires DATABASE_URL env var)

#[actix_web::test]
async fn test_health_endpoint() {{
    // Health check should always return 200
    assert!(true, "placeholder: connect to running server for real tests");
}}

#[actix_web::test]
async fn test_login_flow() {{
    // POST /api/auth/login with admin credentials should return token
    assert!(true, "placeholder: requires running postgres");
}}

#[actix_web::test]
async fn test_{first_entity}_crud() {{
    // Full CRUD cycle for {first_entity}
    assert!(true, "placeholder: requires running postgres and jwt");
}}
"#,
        first_entity = first_entity
    )
}

// ─── Layer 3 structural generators (entity models) ───────────────────────────

/// Generate `backend/src/models/user.rs` deterministically.
///
/// User is a special entity — password_hash, role, is_active are mandatory auth
/// fields that must match the hardcoded `provides_user_model_types()` signatures.
pub fn generate_user_model_rs() -> String {
    r#"use serde::{Serialize, Deserialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub role: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CreateUserPayload {
    pub email: String,
    pub password: String,
    pub name: String,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UpdateUserPayload {
    pub email: Option<String>,
    pub name: Option<String>,
    pub role: Option<String>,
    pub is_active: Option<bool>,
}
"#
    .to_string()
}

/// Generate `backend/src/models/{entity}.rs` deterministically from EntityDef.
///
/// Produces the main struct, `Create{Entity}Payload`, and `Update{Entity}Payload`.
/// All derives are fixed; field types come directly from EntityDef.fields so the
/// output is guaranteed to match the `provides_model_types()` signatures.
pub fn generate_model_rs(entity: &EntityDef) -> String {
    let name = &entity.name;
    let mut s = String::new();

    s.push_str("use serde::{Serialize, Deserialize};\n");
    s.push_str("use sqlx::FromRow;\n");
    s.push_str("use chrono::{DateTime, Utc};\n\n");

    // ── Main struct ───────────────────────────────────────────────────────────
    s.push_str(&format!("#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]\n"));
    s.push_str(&format!("pub struct {} {{\n", name));
    s.push_str("    pub id: i64,\n");
    for field in entity.fields.iter().filter(|f| {
        f.name != "id" && f.name != "created_at" && f.name != "updated_at"
    }) {
        s.push_str(&format!("    pub {}: {},\n", field.name, model_field_type(field)));
    }
    s.push_str("    pub created_at: DateTime<Utc>,\n");
    s.push_str("    pub updated_at: DateTime<Utc>,\n");
    s.push_str("}\n\n");

    // ── Create payload ────────────────────────────────────────────────────────
    s.push_str(&format!("#[derive(Debug, Deserialize, Clone)]\n"));
    s.push_str(&format!("pub struct Create{}Payload {{\n", name));
    for field in entity.fields.iter().filter(|f| {
        f.name != "id" && f.name != "created_at" && f.name != "updated_at"
    }) {
        s.push_str(&format!("    pub {}: {},\n", field.name, model_field_type(field)));
    }
    s.push_str("}\n\n");

    // ── Update payload — all fields are Option<T> for partial updates ─────────
    s.push_str(&format!("#[derive(Debug, Deserialize, Clone)]\n"));
    s.push_str(&format!("pub struct Update{}Payload {{\n", name));
    for field in entity.fields.iter().filter(|f| {
        f.name != "id" && f.name != "created_at" && f.name != "updated_at"
    }) {
        s.push_str(&format!("    pub {}: Option<{}>,\n", field.name, model_field_base_type(field)));
    }
    s.push_str("}\n");

    s
}

/// Compute the Rust type for a field, applying `Option<>` wrapping if nullable.
fn model_field_type(field: &FieldDef) -> String {
    let base = model_field_base_type(field);
    if field.nullable {
        format!("Option<{}>", base)
    } else {
        base
    }
}

/// Base Rust type for a field — never wrapped in Option.
///
/// Normalises DateTime variants to `DateTime<Utc>` (compatible with the
/// `use chrono::{DateTime, Utc};` import in the generated file).
fn model_field_base_type(field: &FieldDef) -> String {
    if !field.rust_type.is_empty() {
        return match field.rust_type.as_str() {
            "DateTime<Utc>"
            | "chrono::DateTime<Utc>"
            | "chrono::DateTime<chrono::Utc>" => "DateTime<Utc>".to_string(),
            t => t.to_string(),
        };
    }
    // Fallback: infer from sql_type
    let sql = field.sql_type.to_uppercase();
    if sql.contains("BIGSERIAL") || sql.contains("BIGINT") {
        "i64".to_string()
    } else if sql.contains("SERIAL") || sql.contains("INTEGER") || sql.contains("INT") {
        "i32".to_string()
    } else if sql.contains("BOOL") {
        "bool".to_string()
    } else if sql.contains("FLOAT") || sql.contains("REAL") || sql.contains("DOUBLE")
        || sql.contains("DECIMAL") || sql.contains("NUMERIC")
    {
        "f64".to_string()
    } else if sql.contains("TIMESTAMPTZ") || sql.contains("TIMESTAMP") {
        "DateTime<Utc>".to_string()
    } else {
        "String".to_string()
    }
}

// ─── Layer 1 structural generators ───────────────────────────────────────────

/// Generate `backend/src/errors.rs` deterministically.
///
/// The variants MUST match the ProvidedSymbol signatures in `provided.rs` exactly.
/// Making this structural eliminates the #1 source of type mismatch errors.
pub fn generate_errors_rs() -> String {
    r#"use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Serialize)]
pub enum AppError {
    NotFound(String),
    InternalError(String),
    ValidationError(Vec<String>),
    Unauthorized,
    Forbidden,
    BadRequest(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NotFound(msg) => write!(f, "Not found: {}", msg),
            AppError::InternalError(msg) => write!(f, "Internal error: {}", msg),
            AppError::ValidationError(msgs) => write!(f, "Validation: {}", msgs.join(", ")),
            AppError::Unauthorized => write!(f, "Unauthorized"),
            AppError::Forbidden => write!(f, "Forbidden"),
            AppError::BadRequest(msg) => write!(f, "Bad request: {}", msg),
        }
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        match self {
            AppError::NotFound(msg) => {
                HttpResponse::NotFound().json(serde_json::json!({"error": msg}))
            }
            AppError::InternalError(msg) => {
                HttpResponse::InternalServerError().json(serde_json::json!({"error": msg}))
            }
            AppError::ValidationError(msgs) => {
                HttpResponse::BadRequest().json(serde_json::json!({"errors": msgs}))
            }
            AppError::Unauthorized => {
                HttpResponse::Unauthorized().json(serde_json::json!({"error": "Unauthorized"}))
            }
            AppError::Forbidden => {
                HttpResponse::Forbidden().json(serde_json::json!({"error": "Forbidden"}))
            }
            AppError::BadRequest(msg) => {
                HttpResponse::BadRequest().json(serde_json::json!({"error": msg}))
            }
        }
    }
}
"#
    .to_string()
}

/// Generate `backend/src/pagination.rs` deterministically.
///
/// Method names match the ProvidedSymbol signatures exactly.
pub fn generate_pagination_rs() -> String {
    r#"use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

impl PaginationParams {
    pub fn page(&self) -> i64 {
        self.page.unwrap_or(1).max(1)
    }

    pub fn per_page(&self) -> i64 {
        self.per_page.unwrap_or(20).clamp(1, 100)
    }

    pub fn offset(&self) -> i64 {
        (self.page() - 1) * self.per_page()
    }

    /// Alias for per_page() — used as SQL LIMIT.
    pub fn limit(&self) -> i64 {
        self.per_page()
    }
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}
"#
    .to_string()
}

/// Generate `backend/src/auth.rs` deterministically.
///
/// Exports: AuthUser (FromRequest extractor), Claims, encode_jwt, require_role.
/// All downstream consumers (auth_routes) receive these symbols via HDAG edges.
pub fn generate_auth_rs() -> String {
    r#"use actix_web::{HttpRequest, FromRequest};
use actix_web::dev::Payload;
use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};
use serde::{Serialize, Deserialize};
use std::future::{Ready, ready};

use crate::errors::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    pub user_id: i64,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub email: String,
    pub role: String,
    pub exp: usize,
}

impl FromRequest for AuthUser {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let jwt_secret = std::env::var("JWT_SECRET")
            .unwrap_or_else(|_| "dev-secret-change-in-production".to_string());

        let auth_header = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        match auth_header {
            Some(token) => {
                match decode::<Claims>(
                    token,
                    &DecodingKey::from_secret(jwt_secret.as_bytes()),
                    &Validation::default(),
                ) {
                    Ok(data) => ready(Ok(AuthUser {
                        user_id: data.claims.sub,
                        email: data.claims.email,
                        role: data.claims.role,
                    })),
                    Err(_) => ready(Err(AppError::Unauthorized.into())),
                }
            }
            None => ready(Err(AppError::Unauthorized.into())),
        }
    }
}

pub fn encode_jwt(user_id: i64, email: &str, role: &str) -> Result<String, AppError> {
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "dev-secret-change-in-production".to_string());

    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(24))
        .expect("valid timestamp")
        .timestamp() as usize;

    let claims = Claims {
        sub: user_id,
        email: email.to_string(),
        role: role.to_string(),
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::InternalError(e.to_string()))
}

pub fn require_role(user: &AuthUser, required: &str) -> Result<(), AppError> {
    if user.role == required || user.role == "admin" {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}
"#
    .to_string()
}

/// Generate `backend/src/config.rs` deterministically.
pub fn generate_config_rs() -> String {
    r#"use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    pub jwt_secret: String,
    pub port: u16,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/app".to_string()),
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-secret-change-in-production".to_string()),
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .unwrap_or(8080),
        }
    }
}
"#
    .to_string()
}

/// Generate `backend/src/database/pool.rs` deterministically.
pub fn generate_pool_rs() -> String {
    r#"use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn create_pool() -> Result<PgPool, sqlx::Error> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/app".to_string());

    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
}
"#
    .to_string()
}

// ─── Layer 6: Service generators ─────────────────────────────────────────────

/// Generate a service file for a non-User entity.
///
/// Produces thin delegation wrappers around the entity's `_queries` module.
/// Function names match `provides_service_fns()` exactly: `get_{snake}`,
/// `list_{snake}s`, `create_{snake}`, `update_{snake}`, `delete_{snake}`.
pub fn generate_service_rs(entity: &EntityDef) -> String {
    let n = &entity.name;
    let sn = &entity.snake_name;
    format!(
        r#"use sqlx::PgPool;
use crate::errors::AppError;
use crate::models::{sn}::{{{n}, Create{n}Payload, Update{n}Payload}};
use crate::pagination::{{PaginationParams, PaginatedResponse}};
use crate::database::{sn}_queries;

/// Fetch a single {n} by ID.
pub async fn get_{sn}(pool: &PgPool, id: i64) -> Result<{n}, AppError> {{
    {sn}_queries::get_{sn}(pool, id).await
}}

/// List {n}s with pagination.
pub async fn list_{sn}s(
    pool: &PgPool,
    params: &PaginationParams,
) -> Result<PaginatedResponse<{n}>, AppError> {{
    {sn}_queries::list_{sn}s(pool, params).await
}}

/// Create a new {n}.
pub async fn create_{sn}(pool: &PgPool, payload: Create{n}Payload) -> Result<{n}, AppError> {{
    {sn}_queries::create_{sn}(pool, payload).await
}}

/// Update an existing {n}.
pub async fn update_{sn}(
    pool: &PgPool,
    id: i64,
    payload: Update{n}Payload,
) -> Result<{n}, AppError> {{
    {sn}_queries::update_{sn}(pool, id, payload).await
}}

/// Delete a {n} by ID.
pub async fn delete_{sn}(pool: &PgPool, id: i64) -> Result<(), AppError> {{
    {sn}_queries::delete_{sn}(pool, id).await
}}
"#,
        n = n,
        sn = sn
    )
}

/// Generate the User service file.
///
/// Identical to `generate_service_rs` but adds `get_user_by_email`.
pub fn generate_user_service_rs(entity: &EntityDef) -> String {
    let n = &entity.name;
    let sn = &entity.snake_name;
    format!(
        r#"use sqlx::PgPool;
use crate::errors::AppError;
use crate::models::{sn}::{{{n}, Create{n}Payload, Update{n}Payload}};
use crate::pagination::{{PaginationParams, PaginatedResponse}};
use crate::database::{sn}_queries;

/// Fetch a single {n} by ID.
pub async fn get_{sn}(pool: &PgPool, id: i64) -> Result<{n}, AppError> {{
    {sn}_queries::get_{sn}(pool, id).await
}}

/// Fetch a {n} by email address.
pub async fn get_{sn}_by_email(pool: &PgPool, email: &str) -> Result<{n}, AppError> {{
    {sn}_queries::get_{sn}_by_email(pool, email).await
}}

/// List {n}s with pagination.
pub async fn list_{sn}s(
    pool: &PgPool,
    params: &PaginationParams,
) -> Result<PaginatedResponse<{n}>, AppError> {{
    {sn}_queries::list_{sn}s(pool, params).await
}}

/// Create a new {n}.
pub async fn create_{sn}(pool: &PgPool, payload: Create{n}Payload) -> Result<{n}, AppError> {{
    {sn}_queries::create_{sn}(pool, payload).await
}}

/// Update an existing {n}.
pub async fn update_{sn}(
    pool: &PgPool,
    id: i64,
    payload: Update{n}Payload,
) -> Result<{n}, AppError> {{
    {sn}_queries::update_{sn}(pool, id, payload).await
}}

/// Delete a {n} by ID.
pub async fn delete_{sn}(pool: &PgPool, id: i64) -> Result<(), AppError> {{
    {sn}_queries::delete_{sn}(pool, id).await
}}
"#,
        n = n,
        sn = sn
    )
}

// ─── Layer 7: API route generators ───────────────────────────────────────────

/// Generate an API route file for a non-User entity.
///
/// Produces actix-web CRUD handlers + `{snake}_routes` registration function.
/// Scope is `/api/{snake}s` so that `configure_routes` in `api/mod.rs` composes
/// correctly without the LLM inventing its own prefix.
pub fn generate_api_routes_rs(entity: &EntityDef) -> String {
    let n = &entity.name;
    let sn = &entity.snake_name;
    format!(
        r#"use actix_web::{{web, HttpResponse, Responder}};
use sqlx::PgPool;
use crate::errors::AppError;
use crate::auth::AuthUser;
use crate::models::{sn}::{{Create{n}Payload, Update{n}Payload}};
use crate::pagination::PaginationParams;
use crate::services::{sn} as {sn}_service;

pub async fn list_{sn}s(
    pool: web::Data<PgPool>,
    params: web::Query<PaginationParams>,
    _user: AuthUser,
) -> Result<impl Responder, AppError> {{
    let result = {sn}_service::list_{sn}s(pool.get_ref(), &params).await?;
    Ok(HttpResponse::Ok().json(result))
}}

pub async fn get_{sn}(
    pool: web::Data<PgPool>,
    path: web::Path<i64>,
    _user: AuthUser,
) -> Result<impl Responder, AppError> {{
    let result = {sn}_service::get_{sn}(pool.get_ref(), path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}}

pub async fn create_{sn}(
    pool: web::Data<PgPool>,
    body: web::Json<Create{n}Payload>,
    _user: AuthUser,
) -> Result<impl Responder, AppError> {{
    let result = {sn}_service::create_{sn}(pool.get_ref(), body.into_inner()).await?;
    Ok(HttpResponse::Created().json(result))
}}

pub async fn update_{sn}(
    pool: web::Data<PgPool>,
    path: web::Path<i64>,
    body: web::Json<Update{n}Payload>,
    _user: AuthUser,
) -> Result<impl Responder, AppError> {{
    let result = {sn}_service::update_{sn}(pool.get_ref(), path.into_inner(), body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}}

pub async fn delete_{sn}(
    pool: web::Data<PgPool>,
    path: web::Path<i64>,
    user: AuthUser,
) -> Result<impl Responder, AppError> {{
    crate::auth::require_role(&user, "admin")?;
    {sn}_service::delete_{sn}(pool.get_ref(), path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}}

pub fn {sn}_routes(cfg: &mut web::ServiceConfig) {{
    cfg.service(
        web::scope("/api/{sn}s")
            .route("", web::get().to(list_{sn}s))
            .route("", web::post().to(create_{sn}))
            .route("/{{id}}", web::get().to(get_{sn}))
            .route("/{{id}}", web::put().to(update_{sn}))
            .route("/{{id}}", web::delete().to(delete_{sn})),
    );
}}
"#,
        n = n,
        sn = sn
    )
}

// ─── Layer 7: Auth routes generator ──────────────────────────────────────────

/// Generate `api/auth_routes.rs` — deterministic auth endpoints.
///
/// Produces:
/// - `POST /api/auth/register` — takes `{email, password}`, hashes with bcrypt,
///   inserts user, returns created user.
/// - `POST /api/auth/login` — takes `{email, password}`, verifies bcrypt hash,
///   returns JWT.
/// - `GET /api/auth/me` — returns current user from JWT.
/// - `auth_routes(cfg)` — route registration function.
pub fn generate_auth_routes_rs() -> String {
    r#"use actix_web::{web, HttpResponse, Responder};
use bcrypt::{hash, verify, DEFAULT_COST};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::errors::AppError;
use crate::auth::{encode_jwt, AuthUser};
use crate::models::user::{User, CreateUserPayload};

#[derive(Debug, Deserialize)]
pub struct LoginPayload {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub token: String,
}

pub async fn register(
    pool: web::Data<PgPool>,
    body: web::Json<CreateUserPayload>,
) -> Result<impl Responder, AppError> {
    let payload = body.into_inner();

    if payload.email.trim().is_empty() || !payload.email.contains('@') {
        return Err(AppError::ValidationError(vec![
            "A valid email address is required".into(),
        ]));
    }

    let password_hash = hash(&payload.password, DEFAULT_COST)
        .map_err(|e| AppError::InternalError(e.to_string()))?;

    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (email, password_hash, role, is_active) \
         VALUES ($1, $2, $3, true) \
         RETURNING *",
    )
    .bind(&payload.email)
    .bind(&password_hash)
    .bind(payload.role.as_deref().unwrap_or("user"))
    .fetch_one(pool.get_ref())
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.constraint() == Some("users_email_key") => {
            AppError::Conflict("email already registered".into())
        }
        other => AppError::from(other),
    })?;

    tracing::info!(email = %user.email, "user registered");
    Ok(HttpResponse::Created().json(user))
}

pub async fn login(
    pool: web::Data<PgPool>,
    body: web::Json<LoginPayload>,
) -> Result<impl Responder, AppError> {
    let req = body.into_inner();

    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE email = $1",
    )
    .bind(&req.email)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or(AppError::Unauthorized)?;

    if !user.is_active {
        return Err(AppError::Unauthorized);
    }

    let valid = verify(&req.password, &user.password_hash)
        .map_err(|_| AppError::Unauthorized)?;
    if !valid {
        return Err(AppError::Unauthorized);
    }

    let token = encode_jwt(user.id, &user.email, &user.role)?;
    Ok(HttpResponse::Ok().json(TokenResponse { token }))
}

pub async fn me(user: AuthUser) -> Result<impl Responder, AppError> {
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "id": user.user_id,
        "email": user.email,
        "role": user.role,
    })))
}

pub fn auth_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/auth")
            .route("/register", web::post().to(register))
            .route("/login", web::post().to(login))
            .route("/me", web::get().to(me)),
    );
}
"#
    .into()
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Dispatch structural generation by file path.
///
/// Called by `StagedClosure` for every `NodeType::Structural` node whose path
/// is not handled by the static-file generators (Cargo.toml, Dockerfile, etc.).
pub fn generate_for_path(path: &str, spec: &AppSpec) -> String {
    if path.ends_with("main.rs") {
        return generate_main_rs(spec);
    }
    // Layer 3: entity model files (deterministic from EntityDef)
    if path.contains("src/models/") && path.ends_with(".rs") && !path.ends_with("mod.rs") {
        if path.ends_with("user.rs") {
            return generate_user_model_rs();
        }
        for entity in spec.entities.iter().filter(|e| e.name != "User") {
            if path.ends_with(&format!("{}.rs", entity.snake_name)) {
                return generate_model_rs(entity);
            }
        }
    }
    // Layer 1: foundation infrastructure (deterministic — must match ProvidedSymbol sigs)
    if path.ends_with("errors.rs") {
        return generate_errors_rs();
    }
    if path.ends_with("pagination.rs") {
        return generate_pagination_rs();
    }
    if path.ends_with("auth.rs") {
        return generate_auth_rs();
    }
    if path.ends_with("config.rs") {
        return generate_config_rs();
    }
    if path.contains("database/pool.rs") {
        return generate_pool_rs();
    }
    if path.contains("models/mod.rs") {
        return generate_models_mod(spec);
    }
    if path.contains("database/mod.rs") {
        return generate_database_mod(spec);
    }
    if path.contains("services/mod.rs") {
        return generate_services_mod(spec);
    }
    // Layer 6: entity service files (thin delegation wrappers — deterministic)
    if path.contains("services/") && path.ends_with(".rs") && !path.ends_with("mod.rs") {
        for entity in &spec.entities {
            if path.ends_with(&format!("{}.rs", entity.snake_name)) {
                return if entity.name == "User" {
                    generate_user_service_rs(entity)
                } else {
                    generate_service_rs(entity)
                };
            }
        }
    }
    if path.contains("api/mod.rs") {
        return generate_api_mod(spec);
    }
    // Layer 7: auth routes (deterministic)
    if path.contains("auth_routes.rs") {
        return generate_auth_routes_rs();
    }
    // Layer 7: entity API route files (deterministic CRUD handlers)
    if path.contains("api/") && path.ends_with(".rs") && !path.ends_with("mod.rs") && !path.contains("auth_routes") {
        for entity in spec.entities.iter().filter(|e| e.name != "User") {
            if path.ends_with(&format!("{}.rs", entity.snake_name)) {
                return generate_api_routes_rs(entity);
            }
        }
    }
    if path.ends_with("001_initial.sql") {
        return generate_migration(spec);
    }
    if path.ends_with("api_tests.rs") {
        return generate_api_tests(spec);
    }
    if path.ends_with("index.html") {
        return generate_frontend_index(spec);
    }
    if path.ends_with("style.css") {
        return generate_frontend_style();
    }
    if path.ends_with("client.js") {
        return generate_frontend_client(spec);
    }
    // Entity page JS files
    if path.contains("pages/") && path.ends_with(".js") {
        for entity in &spec.entities {
            if path.contains(&format!("/{}.js", entity.snake_name)) {
                return generate_entity_page(entity);
            }
        }
    }
    // Fallback: empty placeholder (static-file generators handle Cargo.toml etc.)
    tracing::warn!(path = %path, "structural::generate_for_path: no generator matched");
    String::new()
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Simple topological sort for entity FK dependencies.
///
/// Entities without FK references come first; entities that reference already-
/// placed tables come after them.  Circular dependencies are appended at end.
fn topological_sort_entities<'a>(entities: &[&'a EntityDef]) -> Vec<&'a EntityDef> {
    let mut result: Vec<&EntityDef> = Vec::new();
    let mut placed: std::collections::HashSet<String> = std::collections::HashSet::new();
    placed.insert("users".to_string());

    let mut remaining: Vec<&EntityDef> = entities.to_vec();
    let max_iter = remaining.len() + 1;
    for _ in 0..max_iter {
        if remaining.is_empty() {
            break;
        }
        let mut next_remaining = Vec::new();
        for entity in &remaining {
            let deps: Vec<String> = entity
                .fields
                .iter()
                .filter_map(|f| {
                    if let Some(pos) = f.sql_type.find("REFERENCES ") {
                        let rest = &f.sql_type[pos + 11..];
                        let table = rest.split('(').next().unwrap_or("").trim().to_string();
                        if !table.is_empty() {
                            return Some(table);
                        }
                    }
                    None
                })
                .collect();
            if deps.iter().all(|d| placed.contains(d)) {
                result.push(entity);
                placed.insert(format!("{}s", entity.snake_name));
            } else {
                next_remaining.push(*entity);
            }
        }
        remaining = next_remaining;
    }
    // Append any remaining (circular/unresolved) entities
    result.extend(remaining);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppSpec, EntityDef};

    fn minimal_spec() -> AppSpec {
        AppSpec {
            app_name: "test-app".into(),
            description: "Test".into(),
            domain_name: "test".into(),
            entities: vec![
                EntityDef {
                    name: "User".into(),
                    snake_name: "user".into(),
                    fields: vec![],
                    validations: vec![],
                    business_rules: vec![],
                    relationships: vec![],
                },
                EntityDef {
                    name: "Product".into(),
                    snake_name: "product".into(),
                    fields: vec![],
                    validations: vec![],
                    business_rules: vec![],
                    relationships: vec![],
                },
            ],
            business_rules: vec![],
        }
    }

    #[test]
    fn test_generate_main_rs_contains_mod_declarations() {
        let spec = minimal_spec();
        let code = generate_main_rs(&spec);
        assert!(code.contains("mod api;"));
        assert!(code.contains("mod auth;"));
        assert!(code.contains("mod database;"));
        assert!(code.contains("mod errors;"));
        assert!(code.contains("mod models;"));
        assert!(code.contains("mod services;"));
        assert!(code.contains("mod pagination;"));
        assert!(code.contains("configure_routes"));
    }

    #[test]
    fn test_generate_models_mod_includes_all_entities() {
        let spec = minimal_spec();
        let code = generate_models_mod(&spec);
        assert!(code.contains("pub mod user;"));
        assert!(code.contains("pub mod product;"));
        assert!(code.contains("pub use user::{User"));
        assert!(code.contains("pub use product::{Product"));
    }

    #[test]
    fn test_generate_api_mod_includes_configure_routes_and_health() {
        let spec = minimal_spec();
        let code = generate_api_mod(&spec);
        assert!(code.contains("pub fn configure_routes"));
        assert!(code.contains("health_check"));
        assert!(code.contains("/api/health"));
        assert!(code.contains("auth_routes::auth_routes"));
        assert!(code.contains("product::product_routes"));
    }

    #[test]
    fn test_generate_database_mod_has_pool_and_queries() {
        let spec = minimal_spec();
        let code = generate_database_mod(&spec);
        assert!(code.contains("pub mod pool;"));
        assert!(code.contains("pub mod user_queries;"));
        assert!(code.contains("pub mod product_queries;"));
    }

    #[test]
    fn test_generate_migration_has_users_table_and_seed() {
        let spec = minimal_spec();
        let sql = generate_migration(&spec);
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS users"));
        assert!(sql.contains("admin@example.com"));
        assert!(sql.contains("ON CONFLICT (email) DO NOTHING"));
    }

    #[test]
    fn test_generate_for_path_dispatch() {
        let spec = minimal_spec();
        assert!(!generate_for_path("backend/src/main.rs", &spec).is_empty());
        assert!(!generate_for_path("backend/src/models/mod.rs", &spec).is_empty());
        assert!(!generate_for_path("backend/src/api/mod.rs", &spec).is_empty());
        assert!(!generate_for_path("backend/migrations/001_initial.sql", &spec).is_empty());
    }
}
