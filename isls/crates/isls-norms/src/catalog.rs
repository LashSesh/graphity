// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Built-in norm catalog for ISLS v3.0.
//!
//! Contains 21 molecule norms (0042–0240) and 3 organism norms (0500–0502).

use crate::types::{
    ApiArtifact, ConfigArtifact, DatabaseArtifact, FrontendArtifact, FrontendComponent,
    ModelArtifact, Norm, NormEvidence, NormLevel, NormLayers, NormParameter, NormVariant,
    QueryArtifact, ServiceArtifact, TestArtifact, TriggerPattern, ValidationSpec, FieldSpec,
    FieldSource,
};

/// Return all built-in norms.
pub fn builtin_norms() -> Vec<Norm> {
    vec![
        // ── Molecules ──────────────────────────────────────────────────────────
        norm_crud_entity(),
        norm_crud_soft_delete(),
        norm_crud_audit_trail(),
        norm_jwt_auth(),
        norm_pagination(),
        norm_error_system(),
        norm_inventory(),
        norm_state_machine(),
        norm_search(),
        norm_filter(),
        norm_sort(),
        norm_barcode(),
        norm_file_upload(),
        norm_export(),
        norm_dashboard_kpi(),
        norm_chart(),
        norm_notification(),
        norm_config(),
        norm_health_check(),
        norm_docker_deploy(),
        norm_readme(),
        // ── Organisms ──────────────────────────────────────────────────────────
        norm_warehouse(),
        norm_ecommerce(),
        norm_project_tracker(),
    ]
}

// ─── Helper constructors ─────────────────────────────────────────────────────

fn mk_norm(
    id: &str,
    name: &str,
    level: NormLevel,
    keywords: Vec<&str>,
    concepts: Vec<&str>,
    requires: Vec<&str>,
    layers: NormLayers,
) -> Norm {
    Norm {
        id: id.to_string(),
        name: name.to_string(),
        level,
        triggers: vec![TriggerPattern {
            keywords: keywords.into_iter().map(|s| s.to_string()).collect(),
            concepts: concepts.into_iter().map(|s| s.to_string()).collect(),
            min_confidence: 0.3,
            excludes: vec![],
        }],
        layers,
        parameters: vec![],
        requires: requires.into_iter().map(|s| s.to_string()).collect(),
        variants: vec![],
        version: "1.0.0".to_string(),
        evidence: NormEvidence { builtin: true, ..Default::default() },
    }
}

fn simple_model(name: &str, fields: Vec<FieldSpec>) -> ModelArtifact {
    ModelArtifact {
        struct_name: name.to_string(),
        fields,
        derives: vec!["Debug".into(), "Clone".into(), "Serialize".into(), "Deserialize".into()],
        validations: vec![],
    }
}

fn simple_api(method: &str, path: &str, role: &str, resp: &str) -> ApiArtifact {
    ApiArtifact {
        method: method.to_string(),
        path: path.to_string(),
        auth_required: true,
        min_role: role.to_string(),
        request_body: None,
        response_type: resp.to_string(),
        description: format!("{} {}", method, path),
    }
}

fn id_field() -> FieldSpec {
    FieldSpec {
        name: "id".into(), rust_type: "i64".into(), sql_type: "BIGSERIAL PRIMARY KEY".into(),
        nullable: false, default_value: None, indexed: false, unique: true,
        source: FieldSource::SystemGenerated, description: "Primary key".into(),
    }
}

fn ts_fields() -> Vec<FieldSpec> {
    vec![
        FieldSpec { name: "created_at".into(), rust_type: "String".into(), sql_type: "TIMESTAMPTZ NOT NULL DEFAULT NOW()".into(), nullable: false, default_value: Some("NOW()".into()), indexed: false, unique: false, source: FieldSource::SystemGenerated, description: "Creation timestamp".into() },
        FieldSpec { name: "updated_at".into(), rust_type: "String".into(), sql_type: "TIMESTAMPTZ NOT NULL DEFAULT NOW()".into(), nullable: false, default_value: Some("NOW()".into()), indexed: false, unique: false, source: FieldSource::SystemGenerated, description: "Update timestamp".into() },
    ]
}

