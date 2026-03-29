// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! ProvidedSymbol factories for the Codegen-HDAG.
//!
//! Each `provides_*` function returns the exact set of [`ProvidedSymbol`]s that
//! a module exposes to its successors.  These are injected verbatim into LLM
//! prompts, eliminating import-path guessing and type-signature hallucination.

use crate::EntityDef;
use crate::hdag::{ProvidedSymbol, SymbolKind};

// ─── Foundation symbols ───────────────────────────────────────────────────────

/// Symbols provided by `errors.rs` to all successor nodes.
pub fn provides_apperror() -> Vec<ProvidedSymbol> {
    vec![ProvidedSymbol {
        import_path: "crate::errors::AppError".into(),
        kind: SymbolKind::Enum,
        signature: r#"#[derive(Debug, Serialize)]
pub enum AppError {
    NotFound(String),
    InternalError(String),
    ValidationError(Vec<String>),
    Unauthorized,
    Forbidden,
    BadRequest(String),
}
impl actix_web::ResponseError for AppError {
    fn error_response(&self) -> actix_web::HttpResponse { ... }
    fn status_code(&self) -> actix_web::http::StatusCode { ... }
}"#.into(),
    }]
}

/// Symbols provided by `pagination.rs` to service and API nodes.
pub fn provides_pagination() -> Vec<ProvidedSymbol> {
    vec![
        ProvidedSymbol {
            import_path: "crate::pagination::PaginationParams".into(),
            kind: SymbolKind::Struct,
            signature: "#[derive(Debug, Deserialize, Clone)]\npub struct PaginationParams {\n    pub page: Option<i64>,\n    pub per_page: Option<i64>,\n}\nimpl PaginationParams {\n    pub fn offset(&self) -> i64 { ... }\n    pub fn limit(&self) -> i64 { ... }\n}".into(),
        },
        ProvidedSymbol {
            import_path: "crate::pagination::PaginatedResponse".into(),
            kind: SymbolKind::Struct,
            signature: "#[derive(Debug, Serialize)]\npub struct PaginatedResponse<T: Serialize> {\n    pub items: Vec<T>,\n    pub total: i64,\n    pub page: i64,\n    pub per_page: i64,\n}".into(),
        },
    ]
}

/// Symbols provided by `auth.rs` to API handler nodes.
///
/// Includes AuthUser (extractor), encode_jwt, and require_role — all exported
/// by the structural auth.rs generator.  auth_routes needs encode_jwt for login.
pub fn provides_authuser() -> Vec<ProvidedSymbol> {
    vec![
        ProvidedSymbol {
            import_path: "crate::auth::AuthUser".into(),
            kind: SymbolKind::Struct,
            signature: "/// Extractor that validates Bearer JWT and exposes claims.\n#[derive(Debug, Clone)]\npub struct AuthUser {\n    pub user_id: i64,\n    pub email: String,\n    pub role: String,\n}\nimpl actix_web::FromRequest for AuthUser { ... }".into(),
        },
        ProvidedSymbol {
            import_path: "crate::auth::encode_jwt".into(),
            kind: SymbolKind::Function,
            signature: "pub fn encode_jwt(user_id: i64, email: &str, role: &str) -> Result<String, AppError>;".into(),
        },
        ProvidedSymbol {
            import_path: "crate::auth::require_role".into(),
            kind: SymbolKind::Function,
            signature: "pub fn require_role(user: &AuthUser, required: &str) -> Result<(), AppError>;".into(),
        },
    ]
}

/// Symbols provided by `config.rs` to nodes that need AppConfig.
pub fn provides_config() -> Vec<ProvidedSymbol> {
    vec![ProvidedSymbol {
        import_path: "crate::config::AppConfig".into(),
        kind: SymbolKind::Struct,
        signature: "#[derive(Debug, Clone)]\npub struct AppConfig {\n    pub database_url: String,\n    pub jwt_secret: String,\n    pub port: u16,\n}\nimpl AppConfig {\n    pub fn from_env() -> Self { ... }\n}".into(),
    }]
}

/// Symbols provided by `database/pool.rs` to query and service nodes.
///
/// Note: create_pool() returns sqlx::Error (not AppError) — matches the
/// structural generator exactly.  Downstream query/service nodes receive
/// &PgPool as a parameter and never call create_pool() directly.
pub fn provides_pool() -> Vec<ProvidedSymbol> {
    vec![ProvidedSymbol {
        import_path: "crate::database::pool::create_pool".into(),
        kind: SymbolKind::Function,
        signature: "pub async fn create_pool() -> Result<sqlx::PgPool, sqlx::Error>;".into(),
    }]
}

