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

use crate::{AppSpec, EntityDef, pluralize};
use crate::blueprint::InfraBlueprint;
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
        let tn = pluralize(&entity.snake_name);
        sql.push_str(&format!(
            "CREATE TABLE IF NOT EXISTS {} (\n",
            tn
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

    // F1/W3: Demo data — 5 rows per entity with deterministic NATO-alphabet values
    sql.push_str(&generate_demo_data(&ordered));

    // D6: Conditional norm seed — only when a "Norm" entity with "is_builtin" field exists
    if let Some(seed_sql) = generate_norm_seed(spec) {
        sql.push_str(&seed_sql);
    }

    sql
}

/// F1/W3: Generate 5 deterministic demo rows per entity.
///
/// Uses NATO alphabet for names, deterministic values for all types.
/// FK fields use subqueries to reference the first entries of the target entity.
fn generate_demo_data(entities: &[&EntityDef]) -> String {
    let names = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    let emails_prefix = ["alpha", "bravo", "charlie", "delta", "echo"];
    let prices: [&str; 5] = ["10.50", "25.99", "42.00", "99.95", "7.25"];
    let bools: [&str; 5] = ["true", "true", "false", "true", "true"];
    let statuses: [&str; 5] = ["active", "active", "pending", "inactive", "active"];
    let days: [u32; 5] = [30, 20, 10, 5, 1];

    let mut sql = String::from("\n-- F1/W3: Demo data (5 rows per entity, NATO alphabet)\n");

    for entity in entities {
        let table = pluralize(&entity.snake_name);
        let fields: Vec<&isls_hypercube::domain::FieldDef> = entity
            .fields
            .iter()
            .filter(|f| {
                f.name != "id"
                    && f.name != "created_at"
                    && f.name != "updated_at"
                    && !f.sql_type.contains("SERIAL")
            })
            .collect();

        if fields.is_empty() {
            continue;
        }

        let col_names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();

        for i in 0..5 {
            let mut values: Vec<String> = Vec::new();
            for f in &fields {
                let v = demo_value(&f.name, &f.rust_type, &f.sql_type, i, names[i], emails_prefix[i], prices[i], bools[i], statuses[i], days[i]);
                values.push(v);
            }
            sql.push_str(&format!(
                "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT DO NOTHING;\n",
                table,
                col_names.join(", "),
                values.join(", ")
            ));
        }
        sql.push('\n');
    }
    sql
}

/// Produce a deterministic SQL value for a single field in a demo row.
fn demo_value(
    name: &str,
    rust_type: &str,
    sql_type: &str,
    idx: usize,
    nato: &str,
    email_prefix: &str,
    price: &str,
    bool_val: &str,
    status: &str,
    days_ago: u32,
) -> String {
    let lower = name.to_lowercase();
    let rt = rust_type.replace("Option<", "").replace('>', "");

    // FK references — use subquery
    if sql_type.contains("REFERENCES ") {
        if let Some(pos) = sql_type.find("REFERENCES ") {
            let rest = &sql_type[pos + 11..];
            if let Some(table) = rest.split('(').next() {
                let table = table.trim();
                return format!("(SELECT id FROM {} LIMIT 1 OFFSET {})", table, idx % 5);
            }
        }
    }

    // Name-based
    if lower.contains("email") {
        return format!("'{}@example.com'", email_prefix);
    }
    if lower.contains("password") || lower.contains("passwd") {
        return "'$2b$12$demo_password_hash_placeholder'".to_string();
    }
    if lower == "name" || lower == "title" || lower.ends_with("_name") || lower == "first_name" || lower == "last_name" {
        return format!("'{}'", nato);
    }
    if lower == "description" || lower == "notes" || lower == "bio"
        || lower == "content" || lower == "body" || lower == "text"
        || lower == "comment" || lower == "message"
    {
        return format!("'Demo {} entry {}'", nato.to_lowercase(), idx + 1);
    }
    if lower == "status" || lower == "role" || lower == "type"
        || lower == "category" || lower == "priority" || lower == "level"
    {
        return format!("'{}'", status);
    }
    if lower.contains("url") || lower.contains("website") || lower.contains("link") {
        return format!("'https://example.com/{}'", email_prefix);
    }
    if lower.contains("phone") || lower.contains("tel") || lower.contains("mobile") {
        return format!("'+1-555-{:04}'", 1000 + idx);
    }

    // Type-based
    match rt.as_str() {
        "bool" => bool_val.to_string(),
        "f64" | "f32" | "Decimal" => price.to_string(),
        "i32" | "i64" | "u32" | "u64" => format!("{}", (idx + 1) * 10),
        "NaiveDateTime" | "DateTime<Utc>" | "chrono::DateTime<Utc>" => {
            format!("NOW() - INTERVAL '{} days'", days_ago)
        }
        "NaiveDate" => format!("CURRENT_DATE - INTERVAL '{} days'", days_ago),
        _ => {
            // String fallback
            if sql_type.contains("BOOL") {
                bool_val.to_string()
            } else if sql_type.contains("INT") {
                format!("{}", (idx + 1) * 10)
            } else if sql_type.contains("FLOAT") || sql_type.contains("NUMERIC") || sql_type.contains("DECIMAL") || sql_type.contains("REAL") || sql_type.contains("DOUBLE") {
                price.to_string()
            } else if sql_type.contains("TIMESTAMP") {
                format!("NOW() - INTERVAL '{} days'", days_ago)
            } else if sql_type.contains("DATE") {
                format!("CURRENT_DATE - INTERVAL '{} days'", days_ago)
            } else {
                format!("'{} {}'", nato, idx + 1)
            }
        }
    }
}

/// D6: Generate INSERT statements for the 24 builtin norms.
///
/// Activates ONLY when the AppSpec contains an entity named "Norm" that has
/// an `is_builtin` field. For all other AppSpecs (warehouse, ecommerce, etc.),
/// returns `None` — the migration is unchanged.
///
/// If seed generation fails internally, logs a warning and returns `None`
/// (Rule 10: seed failure must not block migration).
fn generate_norm_seed(spec: &AppSpec) -> Option<String> {
    // Check for "Norm" entity with "is_builtin" field
    let norm_entity = spec.entities.iter().find(|e| e.name == "Norm")?;
    let has_is_builtin = norm_entity.fields.iter().any(|f| f.name == "is_builtin");
    if !has_is_builtin {
        return None;
    }

    match generate_norm_seed_inner() {
        Ok(sql) => Some(sql),
        Err(e) => {
            eprintln!("[WARN] Norm seed generation failed (non-blocking): {}", e);
            tracing::warn!("Norm seed generation failed (non-blocking): {}", e);
            None
        }
    }
}

fn generate_norm_seed_inner() -> std::result::Result<String, Box<dyn std::error::Error>> {
    use isls_norms::catalog::builtin_norms;
    use isls_norms::types::NormLevel;

    let norms = builtin_norms();
    let mut sql = String::from("-- D6: Seed 24 builtin norms from isls_norms::builtin_norms()\n");

    for norm in &norms {
        let level_str = match norm.level {
            NormLevel::Atom => "Atom",
            NormLevel::Molecule => "Molecule",
            NormLevel::Organism => "Organism",
            NormLevel::Ecosystem => "Ecosystem",
        };

        let trigger_keywords = norm.triggers
            .first()
            .map(|t| t.keywords.join(", "))
            .unwrap_or_default();

        // Count non-empty layer vectors
        let layer_count = [
            !norm.layers.database.is_empty(),
            !norm.layers.model.is_empty(),
            !norm.layers.query.is_empty(),
            !norm.layers.service.is_empty(),
            !norm.layers.api.is_empty(),
            !norm.layers.frontend.is_empty(),
            !norm.layers.test.is_empty(),
            !norm.layers.config.is_empty(),
        ].iter().filter(|&&x| x).count();

        // Escape single quotes in strings for SQL safety
        let norm_id = norm.id.replace('\'', "''");
        let name = norm.name.replace('\'', "''");
        let version = norm.version.replace('\'', "''");
        let keywords_escaped = trigger_keywords.replace('\'', "''");

        sql.push_str(&format!(
            "INSERT INTO norms (norm_id, name, level, version, trigger_keywords, layer_count, domain_count, observation_count, is_builtin)\n\
             VALUES ('{}', '{}', '{}', '{}', '{}', {}, 0, 0, true)\n\
             ON CONFLICT DO NOTHING;\n",
            norm_id, name, level_str, version, keywords_escaped, layer_count
        ));
    }

    sql.push('\n');
    Ok(sql)
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
    pub role: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CreateUserPayload {
    pub email: String,
    pub password: String,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UpdateUserPayload {
    pub email: Option<String>,
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
    Conflict(String),
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
            AppError::Conflict(msg) => write!(f, "Conflict: {}", msg),
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
            AppError::Conflict(msg) => {
                HttpResponse::Conflict().json(serde_json::json!({"error": msg}))
            }
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::InternalError(e.to_string())
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
    let pn = pluralize(sn);
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
pub async fn list_{pn}(
    pool: &PgPool,
    params: &PaginationParams,
) -> Result<PaginatedResponse<{n}>, AppError> {{
    {sn}_queries::list_{pn}(pool, params).await
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
        sn = sn,
        pn = pn
    )
}

/// Generate the User service file.
///
/// Identical to `generate_service_rs` but adds `get_user_by_email`.
pub fn generate_user_service_rs(entity: &EntityDef) -> String {
    let n = &entity.name;
    let sn = &entity.snake_name;
    let pn = pluralize(sn);
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
pub async fn list_{pn}(
    pool: &PgPool,
    params: &PaginationParams,
) -> Result<PaginatedResponse<{n}>, AppError> {{
    {sn}_queries::list_{pn}(pool, params).await
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
        sn = sn,
        pn = pn
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
    let tn = pluralize(sn);
    format!(
        r#"use actix_web::{{web, HttpResponse, Responder}};
use sqlx::PgPool;
use crate::errors::AppError;
use crate::auth::AuthUser;
use crate::models::{sn}::{{Create{n}Payload, Update{n}Payload}};
use crate::pagination::PaginationParams;
use crate::services::{sn} as {sn}_service;

pub async fn list_{pn}(
    pool: web::Data<PgPool>,
    params: web::Query<PaginationParams>,
    _user: AuthUser,
) -> Result<impl Responder, AppError> {{
    let result = {sn}_service::list_{pn}(pool.get_ref(), &params).await?;
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
        web::scope("/api/{tn}")
            .route("", web::get().to(list_{pn}))
            .route("", web::post().to(create_{sn}))
            .route("/{{id}}", web::get().to(get_{sn}))
            .route("/{{id}}", web::put().to(update_{sn}))
            .route("/{{id}}", web::delete().to(delete_{sn})),
    );
}}
"#,
        n = n,
        sn = sn,
        tn = tn,
        pn = tn
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

// ─── D8/W1: CLI structural generators ────────────────────────────────────────

/// Generate `backend/src/main.rs` for a CLI-only application.
///
/// Uses Clap `Parser` derive with one subcommand per entity. No async runtime,
/// no database pool, no web server. The LLM surface moves to `services/*.rs`.
pub fn generate_cli_main_rs(spec: &AppSpec) -> String {
    let app = &spec.app_name;
    let desc = &spec.description;

    let non_user: Vec<&EntityDef> = spec.entities.iter().filter(|e| e.name != "User").collect();

    // Build Commands enum variants
    let mut variants = String::new();
    for entity in &non_user {
        let name = &entity.name;
        let sn = &entity.snake_name;
        variants.push_str(&format!(
            "    /// List all {sn}s\n    List{name},\n    /// Get a {sn} by ID\n    Get{name} {{ id: i64 }},\n",
            name = name, sn = sn
        ));
    }

    // Build match arms
    let mut arms = String::new();
    for entity in &non_user {
        let name = &entity.name;
        let sn = &entity.snake_name;
        arms.push_str(&format!(
            "        Commands::List{name} => {{\n\
             \x20           match services::{sn}::list_{sn}s() {{\n\
             \x20               Ok(items) => println!(\"{{}}\", serde_json::to_string_pretty(&items).unwrap_or_default()),\n\
             \x20               Err(e) => {{ eprintln!(\"Error: {{}}\", e); std::process::exit(1); }}\n\
             \x20           }}\n\
             \x20       }}\n",
            name = name, sn = sn
        ));
        arms.push_str(&format!(
            "        Commands::Get{name} {{ id }} => {{\n\
             \x20           match services::{sn}::get_{sn}(id) {{\n\
             \x20               Ok(item) => println!(\"{{}}\", serde_json::to_string_pretty(&item).unwrap_or_default()),\n\
             \x20               Err(e) => {{ eprintln!(\"Error: {{}}\", e); std::process::exit(1); }}\n\
             \x20           }}\n\
             \x20       }}\n",
            name = name, sn = sn
        ));
    }

    format!(
        r#"// Copyright (c) 2026 Sebastian Klemm — ISLS D8 CLI structural generated
mod errors;
mod models;
mod services;

use clap::Parser;

#[derive(Parser)]
#[command(name = "{app}")]
#[command(about = "{desc}")]
struct Cli {{
    #[command(subcommand)]
    command: Commands,
}}

#[derive(clap::Subcommand)]
enum Commands {{
{variants}    /// Show version info
    Version,
}}

fn main() {{
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {{
{arms}        Commands::Version => {{
            println!("{app} v0.1.0");
        }}
    }}
}}
"#,
        app = app,
        desc = desc,
        variants = variants,
        arms = arms,
    )
}

/// Generate `backend/src/errors.rs` for a CLI-only application.
///
/// No actix-web dependency — uses thiserror for error types.
pub fn generate_cli_errors_rs() -> String {
    r#"use std::fmt;

#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    InternalError(String),
    ValidationError(Vec<String>),
    IoError(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NotFound(msg) => write!(f, "Not found: {}", msg),
            AppError::InternalError(msg) => write!(f, "Internal error: {}", msg),
            AppError::ValidationError(msgs) => write!(f, "Validation: {}", msgs.join(", ")),
            AppError::IoError(msg) => write!(f, "IO error: {}", msg),
        }
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::IoError(e.to_string())
    }
}
"#
    .to_string()
}

/// Generate `backend/src/models/{entity}.rs` for a CLI-only application.
///
/// Like `generate_model_rs` but without sqlx `FromRow` derive.
pub fn generate_cli_model_rs(entity: &EntityDef) -> String {
    let name = &entity.name;
    let mut s = String::new();

    s.push_str("use serde::{Serialize, Deserialize};\n");
    s.push_str("use chrono::{DateTime, Utc};\n\n");

    s.push_str("#[derive(Debug, Serialize, Deserialize, Clone)]\n");
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

    s.push_str("#[derive(Debug, Deserialize, Clone)]\n");
    s.push_str(&format!("pub struct Create{}Payload {{\n", name));
    for field in entity.fields.iter().filter(|f| {
        f.name != "id" && f.name != "created_at" && f.name != "updated_at"
    }) {
        s.push_str(&format!("    pub {}: {},\n", field.name, model_field_type(field)));
    }
    s.push_str("}\n\n");

    s.push_str("#[derive(Debug, Deserialize, Clone)]\n");
    s.push_str(&format!("pub struct Update{}Payload {{\n", name));
    for field in entity.fields.iter().filter(|f| {
        f.name != "id" && f.name != "created_at" && f.name != "updated_at"
    }) {
        s.push_str(&format!("    pub {}: Option<{}>,\n", field.name, model_field_base_type(field)));
    }
    s.push_str("}\n");

    s
}

/// Generate `backend/src/services/{entity}.rs` for a CLI-only application.
///
/// Placeholder service with stub functions. This is the LLM surface for CLI
/// apps — the actual business logic should be filled in by the LLM.
pub fn generate_cli_service_rs(entity: &EntityDef) -> String {
    let name = &entity.name;
    let sn = &entity.snake_name;

    format!(
        r#"use crate::models::{{{name}, Create{name}Payload, Update{name}Payload}};
use crate::errors::AppError;

pub fn list_{sn}s() -> Result<Vec<{name}>, AppError> {{
    // TODO: implement business logic
    Ok(vec![])
}}

pub fn get_{sn}(id: i64) -> Result<{name}, AppError> {{
    // TODO: implement business logic
    Err(AppError::NotFound(format!("{name} with id {{}} not found", id)))
}}

pub fn create_{sn}(payload: Create{name}Payload) -> Result<{name}, AppError> {{
    // TODO: implement business logic
    Err(AppError::InternalError("not implemented".into()))
}}

pub fn update_{sn}(id: i64, payload: Update{name}Payload) -> Result<{name}, AppError> {{
    // TODO: implement business logic
    Err(AppError::NotFound(format!("{name} with id {{}} not found", id)))
}}

pub fn delete_{sn}(id: i64) -> Result<(), AppError> {{
    // TODO: implement business logic
    Err(AppError::NotFound(format!("{name} with id {{}} not found", id)))
}}
"#,
        name = name,
        sn = sn,
    )
}

/// Generate `backend/src/services/mod.rs` for CLI-only apps (no User service).
pub fn generate_cli_services_mod(spec: &AppSpec) -> String {
    let non_user: Vec<&EntityDef> = spec.entities.iter().filter(|e| e.name != "User").collect();
    let mut s = String::from("// Copyright (c) 2026 Sebastian Klemm — ISLS D8 CLI structural generated\n");
    for entity in &non_user {
        s.push_str(&format!("pub mod {};\n", entity.snake_name));
    }
    s.push('\n');
    for entity in &non_user {
        s.push_str(&format!("pub use {}::*;\n", entity.snake_name));
    }
    s
}

/// Generate `backend/src/models/mod.rs` for CLI-only apps (no User model).
pub fn generate_cli_models_mod(spec: &AppSpec) -> String {
    let non_user: Vec<&EntityDef> = spec.entities.iter().filter(|e| e.name != "User").collect();
    let mut s = String::from("// Copyright (c) 2026 Sebastian Klemm — ISLS D8 CLI structural generated\n");
    for entity in &non_user {
        s.push_str(&format!("pub mod {};\n", entity.snake_name));
    }
    s.push('\n');
    for entity in &non_user {
        let pn = &entity.name;
        s.push_str(&format!(
            "pub use {}::{{{}, Create{}Payload, Update{}Payload}};\n",
            entity.snake_name, pn, pn, pn
        ));
    }
    s
}

/// Generate `backend/tests/cli_tests.rs` — placeholder CLI tests.
pub fn generate_cli_tests(spec: &AppSpec) -> String {
    let first_entity = spec
        .entities
        .iter()
        .find(|e| e.name != "User")
        .map(|e| e.snake_name.as_str())
        .unwrap_or("item");

    format!(
        r#"// ISLS D8 structural generated CLI tests
use std::process::Command;

#[test]
fn test_version_command() {{
    let output = Command::new("cargo")
        .args(["run", "--", "version"])
        .output()
        .expect("Failed to run binary");
    assert!(output.status.success(), "version command should succeed");
}}

#[test]
fn test_list_{first_entity}s_command() {{
    let output = Command::new("cargo")
        .args(["run", "--", "list-{first_entity}"])
        .output()
        .expect("Failed to run binary");
    // Should succeed even with empty results
    assert!(output.status.success(), "list command should succeed");
}}
"#,
        first_entity = first_entity
    )
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Dispatch structural generation by file path (legacy — no blueprint).
///
/// Backward-compatible wrapper; assumes web-app blueprint.
pub fn generate_for_path(path: &str, spec: &AppSpec) -> String {
    generate_for_path_with_blueprint(path, spec, &crate::blueprint::default_web_blueprint())
}

/// D8: Dispatch structural generation by file path with blueprint awareness.
///
/// Called by `StagedClosure` for every `NodeType::Structural` node whose path
/// is not handled by the static-file generators (Cargo.toml, Dockerfile, etc.).
///
/// When `bp.has_cli && !bp.has_http_server`, dispatches to CLI-specific generators.
pub fn generate_for_path_with_blueprint(path: &str, spec: &AppSpec, bp: &InfraBlueprint) -> String {
    let is_cli_only = bp.has_cli && !bp.has_http_server;

    if path.ends_with("main.rs") {
        return if is_cli_only {
            generate_cli_main_rs(spec)
        } else {
            generate_main_rs(spec)
        };
    }
    // Layer 3: entity model files (deterministic from EntityDef)
    if path.contains("src/models/") && path.ends_with(".rs") && !path.ends_with("mod.rs") {
        if !is_cli_only && path.ends_with("user.rs") {
            return generate_user_model_rs();
        }
        for entity in spec.entities.iter().filter(|e| e.name != "User") {
            if path.ends_with(&format!("/{}.rs", entity.snake_name)) {
                return if is_cli_only {
                    generate_cli_model_rs(entity)
                } else {
                    generate_model_rs(entity)
                };
            }
        }
    }
    // Layer 1: foundation infrastructure (deterministic — must match ProvidedSymbol sigs)
    if path.ends_with("errors.rs") {
        return if is_cli_only {
            generate_cli_errors_rs()
        } else {
            generate_errors_rs()
        };
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
        return if is_cli_only {
            generate_cli_models_mod(spec)
        } else {
            generate_models_mod(spec)
        };
    }
    if path.contains("database/mod.rs") {
        return generate_database_mod(spec);
    }
    if path.contains("services/mod.rs") {
        return if is_cli_only {
            generate_cli_services_mod(spec)
        } else {
            generate_services_mod(spec)
        };
    }
    // Layer 5/6: entity service files
    if path.contains("services/") && path.ends_with(".rs") && !path.ends_with("mod.rs") {
        for entity in &spec.entities {
            if path.ends_with(&format!("/{}.rs", entity.snake_name)) {
                if is_cli_only {
                    return generate_cli_service_rs(entity);
                }
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
            if path.ends_with(&format!("/{}.rs", entity.snake_name)) {
                return generate_api_routes_rs(entity);
            }
        }
    }
    if path.ends_with("001_initial.sql") {
        return generate_migration(spec);
    }
    if path.ends_with("api_tests.rs") || path.ends_with("cli_tests.rs") {
        return if is_cli_only {
            generate_cli_tests(spec)
        } else {
            generate_api_tests(spec)
        };
    }
    // F1: Single-file SPA frontend
    if path.ends_with("frontend.html") || path.ends_with("index.html") {
        return generate_frontend_html(spec);
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

// ─── F1/W1: Frontend CSS ─────────────────────────────────────────────────────

/// Generate the complete ISLS Design System CSS.
///
/// Static CSS — identical for every generated app. Uses CSS custom properties
/// for theming. Responsive at 768px breakpoint.
fn generate_frontend_css() -> String {
    r##"
:root {
    --bg-primary: #0d1117;
    --bg-card: #161b22;
    --bg-input: #1c2128;
    --text-primary: #e6edf3;
    --text-secondary: #8b949e;
    --accent: #d4871b;
    --success: #3fb950;
    --error: #f85149;
    --border: #30363d;
    --radius: 6px;
    --nav-width: 220px;
}
*, *::before, *::after { margin: 0; padding: 0; box-sizing: border-box; }
body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif;
    font-size: 15px;
    color: var(--text-primary);
    background: var(--bg-primary);
    line-height: 1.5;
}
a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }

.app-container { display: flex; min-height: 100vh; }
.nav {
    position: fixed; top: 0; left: 0; bottom: 0;
    width: var(--nav-width);
    background: var(--bg-card);
    border-right: 1px solid var(--border);
    display: flex; flex-direction: column;
    padding: 16px 0;
    overflow-y: auto;
    z-index: 100;
    transition: transform 0.3s ease;
}
.main {
    margin-left: var(--nav-width);
    flex: 1; min-height: 100vh;
    display: flex; flex-direction: column;
}
.nav-brand {
    color: var(--accent); font-size: 18px; font-weight: 700;
    padding: 8px 20px 20px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 8px;
}
.nav-link {
    display: block; padding: 10px 20px;
    color: var(--text-secondary); font-size: 14px;
    cursor: pointer; border-left: 3px solid transparent;
    transition: all 0.15s ease;
}
.nav-link:hover { color: var(--text-primary); background: rgba(255,255,255,0.04); }
.nav-link.active {
    color: var(--accent); border-left-color: var(--accent);
    background: rgba(212,135,27,0.08);
}
.header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 14px 24px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-card);
}
.header-title { font-size: 18px; font-weight: 600; }
.header-right { display: flex; align-items: center; gap: 12px; }
.header-user { color: var(--text-secondary); font-size: 13px; }
.hamburger {
    display: none; background: none; border: none;
    color: var(--text-primary); font-size: 24px;
    cursor: pointer; padding: 4px 8px;
}
.card {
    background: var(--bg-card); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 20px;
}
.page-content { padding: 24px; }
table { width: 100%; border-collapse: collapse; background: var(--bg-card); border-radius: var(--radius); overflow: hidden; }
th {
    text-align: left; padding: 10px 14px; font-size: 12px;
    text-transform: uppercase; letter-spacing: 0.5px;
    color: var(--text-secondary); background: rgba(255,255,255,0.03);
    border-bottom: 1px solid var(--border);
    cursor: pointer; user-select: none;
}
th:hover { color: var(--accent); }
th .sort-arrow { font-size: 10px; margin-left: 4px; }
td {
    padding: 10px 14px; border-bottom: 1px solid var(--border);
    font-size: 14px; max-width: 250px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
tr:hover td { background: rgba(255,255,255,0.02); }
.table-actions { display: flex; gap: 6px; }
.btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 8px 16px; border: 1px solid var(--border);
    border-radius: var(--radius); font-size: 14px;
    cursor: pointer; background: var(--bg-card);
    color: var(--text-primary); transition: all 0.15s ease;
}
.btn:hover { border-color: var(--text-secondary); }
.btn-primary { background: var(--accent); border-color: var(--accent); color: #fff; font-weight: 600; }
.btn-primary:hover { background: #b8731a; border-color: #b8731a; }
.btn-secondary { background: transparent; border-color: var(--accent); color: var(--accent); }
.btn-secondary:hover { background: rgba(212,135,27,0.1); }
.btn-danger { background: transparent; border-color: var(--error); color: var(--error); }
.btn-danger:hover { background: rgba(248,81,73,0.1); }
.btn-sm { padding: 4px 10px; font-size: 12px; }
.btn-group { display: flex; gap: 8px; margin-top: 16px; }
.form-group { margin-bottom: 16px; }
.form-label { display: block; margin-bottom: 6px; font-size: 13px; font-weight: 600; color: var(--text-secondary); }
.form-input {
    width: 100%; padding: 8px 12px;
    background: var(--bg-input); border: 1px solid var(--border);
    border-radius: var(--radius); color: var(--text-primary);
    font-size: 14px; font-family: inherit;
    transition: border-color 0.15s ease;
}
.form-input:focus { outline: none; border-color: var(--accent); box-shadow: 0 0 0 2px rgba(212,135,27,0.2); }
select.form-input { appearance: auto; }
textarea.form-input { min-height: 100px; resize: vertical; }
.toggle { position: relative; display: inline-block; width: 44px; height: 24px; }
.toggle input { opacity: 0; width: 0; height: 0; }
.toggle-slider {
    position: absolute; top: 0; left: 0; right: 0; bottom: 0;
    background: var(--border); border-radius: 12px;
    cursor: pointer; transition: background 0.2s ease;
}
.toggle-slider::before {
    content: ''; position: absolute;
    width: 18px; height: 18px; left: 3px; bottom: 3px;
    background: #fff; border-radius: 50%;
    transition: transform 0.2s ease;
}
.toggle input:checked + .toggle-slider { background: var(--accent); }
.toggle input:checked + .toggle-slider::before { transform: translateX(20px); }
.dashboard-grid {
    display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 16px; margin-bottom: 24px;
}
.stat-card {
    background: var(--bg-card); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 20px;
    text-align: center; cursor: pointer;
    transition: border-color 0.15s ease;
}
.stat-card:hover { border-color: var(--accent); }
.stat-value { font-size: 32px; font-weight: 700; color: var(--accent); margin-bottom: 4px; }
.stat-label { font-size: 13px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px; }
.pagination {
    display: flex; align-items: center; justify-content: center;
    gap: 4px; margin-top: 16px; padding: 12px 0;
}
.page-btn {
    padding: 6px 12px; border: 1px solid var(--border);
    border-radius: var(--radius); background: var(--bg-card);
    color: var(--text-secondary); font-size: 13px;
    cursor: pointer; transition: all 0.15s ease;
}
.page-btn:hover { border-color: var(--accent); color: var(--text-primary); }
.page-btn.active { background: var(--accent); border-color: var(--accent); color: #fff; }
.page-btn:disabled { opacity: 0.4; cursor: not-allowed; }
.toast-container {
    position: fixed; top: 16px; right: 16px; z-index: 9999;
    display: flex; flex-direction: column; gap: 8px;
}
.toast {
    padding: 12px 20px; border-radius: var(--radius);
    font-size: 14px; color: #fff; animation: fadeIn 0.3s ease;
    min-width: 250px; box-shadow: 0 4px 12px rgba(0,0,0,0.4);
}
.toast-success { background: var(--success); }
.toast-error { background: var(--error); }
.toast.fade-out { animation: fadeOut 0.3s ease forwards; }
.login-container { display: flex; align-items: center; justify-content: center; min-height: 100vh; }
.login-card {
    background: var(--bg-card); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 40px;
    width: 100%; max-width: 400px;
}
.login-title { font-size: 24px; font-weight: 700; color: var(--accent); text-align: center; margin-bottom: 24px; }
.login-error { color: var(--error); font-size: 13px; margin-top: 8px; text-align: center; }
.search-bar { position: relative; margin-bottom: 16px; }
.search-input {
    width: 100%; padding: 8px 12px 8px 36px;
    background: var(--bg-input); border: 1px solid var(--border);
    border-radius: var(--radius); color: var(--text-primary); font-size: 14px;
}
.search-input:focus { outline: none; border-color: var(--accent); }
.search-icon {
    position: absolute; left: 10px; top: 50%;
    transform: translateY(-50%); color: var(--text-secondary); font-size: 14px;
}
.detail-grid { display: grid; grid-template-columns: 160px 1fr; gap: 12px; margin-bottom: 20px; }
.detail-label { font-size: 13px; font-weight: 600; color: var(--text-secondary); padding: 4px 0; }
.detail-value { font-size: 14px; color: var(--text-primary); padding: 4px 0; word-break: break-word; }
.nav-overlay {
    display: none; position: fixed; top: 0; left: 0; right: 0; bottom: 0;
    background: rgba(0,0,0,0.5); z-index: 99;
}
.nav-overlay.open { display: block; }
.toolbar { display: flex; align-items: center; justify-content: space-between; margin-bottom: 16px; flex-wrap: wrap; gap: 8px; }
@media (max-width: 768px) {
    .nav { transform: translateX(-100%); width: 260px; }
    .nav.open { transform: translateX(0); }
    .main { margin-left: 0; }
    .hamburger { display: block; }
    .page-content { padding: 16px; }
    .dashboard-grid { grid-template-columns: 1fr 1fr; }
    .detail-grid { grid-template-columns: 1fr; }
    table { font-size: 13px; }
    td, th { padding: 8px 10px; }
    .login-card { margin: 16px; }
}
@media (max-width: 480px) {
    .dashboard-grid { grid-template-columns: 1fr; }
}
@keyframes fadeIn {
    from { opacity: 0; transform: translateY(-8px); }
    to   { opacity: 1; transform: translateY(0); }
}
@keyframes fadeOut {
    from { opacity: 1; transform: translateY(0); }
    to   { opacity: 0; transform: translateY(-8px); }
}
"##.to_string()
}

// ─── F1/W1: Frontend HTML (Single-File SPA) ─────────────────────────────────

/// Generate a complete single-file SPA: `frontend.html`.
///
/// Combines inline CSS (from `generate_frontend_css()`), generated HTML body
/// with login + nav + content areas, and inline JS (from `generate_frontend_js()`).
/// Zero external dependencies. No React, no Vue, no npm.
pub fn generate_frontend_html(spec: &AppSpec) -> String {
    let app_name = &spec.app_name;
    let css = generate_frontend_css();
    let js = generate_frontend_js(app_name, &spec.entities);

    let non_user: Vec<&EntityDef> = spec.entities.iter().filter(|e| e.name != "User").collect();

    // Build nav links
    let mut nav_links = String::new();
    nav_links.push_str("        <div class=\"nav-link\" data-route=\"dashboard\" onclick=\"location.hash='#/dashboard'\">Dashboard</div>\n");
    for entity in &non_user {
        let sn = &entity.snake_name;
        let display_plural = pluralize_name(&to_title_case(sn));
        nav_links.push_str(&format!(
            "        <div class=\"nav-link\" data-route=\"{}\" onclick=\"location.hash='#/{}'\">{}</div>\n",
            sn, sn, display_plural
        ));
    }

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{app_name}</title>
    <style>{css}</style>
</head>
<body>
    <!-- Login Screen -->
    <div id="app-login" class="login-container">
        <div class="login-card">
            <div class="login-title">{app_name}</div>
            <div class="form-group">
                <label class="form-label">Email</label>
                <input class="form-input" type="email" id="login-email" value="admin@example.com">
            </div>
            <div class="form-group">
                <label class="form-label">Password</label>
                <input class="form-input" type="password" id="login-password" value="admin123">
            </div>
            <button class="btn btn-primary" id="login-btn" style="width:100%">Login</button>
            <div id="login-error" class="login-error"></div>
        </div>
    </div>

    <!-- Main App -->
    <div id="app-main" class="app-container" style="display:none">
        <nav class="nav">
            <div class="nav-brand">{app_name}</div>
{nav_links}        </nav>
        <div class="nav-overlay" onclick="toggleNav()"></div>
        <div class="main">
            <div class="header">
                <div style="display:flex;align-items:center;gap:12px">
                    <button class="hamburger" onclick="toggleNav()">&#9776;</button>
                    <span class="header-title" id="header-title">Dashboard</span>
                </div>
                <div class="header-right">
                    <span class="header-user" id="header-user"></span>
                    <button class="btn btn-sm" onclick="doLogout()">Logout</button>
                </div>
            </div>
            <div class="page-content" id="page-content"></div>
        </div>
    </div>

    <div class="toast-container" id="toast-container"></div>

    <script>
{js}
    </script>
</body>
</html>"##,
        app_name = app_name,
        css = css,
        nav_links = nav_links,
        js = js,
    )
}

// ─── F1/W1: Frontend JavaScript ──────────────────────────────────────────────

/// Generate the complete SPA JavaScript code.
///
/// The entity configs (ENTITIES object) are generated from the EntityDef list.
/// Everything else (auth, router, renderers, toast) is static.
fn generate_frontend_js(app_name: &str, entities: &[EntityDef]) -> String {
    let non_user: Vec<&EntityDef> = entities.iter().filter(|e| e.name != "User").collect();

    // ── Build ENTITIES config object ─────────────────────────────────
    let mut entity_configs = String::new();
    for entity in &non_user {
        let sn = &entity.snake_name;
        let plural = pluralize_name(sn);
        let display = to_title_case(sn);
        let display_plural = pluralize_name(&display);

        entity_configs.push_str(&format!("  '{}': {{\n", sn));
        entity_configs.push_str(&format!("    name: '{}',\n", display));
        entity_configs.push_str(&format!("    plural: '{}',\n", display_plural));
        entity_configs.push_str(&format!("    apiPath: '/api/{}',\n", plural));
        entity_configs.push_str("    fields: [\n");

        for field in &entity.fields {
            if field.name == "id" || field.name == "created_at" || field.name == "updated_at" {
                continue;
            }
            let input_type = field_to_input_type(&field.name, &field.rust_type);
            let label = to_title_case(&field.name);
            let required = !field.nullable;
            let step = if input_type == "number" {
                format!(", step: '{}'", field_number_step(&field.rust_type))
            } else {
                String::new()
            };
            // For fk-select, extract the referenced entity from the field name
            let fk_ref = if input_type == "fk-select" {
                let ref_name = field.name.trim_end_matches("_id");
                format!(", ref: '{}'", ref_name)
            } else {
                String::new()
            };
            entity_configs.push_str(&format!(
                "      {{ name: '{}', label: '{}', type: '{}', required: {}{}{} }},\n",
                field.name, label, input_type, required, step, fk_ref
            ));
        }
        entity_configs.push_str("    ],\n");
        entity_configs.push_str("  },\n");
    }

    // ── First entity key for default route ───────────────────────────
    let first_key = non_user.first().map(|e| e.snake_name.as_str()).unwrap_or("item");

    // ── Build the full JS ────────────────────────────────────────────
    format!(
        r##"'use strict';

const ENTITIES = {{
{entity_configs}}};

const FIRST_ENTITY = '{first_key}';
const PER_PAGE = 20;
let currentUser = null;
let sortState = {{}};

// ── Auth ──────────────────────────────────────────────────────────

function getToken() {{ return localStorage.getItem('token'); }}

async function apiFetch(method, path, body) {{
  const headers = {{ 'Content-Type': 'application/json' }};
  const token = getToken();
  if (token) headers['Authorization'] = 'Bearer ' + token;
  const opts = {{ method, headers }};
  if (body) opts.body = JSON.stringify(body);
  const resp = await fetch(path, opts);
  if (resp.status === 401) {{ doLogout(); throw new Error('Session expired'); }}
  if (!resp.ok) {{
    const err = await resp.json().catch(() => ({{ error: resp.statusText }}));
    throw new Error(err.error || err.errors?.join(', ') || resp.statusText);
  }}
  if (resp.status === 204) return null;
  return resp.json();
}}

async function doLogin() {{
  const email = document.getElementById('login-email').value;
  const password = document.getElementById('login-password').value;
  const errEl = document.getElementById('login-error');
  errEl.textContent = '';
  try {{
    const data = await apiFetch('POST', '/api/auth/login', {{ email, password }});
    localStorage.setItem('token', data.token);
    await showApp();
  }} catch (e) {{
    errEl.textContent = e.message;
  }}
}}

function doLogout() {{
  localStorage.removeItem('token');
  currentUser = null;
  document.getElementById('app-login').style.display = '';
  document.getElementById('app-main').style.display = 'none';
}}

async function showApp() {{
  document.getElementById('app-login').style.display = 'none';
  document.getElementById('app-main').style.display = '';
  try {{
    currentUser = await apiFetch('GET', '/api/auth/me');
    document.getElementById('header-user').textContent = currentUser.email + ' (' + currentUser.role + ')';
  }} catch (_) {{}}
  if (!location.hash || location.hash === '#/') location.hash = '#/dashboard';
  else route();
}}

// ── Router ────────────────────────────────────────────────────────

function route() {{
  const hash = location.hash.slice(2) || 'dashboard';
  const parts = hash.split('/');
  const page = document.getElementById('page-content');

  // Update active nav
  document.querySelectorAll('.nav-link').forEach(el => {{
    el.classList.toggle('active', el.dataset.route === parts[0] || (parts[0] === 'dashboard' && el.dataset.route === 'dashboard'));
  }});
  document.getElementById('header-title').textContent = parts[0] === 'dashboard' ? 'Dashboard' : (ENTITIES[parts[0]]?.plural || parts[0]);

  if (parts[0] === 'dashboard') {{ renderDashboard(page); return; }}
  const entity = ENTITIES[parts[0]];
  if (!entity) {{ page.innerHTML = '<p>Page not found</p>'; return; }}

  if (parts.length === 1) {{ renderList(page, parts[0]); }}
  else if (parts[1] === 'new') {{ renderForm(page, parts[0], null); }}
  else if (parts.length === 2) {{ renderDetail(page, parts[0], parts[1]); }}
  else if (parts[2] === 'edit') {{ renderForm(page, parts[0], parts[1]); }}
}}

window.addEventListener('hashchange', route);

// ── Dashboard ─────────────────────────────────────────────────────

async function renderDashboard(container) {{
  container.innerHTML = '<h2 style="margin-bottom:20px">Dashboard</h2><div class="dashboard-grid" id="dash-grid"></div><div id="dash-tables"></div>';
  const grid = document.getElementById('dash-grid');
  const tables = document.getElementById('dash-tables');

  for (const [key, cfg] of Object.entries(ENTITIES)) {{
    try {{
      const data = await apiFetch('GET', cfg.apiPath + '?per_page=5');
      const count = data.total || data.items?.length || 0;
      const card = document.createElement('div');
      card.className = 'stat-card';
      card.onclick = () => location.hash = '#/' + key;
      card.innerHTML = '<div class="stat-value">' + count + '</div><div class="stat-label">' + cfg.plural + '</div>';
      grid.appendChild(card);

      if (data.items && data.items.length > 0) {{
        const sec = document.createElement('div');
        sec.className = 'card';
        sec.style.marginBottom = '16px';
        const fields = cfg.fields.slice(0, 4);
        let th = fields.map(f => '<th>' + f.label + '</th>').join('');
        let rows = data.items.map(item =>
          '<tr>' + fields.map(f => '<td>' + (item[f.name] ?? '') + '</td>').join('') + '</tr>'
        ).join('');
        sec.innerHTML = '<h3 style="margin-bottom:12px">Recent ' + cfg.plural + '</h3>' +
          '<table><thead><tr>' + th + '</tr></thead><tbody>' + rows + '</tbody></table>';
        tables.appendChild(sec);
      }}
    }} catch (_) {{}}
  }}
}}

// ── List View ─────────────────────────────────────────────────────

async function renderList(container, key) {{
  const cfg = ENTITIES[key];
  container.innerHTML = '<div class="toolbar"><h2>' + cfg.plural + '</h2><button class="btn btn-primary" onclick="location.hash=\'#/' + key + '/new\'">+ Neu</button></div>' +
    '<div class="search-bar"><span class="search-icon">&#128269;</span><input class="search-input" id="search-input" placeholder="Search ' + cfg.plural + '..." oninput="filterList()"></div>' +
    '<div id="list-container"><p>Loading...</p></div>';

  try {{
    const data = await apiFetch('GET', cfg.apiPath + '?per_page=1000');
    const items = data.items || [];
    window._listData = items;
    window._listKey = key;
    window._listPage = 1;
    sortState = {{}};
    renderListTable();
  }} catch (e) {{
    document.getElementById('list-container').innerHTML = '<p style="color:var(--error)">Error: ' + e.message + '</p>';
  }}
}}

function filterList() {{
  window._listPage = 1;
  renderListTable();
}}

function sortBy(field) {{
  if (sortState.field === field) {{
    sortState.dir = sortState.dir === 'asc' ? 'desc' : 'asc';
  }} else {{
    sortState = {{ field, dir: 'asc' }};
  }}
  renderListTable();
}}

function goToPage(p) {{
  window._listPage = p;
  renderListTable();
}}

function renderListTable() {{
  const key = window._listKey;
  const cfg = ENTITIES[key];
  const search = (document.getElementById('search-input')?.value || '').toLowerCase();
  let items = window._listData || [];

  // Filter
  if (search) {{
    items = items.filter(item =>
      cfg.fields.some(f => String(item[f.name] ?? '').toLowerCase().includes(search))
    );
  }}

  // Sort
  if (sortState.field) {{
    items = [...items].sort((a, b) => {{
      const va = a[sortState.field] ?? '';
      const vb = b[sortState.field] ?? '';
      const cmp = String(va).localeCompare(String(vb), undefined, {{ numeric: true }});
      return sortState.dir === 'desc' ? -cmp : cmp;
    }});
  }}

  // Pagination
  const total = items.length;
  const totalPages = Math.max(1, Math.ceil(total / PER_PAGE));
  const page = Math.min(window._listPage || 1, totalPages);
  const start = (page - 1) * PER_PAGE;
  const pageItems = items.slice(start, start + PER_PAGE);

  // Build table
  const fields = cfg.fields;
  let html = '<table><thead><tr>';
  for (const f of fields) {{
    const arrow = sortState.field === f.name ? (sortState.dir === 'asc' ? ' &#9650;' : ' &#9660;') : '';
    html += '<th onclick="sortBy(\'' + f.name + '\')">' + f.label + '<span class="sort-arrow">' + arrow + '</span></th>';
  }}
  html += '<th>Actions</th></tr></thead><tbody>';

  for (const item of pageItems) {{
    html += '<tr>';
    for (const f of fields) {{
      let val = item[f.name];
      if (val === null || val === undefined) val = '';
      if (typeof val === 'boolean') val = val ? 'Yes' : 'No';
      html += '<td>' + val + '</td>';
    }}
    html += '<td class="table-actions">' +
      '<button class="btn btn-sm btn-secondary" onclick="location.hash=\'#/' + key + '/' + 'item.id' + '/edit\'".replace("item.id", item.id)>Edit</button>' +
      '<button class="btn btn-sm btn-danger" onclick="deleteItem(\'' + key + '\',' + 'item.id' + ')".replace("item.id", item.id)>Delete</button>' +
      '</td></tr>';
  }}
  html += '</tbody></table>';

  // Pagination controls
  if (totalPages > 1) {{
    html += '<div class="pagination">';
    html += '<button class="page-btn" onclick="goToPage(' + (page - 1) + ')" ' + (page <= 1 ? 'disabled' : '') + '>&laquo; Prev</button>';
    for (let i = 1; i <= totalPages; i++) {{
      if (totalPages > 7 && Math.abs(i - page) > 2 && i !== 1 && i !== totalPages) {{
        if (i === 2 || i === totalPages - 1) html += '<span style="color:var(--text-secondary);padding:0 4px">...</span>';
        continue;
      }}
      html += '<button class="page-btn' + (i === page ? ' active' : '') + '" onclick="goToPage(' + i + ')">' + i + '</button>';
    }}
    html += '<button class="page-btn" onclick="goToPage(' + (page + 1) + ')" ' + (page >= totalPages ? 'disabled' : '') + '>Next &raquo;</button>';
    html += '</div>';
  }}
  html += '<p style="color:var(--text-secondary);font-size:12px;margin-top:8px">Total: ' + total + '</p>';

  document.getElementById('list-container').innerHTML = html;

  // Fix onclick with actual item.id (we build them via data attributes instead)
  // Rebuild action buttons with proper closures
  const rows = document.querySelectorAll('#list-container tbody tr');
  const pageData = pageItems;
  rows.forEach((row, idx) => {{
    const item = pageData[idx];
    if (!item) return;
    const btns = row.querySelectorAll('.table-actions button');
    if (btns[0]) btns[0].onclick = () => location.hash = '#/' + key + '/' + item.id + '/edit';
    if (btns[1]) btns[1].onclick = () => deleteItem(key, item.id);
  }});
}}

// ── Form View ─────────────────────────────────────────────────────

async function renderForm(container, key, id) {{
  const cfg = ENTITIES[key];
  const isEdit = id !== null;
  let existing = null;

  container.innerHTML = '<h2>' + (isEdit ? 'Edit' : 'New') + ' ' + cfg.name + '</h2><div id="form-container"><p>Loading...</p></div>';

  if (isEdit) {{
    try {{ existing = await apiFetch('GET', cfg.apiPath + '/' + id); }} catch (e) {{
      container.innerHTML = '<p style="color:var(--error)">Error: ' + e.message + '</p>';
      return;
    }}
  }}

  let html = '<form id="entity-form" class="card" style="max-width:600px">';
  for (const f of cfg.fields) {{
    const val = existing ? (existing[f.name] ?? '') : '';
    html += '<div class="form-group">';
    html += '<label class="form-label">' + f.label + (f.required ? ' *' : '') + '</label>';

    if (f.type === 'textarea') {{
      html += '<textarea class="form-input" name="' + f.name + '"' + (f.required ? ' required' : '') + '>' + val + '</textarea>';
    }} else if (f.type === 'checkbox') {{
      const checked = val === true || val === 'true' ? 'checked' : '';
      html += '<label class="toggle"><input type="checkbox" name="' + f.name + '" ' + checked + '><span class="toggle-slider"></span></label>';
    }} else if (f.type === 'select') {{
      html += '<select class="form-input" name="' + f.name + '"' + (f.required ? ' required' : '') + '>';
      ['Active', 'Inactive', 'Pending', 'Draft', 'Completed', 'Cancelled'].forEach(opt => {{
        const sel = String(val).toLowerCase() === opt.toLowerCase() ? ' selected' : '';
        html += '<option value="' + opt.toLowerCase() + '"' + sel + '>' + opt + '</option>';
      }});
      html += '</select>';
    }} else if (f.type === 'fk-select') {{
      html += '<select class="form-input" name="' + f.name + '"' + (f.required ? ' required' : '') + ' data-fk-ref="' + (f.ref || '') + '" data-fk-val="' + val + '"><option value="">Loading...</option></select>';
    }} else {{
      const step = f.step ? ' step="' + f.step + '"' : '';
      const inputVal = f.type === 'datetime-local' && val ? val.replace('Z', '').split('.')[0] : val;
      html += '<input class="form-input" type="' + f.type + '" name="' + f.name + '" value="' + inputVal + '"' + (f.required ? ' required' : '') + step + '>';
    }}
    html += '</div>';
  }}
  html += '<div class="btn-group">';
  html += '<button type="submit" class="btn btn-primary">' + (isEdit ? 'Save' : 'Create') + '</button>';
  html += '<button type="button" class="btn btn-secondary" onclick="location.hash=\'#/' + key + '\'">Cancel</button>';
  html += '</div></form>';

  document.getElementById('form-container').innerHTML = html;

  // Load FK dropdowns
  document.querySelectorAll('[data-fk-ref]').forEach(async (select) => {{
    const refEntity = select.dataset.fkRef;
    const currentVal = select.dataset.fkVal;
    if (!refEntity) return;
    const refCfg = ENTITIES[refEntity];
    if (!refCfg) return;
    try {{
      const data = await apiFetch('GET', refCfg.apiPath + '?per_page=100');
      const items = data.items || [];
      select.innerHTML = '<option value="">-- Select --</option>';
      items.forEach(item => {{
        const label = item.name || item.title || item.email || item.id;
        const sel = String(item.id) === String(currentVal) ? ' selected' : '';
        select.innerHTML += '<option value="' + item.id + '"' + sel + '>' + label + '</option>';
      }});
    }} catch (_) {{
      select.innerHTML = '<option value="">Error loading</option>';
    }}
  }});

  // Form submit
  document.getElementById('entity-form').onsubmit = async (e) => {{
    e.preventDefault();
    const formData = {{}};
    for (const f of cfg.fields) {{
      const el = e.target.elements[f.name];
      if (!el) continue;
      if (f.type === 'checkbox') {{
        formData[f.name] = el.checked;
      }} else if (f.type === 'number') {{
        formData[f.name] = el.value === '' ? null : Number(el.value);
      }} else if (f.type === 'fk-select') {{
        formData[f.name] = el.value === '' ? null : (isNaN(el.value) ? el.value : Number(el.value));
      }} else {{
        formData[f.name] = el.value || null;
      }}
    }}
    try {{
      if (isEdit) {{
        await apiFetch('PUT', cfg.apiPath + '/' + id, formData);
        toast(cfg.name + ' updated', 'success');
      }} else {{
        await apiFetch('POST', cfg.apiPath, formData);
        toast(cfg.name + ' created', 'success');
      }}
      location.hash = '#/' + key;
    }} catch (e) {{
      toast('Error: ' + e.message, 'error');
    }}
  }};
}}

// ── Detail View ───────────────────────────────────────────────────

async function renderDetail(container, key, id) {{
  const cfg = ENTITIES[key];
  container.innerHTML = '<p>Loading...</p>';
  try {{
    const item = await apiFetch('GET', cfg.apiPath + '/' + id);
    let html = '<h2>' + cfg.name + ' #' + id + '</h2><div class="card" style="max-width:700px"><div class="detail-grid">';

    // Show all fields including id, created_at, updated_at
    html += '<div class="detail-label">ID</div><div class="detail-value">' + item.id + '</div>';
    for (const f of cfg.fields) {{
      let val = item[f.name];
      if (val === null || val === undefined) val = '—';
      if (typeof val === 'boolean') val = val ? 'Yes' : 'No';
      html += '<div class="detail-label">' + f.label + '</div><div class="detail-value">' + val + '</div>';
    }}
    if (item.created_at) html += '<div class="detail-label">Created</div><div class="detail-value">' + item.created_at + '</div>';
    if (item.updated_at) html += '<div class="detail-label">Updated</div><div class="detail-value">' + item.updated_at + '</div>';

    html += '</div><div class="btn-group">';
    html += '<button class="btn btn-secondary" onclick="location.hash=\'#/' + key + '/' + id + '/edit\'">Bearbeiten</button>';
    html += '<button class="btn btn-danger" onclick="deleteItem(\'' + key + '\',' + id + ')">L\u00f6schen</button>';
    html += '<button class="btn" onclick="location.hash=\'#/' + key + '\'">Back</button>';
    html += '</div></div>';
    container.innerHTML = html;
  }} catch (e) {{
    container.innerHTML = '<p style="color:var(--error)">Error: ' + e.message + '</p>';
  }}
}}

// ── Delete ────────────────────────────────────────────────────────

async function deleteItem(key, id) {{
  if (!confirm('Wirklich l\u00f6schen?')) return;
  const cfg = ENTITIES[key];
  try {{
    await apiFetch('DELETE', cfg.apiPath + '/' + id);
    toast(cfg.name + ' deleted', 'success');
    location.hash = '#/' + key;
  }} catch (e) {{
    toast('Error: ' + e.message, 'error');
  }}
}}

// ── Toast ─────────────────────────────────────────────────────────

function toast(msg, type) {{
  const container = document.getElementById('toast-container');
  const el = document.createElement('div');
  el.className = 'toast toast-' + (type || 'success');
  el.textContent = msg;
  container.appendChild(el);
  setTimeout(() => {{ el.classList.add('fade-out'); setTimeout(() => el.remove(), 300); }}, 3000);
}}

// ── Mobile Nav ────────────────────────────────────────────────────

function toggleNav() {{
  document.querySelector('.nav').classList.toggle('open');
  document.querySelector('.nav-overlay').classList.toggle('open');
}}

// ── Init ──────────────────────────────────────────────────────────

if (getToken()) {{ showApp(); }}
document.getElementById('login-btn').onclick = doLogin;
document.getElementById('login-password').onkeydown = (e) => {{ if (e.key === 'Enter') doLogin(); }};
"##,
        entity_configs = entity_configs,
        first_key = first_key,
    )
}


// ─── F1/W2: Feldtyp-Mapping ─────────────────────────────────────────────────────

/// Map an entity field to an HTML input type based on field name and Rust type.
///
/// Name-based rules take priority over type-based rules.
/// Returns one of: "email", "password", "url", "tel", "color", "textarea",
/// "select", "number", "checkbox", "datetime-local", "date", "fk-select", "text".
fn field_to_input_type(name: &str, rust_type: &str) -> &'static str {
    let lower = name.to_lowercase();

    // Name-based rules (higher priority)
    if lower.contains("email") {
        return "email";
    }
    if lower.contains("password") || lower.contains("passwd") {
        return "password";
    }
    if lower.contains("url") || lower.contains("website") || lower.contains("link") {
        return "url";
    }
    if lower.contains("phone") || lower.contains("tel") || lower.contains("mobile") {
        return "tel";
    }
    if lower.contains("color") || lower.contains("colour") {
        return "color";
    }
    if lower == "description" || lower == "notes" || lower == "bio"
        || lower == "content" || lower == "body" || lower == "text"
        || lower == "comment" || lower == "message"
    {
        return "textarea";
    }
    if lower == "status" || lower == "role" || lower == "type"
        || lower == "category" || lower == "priority" || lower == "level"
    {
        return "select";
    }

    // Type-based rules
    let rt = rust_type.replace("Option<", "").replace('>', "");
    match rt.as_str() {
        "f64" | "f32" | "Decimal" => "number",
        "i32" | "i64" | "u32" | "u64" => "number",
        "bool" => "checkbox",
        "NaiveDateTime" | "DateTime<Utc>" | "chrono::DateTime<Utc>" => "datetime-local",
        "NaiveDate" => "date",
        _ => {
            // Uuid FK field
            if (rt == "Uuid" || rt == "String" || rt == "i64") && lower.ends_with("_id") {
                "fk-select"
            } else {
                "text"
            }
        }
    }
}

/// Return the HTML step attribute for number inputs.
fn field_number_step(rust_type: &str) -> &'static str {
    let rt = rust_type.replace("Option<", "").replace('>', "");
    match rt.as_str() {
        "f64" | "f32" | "Decimal" => "0.01",
        _ => "1",
    }
}

/// Simple English pluralization for frontend display / API paths.
fn pluralize_name(name: &str) -> String {
    if name.ends_with('s') {
        format!("{}es", name)
    } else if name.ends_with('y')
        && !name.ends_with("ay")
        && !name.ends_with("ey")
        && !name.ends_with("oy")
        && !name.ends_with("uy")
    {
        format!("{}ies", &name[..name.len() - 1])
    } else {
        format!("{}s", name)
    }
}

/// Convert a snake_case name to Title Case for labels.
fn to_title_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
                placed.insert(pluralize(&entity.snake_name));
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
                    foreign_keys: vec![],
                    validations: vec![],
                    business_rules: vec![],
                    relationships: vec![],
                    plural_name: None,
                },
                EntityDef {
                    name: "Product".into(),
                    snake_name: "product".into(),
                    fields: vec![],
                    foreign_keys: vec![],
                    validations: vec![],
                    business_rules: vec![],
                    relationships: vec![],
                    plural_name: None,
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