// ─── Molecule Norms ───────────────────────────────────────────────────────────

fn norm_crud_entity() -> Norm {
    mk_norm(
        "ISLS-NORM-0042", "CRUD-Entity", NormLevel::Molecule,
        vec!["entity", "crud", "resource", "create", "read", "update", "delete", "list"],
        vec!["data model", "rest api", "database"],
        vec!["ISLS-NORM-0100", "ISLS-NORM-0101"],
        NormLayers {
            database: vec![DatabaseArtifact { table: "{entity}s".into(), ddl: "CREATE TABLE {entity}s (id BIGSERIAL PRIMARY KEY, ...);".into() }],
            model: vec![simple_model("{Entity}", vec![id_field()])],
            query: vec![
                QueryArtifact { name: "get_{entity}".into(), description: "Fetch by id".into(), sql_template: "SELECT * FROM {entity}s WHERE id = $1".into(), parameters: vec!["id: i64".into()], return_type: "{Entity}".into() },
                QueryArtifact { name: "list_{entity}s".into(), description: "Paginated list".into(), sql_template: "SELECT * FROM {entity}s ORDER BY id LIMIT $1 OFFSET $2".into(), parameters: vec!["limit: i64".into(), "offset: i64".into()], return_type: "Vec<{Entity}>".into() },
                QueryArtifact { name: "insert_{entity}".into(), description: "Insert new record".into(), sql_template: "INSERT INTO {entity}s (...) VALUES (...) RETURNING *".into(), parameters: vec!["payload: Create{Entity}Payload".into()], return_type: "{Entity}".into() },
                QueryArtifact { name: "update_{entity}".into(), description: "Update record".into(), sql_template: "UPDATE {entity}s SET ... WHERE id = $1 RETURNING *".into(), parameters: vec!["id: i64".into(), "payload: Update{Entity}Payload".into()], return_type: "{Entity}".into() },
                QueryArtifact { name: "delete_{entity}".into(), description: "Delete record".into(), sql_template: "DELETE FROM {entity}s WHERE id = $1".into(), parameters: vec!["id: i64".into()], return_type: "()".into() },
            ],
            service: vec![ServiceArtifact { name: "{entity}_service".into(), description: "CRUD service layer".into(), method_signatures: vec!["pub async fn get(pool: &PgPool, id: i64, user: &AuthUser) -> Result<{Entity}, AppError>".into(), "pub async fn list(pool: &PgPool, params: &PaginationParams, user: &AuthUser) -> Result<PaginatedResponse<{Entity}>, AppError>".into(), "pub async fn create(pool: &PgPool, payload: Create{Entity}Payload, user: &AuthUser) -> Result<{Entity}, AppError>".into(), "pub async fn update(pool: &PgPool, id: i64, payload: Update{Entity}Payload, user: &AuthUser) -> Result<{Entity}, AppError>".into(), "pub async fn delete(pool: &PgPool, id: i64, user: &AuthUser) -> Result<(), AppError>".into()], business_rules: vec!["validate payload before insert/update".into()] }],
            api: vec![
                simple_api("GET", "/api/{entity}s", "viewer", "PaginatedResponse<{Entity}>"),
                simple_api("GET", "/api/{entity}s/:id", "viewer", "{Entity}"),
                simple_api("POST", "/api/{entity}s", "operator", "{Entity}"),
                simple_api("PUT", "/api/{entity}s/:id", "operator", "{Entity}"),
                simple_api("DELETE", "/api/{entity}s/:id", "manager", "()"),
            ],
            frontend: vec![
                FrontendArtifact { component_type: FrontendComponent::Table, name: "{Entity}Table".into(), api_calls: vec!["GET /api/{entity}s".into()], description: "Paginated list table".into() },
                FrontendArtifact { component_type: FrontendComponent::Form, name: "{Entity}Form".into(), api_calls: vec!["POST /api/{entity}s".into(), "PUT /api/{entity}s/:id".into()], description: "Create/edit form".into() },
            ],
            test: vec![
                TestArtifact { name: "test_{entity}_crud".into(), description: "Full CRUD integration test".into(), test_type: "integration".into(), scenario: "create, read, update, delete entity".into() },
                TestArtifact { name: "test_{entity}_validation".into(), description: "Payload validation test".into(), test_type: "unit".into(), scenario: "invalid payloads are rejected".into() },
                TestArtifact { name: "test_{entity}_auth".into(), description: "Auth enforcement test".into(), test_type: "integration".into(), scenario: "unauthenticated requests return 401".into() },
                TestArtifact { name: "test_{entity}_not_found".into(), description: "Not-found test".into(), test_type: "integration".into(), scenario: "unknown id returns 404".into() },
                TestArtifact { name: "test_{entity}_list_pagination".into(), description: "Pagination test".into(), test_type: "integration".into(), scenario: "list respects per_page and page params".into() },
            ],
            config: vec![],
        },
    )
}

