// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Mock code generators for ISLS v3.1 (no LLM required).
//!
//! These are pure Rust functions that build compilable code strings from
//! entity field definitions.  They are NOT Tera templates — they cannot break
//! due to field-name mismatches because they read the actual field names and
//! Rust types from the entity definition.
//!
//! Mock mode produces thin but compile-correct code.  It is used when:
//! - `--mock-oracle` flag is passed
//! - No OpenAI API key is available
//! - Tests that don't need real LLM output

use crate::{AppSpec, EntityDef, pluralize};

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Return only the "user input" fields (not id/created_at/updated_at).
fn user_fields(entity: &EntityDef) -> Vec<&isls_hypercube::domain::FieldDef> {
    entity
        .fields
        .iter()
        .filter(|f| !matches!(f.name.as_str(), "id" | "created_at" | "updated_at"))
        .collect()
}

/// Make an Option wrapper if the type isn't already Option<…>.
fn as_option(rust_type: &str) -> String {
    if rust_type.starts_with("Option<") {
        rust_type.to_string()
    } else {
        format!("Option<{}>", rust_type)
    }
}

/// Map a field's Rust type to the correct sqlx-compatible type.
/// TIMESTAMPTZ fields must use `chrono::DateTime<chrono::Utc>` instead of `String`
/// because sqlx deserializes PostgreSQL TIMESTAMPTZ to chrono types.
fn sqlx_rust_type(f: &isls_hypercube::domain::FieldDef) -> String {
    if f.sql_type.contains("TIMESTAMPTZ") {
        if f.nullable {
            "Option<chrono::DateTime<chrono::Utc>>".to_string()
        } else {
            "chrono::DateTime<chrono::Utc>".to_string()
        }
    } else {
        f.rust_type.clone()
    }
}

// ─── Foundation ───────────────────────────────────────────────────────────────

/// Generate `backend/src/errors.rs` — compilable AppError enum.
pub fn mock_generate_errors() -> String {
    r#"// Copyright (c) 2026 Sebastian Klemm — ISLS v3.1 mock generated
use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("validation error")]
    ValidationError(Vec<String>),
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("internal error: {0}")]
    InternalError(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    messages: Vec<String>,
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        let (status, messages) = match self {
            AppError::NotFound(msg) => (
                actix_web::http::StatusCode::NOT_FOUND,
                vec![msg.clone()],
            ),
            AppError::ValidationError(msgs) => (
                actix_web::http::StatusCode::UNPROCESSABLE_ENTITY,
                msgs.clone(),
            ),
            AppError::Unauthorized => (actix_web::http::StatusCode::UNAUTHORIZED, vec![]),
            AppError::Forbidden => (actix_web::http::StatusCode::FORBIDDEN, vec![]),
            AppError::Conflict(msg) => (actix_web::http::StatusCode::CONFLICT, vec![msg.clone()]),
            AppError::InternalError(msg) => (
                actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
                vec![msg.clone()],
            ),
        };
        HttpResponse::build(status).json(ErrorBody {
            error: self.to_string(),
            messages,
        })
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => AppError::NotFound("record not found".into()),
            _ => AppError::InternalError(e.to_string()),
        }
    }
}
"#
    .into()
}

/// Generate `backend/src/config.rs` — environment-based configuration.
pub fn mock_generate_config(app_name: &str) -> String {
    format!(
        r#"// ISLS v3.1 mock generated — {app}
use crate::AppError;

#[derive(Clone, Debug)]
pub struct AppConfig {{
    pub database_url: String,
    pub jwt_secret: String,
    pub port: u16,
}}

impl AppConfig {{
    pub fn from_env() -> Result<Self, AppError> {{
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| AppError::InternalError("DATABASE_URL not set".into()))?;
        let jwt_secret = std::env::var("JWT_SECRET")
            .unwrap_or_else(|_| "dev-secret-change-in-production".into());
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "8080".into())
            .parse::<u16>()
            .unwrap_or(8080);
        Ok(Self {{ database_url, jwt_secret, port }})
    }}
}}
"#,
        app = app_name
    )
}

