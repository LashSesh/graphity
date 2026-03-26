// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! File generation order for ISLS v3.1.
//!
//! Returns the complete ordered list of files to generate for a warehouse
//! domain application, structured in dependency layers (0-9).
//!
//! **Layer dependency rules:**
//! - Layer N may only reference types from Layers 0..N-1.
//! - Within a layer, files are ordered so dependencies come first.

use crate::{ForgePlan, GenerationMethod, FileSpec};

/// Return the complete ordered list of files to generate for the given plan.
///
/// Layer 0 (static) files are NOT included — they are written by
/// [`forge::LlmForge::generate_static_files`] before this list is processed.
///
/// The list covers Layers 1-9 in strict dependency order.
pub fn generation_order(plan: &ForgePlan) -> Vec<FileSpec> {
    let entities = &plan.spec.entities;
    let llm = if plan.spec.domain_name == "test" {
        GenerationMethod::Mock
    } else {
        GenerationMethod::Llm
    };

    let mut specs: Vec<FileSpec> = Vec::new();

    // ── Layer 1: Foundation ───────────────────────────────────────────────────
    // No dependencies on other generated files.

    specs.push(FileSpec {
        path: "backend/src/errors.rs".into(),
        layer: 1,
        entity: None,
        purpose: "Define AppError enum with all error variants used by the application. \
                  Include actix_web::ResponseError impl returning appropriate HTTP status codes."
            .into(),
        is_rust: true,
        method: llm.clone(),
    });

    specs.push(FileSpec {
        path: "backend/src/config.rs".into(),
        layer: 1,
        entity: None,
        purpose: "Define AppConfig struct loading from environment variables: \
                  DATABASE_URL, JWT_SECRET, PORT. Use dotenvy for .env loading."
            .into(),
        is_rust: true,
        method: llm.clone(),
    });

    specs.push(FileSpec {
        path: "backend/src/pagination.rs".into(),
        layer: 1,
        entity: None,
        purpose: "Define PaginationParams (page, per_page with defaults) and \
                  PaginatedResponse<T> (items, total, page, per_page). \
                  Derive Serialize/Deserialize. Include helper to compute SQL OFFSET."
            .into(),
        is_rust: true,
        method: llm.clone(),
    });

    // ── Layer 2: Auth ─────────────────────────────────────────────────────────
    // Depends on Layer 1 (errors.rs, config.rs).

    specs.push(FileSpec {
        path: "backend/src/models/user.rs".into(),
        layer: 2,
        entity: Some("User".into()),
        purpose: "Define User struct (id, email, password_hash, role, is_active, created_at), \
                  CreateUserPayload (email, password, role), UpdateUserPayload (all Option). \
                  Derive Serialize, Deserialize, sqlx::FromRow."
            .into(),
        is_rust: true,
        method: llm.clone(),
    });

    specs.push(FileSpec {
        path: "backend/src/auth.rs".into(),
        layer: 2,
        entity: None,
        purpose: "JWT authentication: Claims struct (sub, email, role, exp), \
                  AuthUser extractor (extracts claims from Bearer token), \
                  encode_jwt/decode_jwt helpers using jsonwebtoken crate, \
                  role guard middleware. Use AppError::Unauthorized on failure."
            .into(),
        is_rust: true,
        method: llm.clone(),
    });

    // ── Layer 3: Models ───────────────────────────────────────────────────────
    // Depends on Layer 1 (errors.rs, pagination.rs).

    for entity in entities.iter().filter(|e| e.name != "User") {
        specs.push(FileSpec {
            path: format!("backend/src/models/{}.rs", entity.snake_name),
            layer: 3,
            entity: Some(entity.name.clone()),
            purpose: format!(
                "Define {} struct with all fields, Create{}Payload (user-input fields only), \
                 Update{}Payload (all Option). \
                 Derive Debug, Clone, Serialize, Deserialize, sqlx::FromRow. \
                 Add validate() method returning Vec<String> of error messages.",
                entity.name, entity.name, entity.name
            ),
            is_rust: true,
            method: llm.clone(),
        });
    }

    specs.push(FileSpec {
        path: "backend/src/models/mod.rs".into(),
        layer: 3,
        entity: None,
        purpose: format!(
            "Declare all model submodules: user, {}. Re-export all public types.",
            entities
                .iter()
                .filter(|e| e.name != "User")
                .map(|e| e.snake_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        is_rust: true,
        method: llm.clone(),
    });

    // ── Layer 4: Database ─────────────────────────────────────────────────────
    // Depends on Layer 3 (all models).

    specs.push(FileSpec {
        path: "backend/migrations/001_initial.sql".into(),
        layer: 4,
        entity: None,
        purpose: "CREATE TABLE statements for all entities. \
                  Include foreign keys, indices, and a seed admin user \
                  (email: admin@example.com, bcrypt hash of 'admin123', role: 'admin'). \
                  Use IF NOT EXISTS. Order tables so FK dependencies come first."
            .into(),
        is_rust: false,
        method: llm.clone(),
    });

    specs.push(FileSpec {
        path: "backend/src/database/pool.rs".into(),
        layer: 4,
        entity: None,
        purpose: "Create a PgPool from DATABASE_URL env var using sqlx. \
                  Expose pub async fn create_pool() -> Result<PgPool, AppError>. \
                  Do NOT load migrations here — main.rs handles them. \
                  Do NOT use sqlx::migrate!() macro."
            .into(),
        is_rust: true,
        method: llm.clone(),
    });

    for entity in entities {
        specs.push(FileSpec {
            path: format!("backend/src/database/{}_queries.rs", entity.snake_name),
            layer: 4,
            entity: Some(entity.name.clone()),
            purpose: format!(
                "CRUD query functions for {}: \
                 get_{snake}(pool, id) → Result<{name}>, \
                 list_{snake}s(pool, params) → Result<PaginatedResponse<{name}>>, \
                 create_{snake}(pool, payload) → Result<{name}>, \
                 update_{snake}(pool, id, payload) → Result<{name}>, \
                 delete_{snake}(pool, id) → Result<()>. \
                 Use sqlx::query_as::<_, Type>(sql).bind(x) with exact field names from the model. \
                 NEVER use sqlx::query_as!() compile-time macro.",
                entity.name,
                snake = entity.snake_name,
                name = entity.name
            ),
            is_rust: true,
            method: llm.clone(),
        });
    }

    specs.push(FileSpec {
        path: "backend/src/database/mod.rs".into(),
        layer: 4,
        entity: None,
        purpose: format!(
            "Declare database submodules: pool, {}. Re-export create_pool and all query functions.",
            entities
                .iter()
                .map(|e| format!("{}_queries", e.snake_name))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        is_rust: true,
        method: llm.clone(),
    });

    // ── Layer 5: Services ─────────────────────────────────────────────────────
    // Depends on Layer 3 (models) + Layer 4 (queries).

    for entity in entities {
        specs.push(FileSpec {
            path: format!("backend/src/services/{}.rs", entity.snake_name),
            layer: 5,
            entity: Some(entity.name.clone()),
            purpose: format!(
                "Business logic service for {}: thin wrappers around database queries \
                 with validation, auth checks, and business rules. \
                 All functions take &PgPool and return Result<_, AppError>. \
                 Use tracing::info! for operations. No unwrap().",
                entity.name
            ),
            is_rust: true,
            method: llm.clone(),
        });
    }

    specs.push(FileSpec {
        path: "backend/src/services/mod.rs".into(),
        layer: 5,
        entity: None,
        purpose: format!(
            "Declare service submodules: {}. Re-export all public service functions.",
            entities
                .iter()
                .map(|e| e.snake_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        is_rust: true,
        method: llm.clone(),
    });

    // ── Layer 6: API ──────────────────────────────────────────────────────────
    // Depends on Layer 2 (auth) + Layer 5 (services).

    for entity in entities.iter().filter(|e| e.name != "User") {
        specs.push(FileSpec {
            path: format!("backend/src/api/{}.rs", entity.snake_name),
            layer: 6,
            entity: Some(entity.name.clone()),
            purpose: format!(
                "Actix-web handlers for {}: \
                 GET /api/{snake}s (list, paginated, auth required), \
                 GET /api/{snake}s/{{id}} (get by id, auth required), \
                 POST /api/{snake}s (create, auth required), \
                 PUT /api/{snake}s/{{id}} (update, auth required), \
                 DELETE /api/{snake}s/{{id}} (delete, admin only). \
                 Use web::Data<PgPool>, web::Query<PaginationParams>, AuthUser extractor.",
                entity.name,
                snake = entity.snake_name
            ),
            is_rust: true,
            method: llm.clone(),
        });
    }

    specs.push(FileSpec {
        path: "backend/src/api/auth_routes.rs".into(),
        layer: 6,
        entity: Some("User".into()),
        purpose: "Auth endpoints: \
                  POST /api/auth/register (create user, hash password with bcrypt), \
                  POST /api/auth/login (verify password, return JWT), \
                  GET /api/auth/me (return current user from JWT). \
                  GET /api/health (return 200 OK, no auth). \
                  Use User model from context above."
            .into(),
        is_rust: true,
        method: llm.clone(),
    });

    specs.push(FileSpec {
        path: "backend/src/api/mod.rs".into(),
        layer: 6,
        entity: None,
        purpose: format!(
            "Declare api submodules: auth_routes, {}. \
             Expose configure_routes(cfg: &mut web::ServiceConfig) that registers all routes.",
            entities
                .iter()
                .filter(|e| e.name != "User")
                .map(|e| e.snake_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        is_rust: true,
        method: llm.clone(),
    });

    // ── Layer 7: Main ─────────────────────────────────────────────────────────
    // Depends on all previous layers.

    specs.push(FileSpec {
        path: "backend/src/main.rs".into(),
        layer: 7,
        entity: None,
        purpose: "Application entry point: load .env, init tracing, create PgPool, \
                  start actix-web HttpServer on PORT. \
                  Configure CORS to allow http://localhost:3000. \
                  Register all routes via api::configure_routes. \
                  Pass PgPool as web::Data to all handlers."
            .into(),
        is_rust: true,
        method: llm.clone(),
    });

    // ── Layer 8: Frontend ─────────────────────────────────────────────────────
    // Depends on API spec (Layer 6).

    specs.push(FileSpec {
        path: "frontend/index.html".into(),
        layer: 8,
        entity: None,
        purpose: "Single-page application shell: login form (email + password), \
                  navigation between entity pages, JWT storage in localStorage. \
                  Include style.css and src/api/client.js. \
                  Load entity page scripts dynamically."
            .into(),
        is_rust: false,
        method: llm.clone(),
    });

    specs.push(FileSpec {
        path: "frontend/style.css".into(),
        layer: 8,
        entity: None,
        purpose: "Clean minimal CSS: login form centered, table for entity lists, \
                  form for create/edit, nav bar, responsive layout."
            .into(),
        is_rust: false,
        method: llm.clone(),
    });

    specs.push(FileSpec {
        path: "frontend/src/api/client.js".into(),
        layer: 8,
        entity: None,
        purpose: "Fetch-based API client: apiFetch(method, path, body) adds Bearer token \
                  from localStorage, handles JSON and errors. \
                  Export: login(email, password), register(email, password), \
                  and entity CRUD functions."
            .into(),
        is_rust: false,
        method: llm.clone(),
    });

    for entity in entities.iter().filter(|e| e.name != "User") {
        specs.push(FileSpec {
            path: format!("frontend/src/pages/{}.js", entity.snake_name),
            layer: 8,
            entity: Some(entity.name.clone()),
            purpose: format!(
                "JavaScript page module for {}: render list table, \
                 create/edit form, delete button. \
                 Use apiFetch from client.js. Export renderPage(container).",
                entity.name
            ),
            is_rust: false,
            method: llm.clone(),
        });
    }

    // ── Layer 9: Tests ────────────────────────────────────────────────────────
    // Depends on all layers.

    specs.push(FileSpec {
        path: "backend/tests/api_tests.rs".into(),
        layer: 9,
        entity: None,
        purpose: "Integration tests: test server startup, \
                  POST /api/auth/register (create user), \
                  POST /api/auth/login (get JWT), \
                  GET /api/health (200 OK), \
                  authenticated CRUD for the first entity. \
                  Use actix-web test infrastructure."
            .into(),
        is_rust: true,
        method: llm.clone(),
    });

    specs
}