fn norm_crud_soft_delete() -> Norm {
    let mut n = norm_crud_entity();
    n.id = "ISLS-NORM-0042-SD".into();
    n.name = "CRUD+SoftDelete".into();
    n.triggers[0].keywords.extend(["soft delete", "archive", "restore"].iter().map(|s| s.to_string()));
    n
}

fn norm_crud_audit_trail() -> Norm {
    let mut n = norm_crud_entity();
    n.id = "ISLS-NORM-0042-AT".into();
    n.name = "CRUD+AuditTrail".into();
    n.triggers[0].keywords.extend(["audit", "history", "changelog", "trail"].iter().map(|s| s.to_string()));
    n
}

fn norm_jwt_auth() -> Norm {
    mk_norm(
        "ISLS-NORM-0088", "JWT-Auth", NormLevel::Molecule,
        vec!["auth", "jwt", "login", "register", "token", "authentication", "authorization"],
        vec!["security", "user management", "access control"],
        vec!["ISLS-NORM-0101"],
        NormLayers {
            database: vec![DatabaseArtifact { table: "users".into(), ddl: "CREATE TABLE users (id BIGSERIAL PRIMARY KEY, email VARCHAR(255) UNIQUE NOT NULL, password_hash VARCHAR(255) NOT NULL, role VARCHAR(50) NOT NULL DEFAULT 'operator', is_active BOOLEAN NOT NULL DEFAULT true, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW());".into() }],
            model: vec![simple_model("User", vec![id_field(), FieldSpec { name: "email".into(), rust_type: "String".into(), sql_type: "VARCHAR(255) NOT NULL UNIQUE".into(), nullable: false, default_value: None, indexed: true, unique: true, source: FieldSource::UserInput, description: "User email".into() }, FieldSpec { name: "role".into(), rust_type: "String".into(), sql_type: "VARCHAR(50) NOT NULL".into(), nullable: false, default_value: Some("'operator'".into()), indexed: false, unique: false, source: FieldSource::SystemGenerated, description: "Role".into() }])],
            query: vec![QueryArtifact { name: "find_user_by_email".into(), description: "Find user by email".into(), sql_template: "SELECT * FROM users WHERE email = $1".into(), parameters: vec!["email: &str".into()], return_type: "Option<User>".into() }],
            service: vec![ServiceArtifact { name: "auth_service".into(), description: "JWT auth service".into(), method_signatures: vec!["pub fn create_token(user: &User, secret: &str) -> Result<String, AppError>".into(), "pub fn verify_token(token: &str, secret: &str) -> Result<Claims, AppError>".into(), "pub async fn login(pool: &PgPool, email: &str, password: &str, secret: &str) -> Result<String, AppError>".into(), "pub async fn register(pool: &PgPool, payload: RegisterPayload, secret: &str) -> Result<(User, String), AppError>".into()], business_rules: vec!["bcrypt password hashing".into(), "JWT with 24h expiry".into(), "role-based access control".into()] }],
            api: vec![simple_api("POST", "/api/auth/login", "", "LoginResponse"), simple_api("POST", "/api/auth/register", "", "LoginResponse"), simple_api("GET", "/api/auth/me", "viewer", "AuthUserInfo")],
            frontend: vec![FrontendArtifact { component_type: FrontendComponent::Form, name: "LoginForm".into(), api_calls: vec!["POST /api/auth/login".into()], description: "Login form with JWT storage".into() }],
            test: vec![TestArtifact { name: "test_auth_login".into(), description: "Login returns JWT".into(), test_type: "integration".into(), scenario: "valid credentials return 200 + token".into() }, TestArtifact { name: "test_auth_invalid".into(), description: "Invalid credentials return 401".into(), test_type: "integration".into(), scenario: "wrong password returns 401".into() }],
            config: vec![ConfigArtifact { name: "jwt_secret".into(), description: "JWT signing secret env var".into(), template: "JWT_SECRET=change_me_in_production".into() }],
        },
    )
}

