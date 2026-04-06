//! S1/auth: Studio authentication — multi-user login with roles.
//!
//! Provides bcrypt password hashing, session tokens (in-memory), and
//! role-based access control (Admin / Editor / Viewer).
//! User database is persisted to `~/.isls/studio_users.json`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

// ─── Data Model ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StudioRole {
    Admin,
    Editor,
    Viewer,
}

impl std::fmt::Display for StudioRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StudioRole::Admin => write!(f, "admin"),
            StudioRole::Editor => write!(f, "editor"),
            StudioRole::Viewer => write!(f, "viewer"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudioUser {
    pub username: String,
    pub password_hash: String,
    pub role: StudioRole,
    pub created_at: String,
}

/// In-memory session entry: maps token → (username, role, created_at).
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub username: String,
    pub role: StudioRole,
    pub created_at: Instant,
}

/// Shared session store (token → SessionEntry).
pub type SessionStore = Arc<Mutex<HashMap<String, SessionEntry>>>;

pub fn new_session_store() -> SessionStore {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Shared user store.
pub type UserStore = Arc<Mutex<Vec<StudioUser>>>;

const MAX_USERS: usize = 20;
const SESSION_DURATION: Duration = Duration::from_secs(24 * 60 * 60); // 24h

// ─── Persistence ───────────────────────────────────────────────────────────

fn users_path() -> PathBuf {
    let base = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(base).join(".isls").join("studio_users.json")
}

pub fn load_users() -> Vec<StudioUser> {
    let path = users_path();
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_users(users: &[StudioUser]) {
    let path = users_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(users) {
        let _ = std::fs::write(&path, json);
    }
}

/// Ensure at least one admin user exists. Called at startup.
pub fn ensure_initial_admin(users: &mut Vec<StudioUser>, studio_password: Option<&str>) {
    if !users.is_empty() {
        return;
    }
    let username = std::env::var("STUDIO_USER").unwrap_or_else(|_| "admin".to_string());
    let password = studio_password
        .map(|s| s.to_string())
        .or_else(|| std::env::var("STUDIO_PASSWORD").ok())
        .unwrap_or_else(|| "admin123".to_string());

    let is_default = password == "admin123";
    let hash = bcrypt::hash(&password, 10).expect("bcrypt hash failed");

    users.push(StudioUser {
        username: username.clone(),
        password_hash: hash,
        role: StudioRole::Admin,
        created_at: chrono::Utc::now().to_rfc3339(),
    });
    save_users(users);

    tracing::info!("Created initial admin user: {}", username);
    if is_default {
        tracing::warn!("WARNING: Studio uses default password. Change it!");
    }
}

// ─── Token Generation ──────────────────────────────────────────────────────

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex::encode(bytes)
}

// ─── Cookie Helpers ────────────────────────────────────────────────────────

fn session_cookie(token: &str) -> String {
    format!(
        "studio_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400",
        token
    )
}

fn clear_cookie() -> String {
    "studio_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0".to_string()
}

/// Extract session token from Cookie header.
pub fn extract_token(cookie_header: Option<&str>) -> Option<String> {
    let header = cookie_header?;
    for part in header.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("studio_session=") {
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

// ─── Session Validation ────────────────────────────────────────────────────

/// Validate a session token and return (username, role) if valid.
pub async fn validate_session(
    sessions: &SessionStore,
    token: &str,
) -> Option<(String, StudioRole)> {
    let store = sessions.lock().await;
    if let Some(entry) = store.get(token) {
        if entry.created_at.elapsed() < SESSION_DURATION {
            return Some((entry.username.clone(), entry.role.clone()));
        }
    }
    None
}

/// Check if a role can perform write operations (forge, scrape, inject, etc.).
pub fn can_write(role: &StudioRole) -> bool {
    matches!(role, StudioRole::Admin | StudioRole::Editor)
}

/// Check if a role can manage users.
pub fn is_admin(role: &StudioRole) -> bool {
    matches!(role, StudioRole::Admin)
}

// ─── Request / Response Types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: StudioRole,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub password: Option<String>,
    pub role: Option<StudioRole>,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub username: String,
    pub role: String,
    pub created_at: String,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

/// POST /api/studio/login
pub async fn api_studio_login(
    State(state): State<super::AppState>,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    let users = state.studio_users.lock().await;
    let user = match users.iter().find(|u| u.username == body.username) {
        Some(u) => u,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid credentials"}))).into_response();
        }
    };

    match bcrypt::verify(&body.password, &user.password_hash) {
        Ok(true) => {}
        _ => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid credentials"}))).into_response();
        }
    }

    let token = generate_token();
    let role = user.role.clone();
    let username = user.username.clone();
    drop(users);

    {
        let mut sessions = state.studio_sessions.lock().await;
        // Cleanup expired sessions while we're here
        sessions.retain(|_, entry| entry.created_at.elapsed() < SESSION_DURATION);
        sessions.insert(
            token.clone(),
            SessionEntry {
                username: username.clone(),
                role: role.clone(),
                created_at: Instant::now(),
            },
        );
    }

    let cookie = session_cookie(&token);
    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({
            "user": username,
            "role": role.to_string()
        })),
    )
        .into_response()
}