/// Generate `backend/src/pagination.rs`.
pub fn mock_generate_pagination() -> String {
    r#"// ISLS v3.1 mock generated
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_page")]
    pub page: u64,
    #[serde(default = "default_per_page")]
    pub per_page: u64,
}

fn default_page() -> u64 { 1 }
fn default_per_page() -> u64 { 20 }

impl PaginationParams {
    pub fn offset(&self) -> i64 {
        ((self.page.saturating_sub(1)) * self.per_page) as i64
    }
    pub fn limit(&self) -> i64 {
        self.per_page as i64
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: u64,
    pub per_page: u64,
}

impl<T: Serialize> PaginatedResponse<T> {
    pub fn new(items: Vec<T>, total: i64, params: &PaginationParams) -> Self {
        Self { items, total, page: params.page, per_page: params.per_page }
    }
}
"#
    .into()
}

/// Generate `backend/src/auth.rs` — JWT skeleton.
pub fn mock_generate_auth() -> String {
    r#"// ISLS v3.1 mock generated
use actix_web::{dev::Payload, FromRequest, HttpRequest};
use futures::future::{ready, Ready};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use crate::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i64,
    pub email: String,
    pub role: String,
    pub exp: usize,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: i64,
    pub email: String,
    pub role: String,
}

pub fn encode_jwt(claims: &Claims, secret: &str) -> Result<String, AppError> {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::InternalError(format!("JWT encode error: {}", e)))
}

pub fn decode_jwt(token: &str, secret: &str) -> Result<Claims, AppError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|d| d.claims)
    .map_err(|_| AppError::Unauthorized)
}

pub fn require_role(user: &AuthUser, min_role: &str) -> Result<(), AppError> {
    let rank = |r: &str| match r {
        "admin" => 3,
        "operator" => 2,
        "viewer" => 1,
        _ => 0,
    };
    if rank(&user.role) >= rank(min_role) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn jwt_secret() -> String {
    std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".into())
}

impl FromRequest for AuthUser {
    type Error = AppError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let token = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");

        let result = decode_jwt(token, &jwt_secret()).map(|claims| AuthUser {
            id: claims.sub,
            email: claims.email,
            role: claims.role,
        });

        ready(result)
    }
}
"#
    .into()
}

// ─── Models ───────────────────────────────────────────────────────────────────

/// Generate a model file for `entity` (struct + Create/Update payloads).
pub fn mock_generate_model(entity: &EntityDef) -> String {
    let n = &entity.name;
    let mut code = String::new();

    code.push_str("// ISLS v3.1 mock generated\n");
    code.push_str("use serde::{Deserialize, Serialize};\n");
    code.push_str("use sqlx::FromRow;\n\n");

    // Main struct — use sqlx_rust_type for correct TIMESTAMPTZ mapping
    code.push_str(&format!(
        "#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]\npub struct {} {{\n",
        n
    ));
    for f in &entity.fields {
        code.push_str(&format!("    pub {}: {},\n", f.name, sqlx_rust_type(f)));
    }
    code.push_str("}\n\n");

    // Create payload — user-input fields only, using sqlx-compatible types
    let ufields = user_fields(entity);
    code.push_str(&format!(
        "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct Create{}Payload {{\n",
        n
    ));
    for f in &ufields {
        code.push_str(&format!("    pub {}: {},\n", f.name, sqlx_rust_type(f)));
    }
    code.push_str("}\n\n");

    // Update payload — all user-input fields as Option, using sqlx-compatible types
    code.push_str(&format!(
        "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct Update{}Payload {{\n",
        n
    ));
    for f in &ufields {
        code.push_str(&format!(
            "    pub {}: {},\n",
            f.name,
            as_option(&sqlx_rust_type(f))
        ));
    }
    code.push_str("}\n\n");

    // validate() method
    code.push_str(&format!("impl {} {{\n", n));
    code.push_str("    pub fn validate(&self) -> Vec<String> {\n");
    code.push_str("        let mut errors: Vec<String> = Vec::new();\n");
    for v in &entity.validations {
        code.push_str(&format!(
            "        if !({}) {{ errors.push(\"{}\".into()); }}\n",
            v.condition, v.message
        ));
    }
    code.push_str("        errors\n");
    code.push_str("    }\n");
    code.push_str("}\n");

    code
}