fn norm_pagination() -> Norm {
    mk_norm(
        "ISLS-NORM-0100", "Pagination", NormLevel::Molecule,
        vec!["pagination", "page", "per_page", "limit", "offset", "list"],
        vec!["api", "data listing"],
        vec![],
        NormLayers {
            model: vec![
                simple_model("PaginationParams", vec![
                    FieldSpec { name: "page".into(), rust_type: "Option<i64>".into(), sql_type: "".into(), nullable: true, default_value: Some("1".into()), indexed: false, unique: false, source: FieldSource::UserInput, description: "Page number (1-based)".into() },
                    FieldSpec { name: "per_page".into(), rust_type: "Option<i64>".into(), sql_type: "".into(), nullable: true, default_value: Some("20".into()), indexed: false, unique: false, source: FieldSource::UserInput, description: "Items per page (max 100)".into() },
                    FieldSpec { name: "sort".into(), rust_type: "Option<String>".into(), sql_type: "".into(), nullable: true, default_value: None, indexed: false, unique: false, source: FieldSource::UserInput, description: "Sort column".into() },
                    FieldSpec { name: "sort_desc".into(), rust_type: "Option<bool>".into(), sql_type: "".into(), nullable: true, default_value: None, indexed: false, unique: false, source: FieldSource::UserInput, description: "Sort descending".into() },
                    FieldSpec { name: "search".into(), rust_type: "Option<String>".into(), sql_type: "".into(), nullable: true, default_value: None, indexed: false, unique: false, source: FieldSource::UserInput, description: "Full-text search query".into() },
                ]),
                simple_model("PaginatedResponse<T>", vec![
                    FieldSpec { name: "items".into(), rust_type: "Vec<T>".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "Page items".into() },
                    FieldSpec { name: "total".into(), rust_type: "i64".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "Total item count".into() },
                    FieldSpec { name: "page".into(), rust_type: "i64".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "Current page".into() },
                    FieldSpec { name: "per_page".into(), rust_type: "i64".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "Items per page".into() },
                    FieldSpec { name: "total_pages".into(), rust_type: "i64".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "Total page count".into() },
                ]),
            ],
            frontend: vec![FrontendArtifact { component_type: FrontendComponent::ActionButton, name: "Pagination".into(), api_calls: vec![], description: "Page navigation controls".into() }],
            ..Default::default()
        },
    )
}

fn norm_error_system() -> Norm {
    mk_norm(
        "ISLS-NORM-0101", "Error-System", NormLevel::Molecule,
        vec!["error", "errors", "error handling", "app error"],
        vec!["error handling", "http responses"],
        vec![],
        NormLayers {
            model: vec![simple_model("AppError", vec![
                FieldSpec { name: "NotFound".into(), rust_type: "String".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "404 not found".into() },
                FieldSpec { name: "ValidationError".into(), rust_type: "Vec<String>".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "400 validation errors".into() },
                FieldSpec { name: "Unauthorized".into(), rust_type: "String".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "401 unauthorized".into() },
                FieldSpec { name: "Forbidden".into(), rust_type: "String".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "403 forbidden".into() },
                FieldSpec { name: "Conflict".into(), rust_type: "String".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "409 conflict".into() },
                FieldSpec { name: "InternalError".into(), rust_type: "String".into(), sql_type: "".into(), nullable: false, default_value: None, indexed: false, unique: false, source: FieldSource::SystemComputed, description: "500 internal error".into() },
            ])],
            ..Default::default()
        },
    )
}

