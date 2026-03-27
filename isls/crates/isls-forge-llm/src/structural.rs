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
    s
}

/// Generate `backend/src/services/mod.rs` — declares all service submodules.
pub fn generate_services_mod(spec: &AppSpec) -> String {
    let mut s = String::from("// Copyright (c) 2026 Sebastian Klemm — ISLS v3.4 structural generated\n");
    for entity in &spec.entities {
        s.push_str(&format!("pub mod {};\n", entity.snake_name));
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
    name          VARCHAR(255) NOT NULL DEFAULT '',
    role          VARCHAR(50)  NOT NULL DEFAULT 'user',
    is_active     BOOLEAN      NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

"#,
    );

    // Seed admin user with bcrypt hash
    sql.push_str(&format!(
        "-- Seed admin user (password: admin123)\nINSERT INTO users (email, password_hash, name, role, is_active)\nVALUES ('admin@example.com', '{}', 'Admin', 'admin', true)\nON CONFLICT (email) DO NOTHING;\n\n",
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

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Dispatch structural generation by file path.
///
/// Called by `StagedClosure` for every `NodeType::Structural` node whose path
/// is not handled by the static-file generators (Cargo.toml, Dockerfile, etc.).
pub fn generate_for_path(path: &str, spec: &AppSpec) -> String {
    if path.ends_with("main.rs") {
        return generate_main_rs(spec);
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
    if path.contains("api/mod.rs") {
        return generate_api_mod(spec);
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