/// Generate `backend/src/models/user.rs`.
pub fn mock_generate_user_model() -> String {
    r#"// ISLS v3.1 mock generated
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: i64,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: String,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateUserPayload {
    pub email: String,
    pub password: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateUserPayload {
    pub email: Option<String>,
    pub role: Option<String>,
    pub is_active: Option<bool>,
}

impl User {
    pub fn validate_email(email: &str) -> Vec<String> {
        let mut errors = Vec::new();
        if email.trim().is_empty() { errors.push("Email must not be empty".into()); }
        if !email.contains('@') { errors.push("Email must contain @".into()); }
        errors
    }
}
"#
    .into()
}

/// Generate `backend/src/database/user_queries.rs` matching the hardcoded User model.
pub fn mock_generate_user_queries() -> String {
    r#"// ISLS v3.1 mock generated
use sqlx::PgPool;
use crate::{AppError, errors::*};
use crate::models::user::{User, CreateUserPayload, UpdateUserPayload};
use crate::pagination::{PaginationParams, PaginatedResponse};

pub async fn get_user(pool: &PgPool, id: i64) -> Result<User, AppError> {
    sqlx::query_as::<_, User>("SELECT id, email, password_hash, role, is_active, created_at FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("User {} not found", id)))
}

pub async fn list_users(
    pool: &PgPool,
    params: &PaginationParams,
) -> Result<PaginatedResponse<User>, AppError> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    let total = row.0;

    let items = sqlx::query_as::<_, User>("SELECT id, email, password_hash, role, is_active, created_at FROM users ORDER BY id DESC LIMIT $1 OFFSET $2")
        .bind(params.limit())
        .bind(params.offset())
        .fetch_all(pool)
        .await?;

    Ok(PaginatedResponse::new(items, total, params))
}

pub async fn create_user(pool: &PgPool, payload: CreateUserPayload) -> Result<User, AppError> {
    let hash = bcrypt::hash(&payload.password, bcrypt::DEFAULT_COST)
        .map_err(|e| AppError::InternalError(e.to_string()))?;
    sqlx::query_as::<_, User>("INSERT INTO users (email, password_hash, role) VALUES ($1, $2, $3) RETURNING id, email, password_hash, role, is_active, created_at, updated_at")
        .bind(&payload.email)
        .bind(&hash)
        .bind(payload.role.as_deref().unwrap_or("user"))
        .fetch_one(pool)
        .await
        .map_err(AppError::from)
}

pub async fn update_user(
    pool: &PgPool,
    id: i64,
    payload: UpdateUserPayload,
) -> Result<User, AppError> {
    let mut current = get_user(pool, id).await?;
    if let Some(v) = payload.email { current.email = v; }
    if let Some(v) = payload.role { current.role = v; }
    if let Some(v) = payload.is_active { current.is_active = v; }
    sqlx::query_as::<_, User>("UPDATE users SET (email, role, is_active, updated_at) = ($2, $3, $4, NOW()) WHERE id = $1 RETURNING id, email, password_hash, role, is_active, created_at, updated_at")
        .bind(id)
        .bind(&current.email)
        .bind(&current.role)
        .bind(&current.is_active)
        .fetch_one(pool)
        .await
        .map_err(AppError::from)
}

pub async fn delete_user(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("User {} not found", id)));
    }
    Ok(())
}
"#
    .into()
}

// ─── Database ─────────────────────────────────────────────────────────────────