fn norm_inventory() -> Norm {
    mk_norm(
        "ISLS-NORM-0112", "Inventory", NormLevel::Molecule,
        vec!["inventory", "stock", "quantity", "reorder", "warehouse"],
        vec!["inventory management", "supply chain"],
        vec!["ISLS-NORM-0042"],
        NormLayers {
            service: vec![ServiceArtifact { name: "inventory_service".into(), description: "Stock management".into(), method_signatures: vec!["pub async fn adjust_stock(pool: &PgPool, product_id: i64, delta: i32, user: &AuthUser) -> Result<(), AppError>".into(), "pub async fn check_reorder_level(pool: &PgPool, product_id: i64) -> Result<bool, AppError>".into()], business_rules: vec!["prevent negative stock".into(), "trigger reorder alert when below threshold".into()] }],
            test: vec![TestArtifact { name: "test_inventory_prevent_negative".into(), description: "Stock cannot go negative".into(), test_type: "integration".into(), scenario: "adjust_stock with too-large delta returns error".into() }],
            ..Default::default()
        },
    )
}

fn norm_state_machine() -> Norm {
    mk_norm(
        "ISLS-NORM-0120", "State-Machine", NormLevel::Molecule,
        vec!["status", "state machine", "transitions", "workflow", "state"],
        vec!["business workflow", "status management"],
        vec!["ISLS-NORM-0042"],
        NormLayers {
            service: vec![ServiceArtifact { name: "state_machine".into(), description: "Status transition enforcement".into(), method_signatures: vec!["pub fn validate_transition(current: &str, next: &str) -> Result<(), AppError>".into(), "pub async fn update_status(pool: &PgPool, id: i64, new_status: &str, user: &AuthUser) -> Result<(), AppError>".into()], business_rules: vec!["only valid transitions allowed".into()] }],
            frontend: vec![FrontendArtifact { component_type: FrontendComponent::StatusBadge, name: "StatusBadge".into(), api_calls: vec![], description: "Coloured status badge".into() }],
            test: vec![TestArtifact { name: "test_invalid_transition".into(), description: "Invalid status transition rejected".into(), test_type: "unit".into(), scenario: "attempt invalid status change returns 400".into() }],
            ..Default::default()
        },
    )
}

fn norm_search() -> Norm {
    mk_norm("ISLS-NORM-0130", "Search", NormLevel::Molecule, vec!["search", "full text", "ilike", "query"], vec!["search", "filtering"], vec!["ISLS-NORM-0100"], NormLayers { frontend: vec![FrontendArtifact { component_type: FrontendComponent::SearchBar, name: "SearchBar".into(), api_calls: vec!["GET /api/{entity}s?search=...".into()], description: "Debounced search input".into() }], ..Default::default() })
}

fn norm_filter() -> Norm {
    mk_norm("ISLS-NORM-0140", "Filter", NormLevel::Molecule, vec!["filter", "where", "dropdown", "criteria"], vec!["data filtering"], vec!["ISLS-NORM-0100"], NormLayers { frontend: vec![FrontendArtifact { component_type: FrontendComponent::ActionButton, name: "FilterDropdown".into(), api_calls: vec![], description: "Filter dropdown".into() }], ..Default::default() })
}

fn norm_sort() -> Norm {
    mk_norm("ISLS-NORM-0150", "Sort", NormLevel::Molecule, vec!["sort", "order by", "ascending", "descending"], vec!["data sorting"], vec!["ISLS-NORM-0100"], NormLayers { frontend: vec![FrontendArtifact { component_type: FrontendComponent::Table, name: "SortableTable".into(), api_calls: vec![], description: "Column-click sorting".into() }], ..Default::default() })
}