/// POST /api/studio/logout
pub async fn api_studio_logout(
    State(state): State<super::AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok());
    if let Some(token) = extract_token(cookie_header) {
        let mut sessions = state.studio_sessions.lock().await;
        sessions.remove(&token);
    }
    (
        StatusCode::OK,
        [(header::SET_COOKIE, clear_cookie())],
        Json(serde_json::json!({"ok": true})),
    )
}

/// GET /api/studio/session
pub async fn api_studio_session(
    State(state): State<super::AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok());
    let token = match extract_token(cookie_header) {
        Some(t) => t,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "No session"}))).into_response();
        }
    };

    match validate_session(&state.studio_sessions, &token).await {
        Some((user, role)) => {
            Json(serde_json::json!({
                "user": user,
                "role": role.to_string()
            }))
            .into_response()
        }
        None => {
            (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Session expired"}))).into_response()
        }
    }
}

/// GET /api/studio/users — Admin only
pub async fn api_list_users(
    State(state): State<super::AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let (_, role) = match require_session(&state, &headers).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    if !is_admin(&role) {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "Admin required"}))).into_response();
    }

    let users = state.studio_users.lock().await;
    let list: Vec<UserInfo> = users
        .iter()
        .map(|u| UserInfo {
            username: u.username.clone(),
            role: u.role.to_string(),
            created_at: u.created_at.clone(),
        })
        .collect();
    Json(list).into_response()
}

/// POST /api/studio/users — Admin only
pub async fn api_create_user(
    State(state): State<super::AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<CreateUserRequest>,
) -> impl IntoResponse {
    let (_, role) = match require_session(&state, &headers).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    if !is_admin(&role) {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "Admin required"}))).into_response();
    }

    let mut users = state.studio_users.lock().await;
    if users.len() >= MAX_USERS {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Max 20 users reached"}))).into_response();
    }
    if users.iter().any(|u| u.username == body.username) {
        return (StatusCode::CONFLICT, Json(serde_json::json!({"error": "Username already exists"}))).into_response();
    }
    if body.username.is_empty() || body.password.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Username and password required"}))).into_response();
    }

    let hash = match bcrypt::hash(&body.password, 10) {
        Ok(h) => h,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Hash failed"}))).into_response(),
    };

    users.push(StudioUser {
        username: body.username.clone(),
        password_hash: hash,
        role: body.role.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    });
    save_users(&users);
    (StatusCode::CREATED, Json(serde_json::json!({"ok": true, "username": body.username}))).into_response()
}

/// PUT /api/studio/users/{username} — Admin only
pub async fn api_update_user(
    State(state): State<super::AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(target_username): axum::extract::Path<String>,
    Json(body): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    let (_, role) = match require_session(&state, &headers).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    if !is_admin(&role) {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "Admin required"}))).into_response();
    }

    let mut users = state.studio_users.lock().await;
    let user = match users.iter_mut().find(|u| u.username == target_username) {
        Some(u) => u,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "User not found"}))).into_response(),
    };

    if let Some(ref new_password) = body.password {
        if !new_password.is_empty() {
            match bcrypt::hash(new_password, 10) {
                Ok(h) => user.password_hash = h,
                Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Hash failed"}))).into_response(),
            }
        }
    }
    if let Some(ref new_role) = body.role {
        user.role = new_role.clone();
    }
    save_users(&users);
    Json(serde_json::json!({"ok": true})).into_response()
}