/// Generate the initial SQL migration for all entities.
///
/// Tables are emitted in foreign-key dependency order so that referenced
/// tables are created before the tables that reference them.
pub fn mock_generate_migrations(entities: &[EntityDef]) -> String {
    let mut sql = String::new();

    sql.push_str("-- ISLS v3.1 mock generated\n\n");

    // Generate a bcrypt hash for "admin123" at code-generation time
    let admin_hash = bcrypt::hash("admin123", 12)
        .unwrap_or_else(|_| "$2b$12$placeholder".to_string());

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
    sql.push_str(&format!(
        "-- Seed admin user (password: admin123)\n\
         INSERT INTO users (email, password_hash, role, is_active)\n\
         VALUES ('admin@example.com', '{}', 'admin', true)\n\
         ON CONFLICT (email) DO NOTHING;\n\n",
        admin_hash
    ));

    // Sort entities by FK dependency: entities without FK references first,
    // then entities that reference only already-created tables.
    let non_user: Vec<&EntityDef> = entities.iter().filter(|e| e.name != "User").collect();
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
            // Avoid duplicate NOT NULL — only append if not already in sql_type
            let notnull = if f.nullable || sql_type.contains("NOT NULL") {
                ""
            } else {
                " NOT NULL"
            };
            // Avoid duplicate DEFAULT — only append if not already in sql_type
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

/// Simple topological sort for entity FK dependencies.
/// Entities that reference other entities (via REFERENCES in sql_type) come after them.
fn topological_sort_entities<'a>(entities: &[&'a EntityDef]) -> Vec<&'a EntityDef> {
    let names: std::collections::HashSet<String> = entities.iter()
        .map(|e| pluralize(&e.snake_name))
        .collect();

    let mut result: Vec<&EntityDef> = Vec::new();
    let mut placed: std::collections::HashSet<String> = std::collections::HashSet::new();
    placed.insert("users".to_string()); // users already created

    let mut remaining: Vec<&EntityDef> = entities.to_vec();
    let max_iter = remaining.len() + 1;
    for _ in 0..max_iter {
        if remaining.is_empty() {
            break;
        }
        let mut next_remaining = Vec::new();
        for entity in &remaining {
            let deps: Vec<String> = entity.fields.iter()
                .filter_map(|f| {
                    if let Some(pos) = f.sql_type.find("REFERENCES ") {
                        let rest = &f.sql_type[pos + 11..];
                        let table = rest.split('(').next().unwrap_or("").trim();
                        if names.contains(table) || table == "users" {
                            return Some(table.to_string());
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
    result.extend(remaining);
    result
}

/// Generate a `{entity}_queries.rs` file using runtime sqlx queries.
pub fn mock_generate_queries(entity: &EntityDef) -> String {
    let n = &entity.name;
    let sn = &entity.snake_name;
    let tn = pluralize(sn); // table name (correctly pluralized)

    // All columns for SELECT
    let cols: Vec<&str> = entity.fields.iter().map(|f| f.name.as_str()).collect();
    let col_list = cols.join(", ");

    // User-input fields (not id/created_at/updated_at) for INSERT/UPDATE
    let ufields = user_fields(entity);
    let unames: Vec<&str> = ufields.iter().map(|f| f.name.as_str()).collect();
    let placeholders: Vec<String> = (1..=unames.len()).map(|i| format!("${}", i)).collect();

    let mut code = String::new();

    // Header
    code.push_str("// ISLS v3.1 mock generated\n");
    code.push_str("use sqlx::PgPool;\n");
    code.push_str(&format!("use crate::{{AppError, errors::*}};\n"));
    code.push_str(&format!(
        "use crate::models::{}::{{{}, Create{}Payload, Update{}Payload}};\n",
        sn, n, n, n
    ));
    code.push_str("use crate::pagination::{PaginationParams, PaginatedResponse};\n\n");

    // GET by id
    code.push_str(&format!(
        "pub async fn get_{sn}(pool: &PgPool, id: i64) -> Result<{n}, AppError> {{\n"
    ));
    code.push_str(&format!(
        "    sqlx::query_as::<_, {n}>(\"SELECT {col_list} FROM {tn} WHERE id = $1\")\n"
    ));
    code.push_str("        .bind(id)\n");
    code.push_str("        .fetch_optional(pool)\n");
    code.push_str("        .await?\n");
    code.push_str(&format!(
        "        .ok_or_else(|| AppError::NotFound(format!(\"{n} {{}} not found\", id)))\n"
    ));
    code.push_str("}\n\n");

    // LIST with pagination
    code.push_str(&format!(
        "pub async fn list_{tn}(\n    pool: &PgPool,\n    params: &PaginationParams,\n) -> Result<PaginatedResponse<{n}>, AppError> {{\n"
    ));
    code.push_str(&format!(
        "    let row: (i64,) = sqlx::query_as(\"SELECT COUNT(*) FROM {tn}\")\n"
    ));
    code.push_str("        .fetch_one(pool)\n");
    code.push_str("        .await?;\n");
    code.push_str("    let total = row.0;\n\n");
    code.push_str(&format!(
        "    let items = sqlx::query_as::<_, {n}>(\"SELECT {col_list} FROM {tn} ORDER BY id DESC LIMIT $1 OFFSET $2\")\n"
    ));
    code.push_str("        .bind(params.limit())\n");
    code.push_str("        .bind(params.offset())\n");
    code.push_str("        .fetch_all(pool)\n");
    code.push_str("        .await?;\n\n");
    code.push_str("    Ok(PaginatedResponse::new(items, total, params))\n");
    code.push_str("}\n\n");

    // CREATE
    code.push_str(&format!(
        "pub async fn create_{sn}(pool: &PgPool, payload: Create{n}Payload) -> Result<{n}, AppError> {{\n"
    ));
    code.push_str(&format!(
        "    sqlx::query_as::<_, {n}>(\"INSERT INTO {tn} ({uname_list}) VALUES ({ph_list}) RETURNING {col_list}\")\n",
        uname_list = unames.join(", "),
        ph_list = placeholders.join(", "),
    ));
    for f in &ufields {
        code.push_str(&format!("        .bind(&payload.{})\n", f.name));
    }
    code.push_str("        .fetch_one(pool)\n");
    code.push_str("        .await\n");
    code.push_str("        .map_err(AppError::from)\n");
    code.push_str("}\n\n");

    // UPDATE — fetch current, apply patches, update
    code.push_str(&format!(
        "pub async fn update_{sn}(\n    pool: &PgPool,\n    id: i64,\n    payload: Update{n}Payload,\n) -> Result<{n}, AppError> {{\n"
    ));
    code.push_str(&format!(
        "    let mut current = get_{sn}(pool, id).await?;\n"
    ));
    // Field-by-field merge with proper nullable handling
    for f in &ufields {
        if f.nullable {
            code.push_str(&format!(
                "    if let Some(v) = payload.{name} {{ current.{name} = Some(v); }}\n",
                name = f.name
            ));
        } else {
            code.push_str(&format!(
                "    if let Some(v) = payload.{name} {{ current.{name} = v; }}\n",
                name = f.name
            ));
        }
    }
    let set_cols = unames.join(", ");
    let set_ph: Vec<String> = (2..=unames.len() + 1).map(|i| format!("${}", i)).collect();
    code.push_str(&format!(
        "    sqlx::query_as::<_, {n}>(\"UPDATE {tn} SET ({set_cols}) = ({set_ph}) WHERE id = $1 RETURNING {col_list}\")\n",
        set_cols = set_cols,
        set_ph = set_ph.join(", "),
    ));
    code.push_str("        .bind(id)\n");
    for f in &ufields {
        code.push_str(&format!("        .bind(&current.{})\n", f.name));
    }
    code.push_str("        .fetch_one(pool)\n");
    code.push_str("        .await\n");
    code.push_str("        .map_err(AppError::from)\n");
    code.push_str("}\n\n");

    // DELETE
    code.push_str(&format!(
        "pub async fn delete_{sn}(pool: &PgPool, id: i64) -> Result<(), AppError> {{\n"
    ));
    code.push_str(&format!(
        "    let result = sqlx::query(\"DELETE FROM {tn} WHERE id = $1\")\n"
    ));
    code.push_str("        .bind(id)\n");
    code.push_str("        .execute(pool)\n");
    code.push_str("        .await?;\n");
    code.push_str("    if result.rows_affected() == 0 {\n");
    code.push_str(&format!(
        "        return Err(AppError::NotFound(format!(\"{n} {{}} not found\", id)));\n"
    ));
    code.push_str("    }\n");
    code.push_str("    Ok(())\n");
    code.push_str("}\n");

    code
}

/// Generate a `services/{entity}.rs` file.
pub fn mock_generate_service(entity: &EntityDef) -> String {
    let n = &entity.name;
    let sn = &entity.snake_name;
    let pn = pluralize(sn);
    format!(
        r#"// ISLS v3.1 mock generated
use sqlx::PgPool;
use crate::{{AppError}};
use crate::models::{sn}::{{{n}, Create{n}Payload, Update{n}Payload}};
use crate::pagination::{{PaginationParams, PaginatedResponse}};
use crate::database::{sn}_queries;

pub async fn get_{sn}(pool: &PgPool, id: i64) -> Result<{n}, AppError> {{
    tracing::debug!("getting {sn} id={{}}", id);
    {sn}_queries::get_{sn}(pool, id).await
}}

pub async fn list_{pn}(
    pool: &PgPool,
    params: &PaginationParams,
) -> Result<PaginatedResponse<{n}>, AppError> {{
    tracing::debug!("listing {pn} page={{}}", params.page);
    {sn}_queries::list_{pn}(pool, params).await
}}

pub async fn create_{sn}(pool: &PgPool, payload: Create{n}Payload) -> Result<{n}, AppError> {{
    tracing::info!("creating {sn}");
    {sn}_queries::create_{sn}(pool, payload).await
}}

pub async fn update_{sn}(
    pool: &PgPool,
    id: i64,
    payload: Update{n}Payload,
) -> Result<{n}, AppError> {{
    tracing::info!("updating {sn} id={{}}", id);
    {sn}_queries::update_{sn}(pool, id, payload).await
}}

pub async fn delete_{sn}(pool: &PgPool, id: i64) -> Result<(), AppError> {{
    tracing::info!("deleting {sn} id={{}}", id);
    {sn}_queries::delete_{sn}(pool, id).await
}}
"#,
        n = n,
        sn = sn,
        pn = pn
    )
}

/// Generate an `api/{entity}.rs` file.
pub fn mock_generate_api(entity: &EntityDef) -> String {
    let n = &entity.name;
    let sn = &entity.snake_name;
    let tn = pluralize(sn); // correctly pluralized route path
    format!(
        r#"// ISLS v3.1 mock generated
use actix_web::{{web, HttpResponse, Responder}};
use sqlx::PgPool;
use crate::{{AppError, auth::AuthUser}};
use crate::models::{sn}::{{Create{n}Payload, Update{n}Payload}};
use crate::pagination::PaginationParams;
use crate::services::{sn} as {sn}_service;

pub async fn list_{tn}(
    pool: web::Data<PgPool>,
    params: web::Query<PaginationParams>,
    _user: AuthUser,
) -> Result<impl Responder, AppError> {{
    let result = {sn}_service::list_{tn}(&pool, &params).await?;
    Ok(HttpResponse::Ok().json(result))
}}

pub async fn get_{sn}(
    pool: web::Data<PgPool>,
    path: web::Path<i64>,
    _user: AuthUser,
) -> Result<impl Responder, AppError> {{
    let result = {sn}_service::get_{sn}(&pool, path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}}

pub async fn create_{sn}(
    pool: web::Data<PgPool>,
    body: web::Json<Create{n}Payload>,
    _user: AuthUser,
) -> Result<impl Responder, AppError> {{
    let result = {sn}_service::create_{sn}(&pool, body.into_inner()).await?;
    Ok(HttpResponse::Created().json(result))
}}

pub async fn update_{sn}(
    pool: web::Data<PgPool>,
    path: web::Path<i64>,
    body: web::Json<Update{n}Payload>,
    _user: AuthUser,
) -> Result<impl Responder, AppError> {{
    let result = {sn}_service::update_{sn}(&pool, path.into_inner(), body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(result))
}}

pub async fn delete_{sn}(
    pool: web::Data<PgPool>,
    path: web::Path<i64>,
    user: AuthUser,
) -> Result<impl Responder, AppError> {{
    crate::auth::require_role(&user, "admin")?;
    {sn}_service::delete_{sn}(&pool, path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}}

pub fn {sn}_routes(cfg: &mut web::ServiceConfig) {{
    cfg.service(
        web::scope("/api/{tn}")
            .route("", web::get().to(list_{tn}))
            .route("", web::post().to(create_{sn}))
            .route("/{{id}}", web::get().to(get_{sn}))
            .route("/{{id}}", web::put().to(update_{sn}))
            .route("/{{id}}", web::delete().to(delete_{sn})),
    );
}}
"#,
        n = n,
        sn = sn,
        tn = tn
    )
}

/// Generate `backend/src/database/pool.rs`.
pub fn mock_generate_pool() -> String {
    r#"// ISLS v3.1 mock generated
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use crate::AppError;

pub async fn create_pool() -> Result<PgPool, AppError> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| AppError::InternalError("DATABASE_URL not set".into()))?;
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .map_err(|e| AppError::InternalError(format!("database connection failed: {}", e)))
}
"#
    .into()
}

/// Generate `backend/src/main.rs`.
pub fn mock_generate_main(spec: &AppSpec) -> String {
    let entity_routes: Vec<String> = spec
        .entities
        .iter()
        .filter(|e| e.name != "User")
        .map(|e| format!("        api::{}::{}routes(cfg);", e.snake_name, e.snake_name.clone() + "_"))
        .collect();

    format!(
        r#"// ISLS v3.1 mock generated
use actix_cors::Cors;
use actix_web::{{middleware, web, App, HttpServer}};
use tracing_subscriber::EnvFilter;

mod api;
mod auth;
mod config;
mod database;
mod errors;
mod models;
mod pagination;
mod services;

use errors::AppError;

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
        app = spec.app_name
    )
}

/// Generate `backend/src/api/auth_routes.rs`.
pub fn mock_generate_auth_routes() -> String {
    r#"// ISLS v3.1 mock generated
use actix_web::{web, HttpResponse, Responder};
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{AppError, auth::{encode_jwt, Claims, AuthUser}};
use crate::models::user::{User, CreateUserPayload};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
}

pub async fn register(
    pool: web::Data<PgPool>,
    body: web::Json<CreateUserPayload>,
) -> Result<impl Responder, AppError> {
    let payload = body.into_inner();
    let errs = User::validate_email(&payload.email);
    if !errs.is_empty() {
        return Err(AppError::ValidationError(errs));
    }
    let hash = hash(&payload.password, DEFAULT_COST)
        .map_err(|e| AppError::InternalError(e.to_string()))?;

    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (email, password_hash, role) VALUES ($1, $2, $3)
         RETURNING id, email, password_hash, role, is_active, created_at, updated_at",
    )
    .bind(&payload.email)
    .bind(&hash)
    .bind(payload.role.as_deref().unwrap_or("user"))
    .fetch_one(pool.get_ref())
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.constraint() == Some("users_email_key") =>
            AppError::Conflict("email already registered".into()),
        _ => AppError::from(e),
    })?;

    tracing::info!("registered user {}", user.email);
    Ok(HttpResponse::Created().json(user))
}

pub async fn login(
    pool: web::Data<PgPool>,
    body: web::Json<LoginRequest>,
) -> Result<impl Responder, AppError> {
    let req = body.into_inner();
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, role, is_active, created_at FROM users WHERE email = $1",
    )
    .bind(&req.email)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or(AppError::Unauthorized)?;

    let valid = verify(&req.password, &user.password_hash)
        .map_err(|_| AppError::Unauthorized)?;
    if !valid { return Err(AppError::Unauthorized); }

    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".into());
    let exp = (Utc::now() + Duration::hours(24)).timestamp() as usize;
    let claims = Claims { sub: user.id, email: user.email.clone(), role: user.role.clone(), exp };
    let token = encode_jwt(&claims, &secret)?;

    Ok(HttpResponse::Ok().json(LoginResponse { token }))
}

pub async fn me(user: AuthUser) -> Result<impl Responder, AppError> {
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "id": user.id,
        "email": user.email,
        "role": user.role,
    })))
}