// ─── Entity-specific symbols ──────────────────────────────────────────────────

/// Symbols provided by `models/{entity}.rs` to queries, services, and API nodes.
///
/// Builds the struct signatures dynamically from `entity.fields` so the LLM
/// sees the exact field names and types — no hallucination possible.
pub fn provides_model_types(entity: &EntityDef) -> Vec<ProvidedSymbol> {
    let sn = &entity.snake_name;
    let pn = &entity.name;

    // Main struct signature built from entity fields.
    // Uses DateTime<Utc> (not chrono::DateTime<chrono::Utc>) to match the structural
    // generator output which imports `use chrono::{DateTime, Utc};`.
    let mut main_sig = format!(
        "#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]\npub struct {} {{\n    pub id: i64,\n",
        pn
    );
    for field in &entity.fields {
        if field.name == "id" || field.name == "created_at" || field.name == "updated_at" {
            continue;
        }
        let rust_t = field_rust_type_short(field);
        main_sig.push_str(&format!("    pub {}: {},\n", field.name, rust_t));
    }
    main_sig.push_str("    pub created_at: DateTime<Utc>,\n");
    main_sig.push_str("    pub updated_at: DateTime<Utc>,\n}");

    // Create payload — all user-settable fields with COMPLETE type info (no placeholders).
    // Clone is required: the structural model derives it and callers move the payload.
    let mut create_sig = format!(
        "#[derive(Debug, Deserialize, Clone)]\npub struct Create{}Payload {{\n",
        pn
    );
    for field in &entity.fields {
        if field.name == "id" || field.name == "created_at" || field.name == "updated_at" {
            continue;
        }
        let rust_t = field_rust_type_short(field);
        create_sig.push_str(&format!("    pub {}: {},\n", field.name, rust_t));
    }
    create_sig.push('}');

    // Update payload — ALL fields as Option<T> for partial updates.
    // Complete field list so LLM query code knows exact types when binding SQL params.
    let mut update_sig = format!(
        "#[derive(Debug, Deserialize, Clone)]\npub struct Update{}Payload {{\n",
        pn
    );
    for field in &entity.fields {
        if field.name == "id" || field.name == "created_at" || field.name == "updated_at" {
            continue;
        }
        let base_t = field_base_rust_type_short(field);
        update_sig.push_str(&format!("    pub {}: Option<{}>,\n", field.name, base_t));
    }
    update_sig.push('}');

    vec![
        ProvidedSymbol {
            import_path: format!("crate::models::{}::{}", sn, pn),
            kind: SymbolKind::Struct,
            signature: main_sig,
        },
        ProvidedSymbol {
            import_path: format!("crate::models::{}::Create{}Payload", sn, pn),
            kind: SymbolKind::Struct,
            signature: create_sig,
        },
        ProvidedSymbol {
            import_path: format!("crate::models::{}::Update{}Payload", sn, pn),
            kind: SymbolKind::Struct,
            signature: update_sig,
        },
    ]
}

/// Symbols provided by `models/user.rs` (hardcoded User model types).
pub fn provides_user_model_types() -> Vec<ProvidedSymbol> {
    vec![
        ProvidedSymbol {
            import_path: "crate::models::user::User".into(),
            kind: SymbolKind::Struct,
            signature: "#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]\npub struct User {\n    pub id: i64,\n    pub email: String,\n    pub password_hash: String,\n    pub role: String,\n    pub is_active: bool,\n    pub created_at: chrono::DateTime<chrono::Utc>,\n    pub updated_at: chrono::DateTime<chrono::Utc>,\n}".into(),
        },
        ProvidedSymbol {
            import_path: "crate::models::user::CreateUserPayload".into(),
            kind: SymbolKind::Struct,
            signature: "#[derive(Debug, Deserialize)]\npub struct CreateUserPayload {\n    pub email: String,\n    pub password: String,\n    pub role: Option<String>,\n}".into(),
        },
        ProvidedSymbol {
            import_path: "crate::models::user::UpdateUserPayload".into(),
            kind: SymbolKind::Struct,
            signature: "#[derive(Debug, Deserialize)]\npub struct UpdateUserPayload {\n    pub email: Option<String>,\n    pub role: Option<String>,\n    pub is_active: Option<bool>,\n}".into(),
        },
    ]
}