fn norm_barcode() -> Norm {
    mk_norm("ISLS-NORM-0156", "Barcode", NormLevel::Molecule, vec!["barcode", "sku", "scan", "qr code", "scanner"], vec!["inventory tracking", "logistics"], vec!["ISLS-NORM-0042"], NormLayers { api: vec![simple_api("GET", "/api/{entity}s/by-barcode/:code", "operator", "{Entity}")], frontend: vec![FrontendArtifact { component_type: FrontendComponent::Scanner, name: "BarcodeScanner".into(), api_calls: vec!["GET /api/{entity}s/by-barcode/:code".into()], description: "Barcode/QR scanner input".into() }], ..Default::default() })
}

fn norm_file_upload() -> Norm {
    mk_norm("ISLS-NORM-0160", "File-Upload", NormLevel::Molecule, vec!["file", "upload", "image", "attachment", "document"], vec!["file management"], vec![], NormLayers { api: vec![simple_api("POST", "/api/{entity}s/:id/upload", "operator", "UploadResponse")], frontend: vec![FrontendArtifact { component_type: FrontendComponent::Form, name: "FileUpload".into(), api_calls: vec!["POST /api/{entity}s/:id/upload".into()], description: "File upload with preview".into() }], ..Default::default() })
}

fn norm_export() -> Norm {
    mk_norm("ISLS-NORM-0170", "Export", NormLevel::Molecule, vec!["export", "csv", "download", "report"], vec!["data export"], vec![], NormLayers { api: vec![simple_api("GET", "/api/{entity}s/export", "manager", "ByteStream")], frontend: vec![FrontendArtifact { component_type: FrontendComponent::ActionButton, name: "ExportButton".into(), api_calls: vec!["GET /api/{entity}s/export".into()], description: "CSV/JSON export button".into() }], ..Default::default() })
}

fn norm_dashboard_kpi() -> Norm {
    mk_norm("ISLS-NORM-0180", "Dashboard-KPI", NormLevel::Molecule, vec!["dashboard", "kpi", "metric", "summary", "analytics"], vec!["reporting", "metrics"], vec![], NormLayers { api: vec![simple_api("GET", "/api/dashboard/kpis", "viewer", "Vec<KpiCard>")], frontend: vec![FrontendArtifact { component_type: FrontendComponent::DashboardCard, name: "KpiCards".into(), api_calls: vec!["GET /api/dashboard/kpis".into()], description: "KPI summary cards".into() }], ..Default::default() })
}

fn norm_chart() -> Norm {
    mk_norm("ISLS-NORM-0190", "Chart", NormLevel::Molecule, vec!["chart", "graph", "bar chart", "line chart", "pie chart", "trend"], vec!["data visualization"], vec![], NormLayers { api: vec![simple_api("GET", "/api/reports/chart-data", "viewer", "ChartData")], frontend: vec![FrontendArtifact { component_type: FrontendComponent::Chart, name: "Chart".into(), api_calls: vec!["GET /api/reports/chart-data".into()], description: "Canvas-based chart (bar/line/pie)".into() }], ..Default::default() })
}

fn norm_notification() -> Norm {
    mk_norm("ISLS-NORM-0200", "Notification", NormLevel::Molecule, vec!["notification", "alert", "notify", "reminder", "badge"], vec!["alerts", "messaging"], vec!["ISLS-NORM-0042"], NormLayers { frontend: vec![FrontendArtifact { component_type: FrontendComponent::StatusBadge, name: "NotificationBadge".into(), api_calls: vec!["GET /api/notifications".into()], description: "Notification count badge".into() }], ..Default::default() })
}

fn norm_config() -> Norm {
    mk_norm("ISLS-NORM-0210", "Config", NormLevel::Molecule, vec!["config", "configuration", "env", "environment", "settings"], vec!["application config"], vec![], NormLayers { config: vec![ConfigArtifact { name: "AppConfig".into(), description: "Application configuration from environment variables".into(), template: "DATABASE_URL=postgres://...\nJWT_SECRET=change_me\nPORT=8080\nRUST_LOG=info".into() }], ..Default::default() })
}