pub async fn health(pool: web::Data<PgPool>) -> impl Responder {
    let db_ok = sqlx::query("SELECT 1").execute(pool.get_ref()).await.is_ok();
    HttpResponse::Ok().json(serde_json::json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "database": if db_ok { "connected" } else { "disconnected" },
        "version": "1.0.0"
    }))
}

pub fn auth_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .route("/health", web::get().to(health))
            .route("/auth/register", web::post().to(register))
            .route("/auth/login", web::post().to(login))
            .route("/auth/me", web::get().to(me)),
    );
}
"#
    .into()
}

// ─── Frontend ─────────────────────────────────────────────────────────────────

/// Generate `frontend/index.html` — working SPA with login, product list, and create form.
pub fn mock_generate_frontend_index(spec: &AppSpec) -> String {
    let app = &spec.app_name;
    // Build nav links for all non-User entities
    let nav_links: Vec<String> = spec.entities.iter()
        .filter(|e| e.name != "User")
        .map(|e| format!(
            "      <button onclick=\"loadPage('{}')\">{}</button>",
            e.snake_name, e.name
        ))
        .collect();
    let nav_html = nav_links.join("\n");

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
      loadPage('product');
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
        html += '<button onclick="showCreateForm(\'' + entity + '\')">+ Create</button>';
        html += '<div id="create-form" style="display:none;margin:1rem 0;padding:1rem;background:#f9f9f9;border-radius:8px"></div>';
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

    function showCreateForm(entity) {{
      const form = document.getElementById('create-form');
      if (form.style.display !== 'none') {{ form.style.display = 'none'; return; }}
      let fields = '';
      if (entity === 'product') {{
        fields = '<input id="cf-sku" placeholder="SKU" style="margin:4px"><input id="cf-name" placeholder="Name" style="margin:4px"><input id="cf-price" placeholder="Price (cents)" type="number" style="margin:4px"><input id="cf-qty" placeholder="Quantity" type="number" style="margin:4px"><input id="cf-reorder" placeholder="Reorder level" type="number" value="10" style="margin:4px"><input id="cf-reorderqty" placeholder="Reorder qty" type="number" value="50" style="margin:4px">';
        fields += '<br><button onclick="createProduct()">Create Product</button>';
      }} else {{
        fields = '<p>Use the API to create ' + entity + 's.</p>';
      }}
      form.innerHTML = fields;
      form.style.display = 'block';
    }}

    async function createProduct() {{
      try {{
        const payload = {{
          sku: document.getElementById('cf-sku').value,
          name: document.getElementById('cf-name').value,
          unit_price_cents: parseInt(document.getElementById('cf-price').value) || 0,
          cost_price_cents: 0,
          quantity_on_hand: parseInt(document.getElementById('cf-qty').value) || 0,
          reorder_level: parseInt(document.getElementById('cf-reorder').value) || 10,
          reorder_quantity: parseInt(document.getElementById('cf-reorderqty').value) || 50,
          is_active: true,
          warehouse_id: 1
        }};
        await apiFetch('POST', '/api/products', payload);
        loadPage('product');
      }} catch (e) {{
        alert('Error: ' + e.message);
      }}
    }}
  </script>
</body>
</html>
"#,
        app = app,
        nav_html = nav_html
    )
}

