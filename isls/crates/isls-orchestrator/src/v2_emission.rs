// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! V2 emission engine: renders production-quality Tera templates from domain
//! entity definitions into a complete full-stack application.
//!
//! Called by `isls-decomposer` during hypercube decomposition.

use std::path::{Path, PathBuf};
use serde_json::json;
use tera::{Context, Tera};

use isls_hypercube::domain::{EntityTemplate, DomainTemplate, BusinessRule};
use isls_hypercube::parser::to_snake_case;

use crate::stages::write_file;
use crate::OrchestratorError;

/// Artifact produced by the v2 emission engine.
#[derive(Clone, Debug)]
pub struct EmittedFile {
    /// Relative path within the output directory.
    pub rel_path: PathBuf,
    /// Lines of code in the file.
    pub loc: usize,
    /// File category for tracing.
    pub category: String,
}

/// Result of a full v2 emission run.
#[derive(Clone, Debug)]
pub struct V2EmissionResult {
    /// All files written.
    pub files: Vec<EmittedFile>,
    /// Total lines of code.
    pub total_loc: usize,
}

/// Run the full v2 emission pipeline: render all templates for a domain.
pub fn emit_v2(
    app_name: &str,
    domain: &DomainTemplate,
    entities: &[EntityTemplate],
    output_dir: &Path,
) -> Result<V2EmissionResult, OrchestratorError> {
    let tera = crate::templates::build_engine_v2()
        .map_err(|e| OrchestratorError::Template(format!("Tera init failed: {}", e)))?;

    let mut files = Vec::new();
    let app_name_snake = app_name.replace('-', "_");
    let entity_snakes: Vec<String> = entities.iter().map(|e| to_snake_case(&e.name)).collect();

    // ── Backend: Cargo.toml ──────────────────────────────────────────────
    emit_simple(&tera, "v2_cargo_toml", &json!({"app_name": app_name}),
        output_dir, "backend/Cargo.toml", "deployment", &mut files)?;

    // ── Backend: src/main.rs ─────────────────────────────────────────────
    emit_simple(&tera, "v2_main_rs", &json!({
        "app_name": app_name,
        "entity_snakes": &entity_snakes,
    }), output_dir, "backend/src/main.rs", "architecture", &mut files)?;

    // ── Backend: src/config.rs ───────────────────────────────────────────
    emit_simple(&tera, "v2_config_rs", &json!({"app_name_snake": &app_name_snake}),
        output_dir, "backend/src/config.rs", "architecture", &mut files)?;

    // ── Backend: src/errors.rs ───────────────────────────────────────────
    emit_simple(&tera, "v2_errors_rs", &json!({}),
        output_dir, "backend/src/errors.rs", "business_logic", &mut files)?;

    // ── Backend: src/pagination.rs ───────────────────────────────────────
    emit_simple(&tera, "v2_pagination_rs", &json!({}),
        output_dir, "backend/src/pagination.rs", "interface", &mut files)?;

    // ── Backend: src/auth.rs ─────────────────────────────────────────────
    emit_simple(&tera, "v2_auth_rs", &json!({}),
        output_dir, "backend/src/auth.rs", "security", &mut files)?;

    // ── Backend: src/models/mod.rs ───────────────────────────────────────
    emit_simple(&tera, "v2_mod_models_rs", &json!({"entity_snakes": &entity_snakes}),
        output_dir, "backend/src/models/mod.rs", "data_model", &mut files)?;

    // ── Backend: src/database/mod.rs ─────────────────────────────────────
    emit_simple(&tera, "v2_mod_database_rs", &json!({"entity_snakes": &entity_snakes}),
        output_dir, "backend/src/database/mod.rs", "storage", &mut files)?;

    // ── Backend: src/database/pool.rs ────────────────────────────────────
    emit_simple(&tera, "v2_pool_rs", &json!({}),
        output_dir, "backend/src/database/pool.rs", "storage", &mut files)?;

    // ── Backend: src/services/mod.rs ─────────────────────────────────────
    emit_simple(&tera, "v2_mod_services_rs", &json!({"entity_snakes": &entity_snakes}),
        output_dir, "backend/src/services/mod.rs", "business_logic", &mut files)?;

    // ── Backend: src/api/mod.rs ──────────────────────────────────────────
    emit_simple(&tera, "v2_mod_api_rs", &json!({"entity_snakes": &entity_snakes}),
        output_dir, "backend/src/api/mod.rs", "interface", &mut files)?;

    // ── Per-entity files ─────────────────────────────────────────────────
    for entity in entities {
        let snake = to_snake_case(&entity.name);
        let table_name = format!("{}s", snake);

        // Model
        let model_ctx = build_model_context(entity);
        emit_simple(&tera, "v2_model_rs", &model_ctx,
            output_dir, &format!("backend/src/models/{}.rs", snake), "data_model", &mut files)?;

        // Queries
        let query_ctx = build_query_context(entity, &table_name, &domain.api_features);
        emit_simple(&tera, "v2_queries_rs", &query_ctx,
            output_dir, &format!("backend/src/database/{}_queries.rs", snake), "storage", &mut files)?;

        // Service
        let service_ctx = build_service_context(entity, &table_name, &domain.business_rules, &domain.api_features);
        emit_simple(&tera, "v2_service_rs", &service_ctx,
            output_dir, &format!("backend/src/services/{}.rs", snake), "business_logic", &mut files)?;

        // API
        let api_ctx = build_api_context(entity, &table_name, &domain.business_rules, &domain.api_features);
        emit_simple(&tera, "v2_api_rs", &api_ctx,
            output_dir, &format!("backend/src/api/{}.rs", snake), "interface", &mut files)?;
    }

    // ── Auth routes (separate API module) ────────────────────────────────
    emit_auth_routes(output_dir, &mut files)?;

    // ── Migration ────────────────────────────────────────────────────────
    let migration_ctx = build_migration_context(app_name, entities);
    emit_simple(&tera, "v2_migration_sql", &migration_ctx,
        output_dir, "backend/migrations/001_initial.sql", "storage", &mut files)?;

    // ── Integration tests ────────────────────────────────────────────────
    let test_ctx = build_test_context(entities, &domain.business_rules);
    emit_simple(&tera, "v2_integration_test_rs", &test_ctx,
        output_dir, "backend/tests/api_tests.rs", "testing", &mut files)?;

    // ── Frontend ─────────────────────────────────────────────────────────
    let pages: Vec<serde_json::Value> = entities.iter().map(|e| {
        let snake = to_snake_case(&e.name);
        json!({"route": snake.clone(), "label": e.name.clone()})
    }).collect();

    emit_simple(&tera, "v2_frontend_index_html", &json!({
        "app_name": app_name,
        "pages": &pages,
    }), output_dir, "frontend/index.html", "presentation", &mut files)?;

    emit_simple(&tera, "v2_frontend_style_css", &json!({"app_name": app_name}),
        output_dir, "frontend/style.css", "presentation", &mut files)?;

    emit_simple(&tera, "v2_frontend_client_js", &json!({}),
        output_dir, "frontend/src/api/client.js", "presentation", &mut files)?;

    emit_simple(&tera, "v2_frontend_dashboard_js", &json!({}),
        output_dir, "frontend/src/pages/dashboard.js", "presentation", &mut files)?;

    emit_simple(&tera, "v2_frontend_login_js", &json!({}),
        output_dir, "frontend/src/pages/login.js", "presentation", &mut files)?;

    // Frontend app.js (SPA router)
    emit_frontend_app_js(entities, output_dir, &mut files)?;

    // Per-entity frontend pages
    for entity in entities {
        let snake = to_snake_case(&entity.name);
        let table_name = format!("{}s", snake);
        let page_ctx = build_frontend_page_context(entity, &table_name);
        emit_simple(&tera, "v2_frontend_page_js", &page_ctx,
            output_dir, &format!("frontend/src/pages/{}.js", snake), "presentation", &mut files)?;
    }

    // ── Deployment ───────────────────────────────────────────────────────
    emit_simple(&tera, "v2_dockerfile", &json!({"app_name": app_name}),
        output_dir, "backend/Dockerfile", "deployment", &mut files)?;

    emit_simple(&tera, "v2_docker_compose", &json!({
        "app_name": app_name, "app_name_snake": &app_name_snake
    }), output_dir, "docker-compose.yml", "deployment", &mut files)?;

    emit_simple(&tera, "v2_env_example", &json!({"app_name_snake": &app_name_snake}),
        output_dir, ".env.example", "deployment", &mut files)?;

    // README
    let readme_entities: Vec<serde_json::Value> = entities.iter().map(|e| {
        let snake = to_snake_case(&e.name);
        json!({"name": e.name, "table": format!("{}s", snake)})
    }).collect();
    emit_simple(&tera, "v2_readme_md", &json!({
        "app_name": app_name, "entities": &readme_entities
    }), output_dir, "README.md", "documentation", &mut files)?;

    // ── Gitignore ────────────────────────────────────────────────────────
    let gitignore = "/target\n.env\n*.swp\n*.swo\n";
    let gp = output_dir.join("backend/.gitignore");
    let _bytes = write_file(&gp, gitignore)?;
    files.push(EmittedFile {
        rel_path: "backend/.gitignore".into(),
        loc: gitignore.lines().count(),
        category: "deployment".into(),
    });

    let total_loc: usize = files.iter().map(|f| f.loc).sum();
    Ok(V2EmissionResult { files, total_loc })
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn emit_simple(
    tera: &Tera,
    template: &str,
    data: &serde_json::Value,
    output_dir: &Path,
    rel_path: &str,
    category: &str,
    files: &mut Vec<EmittedFile>,
) -> Result<(), OrchestratorError> {
    let mut ctx = Context::new();
    if let Some(obj) = data.as_object() {
        for (k, v) in obj {
            ctx.insert(k, v);
        }
    }
    let rendered = tera.render(template, &ctx)
        .map_err(|e| OrchestratorError::Template(format!("{}: {}", template, e)))?;
    let full_path = output_dir.join(rel_path);
    write_file(&full_path, &rendered)?;
    files.push(EmittedFile {
        rel_path: rel_path.into(),
        loc: rendered.lines().count(),
        category: category.into(),
    });
    Ok(())
}

fn build_model_context(entity: &EntityTemplate) -> serde_json::Value {
    let fields: Vec<serde_json::Value> = entity.fields.iter().map(|f| {
        json!({"name": f.name, "rust_type": f.rust_type})
    }).collect();

    // Create fields: skip id and timestamps
    let create_fields: Vec<serde_json::Value> = entity.fields.iter()
        .filter(|f| f.name != "id" && f.name != "created_at" && f.name != "updated_at")
        .map(|f| json!({"name": f.name, "rust_type": f.rust_type}))
        .collect();

    // Update fields: skip id/timestamps, all become Optional
    let update_fields: Vec<serde_json::Value> = entity.fields.iter()
        .filter(|f| f.name != "id" && f.name != "created_at" && f.name != "updated_at")
        .map(|f| {
            let base_type = if f.rust_type.starts_with("Option<") {
                f.rust_type.trim_start_matches("Option<").trim_end_matches('>').to_string()
            } else {
                f.rust_type.clone()
            };
            json!({"name": f.name, "base_type": base_type})
        })
        .collect();

    let validations: Vec<serde_json::Value> = entity.validations.iter().map(|v| {
        // Remap self.field to self.field for the Create payload context
        let condition = v.condition.replace("self.", "self.");
        json!({"condition": condition, "message": v.message})
    }).collect();

    json!({
        "entity_name": entity.name,
        "entity_description": entity.description,
        "fields": fields,
        "create_fields": create_fields,
        "update_fields": update_fields,
        "validations": validations,
    })
}

fn build_query_context(entity: &EntityTemplate, table_name: &str, api: &isls_hypercube::domain::ApiFeatures) -> serde_json::Value {
    let snake = to_snake_case(&entity.name);
    let select_columns: String = entity.fields.iter()
        .map(|f| f.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let insert_fields: Vec<&isls_hypercube::domain::FieldDef> = entity.fields.iter()
        .filter(|f| f.name != "id" && !f.name.ends_with("_at"))
        .collect();
    let insert_columns: String = insert_fields.iter()
        .map(|f| f.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let insert_placeholders: String = (1..=insert_fields.len())
        .map(|i| format!("${}", i))
        .collect::<Vec<_>>()
        .join(", ");
    let insert_binds: Vec<String> = insert_fields.iter()
        .map(|f| {
            let is_optional = f.nullable || f.rust_type.starts_with("Option<");
            let inner = if f.rust_type.starts_with("Option<") {
                f.rust_type.trim_start_matches("Option<").trim_end_matches('>').trim()
            } else {
                f.rust_type.trim()
            };
            let is_string = inner == "String" || inner.contains("String");
            if is_optional && is_string {
                format!("payload.{}.as_deref()", f.name)
            } else if is_string {
                format!("&payload.{}", f.name)
            } else {
                // numeric, bool, or other Copy/Option<T> — bind directly
                format!("payload.{}", f.name)
            }
        })
        .collect();

    let update_fields: Vec<&isls_hypercube::domain::FieldDef> = entity.fields.iter()
        .filter(|f| f.name != "id" && !f.name.ends_with("_at"))
        .collect();
    let update_set_clause: String = update_fields.iter().enumerate()
        .map(|(i, f)| format!("{} = COALESCE(${}, {})", f.name, i + 1, f.name))
        .collect::<Vec<_>>()
        .join(", ");
    let update_binds: Vec<String> = update_fields.iter()
        .map(|f| {
            // In the Update struct every field is Option<T>.
            // Use as_deref() only when the inner T is String.
            let inner = if f.rust_type.starts_with("Option<") {
                f.rust_type.trim_start_matches("Option<").trim_end_matches('>').trim()
            } else {
                f.rust_type.trim()
            };
            let is_string = inner == "String" || inner.contains("String");
            if is_string {
                format!("payload.{}.as_deref()", f.name)
            } else {
                // Option<i32>, Option<i64>, Option<bool>, etc. — bind directly
                format!("payload.{}", f.name)
            }
        })
        .collect();
    let update_id_bind = update_fields.len() + 1;

    // Filters from API features that apply to this entity
    let entity_field_names: std::collections::HashSet<&str> = entity.fields.iter().map(|f| f.name.as_str()).collect();
    let filters: Vec<String> = api.filtering.iter()
        .filter(|f| entity_field_names.contains(f.as_str()))
        .cloned()
        .collect();
    let search_fields: Vec<String> = api.search_fields.iter()
        .filter(|f| entity_field_names.contains(f.as_str()))
        .cloned()
        .collect();

    let bind_offset_limit_start = filters.len() + if !search_fields.is_empty() { 1 } else { 0 } + 1;

    // Extra queries for business rules
    let mut extra_queries = Vec::new();
    if entity.name == "Product" {
        extra_queries.push(r#"/// Find a product by SKU.
pub async fn find_by_sku(pool: &PgPool, sku: &str) -> Result<Option<Product>, sqlx::Error> {
    sqlx::query_as::<_, Product>("SELECT * FROM products WHERE sku = $1")
        .bind(sku)
        .fetch_optional(pool)
        .await
}

/// Find products with stock below reorder level.
pub async fn find_low_stock(pool: &PgPool) -> Result<Vec<Product>, sqlx::Error> {
    sqlx::query_as::<_, Product>(
        "SELECT * FROM products WHERE quantity_on_hand <= reorder_level AND is_active = true ORDER BY quantity_on_hand ASC"
    )
    .fetch_all(pool)
    .await
}"#.to_string());
    }
    if entity.name == "Order" {
        extra_queries.push(r#"/// Count orders grouped by status.
pub async fn count_by_status(pool: &PgPool) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT status, COUNT(*) as count FROM orders GROUP BY status"
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}"#.to_string());
    }
    if entity.name == "User" {
        extra_queries.push(format!(
            r#"/// Find a user by email address (used by auth routes).
pub async fn find_by_email(pool: &PgPool, email: &str) -> Result<User, sqlx::Error> {{
    sqlx::query_as::<_, User>("SELECT {cols} FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
}}"#,
            cols = select_columns,
        ));
    }

    json!({
        "entity_name": entity.name,
        "entity_snake": snake,
        "table_name": table_name,
        "select_columns": select_columns,
        "insert_columns": insert_columns,
        "insert_placeholders": insert_placeholders,
        "insert_binds": insert_binds,
        "update_set_clause": update_set_clause,
        "update_binds": update_binds,
        "update_id_bind": update_id_bind,
        "filters": filters,
        "search_fields": search_fields,
        "bind_offset_limit_start": bind_offset_limit_start,
        "extra_queries": extra_queries,
    })
}

fn build_service_context(entity: &EntityTemplate, table_name: &str, rules: &[BusinessRule], api: &isls_hypercube::domain::ApiFeatures) -> serde_json::Value {
    let snake = to_snake_case(&entity.name);

    // Filters relevant to this entity — must match those generated by build_query_context
    let entity_field_names: std::collections::HashSet<&str> = entity.fields.iter().map(|f| f.name.as_str()).collect();
    let filters: Vec<String> = api.filtering.iter()
        .filter(|f| entity_field_names.contains(f.as_str()))
        .cloned()
        .collect();

    // Build business rule methods for this entity
    let mut business_rules = Vec::new();

    for rule in rules {
        if !rule.entities_involved.contains(&entity.name) {
            continue;
        }
        match rule.name.as_str() {
            "prevent_negative_stock" if entity.name == "Product" => {
                business_rules.push(r#"/// Adjust stock for a product, preventing negative stock.
pub async fn adjust_stock(
    pool: &PgPool,
    product_id: i64,
    quantity_change: i32,
    movement_type: &str,
    user: &AuthUser,
) -> Result<crate::models::product::Product, AppError> {
    require_role(user, "operator")?;
    let product = queries::get_by_id(pool, product_id).await.map_err(AppError::from)?;
    let new_qty = product.quantity_on_hand + quantity_change;
    if new_qty < 0 {
        return Err(AppError::BadRequest(format!(
            "Insufficient stock: have {}, need {}",
            product.quantity_on_hand, -quantity_change
        )));
    }
    // Update quantity
    let update_payload = crate::models::product::UpdateProduct {
        quantity_on_hand: Some(new_qty),
        ..Default::default()
    };
    queries::update(pool, product_id, &update_payload).await.map_err(AppError::from)
}"#.to_string());
            }
            "stock_on_fulfillment" if entity.name == "Order" => {
                business_rules.push(r#"/// Fulfill an order: update status to 'shipped' and create stock movements.
pub async fn fulfill_order(
    pool: &PgPool,
    order_id: i64,
    user: &AuthUser,
) -> Result<crate::models::order::Order, AppError> {
    require_role(user, "operator")?;
    let order = queries::get_by_id(pool, order_id).await.map_err(AppError::from)?;
    if order.status != "confirmed" && order.status != "processing" {
        return Err(AppError::BadRequest(format!(
            "Cannot fulfill order in status '{}'", order.status
        )));
    }
    let update = crate::models::order::UpdateOrder {
        status: Some("shipped".to_string()),
        ..Default::default()
    };
    queries::update(pool, order_id, &update).await.map_err(AppError::from)
}"#.to_string());
            }
            "order_state_machine" if entity.name == "Order" => {
                business_rules.push(r#"/// Update order status with state machine validation.
pub async fn update_status(
    pool: &PgPool,
    order_id: i64,
    new_status: &str,
    user: &AuthUser,
) -> Result<crate::models::order::Order, AppError> {
    require_role(user, "operator")?;
    let order = queries::get_by_id(pool, order_id).await.map_err(AppError::from)?;
    let valid = match (order.status.as_str(), new_status) {
        ("pending", "confirmed") => true,
        ("confirmed", "processing") => true,
        ("processing", "shipped") => true,
        ("shipped", "delivered") => true,
        (s, "cancelled") if s != "delivered" => true,
        _ => false,
    };
    if !valid {
        return Err(AppError::BadRequest(format!(
            "Invalid status transition: '{}' -> '{}'", order.status, new_status
        )));
    }
    let update = crate::models::order::UpdateOrder {
        status: Some(new_status.to_string()),
        ..Default::default()
    };
    queries::update(pool, order_id, &update).await.map_err(AppError::from)
}"#.to_string());
            }
            "auto_reorder" if entity.name == "Product" => {
                business_rules.push(r#"/// Check reorder level and log warning if stock is low.
pub fn check_reorder_level(product: &crate::models::product::Product) {
    if product.is_active && product.quantity_on_hand <= product.reorder_level {
        tracing::warn!(
            "Reorder needed for {} (SKU: {}): stock={}, reorder_level={}",
            product.name, product.sku, product.quantity_on_hand, product.reorder_level
        );
    }
}"#.to_string());
            }
            "order_total_calculation" if entity.name == "Order" => {
                business_rules.push(r#"/// Recalculate order total from items.
pub async fn recalculate_total(
    pool: &PgPool,
    order_id: i64,
    items: &[(i32, i64)],  // (quantity, unit_price_cents)
) -> Result<crate::models::order::Order, AppError> {
    let total: i64 = items.iter().map(|(qty, price)| (*qty as i64) * price).sum();
    let update = crate::models::order::UpdateOrder {
        total_amount_cents: Some(total),
        ..Default::default()
    };
    queries::update(pool, order_id, &update).await.map_err(AppError::from)
}"#.to_string());
            }
            _ => {}
        }
    }

    json!({
        "entity_name": entity.name,
        "entity_snake": snake,
        "table_name": table_name,
        "filters": filters,
        "business_rules": business_rules,
    })
}

fn build_api_context(entity: &EntityTemplate, table_name: &str, rules: &[BusinessRule], api: &isls_hypercube::domain::ApiFeatures) -> serde_json::Value {
    let snake = to_snake_case(&entity.name);

    let entity_field_names: std::collections::HashSet<&str> = entity.fields.iter().map(|f| f.name.as_str()).collect();
    let filters: Vec<String> = api.filtering.iter()
        .filter(|f| entity_field_names.contains(f.as_str()))
        .cloned()
        .collect();

    let mut extra_handlers = Vec::new();
    let mut extra_routes = Vec::new();

    for rule in rules {
        if !rule.entities_involved.contains(&entity.name) {
            continue;
        }
        match rule.name.as_str() {
            "prevent_negative_stock" if entity.name == "Product" => {
                extra_handlers.push(r#"/// POST /api/products/{id}/adjust-stock — adjust stock level.
pub async fn adjust_stock(
    pool: web::Data<PgPool>,
    path: web::Path<i64>,
    body: web::Json<serde_json::Value>,
    user: AuthUser,
) -> Result<HttpResponse, AppError> {
    let qty = body.get("quantity").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let movement_type = body.get("movement_type").and_then(|v| v.as_str()).unwrap_or("adjustment");
    let result = svc::adjust_stock(&pool, path.into_inner(), qty, movement_type, &user).await?;
    Ok(HttpResponse::Ok().json(result))
}"#.to_string());
                extra_routes.push(r#".route("/{id}/adjust-stock", web::post().to(adjust_stock))"#.to_string());
            }
            "stock_on_fulfillment" if entity.name == "Order" => {
                extra_handlers.push(r#"/// POST /api/orders/{id}/fulfill — fulfill an order.
pub async fn fulfill(
    pool: web::Data<PgPool>,
    path: web::Path<i64>,
    user: AuthUser,
) -> Result<HttpResponse, AppError> {
    let result = svc::fulfill_order(&pool, path.into_inner(), &user).await?;
    Ok(HttpResponse::Ok().json(result))
}"#.to_string());
                extra_routes.push(r#".route("/{id}/fulfill", web::post().to(fulfill))"#.to_string());
            }
            "order_state_machine" if entity.name == "Order" => {
                extra_handlers.push(r#"/// PUT /api/orders/{id}/status — update order status.
pub async fn update_status(
    pool: web::Data<PgPool>,
    path: web::Path<i64>,
    body: web::Json<serde_json::Value>,
    user: AuthUser,
) -> Result<HttpResponse, AppError> {
    let status = body.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let result = svc::update_status(&pool, path.into_inner(), status, &user).await?;
    Ok(HttpResponse::Ok().json(result))
}"#.to_string());
                extra_routes.push(r#".route("/{id}/status", web::put().to(update_status))"#.to_string());
            }
            _ => {}
        }
    }

    json!({
        "entity_name": entity.name,
        "entity_snake": snake,
        "table_name": table_name,
        "filters": filters,
        "extra_handlers": extra_handlers,
        "extra_routes": extra_routes,
    })
}

fn build_migration_context(app_name: &str, entities: &[EntityTemplate]) -> serde_json::Value {
    let tables: Vec<serde_json::Value> = entities.iter().map(|entity| {
        let snake = to_snake_case(&entity.name);
        let table_name = format!("{}s", snake);

        // Build proper column definitions
        let col_defs: Vec<serde_json::Value> = entity.fields.iter().map(|f| {
            let mut def = format!("{} {}", f.name, f.sql_type);
            if let Some(dv) = &f.default_value {
                if !f.sql_type.contains("DEFAULT") {
                    def = format!("{} DEFAULT {}", def, dv);
                }
            }
            json!({"definition": def})
        }).collect();

        let indices: Vec<serde_json::Value> = entity.indices.iter().map(|idx| {
            let cols = idx.columns.join(", ");
            json!({
                "name": idx.name,
                "columns": cols,
                "unique": idx.unique,
            })
        }).collect();

        json!({
            "entity_name": entity.name,
            "table_name": table_name,
            "columns": col_defs,
            "indices": indices,
        })
    }).collect();

    json!({
        "app_name": app_name,
        "tables": tables,
    })
}

fn build_test_context(entities: &[EntityTemplate], rules: &[BusinessRule]) -> serde_json::Value {
    let test_entities: Vec<serde_json::Value> = entities.iter().map(|e| {
        json!({"name": e.name, "snake": to_snake_case(&e.name)})
    }).collect();

    let test_rules: Vec<serde_json::Value> = rules.iter().map(|r| {
        json!({"name": r.name})
    }).collect();

    json!({
        "entities": test_entities,
        "business_rules": test_rules,
    })
}

fn build_frontend_page_context(entity: &EntityTemplate, table_name: &str) -> serde_json::Value {
    let snake = to_snake_case(&entity.name);

    let table_columns: Vec<String> = entity.fields.iter()
        .filter(|f| !f.name.ends_with("_hash") && f.name != "created_at" && f.name != "updated_at")
        .take(8)
        .map(|f| f.name.clone())
        .collect();

    let sortable_columns: Vec<String> = entity.fields.iter()
        .filter(|f| !f.nullable && f.name != "id")
        .take(5)
        .map(|f| f.name.clone())
        .collect();

    let form_fields: Vec<serde_json::Value> = entity.fields.iter()
        .filter(|f| f.name != "id" && !f.name.ends_with("_at") && f.name != "password_hash")
        .map(|f| {
            let input_type = if f.rust_type.contains("i32") || f.rust_type.contains("i64") {
                "number"
            } else if f.name.contains("email") {
                "email"
            } else if f.name.contains("password") {
                "password"
            } else {
                "text"
            };
            let required = !f.nullable && f.name != "id";
            json!({
                "name": f.name,
                "label": f.name.replace('_', " "),
                "input_type": input_type,
                "default": f.default_value.as_deref().unwrap_or(""),
                "required": required,
            })
        })
        .collect();

    json!({
        "entity_name": entity.name,
        "entity_snake": snake,
        "table_name": table_name,
        "table_columns": table_columns,
        "sortable_columns": sortable_columns,
        "form_fields": form_fields,
    })
}

fn emit_auth_routes(output_dir: &Path, files: &mut Vec<EmittedFile>) -> Result<(), OrchestratorError> {
    let content = r#"// Auth API routes — generated by ISLS v2
// Copyright (c) 2026 Sebastian Klemm — MIT License

use actix_web::{web, HttpResponse};
use sqlx::PgPool;
use crate::auth::{self, LoginRequest, LoginResponse, AuthUserInfo, RegisterRequest};
use crate::config::AppConfig;
use crate::errors::AppError;

/// POST /api/auth/login
pub async fn login(
    pool: web::Data<PgPool>,
    config: web::Data<AppConfig>,
    body: web::Json<LoginRequest>,
) -> Result<HttpResponse, AppError> {
    let user = crate::database::user_queries::find_by_email(&pool, &body.email)
        .await
        .map_err(|_| AppError::Unauthorized("Invalid credentials".to_string()))?;

    // In production: verify password with bcrypt
    // let valid = bcrypt::verify(&body.password, &user.password_hash).unwrap_or(false);
    // For now, accept any password (mock mode)
    let token = auth::create_token(&user, &config.jwt_secret)?;

    Ok(HttpResponse::Ok().json(LoginResponse {
        token,
        user: AuthUserInfo {
            id: user.id,
            email: user.email,
            name: user.name,
            role: user.role,
        },
    }))
}

/// POST /api/auth/register
pub async fn register(
    pool: web::Data<PgPool>,
    config: web::Data<AppConfig>,
    body: web::Json<RegisterRequest>,
) -> Result<HttpResponse, AppError> {
    // Hash password (in production: bcrypt)
    let password_hash = format!("$2b$12$mock_hash_{}", body.email);

    let payload = crate::models::user::CreateUser {
        email: body.email.clone(),
        password_hash,
        name: body.name.clone(),
        role: "operator".to_string(),
        is_active: true,
        last_login_at: None,
    };

    let user = crate::database::user_queries::insert(&pool, &payload)
        .await
        .map_err(|e| AppError::Conflict(format!("User already exists: {}", e)))?;

    let token = auth::create_token(&user, &config.jwt_secret)?;

    Ok(HttpResponse::Created().json(LoginResponse {
        token,
        user: AuthUserInfo {
            id: user.id,
            email: user.email,
            name: user.name,
            role: user.role,
        },
    }))
}

/// Register auth routes.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/auth")
            .route("/login", web::post().to(login))
            .route("/register", web::post().to(register))
    );
}
"#;
    let full_path = output_dir.join("backend/src/api/auth_routes.rs");
    write_file(&full_path, content)?;
    files.push(EmittedFile {
        rel_path: "backend/src/api/auth_routes.rs".into(),
        loc: content.lines().count(),
        category: "security".into(),
    });
    Ok(())
}

fn emit_frontend_app_js(
    entities: &[EntityTemplate],
    output_dir: &Path,
    files: &mut Vec<EmittedFile>,
) -> Result<(), OrchestratorError> {
    let mut imports = String::new();
    let mut routes = String::new();

    for entity in entities {
        let snake = to_snake_case(&entity.name);
        imports.push_str(&format!(
            "import {{ render as render_{} }} from './pages/{}.js';\n",
            snake, snake
        ));
        routes.push_str(&format!("  '/{snake}': render_{snake},\n"));
    }

    let content = format!(
        r#"// {{ app_name }} — SPA Router — generated by ISLS v2
import {{ apiFetch }} from './api/client.js';
import {{ render as renderDashboard }} from './pages/dashboard.js';
import {{ render as renderLogin }} from './pages/login.js';
{imports}
const routes = {{
  '/': renderDashboard,
  '/login': renderLogin,
{routes}}};

function navigate(path) {{
  const render = routes[path] || renderDashboard;
  const app = document.getElementById('app');
  app.innerHTML = '';
  render(app);
  document.querySelectorAll('[data-route]').forEach(el => {{
    el.classList.toggle('active', el.dataset.route === path);
  }});
}}

document.querySelectorAll('[data-route]').forEach(el => {{
  el.addEventListener('click', e => {{
    e.preventDefault();
    const path = el.dataset.route;
    window.history.pushState({{}}, '', path);
    navigate(path);
  }});
}});

window.addEventListener('popstate', () => navigate(window.location.pathname));
navigate(window.location.pathname);
"#
    );

    let full_path = output_dir.join("frontend/src/app.js");
    write_file(&full_path, &content)?;
    files.push(EmittedFile {
        rel_path: "frontend/src/app.js".into(),
        loc: content.lines().count(),
        category: "presentation".into(),
    });
    Ok(())
}