fn norm_health_check() -> Norm {
    mk_norm("ISLS-NORM-0220", "Health-Check", NormLevel::Molecule, vec!["health", "healthcheck", "status", "ping", "liveness"], vec!["observability", "devops"], vec![], NormLayers { api: vec![simple_api("GET", "/health", "", "HealthStatus")], test: vec![TestArtifact { name: "test_health_endpoint".into(), description: "Health endpoint returns 200".into(), test_type: "integration".into(), scenario: "GET /health returns 200 with DB ping".into() }], ..Default::default() })
}

fn norm_docker_deploy() -> Norm {
    mk_norm("ISLS-NORM-0230", "Docker-Deploy", NormLevel::Molecule, vec!["docker", "container", "compose", "deployment", "deploy"], vec!["containerisation", "devops"], vec!["ISLS-NORM-0210"], NormLayers { config: vec![ConfigArtifact { name: "docker-compose.yml".into(), description: "Docker Compose for backend + database".into(), template: "version: '3.8'\nservices:\n  backend:\n    build: ./backend\n    ports:\n      - \"8080:8080\"\n  db:\n    image: postgres:16-alpine\n    environment:\n      POSTGRES_DB: app\n      POSTGRES_PASSWORD: secret".into() }], ..Default::default() })
}

fn norm_readme() -> Norm {
    mk_norm("ISLS-NORM-0240", "README", NormLevel::Molecule, vec!["readme", "documentation", "docs", "setup", "guide"], vec!["documentation"], vec![], NormLayers { config: vec![ConfigArtifact { name: "README.md".into(), description: "Project README".into(), template: "# {app_name}\n\nGenerated by ISLS v3.0.\n\n## Setup\n\n```bash\ndocker compose up -d\ncargo run\n```\n".into() }], ..Default::default() })
}

// ─── Organism Norms ───────────────────────────────────────────────────────────

fn norm_warehouse() -> Norm {
    mk_norm(
        "ISLS-NORM-0500", "Warehouse", NormLevel::Organism,
        vec!["warehouse", "inventory", "stock", "product", "sku", "reorder", "shipment", "fulfillment", "logistics"],
        vec!["warehouse management", "wms", "supply chain"],
        vec![
            "ISLS-NORM-0042", "ISLS-NORM-0088", "ISLS-NORM-0100", "ISLS-NORM-0101",
            "ISLS-NORM-0112", "ISLS-NORM-0120", "ISLS-NORM-0156",
            "ISLS-NORM-0180", "ISLS-NORM-0190", "ISLS-NORM-0170", "ISLS-NORM-0230",
        ],
        NormLayers {
            model: vec![
                simple_model("Product",       vec![id_field()]),
                simple_model("Warehouse",     vec![id_field()]),
                simple_model("Supplier",      vec![id_field()]),
                simple_model("Order",         vec![id_field()]),
                simple_model("OrderItem",     vec![id_field()]),
                simple_model("StockMovement", vec![id_field()]),
                simple_model("User",          vec![id_field()]),
            ],
            service: vec![
                ServiceArtifact { name: "inventory_service".into(), description: "Stock level management".into(), method_signatures: vec!["pub async fn adjust_stock(pool: &PgPool, product_id: i64, delta: i32, user: &AuthUser) -> Result<(), AppError>".into()], business_rules: vec!["prevent negative stock".into(), "auto-reorder alert".into()] },
                ServiceArtifact { name: "order_service".into(), description: "Order fulfillment".into(), method_signatures: vec!["pub async fn fulfill_order(pool: &PgPool, order_id: i64, user: &AuthUser) -> Result<(), AppError>".into()], business_rules: vec!["state machine: pending→confirmed→shipped→delivered".into()] },
            ],
            api: vec![
                simple_api("GET", "/api/products", "viewer", "PaginatedResponse<Product>"),
                simple_api("GET", "/api/orders", "viewer", "PaginatedResponse<Order>"),
                simple_api("GET", "/api/dashboard/kpis", "viewer", "Vec<KpiCard>"),
            ],
            ..Default::default()
        },
    )
}