/// Generate `frontend/style.css`.
pub fn mock_generate_style_css() -> String {
    r#"/* ISLS v3.1 mock generated */
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
"#
    .into()
}

/// Generate `frontend/src/api/client.js`.
pub fn mock_generate_api_client() -> String {
    r#"// ISLS v3.1 mock generated — fetch-based API client
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

async function register(email, password, role) {
  return apiFetch('POST', '/api/auth/register', { email, password, role });
}

async function getMe() {
  return apiFetch('GET', '/api/auth/me');
}
"#
    .into()
}

/// Generate a frontend page JS module for the given entity.
pub fn mock_generate_entity_page(entity: &EntityDef) -> String {
    let n = &entity.name;
    let sn = &entity.snake_name;
    format!(
        r#"// ISLS v3.1 mock generated — {n} page
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

/// Generate `backend/tests/api_tests.rs`.
pub fn mock_generate_integration_tests(spec: &AppSpec) -> String {
    let first_entity = spec
        .entities
        .iter()
        .find(|e| e.name != "User")
        .map(|e| e.snake_name.as_str())
        .unwrap_or("item");

    format!(
        r#"// ISLS v3.1 mock generated integration tests
// Run with: cargo test --test api_tests (requires DATABASE_URL env var)
use actix_web::{{test, web, App}};

#[actix_web::test]
async fn test_health_endpoint() {{
    // Health check should always return 200
    // Full integration test setup omitted in mock mode
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