/// DELETE /api/studio/users/{username} — Admin only
pub async fn api_delete_user(
    State(state): State<super::AppState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(target_username): axum::extract::Path<String>,
) -> impl IntoResponse {
    let (current_user, role) = match require_session(&state, &headers).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    if !is_admin(&role) {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "Admin required"}))).into_response();
    }
    if current_user == target_username {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "Cannot delete yourself"}))).into_response();
    }

    let mut users = state.studio_users.lock().await;
    let len_before = users.len();
    users.retain(|u| u.username != target_username);
    if users.len() == len_before {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "User not found"}))).into_response();
    }
    save_users(&users);

    // Also remove any active sessions for this user
    let mut sessions = state.studio_sessions.lock().await;
    sessions.retain(|_, entry| entry.username != target_username);

    Json(serde_json::json!({"ok": true})).into_response()
}

// ─── Middleware Helper ─────────────────────────────────────────────────────

/// Extract and validate session from request headers. Returns (username, role)
/// or an error response.
pub async fn require_session(
    state: &super::AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(String, StudioRole), axum::response::Response> {
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok());
    let token = extract_token(cookie_header).ok_or_else(|| {
        (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Authentication required"}))).into_response()
    })?;
    validate_session(&state.studio_sessions, &token).await.ok_or_else(|| {
        (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Session expired"}))).into_response()
    })
}

/// Require at least Editor role (Admin or Editor).
pub async fn require_writer(
    state: &super::AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(String, StudioRole), axum::response::Response> {
    let (user, role) = require_session(state, headers).await?;
    if !can_write(&role) {
        return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "Write access required"}))).into_response());
    }
    Ok((user, role))
}

/// Require Admin role.
pub async fn require_admin(
    state: &super::AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(String, StudioRole), axum::response::Response> {
    let (user, role) = require_session(state, headers).await?;
    if !is_admin(&role) {
        return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "Admin required"}))).into_response());
    }
    Ok((user, role))
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_token_from_cookie() {
        let cookie = "studio_session=abc123def456; other=value";
        assert_eq!(extract_token(Some(cookie)), Some("abc123def456".to_string()));
    }

    #[test]
    fn test_extract_token_none() {
        assert_eq!(extract_token(None), None);
        assert_eq!(extract_token(Some("other=value")), None);
    }

    #[test]
    fn test_extract_token_empty_value() {
        assert_eq!(extract_token(Some("studio_session=; other=x")), None);
    }

    #[test]
    fn test_role_display() {
        assert_eq!(StudioRole::Admin.to_string(), "admin");
        assert_eq!(StudioRole::Editor.to_string(), "editor");
        assert_eq!(StudioRole::Viewer.to_string(), "viewer");
    }

    #[test]
    fn test_can_write_roles() {
        assert!(can_write(&StudioRole::Admin));
        assert!(can_write(&StudioRole::Editor));
        assert!(!can_write(&StudioRole::Viewer));
    }

    #[test]
    fn test_is_admin() {
        assert!(is_admin(&StudioRole::Admin));
        assert!(!is_admin(&StudioRole::Editor));
        assert!(!is_admin(&StudioRole::Viewer));
    }

    #[test]
    fn test_generate_token_length() {
        let token = generate_token();
        assert_eq!(token.len(), 64); // 32 bytes = 64 hex chars
    }

    #[test]
    fn test_ensure_initial_admin_creates_user() {
        let mut users = Vec::new();
        // Use a custom password to avoid relying on env vars
        ensure_initial_admin(&mut users, Some("test-pass-123"));
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].role, StudioRole::Admin);
        assert!(bcrypt::verify("test-pass-123", &users[0].password_hash).unwrap());
    }

    #[test]
    fn test_ensure_initial_admin_skips_if_users_exist() {
        let mut users = vec![StudioUser {
            username: "existing".into(),
            password_hash: "hash".into(),
            role: StudioRole::Viewer,
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        ensure_initial_admin(&mut users, Some("password"));
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].username, "existing");
    }

    #[test]
    fn test_role_serde_roundtrip() {
        let role = StudioRole::Editor;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"editor\"");
        let parsed: StudioRole = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, StudioRole::Editor);
    }
}