/// Symbols provided by `database/{entity}_queries.rs` to service nodes.
///
/// Returns the exact CRUD function signatures the service layer must use.
pub fn provides_query_fns(entity: &EntityDef) -> Vec<ProvidedSymbol> {
    let sn = &entity.snake_name;
    let pn = &entity.name;
    vec![
        ProvidedSymbol {
            import_path: format!("crate::database::{}_queries::get_{}", sn, sn),
            kind: SymbolKind::Function,
            signature: format!("pub async fn get_{sn}(pool: &sqlx::PgPool, id: i64) -> Result<{pn}, AppError>;"),
        },
        ProvidedSymbol {
            import_path: format!("crate::database::{}_queries::list_{}s", sn, sn),
            kind: SymbolKind::Function,
            signature: format!("pub async fn list_{sn}s(pool: &sqlx::PgPool, params: &PaginationParams) -> Result<PaginatedResponse<{pn}>, AppError>;"),
        },
        ProvidedSymbol {
            import_path: format!("crate::database::{}_queries::create_{}", sn, sn),
            kind: SymbolKind::Function,
            // Owned payload — no & prefix — callers move the payload in
            signature: format!("pub async fn create_{sn}(pool: &sqlx::PgPool, payload: Create{pn}Payload) -> Result<{pn}, AppError>;"),
        },
        ProvidedSymbol {
            import_path: format!("crate::database::{}_queries::update_{}", sn, sn),
            kind: SymbolKind::Function,
            signature: format!("pub async fn update_{sn}(pool: &sqlx::PgPool, id: i64, payload: Update{pn}Payload) -> Result<{pn}, AppError>;"),
        },
        ProvidedSymbol {
            import_path: format!("crate::database::{}_queries::delete_{}", sn, sn),
            kind: SymbolKind::Function,
            signature: format!("pub async fn delete_{sn}(pool: &sqlx::PgPool, id: i64) -> Result<(), AppError>;"),
        },
    ]
}

/// Symbols provided by `database/user_queries.rs` to auth_routes.
pub fn provides_query_fns_user() -> Vec<ProvidedSymbol> {
    vec![
        ProvidedSymbol {
            import_path: "crate::database::user_queries::get_user_by_email".into(),
            kind: SymbolKind::Function,
            signature: "pub async fn get_user_by_email(pool: &sqlx::PgPool, email: &str) -> Result<User, AppError>;".into(),
        },
        ProvidedSymbol {
            import_path: "crate::database::user_queries::create_user".into(),
            kind: SymbolKind::Function,
            signature: "pub async fn create_user(pool: &sqlx::PgPool, payload: CreateUserPayload) -> Result<User, AppError>;".into(),
        },
        ProvidedSymbol {
            import_path: "crate::database::user_queries::get_user".into(),
            kind: SymbolKind::Function,
            signature: "pub async fn get_user(pool: &sqlx::PgPool, id: i64) -> Result<User, AppError>;".into(),
        },
    ]
}