fn norm_ecommerce() -> Norm {
    mk_norm(
        "ISLS-NORM-0501", "E-Commerce", NormLevel::Organism,
        vec!["ecommerce", "e-commerce", "shop", "store", "cart", "checkout", "payment", "catalog"],
        vec!["e-commerce", "online store", "shopping"],
        vec![
            "ISLS-NORM-0042", "ISLS-NORM-0088", "ISLS-NORM-0100", "ISLS-NORM-0101",
            "ISLS-NORM-0120", "ISLS-NORM-0130", "ISLS-NORM-0140",
            "ISLS-NORM-0180", "ISLS-NORM-0190", "ISLS-NORM-0230",
        ],
        NormLayers {
            model: vec![
                simple_model("Product",   vec![id_field()]),
                simple_model("Category",  vec![id_field()]),
                simple_model("Customer",  vec![id_field()]),
                simple_model("Cart",      vec![id_field()]),
                simple_model("CartItem",  vec![id_field()]),
                simple_model("Order",     vec![id_field()]),
                simple_model("OrderLine", vec![id_field()]),
                simple_model("Review",    vec![id_field()]),
                simple_model("Address",   vec![id_field()]),
            ],
            service: vec![
                ServiceArtifact { name: "cart_service".into(), description: "Cart management".into(), method_signatures: vec!["pub async fn add_item(pool: &PgPool, cart_id: i64, product_id: i64, qty: i32) -> Result<CartItem, AppError>".into(), "pub async fn checkout(pool: &PgPool, cart_id: i64, customer_id: i64) -> Result<Order, AppError>".into()], business_rules: vec!["inventory check before add".into(), "cart-to-order conversion".into()] },
            ],
            api: vec![
                simple_api("GET", "/api/products", "viewer", "PaginatedResponse<Product>"),
                simple_api("POST", "/api/cart/items", "viewer", "CartItem"),
                simple_api("POST", "/api/cart/checkout", "viewer", "Order"),
            ],
            ..Default::default()
        },
    )
}

fn norm_project_tracker() -> Norm {
    mk_norm(
        "ISLS-NORM-0502", "Project-Tracker", NormLevel::Organism,
        vec!["project", "task", "sprint", "kanban", "scrum", "agile", "tracker", "milestone"],
        vec!["project management", "task tracking", "agile"],
        vec![
            "ISLS-NORM-0042", "ISLS-NORM-0088", "ISLS-NORM-0100", "ISLS-NORM-0101",
            "ISLS-NORM-0120", "ISLS-NORM-0130",
            "ISLS-NORM-0180", "ISLS-NORM-0190", "ISLS-NORM-0230",
        ],
        NormLayers {
            model: vec![
                simple_model("Project",    vec![id_field()]),
                simple_model("Sprint",     vec![id_field()]),
                simple_model("Task",       vec![id_field()]),
                simple_model("Comment",    vec![id_field()]),
                simple_model("Label",      vec![id_field()]),
                simple_model("TaskLabel",  vec![id_field()]),
                simple_model("TeamMember", vec![id_field()]),
                simple_model("User",       vec![id_field()]),
            ],
            service: vec![
                ServiceArtifact { name: "task_service".into(), description: "Task management".into(), method_signatures: vec!["pub async fn update_status(pool: &PgPool, task_id: i64, new_status: &str, user: &AuthUser) -> Result<Task, AppError>".into(), "pub async fn assign(pool: &PgPool, task_id: i64, assignee_id: i64, user: &AuthUser) -> Result<Task, AppError>".into()], business_rules: vec!["task state machine".into(), "role-based assignment".into()] },
                ServiceArtifact { name: "sprint_service".into(), description: "Sprint velocity".into(), method_signatures: vec!["pub async fn calculate_velocity(pool: &PgPool, sprint_id: i64) -> Result<f64, AppError>".into()], business_rules: vec!["sum estimate_hours of completed tasks".into()] },
            ],
            api: vec![
                simple_api("GET", "/api/projects", "viewer", "PaginatedResponse<Project>"),
                simple_api("GET", "/api/tasks", "viewer", "PaginatedResponse<Task>"),
                simple_api("PUT", "/api/tasks/:id/status", "member", "Task"),
            ],
            ..Default::default()
        },
    )
}