/// Symbols provided by `services/{entity}.rs` to API handler nodes.
pub fn provides_service_fns(entity: &EntityDef) -> Vec<ProvidedSymbol> {
    let sn = &entity.snake_name;
    let pn = &entity.name;
    vec![
        ProvidedSymbol {
            import_path: format!("crate::services::{}::get_{}", sn, sn),
            kind: SymbolKind::Function,
            signature: format!("pub async fn get_{sn}(pool: &sqlx::PgPool, id: i64) -> Result<{pn}, AppError>;"),
        },
        ProvidedSymbol {
            import_path: format!("crate::services::{}::list_{}s", sn, sn),
            kind: SymbolKind::Function,
            signature: format!("pub async fn list_{sn}s(pool: &sqlx::PgPool, params: &PaginationParams) -> Result<PaginatedResponse<{pn}>, AppError>;"),
        },
        ProvidedSymbol {
            import_path: format!("crate::services::{}::create_{}", sn, sn),
            kind: SymbolKind::Function,
            // Owned payload — callers move payload in (e.g. payload.into_inner() from web::Json)
            signature: format!("pub async fn create_{sn}(pool: &sqlx::PgPool, payload: Create{pn}Payload) -> Result<{pn}, AppError>;"),
        },
        ProvidedSymbol {
            import_path: format!("crate::services::{}::update_{}", sn, sn),
            kind: SymbolKind::Function,
            signature: format!("pub async fn update_{sn}(pool: &sqlx::PgPool, id: i64, payload: Update{pn}Payload) -> Result<{pn}, AppError>;"),
        },
        ProvidedSymbol {
            import_path: format!("crate::services::{}::delete_{}", sn, sn),
            kind: SymbolKind::Function,
            signature: format!("pub async fn delete_{sn}(pool: &sqlx::PgPool, id: i64) -> Result<(), AppError>;"),
        },
    ]
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Map an entity field to its Rust type (respecting nullable → Option<T>).
/// Uses fully-qualified `chrono::DateTime<chrono::Utc>` form.
fn field_rust_type(field: &isls_hypercube::domain::FieldDef) -> String {
    if field.nullable {
        format!("Option<{}>", field_base_rust_type(field))
    } else {
        field_base_rust_type(field)
    }
}

/// Base Rust type for a field (without Option wrapper).
/// Uses fully-qualified `chrono::DateTime<chrono::Utc>` form.
fn field_base_rust_type(field: &isls_hypercube::domain::FieldDef) -> String {
    // Use existing rust_type if provided
    if !field.rust_type.is_empty() {
        // Normalise common aliases
        return match field.rust_type.as_str() {
            "DateTime<Utc>" | "chrono::DateTime<Utc>" => "chrono::DateTime<chrono::Utc>".into(),
            t => t.to_string(),
        };
    }
    // Fallback: infer from sql_type
    let sql = field.sql_type.to_uppercase();
    if sql.contains("BIGSERIAL") || sql.contains("BIGINT") {
        "i64".into()
    } else if sql.contains("SERIAL") || sql.contains("INTEGER") || sql.contains("INT") {
        "i32".into()
    } else if sql.contains("BOOL") {
        "bool".into()
    } else if sql.contains("FLOAT") || sql.contains("REAL") || sql.contains("DOUBLE") {
        "f64".into()
    } else if sql.contains("DECIMAL") || sql.contains("NUMERIC") {
        "f64".into()
    } else if sql.contains("TIMESTAMPTZ") || sql.contains("TIMESTAMP") {
        "chrono::DateTime<chrono::Utc>".into()
    } else {
        "String".into()
    }
}

/// Like `field_rust_type` but normalises DateTime to the short `DateTime<Utc>` form,
/// matching the structural generator which imports `use chrono::{DateTime, Utc};`.
fn field_rust_type_short(field: &isls_hypercube::domain::FieldDef) -> String {
    if field.nullable {
        format!("Option<{}>", field_base_rust_type_short(field))
    } else {
        field_base_rust_type_short(field)
    }
}

/// Like `field_base_rust_type` but normalises DateTime to `DateTime<Utc>` (short form).
fn field_base_rust_type_short(field: &isls_hypercube::domain::FieldDef) -> String {
    if !field.rust_type.is_empty() {
        return match field.rust_type.as_str() {
            "DateTime<Utc>"
            | "chrono::DateTime<Utc>"
            | "chrono::DateTime<chrono::Utc>" => "DateTime<Utc>".into(),
            t => t.to_string(),
        };
    }
    let sql = field.sql_type.to_uppercase();
    if sql.contains("BIGSERIAL") || sql.contains("BIGINT") {
        "i64".into()
    } else if sql.contains("SERIAL") || sql.contains("INTEGER") || sql.contains("INT") {
        "i32".into()
    } else if sql.contains("BOOL") {
        "bool".into()
    } else if sql.contains("FLOAT") || sql.contains("REAL") || sql.contains("DOUBLE")
        || sql.contains("DECIMAL") || sql.contains("NUMERIC")
    {
        "f64".into()
    } else if sql.contains("TIMESTAMPTZ") || sql.contains("TIMESTAMP") {
        "DateTime<Utc>".into()
    } else {
        "String".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provides_apperror_has_import_path() {
        let syms = provides_apperror();
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].import_path, "crate::errors::AppError");
    }

    #[test]
    fn test_provides_pagination_has_two_symbols() {
        let syms = provides_pagination();
        assert_eq!(syms.len(), 2);
    }

    #[test]
    fn test_provides_model_types_uses_entity_fields() {
        use isls_hypercube::domain::FieldDef;
        let entity = EntityDef {
            name: "Product".into(),
            snake_name: "product".into(),
            fields: vec![
                FieldDef { name: "id".into(), rust_type: "i64".into(), sql_type: "BIGSERIAL PRIMARY KEY".into(), nullable: false, default_value: None, description: "PK".into() },
                FieldDef { name: "sku".into(), rust_type: "String".into(), sql_type: "VARCHAR(255)".into(), nullable: false, default_value: None, description: "SKU".into() },
                FieldDef { name: "price".into(), rust_type: "i64".into(), sql_type: "BIGINT".into(), nullable: false, default_value: None, description: "Price in cents".into() },
            ],
            validations: vec![],
            business_rules: vec![],
            relationships: vec![],
        };
        let syms = provides_model_types(&entity);
        assert_eq!(syms.len(), 3, "should provide main struct + create + update payloads");
        // Main struct must contain 'sku'
        assert!(syms[0].signature.contains("sku"), "main struct must contain sku field");
        // Import path must reference crate::models::product
        assert!(syms[0].import_path.contains("crate::models::product"));
    }
}
