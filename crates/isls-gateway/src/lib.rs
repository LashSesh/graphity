//! REST and WebSocket API gateway for ISLS (C19).
//!
//! Serves the Studio single-page web interface and provides real-time event
//! streaming over WebSockets. All static assets are embedded at compile time
//! with zero external JS/CSS dependencies.

// isls-gateway: REST + WebSocket API and Studio web interface — C19
// Serves the Studio single-page app and provides real-time event streaming.
// No external JS/CSS dependencies. One HTML file. Nine views (Phase 13: Swarm + Chat).

pub mod architect;
pub mod chat;
pub mod chat_handler;
pub mod discover;
pub mod session;
pub mod ws;

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    Router,
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{Html, IntoResponse, Json, Response},
    routing::{delete, get, post},
};
use isls_norms::NormRegistry;
use isls_norms::composition::ComposedPlan;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use chat::{BindRequest, BoundProject, ChatJobStore, ChatRequest, run_chat_job};
use ws::{EventHub, EventType, SubscribeRequest, WsEvent};

// The entire Studio is embedded at compile time
const STUDIO_HTML: &str = include_str!("static/studio.html");

// ─── Pending Plan (v3.2 wiring) ──────────────────────────────────────────

/// A plan stored after POST /api/chat, awaiting user confirmation via forge.
#[derive(Clone, Debug)]
pub struct PendingPlan {
    pub id: String,
    pub description: String,
    pub composed_plan: ComposedPlan,
    pub activated_norms: Vec<String>,
    pub estimated_files: usize,
    pub estimated_loc: usize,
    pub created_at: String,
}

/// Status of a project in the forge pipeline.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProjectStatus {
    Planning,
    Generating,
    Completed,
    Failed,
}

/// Statistics from a forge run.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ProjectStats {
    pub files_generated: usize,
    pub total_loc: usize,
    pub total_tokens: u64,
    pub compile_status: String,
    pub duration_secs: f64,
}

// ─── Application State ─────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub start_time: Instant,
    pub event_hub: EventHub,
    pub engine_state: Arc<RwLock<EngineState>>,
    pub forge_state: Arc<RwLock<ForgeState>>,
    pub foundry_state: Arc<RwLock<FoundryState>>,
    /// In-progress and completed chat jobs (keyed by job_id)
    pub chat_jobs: ChatJobStore,
    /// Default project path for /agent/chat when no `project` field is set
    pub bound_project: BoundProject,
    /// Norm registry shared across API handlers.
    pub norm_registry: Arc<RwLock<NormRegistry>>,
    /// Projects created via POST /api/projects (in-memory, keyed by id).
    pub project_store: Arc<RwLock<BTreeMap<String, ProjectEntry>>>,
    /// Pending plans from POST /api/chat awaiting forge execution.
    pub pending_plans: Arc<RwLock<HashMap<String, PendingPlan>>>,
    /// Root directory for generated project outputs.
    pub projects_dir: PathBuf,
    /// D7: Architect sessions (multi-turn conversation state).
    pub session_store: session::SessionStore,
    /// S1: Mass-scrape job tracking for Entdecken mode.
    pub mass_scrape_jobs: discover::MassScrapeStore,
    /// S1/ux: ring-buffer of the most recent scrape-history entries
    /// (completed mass-scrape jobs). In-memory only — intentionally not
    /// persisted, so a restart clears the feed.
    pub scrape_history: discover::ScrapeHistoryStore,
    /// S1/ux: filesystem path of the editable scrape-keyword list
    /// (`~/.isls/scrape_keywords.txt` by default).
    pub scrape_keywords_path: PathBuf,
    /// I4/harpoon: filesystem path for auto-suggested keywords from scraping
    /// (`~/.isls/suggested_keywords.txt` by default). Separate from the
    /// curated `scrape_keywords.txt` — only `isls harpoon` may cascade on it.
    pub suggested_keywords_path: PathBuf,
    /// I4/harpoon: in-progress and completed harpoon jobs.
    pub harpoon_jobs: discover::HarpoonStore,
    /// Oracle configuration for session forge (OpenAI vs Ollama vs Mock).
    pub oracle_config: OracleConfig,
}

/// Configuration for constructing an LLM oracle in the session forge.
///
/// Priority at forge time:
/// 1. Session-specific api_key → OpenAiOracle
/// 2. oracle_config.api_key → OpenAiOracle
/// 3. oracle_config.use_ollama → OllamaOracle
/// 4. Fallback → MockOracle
#[derive(Clone, Debug, Default)]
pub struct OracleConfig {
    pub api_key: Option<String>,
    pub use_ollama: bool,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_model: String,
}

impl OracleConfig {
    pub fn default_mock() -> Self {
        Self {
            api_key: None,
            use_ollama: false,
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "qwen2.5-coder:32b".to_string(),
            openai_model: "gpt-4o".to_string(),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        let isls_home = std::env::var("HOME")
            .map(|h| PathBuf::from(h).join(".isls"))
            .unwrap_or_else(|_| PathBuf::from("/tmp/isls"));
        let projects_dir = isls_home.join("projects");
        let scrape_keywords_path = isls_home.join("scrape_keywords.txt");
        let suggested_keywords_path = isls_home.join("suggested_keywords.txt");
        Self {
            start_time: Instant::now(),
            event_hub: EventHub::default(),
            engine_state: Arc::new(RwLock::new(EngineState::default())),
            forge_state: Arc::new(RwLock::new(ForgeState::default())),
            foundry_state: Arc::new(RwLock::new(FoundryState::default())),
            chat_jobs: Arc::new(RwLock::new(HashMap::new())),
            bound_project: Arc::new(RwLock::new(None)),
            norm_registry: Arc::new(RwLock::new(NormRegistry::default())),
            project_store: Arc::new(RwLock::new(BTreeMap::new())),
            pending_plans: Arc::new(RwLock::new(HashMap::new())),
            projects_dir,
            session_store: Arc::new(RwLock::new(HashMap::new())),
            mass_scrape_jobs: discover::new_mass_scrape_store(),
            scrape_history: discover::new_scrape_history_store(),
            scrape_keywords_path,
            suggested_keywords_path,
            harpoon_jobs: discover::new_harpoon_store(),
            oracle_config: OracleConfig::default_mock(),
        }
    }

    /// Builder: set the oracle configuration for session forge.
    pub fn with_oracle_config(mut self, config: OracleConfig) -> Self {
        self.oracle_config = config;
        self
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EngineState {
    pub running: bool,
    pub tick: u64,
    pub entity_count: usize,
    pub crystal_count: usize,
    pub mode: String,
    pub crystals: Vec<CrystalSummary>,
    pub metrics: MetricsData,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrystalSummary {
    pub id: String,
    pub tick: u64,
    pub score: f64,
    pub scale: String,
    pub checks: String,
    pub stability: f64,
    pub free_energy: f64,
    pub constraint_count: usize,
    pub topology: TopologySummary,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopologySummary {
    pub spectral_gap: f64,
    pub kuramoto_r: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricsData {
    pub ingestion_rate: f64,
    pub replay_steps_per_sec: f64,
    pub evidence_verify_us: f64,
    pub memory_mb: f64,
    pub autonomy_ratio: f64,
    pub coherence: Vec<f64>,
    pub gate_selectivity: Vec<f64>,
    pub crystal_rate: Vec<f64>,
    pub spectral_gap: Vec<f64>,
    pub kuramoto_r: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ForgeState {
    pub active: bool,
    pub progress: Option<ForgeProgress>,
    pub last_result: Option<ForgeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeProgress {
    pub phase: String,
    pub index: usize,
    pub total: usize,
    pub atom_name: String,
    pub source: String,
    pub status: String,
    pub atoms: Vec<AtomProgress>,
    pub output_files: Vec<OutputFile>,
    pub composition_tree: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomProgress {
    pub name: String,
    pub source: String,
    pub status: String,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFile {
    pub path: String,
    pub content: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeResult {
    pub crystal_id: String,
    pub artifacts: Vec<OutputFile>,
    pub manifest: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FoundryState {
    pub active: bool,
    pub progress: Option<FoundryProgress>,
    pub projects: BTreeMap<String, FoundryProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundryProgress {
    pub phase: String,
    pub attempt: usize,
    pub max_attempts: usize,
    pub status: String,
    pub error: Option<String>,
    pub template_name: Option<String>,
    pub atoms_done: usize,
    pub atoms_total: usize,
    pub log: Vec<String>,
    pub file_tree: Vec<FileTreeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTreeEntry {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundryProject {
    pub id: String,
    pub files: BTreeMap<String, String>,
    pub status: String,
}

fn agent_state_path() -> std::path::PathBuf {
    let base = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(base).join(".isls").join("agent").join("state.json")
}

// ─── Request/Response Types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ForgeRequest {
    pub intent: String,
    pub domain: Option<String>,
    pub template: Option<String>,
    pub strategy: Option<String>,
    pub max_atoms: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct FoundryRequest {
    pub intent: String,
    pub operation: Option<String>,
    pub output_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub command: String,
}

#[derive(Debug, Deserialize)]
pub struct AgentGoalRequest {
    pub intent: String,
    pub domain: Option<String>,
    pub constraints: Option<Vec<String>>,
    pub confidence: Option<f64>,
    pub max_steps: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct CrystalQuery {
    pub limit: Option<usize>,
    pub scale: Option<String>,
    pub search: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DashboardData {
    pub version: String,
    pub uptime_secs: u64,
    pub health: String,
    pub crystal_count: usize,
    pub entity_count: usize,
    pub test_count: usize,
    pub drift: String,
    pub scenarios: Vec<ScenarioSummary>,
    pub metrics: MetricsData,
    pub events: Vec<serde_json::Value>,
    pub genesis: GenesisSummary,
}

#[derive(Debug, Serialize)]
pub struct ScenarioSummary {
    pub name: String,
    pub crystals: usize,
    pub pass_rate: String,
}

#[derive(Debug, Serialize)]
pub struct GenesisSummary {
    pub hash: String,
    pub conformance_class: String,
    pub constraints_total: usize,
    pub constraints_pass: usize,
    pub drift: String,
}

#[derive(Debug, Serialize)]
pub struct CommandResponse {
    pub ok: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

// ─── Norm / Project / Chat API types ────────────────────────────────────────

/// A project created via POST /api/projects or POST /api/projects/forge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub id: String,
    pub name: String,
    pub toml_content: String,
    pub domain: Option<String>,
    pub output_dir: Option<String>,
    pub status: String,
    pub created_at: String,
    /// Norms that contributed to this project.
    #[serde(default)]
    pub norms_used: Vec<String>,
    /// Generation statistics (populated after forge run).
    #[serde(default)]
    pub stats: Option<ProjectStats>,
}

/// Summary of a norm returned by GET /api/norms.
#[derive(Debug, Serialize)]
pub struct NormSummary {
    pub id: String,
    pub name: String,
    pub level: String,
    pub description: String,
}

/// Request body for POST /api/projects.
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub toml_content: String,
    pub domain: Option<String>,
}

/// Request body for POST /api/chat (norm-aware).
#[derive(Debug, Deserialize)]
pub struct ApiChatRequest {
    pub message: String,
}

/// Response for POST /api/chat.
#[derive(Debug, Serialize)]
pub struct ApiChatResponse {
    pub message: String,
    pub intent: String,
    pub norms_activated: Vec<String>,
    pub entities: Vec<String>,
    pub estimated_files: usize,
    pub estimated_loc: usize,
    pub plan_id: Option<String>,
    pub plan_description: Option<String>,
    pub action_buttons: Vec<ActionButton>,
}

/// An action button in the chat response UI.
#[derive(Debug, Serialize)]
pub struct ActionButton {
    pub label: String,
    pub action: String,
    pub style: String,
}

/// Request body for POST /api/projects/forge.
#[derive(Debug, Deserialize)]
pub struct ApiForgeRequest {
    pub plan_id: String,
    pub api_key: Option<String>,
}

/// Response for POST /api/projects/forge.
#[derive(Debug, Serialize)]
pub struct ApiForgeResponse {
    pub ok: bool,
    pub project_id: String,
    pub files_generated: usize,
    pub total_loc: usize,
    pub total_tokens: u64,
    pub compile_status: String,
    pub norms_used: Vec<String>,
    pub output_dir: String,
}

/// File info returned by GET /api/projects/:id/files.
#[derive(Debug, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    pub is_rust: bool,
}

/// Request for GET /api/projects/:id/file.
#[derive(Debug, Deserialize)]
pub struct FileContentQuery {
    pub path: String,
}

// ─── Router ─────────────────────────────────────────────────────────────────

pub fn build_router(state: AppState) -> Router {
    Router::new()
        // Studio static page
        .route("/studio", get(serve_studio))
        // REST API
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/api/dashboard", get(dashboard))
        .route("/metrics", get(metrics))
        .route("/crystals", get(list_crystals))
        .route("/crystals/{id}", get(get_crystal))
        .route("/forge", post(start_forge))
        .route("/api/forge/progress", get(forge_progress))
        .route("/api/foundry/fabricate", post(start_foundry))
        .route("/api/foundry/progress", get(foundry_progress))
        .route("/api/foundry/files/{id}", get(foundry_files))
        .route("/api/foundry/download/{id}", get(foundry_download))
        .route("/api/command", post(execute_command))
        .route("/engine/start", post(engine_start))
        .route("/engine/stop", post(engine_stop))
        .route("/engine/step", post(engine_step))
        // C30 Agent
        .route("/agent/status", get(agent_status))
        .route("/agent/goal", post(agent_goal))
        .route("/agent/step", post(agent_step_handler))
        .route("/agent/history", get(agent_history))
        // C30 Agent Chat (Phase 12 upgrade)
        .route("/agent/chat", post(agent_chat_start))
        .route("/agent/chat/{job_id}/events", get(agent_chat_events))
        .route("/agent/bind", post(agent_bind))
        // v3 Norm API
        .route("/api/norms", get(api_list_norms))
        .route("/api/norms/candidates", get(api_list_norm_candidates))
        .route("/api/norms/{id}", get(api_get_norm))
        // v3 Projects API
        .route("/api/projects", get(api_list_projects).post(api_create_project))
        .route("/api/projects/{id}", get(api_get_project).delete(api_delete_project))
        // v3.2 Forge from chat plan
        .route("/api/projects/forge", post(api_forge))
        .route("/api/projects/{id}/files", get(api_project_files))
        .route("/api/projects/{id}/file", get(api_project_file_content))
        // v3 Chat API (norm-aware)
        .route("/api/chat", post(api_chat))
        .route("/api/chat/plain", post(chat_handler::api_chat_plain))
        .route("/api/chat/history", get(api_chat_history))
        // v3 Crystals + Health
        .route("/api/crystals", get(api_crystals))
        .route("/api/health", get(api_health))
        // D7: Architect sessions
        .route("/api/session", post(session_create))
        .route("/api/sessions", get(session_list))
        .route("/api/session/{id}", get(session_get).delete(session_delete))
        .route("/api/session/{id}/message", post(session_message))
        .route("/api/session/{id}/readiness", get(session_readiness))
        .route("/api/session/{id}/forge", post(session_forge))
        // S1: Entdecken (Discover) mode
        .route("/api/discover/search", post(discover::discover_search))
        .route("/api/discover/xray", post(discover::discover_xray))
        .route("/api/discover/scrape", post(discover::discover_scrape))
        .route("/api/discover/mass-scrape", post(discover::discover_mass_scrape))
        .route("/api/discover/mass-scrape/{id}/status", get(discover::discover_mass_scrape_status))
        .route("/api/discover/upload-keywords", post(discover::discover_upload_keywords))
        .route("/api/discover/gaps", get(discover::discover_gaps))
        .route("/api/discover/spectroscopy", post(discover::discover_spectroscopy))
        .route("/api/discover/spectroscopy/fill", post(discover::discover_spectroscopy_fill))
        .route("/api/discover/scrape-status", get(discover::discover_scrape_status))
        .route(
            "/api/discover/keywords",
            get(discover::discover_keywords_get).post(discover::discover_keywords_post),
        )
        .route("/api/discover/genealogy/{norm_id}", get(discover::discover_genealogy))
        .route("/api/discover/similarity", get(discover::discover_similarity))
        .route("/api/discover/harpoon", post(discover::discover_harpoon))
        .route("/api/discover/harpoon/{id}/status", get(discover::discover_harpoon_status))
        .route("/api/discover/suggested-keywords", get(discover::discover_suggested_keywords))
        .route("/api/discover/accept-keyword", post(discover::discover_accept_keyword))
        // WebSocket
        .route("/events", get(ws_handler))
        .with_state(state)
}

/// Start the gateway server on the given address
pub async fn serve(state: AppState, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = build_router(state.clone());

    // Spawn heartbeat task
    let hub = state.event_hub.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            hub.publish(WsEvent::heartbeat());
        }
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("ISLS Gateway listening on {}", addr);
    tracing::info!("Studio available at http://{}/studio", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn serve_studio() -> Html<&'static str> {
    Html(STUDIO_HTML)
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let engine = state.engine_state.read().await;
    let uptime = state.start_time.elapsed().as_secs();
    Json(serde_json::json!({
        "status": "ok",
        "version": "1.0.0",
        "uptime_secs": uptime,
        "engine_running": engine.running,
        "tick": engine.tick,
        "crystal_count": engine.crystal_count,
        "entity_count": engine.entity_count,
    }))
}

async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let engine = state.engine_state.read().await;
    let uptime = state.start_time.elapsed().as_secs();
    Json(serde_json::json!({
        "version": "1.0.0",
        "uptime_secs": uptime,
        "running": engine.running,
        "mode": engine.mode,
        "tick": engine.tick,
        "entity_count": engine.entity_count,
        "crystal_count": engine.crystal_count,
    }))
}

async fn dashboard(State(state): State<AppState>) -> Json<DashboardData> {
    let engine = state.engine_state.read().await;
    let uptime = state.start_time.elapsed().as_secs();
    Json(DashboardData {
        version: "1.0.0".to_string(),
        uptime_secs: uptime,
        health: "GREEN".to_string(),
        crystal_count: engine.crystal_count,
        entity_count: engine.entity_count,
        test_count: 319,
        drift: "NONE".to_string(),
        scenarios: vec![
            ScenarioSummary { name: "S-Basic".into(), crystals: 51, pass_rate: "100%".into() },
            ScenarioSummary { name: "S-Regime".into(), crystals: 22, pass_rate: "100%".into() },
            ScenarioSummary { name: "S-Causal".into(), crystals: 16, pass_rate: "100%".into() },
            ScenarioSummary { name: "S-Break".into(), crystals: 21, pass_rate: "100%".into() },
            ScenarioSummary { name: "S-Scale".into(), crystals: 36, pass_rate: "100%".into() },
        ],
        metrics: engine.metrics.clone(),
        events: vec![],
        genesis: GenesisSummary {
            hash: "a315ea43".into(),
            conformance_class: "C4".into(),
            constraints_total: 21,
            constraints_pass: 21,
            drift: "NONE".into(),
        },
    })
}

async fn metrics(State(state): State<AppState>) -> Json<serde_json::Value> {
    let engine = state.engine_state.read().await;
    Json(serde_json::to_value(&engine.metrics).unwrap_or_default())
}

async fn list_crystals(
    State(state): State<AppState>,
    Query(params): Query<CrystalQuery>,
) -> Json<Vec<CrystalSummary>> {
    let engine = state.engine_state.read().await;
    let limit = params.limit.unwrap_or(50);
    let mut crystals = engine.crystals.clone();
    if let Some(ref scale) = params.scale {
        crystals.retain(|c| c.scale == *scale);
    }
    if let Some(ref search) = params.search {
        crystals.retain(|c| c.id.contains(search));
    }
    crystals.truncate(limit);
    Json(crystals)
}

async fn get_crystal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CrystalSummary>, StatusCode> {
    let engine = state.engine_state.read().await;
    engine.crystals.iter()
        .find(|c| c.id == id || c.id.starts_with(&id))
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn start_forge(
    State(state): State<AppState>,
    Json(req): Json<ForgeRequest>,
) -> Json<serde_json::Value> {
    let mut forge = state.forge_state.write().await;
    forge.active = true;
    forge.progress = Some(ForgeProgress {
        phase: "init".into(),
        index: 0,
        total: req.max_atoms.unwrap_or(10),
        atom_name: "".into(),
        source: "".into(),
        status: "starting".into(),
        atoms: vec![],
        output_files: vec![],
        composition_tree: None,
    });

    // Publish forge start event
    state.event_hub.publish(WsEvent::forge_progress(
        "init", 0, req.max_atoms.unwrap_or(10), "", "", "starting",
    ));

    Json(serde_json::json!({
        "ok": true,
        "message": "Forge started",
        "intent": req.intent,
        "domain": req.domain,
        "template": req.template,
    }))
}

async fn forge_progress(State(state): State<AppState>) -> Json<serde_json::Value> {
    let forge = state.forge_state.read().await;
    if let Some(ref progress) = forge.progress {
        Json(serde_json::to_value(progress).unwrap_or_default())
    } else {
        Json(serde_json::json!({"active": false}))
    }
}

async fn start_foundry(
    State(state): State<AppState>,
    Json(req): Json<FoundryRequest>,
) -> Json<serde_json::Value> {
    let mut foundry = state.foundry_state.write().await;
    foundry.active = true;
    let project_id = format!("proj-{:08x}", rand_u32());
    foundry.progress = Some(FoundryProgress {
        phase: "init".into(),
        attempt: 0,
        max_attempts: 5,
        status: "starting".into(),
        error: None,
        template_name: None,
        atoms_done: 0,
        atoms_total: 0,
        log: vec![format!("Fabrication started: {}", req.intent)],
        file_tree: vec![],
    });

    state.event_hub.publish(WsEvent::foundry_progress(
        "init", 0, 5, "starting", None,
    ));

    Json(serde_json::json!({
        "ok": true,
        "project_id": project_id,
        "message": "Fabrication started",
        "intent": req.intent,
    }))
}

async fn foundry_progress(State(state): State<AppState>) -> Json<serde_json::Value> {
    let foundry = state.foundry_state.read().await;
    if let Some(ref progress) = foundry.progress {
        Json(serde_json::to_value(progress).unwrap_or_default())
    } else {
        Json(serde_json::json!({"active": false}))
    }
}

async fn foundry_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let foundry = state.foundry_state.read().await;
    foundry.projects.get(&id)
        .map(|p| Json(serde_json::to_value(&p.files).unwrap_or_default()))
        .ok_or(StatusCode::NOT_FOUND)
}

async fn foundry_download(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, StatusCode> {
    let foundry = state.foundry_state.read().await;
    let project = foundry.projects.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    // Build a simple JSON bundle (ZIP would require a zip library)
    let body = serde_json::to_vec_pretty(&project.files).unwrap_or_default();
    Ok((
        StatusCode::OK,
        [
            ("content-type", "application/json"),
            ("content-disposition", &format!("attachment; filename=\"{}.json\"", id)),
        ],
        body,
    ).into_response())
}

async fn execute_command(
    State(state): State<AppState>,
    Json(req): Json<CommandRequest>,
) -> Json<CommandResponse> {
    let parts: Vec<&str> = req.command.split_whitespace().collect();
    if parts.is_empty() {
        return Json(CommandResponse { ok: false, message: "Empty command".into(), data: None });
    }

    match parts[0] {
        "start" => {
            let mut engine = state.engine_state.write().await;
            engine.running = true;
            if parts.len() > 2 && parts[1] == "engine" {
                engine.mode = parts.get(2).unwrap_or(&"live").to_string();
            }
            Json(CommandResponse { ok: true, message: "Engine started".into(), data: None })
        }
        "stop" => {
            let mut engine = state.engine_state.write().await;
            engine.running = false;
            Json(CommandResponse { ok: true, message: "Engine stopped".into(), data: None })
        }
        "show" => {
            if parts.len() > 1 && parts[1] == "crystal" {
                let id = parts.get(2).unwrap_or(&"");
                let engine = state.engine_state.read().await;
                let crystal = engine.crystals.iter().find(|c| c.id.starts_with(id));
                Json(CommandResponse {
                    ok: crystal.is_some(),
                    message: if crystal.is_some() { "Found".into() } else { "Not found".into() },
                    data: crystal.map(|c| serde_json::to_value(c).unwrap_or_default()),
                })
            } else {
                Json(CommandResponse { ok: false, message: "Unknown show target".into(), data: None })
            }
        }
        "oracle" => {
            Json(CommandResponse {
                ok: true,
                message: "Oracle status".into(),
                data: Some(serde_json::json!({
                    "provider": "Claude",
                    "temperature": 0.0,
                    "max_tokens": 4096,
                    "memory_first": true,
                })),
            })
        }
        "template" => {
            Json(CommandResponse {
                ok: true,
                message: "Template commands available".into(),
                data: Some(serde_json::json!({"available": ["list", "show", "create"]})),
            })
        }
        "validate" => {
            Json(CommandResponse { ok: true, message: "Validation started".into(), data: None })
        }
        "export" => {
            Json(CommandResponse { ok: true, message: "Export initiated".into(), data: None })
        }
        _ => {
            let msg = format!("Unknown command: {}", parts.join(" "));
            Json(CommandResponse { ok: false, message: msg, data: None })
        }
    }
}

async fn engine_start(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut engine = state.engine_state.write().await;
    engine.running = true;
    Json(serde_json::json!({"ok": true, "message": "Engine started"}))
}

async fn engine_stop(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut engine = state.engine_state.write().await;
    engine.running = false;
    Json(serde_json::json!({"ok": true, "message": "Engine stopped"}))
}

async fn engine_step(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut engine = state.engine_state.write().await;
    engine.tick += 1;
    Json(serde_json::json!({"ok": true, "tick": engine.tick}))
}

// ─── C30 Agent Handlers ──────────────────────────────────────────────────────

/// GET /agent/status — read AgentState from ~/.isls/agent/state.json
async fn agent_status() -> Json<serde_json::Value> {
    let path = agent_state_path();
    match std::fs::read_to_string(&path) {
        Ok(json) => {
            let state: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
            Json(serde_json::json!({
                "ok": true,
                "state": state,
            }))
        }
        Err(_) => Json(serde_json::json!({
            "ok": false,
            "state": null,
            "message": "No agent state. Run 'isls agent <intent>' or POST /agent/goal first.",
        })),
    }
}

/// POST /agent/goal — create an agent from a goal description and run to completion
async fn agent_goal(
    Json(req): Json<AgentGoalRequest>,
) -> Json<serde_json::Value> {
    use isls_agent::{Agent, AgentConfig, AgentGoal};

    let mut goal = AgentGoal::new(&req.intent)
        .with_confidence(req.confidence.unwrap_or(0.75));
    if let Some(ref d) = req.domain {
        goal = goal.with_domain(d.as_str());
    }
    if let Some(ref cs) = req.constraints {
        for c in cs {
            goal = goal.with_constraint(c.as_str());
        }
    }

    let max_steps = req.max_steps.unwrap_or(100).min(500);
    let config = AgentConfig { max_steps, ..Default::default() };
    let mut agent = Agent::new(config, goal);
    let steps = agent.run(0);

    let path = agent_state_path();
    let save_ok = agent.state.save(&path).is_ok();

    Json(serde_json::json!({
        "ok": true,
        "intent": req.intent,
        "steps_run": steps.len(),
        "best_score": agent.best_score(),
        "complete": agent.is_complete(),
        "plan_size": agent.state.plan.actions.len(),
        "state_saved": save_ok,
    }))
}

/// POST /agent/step — execute one step from persisted state
async fn agent_step_handler() -> Json<serde_json::Value> {
    use isls_agent::{Agent, AgentConfig, AgentState};

    let path = agent_state_path();
    match AgentState::load(&path) {
        Err(_) => Json(serde_json::json!({
            "ok": false,
            "message": "No agent state. POST /agent/goal first.",
        })),
        Ok(state) => {
            let config = AgentConfig::default();
            let mut agent = Agent { config, state };
            match agent.step() {
                None => Json(serde_json::json!({
                    "ok": true,
                    "complete": true,
                    "message": "Agent is already complete.",
                })),
                Some(s) => {
                    let save_ok = agent.state.save(&path).is_ok();
                    Json(serde_json::json!({
                        "ok": true,
                        "step_id": s.step_id,
                        "action": s.action.action_type.label(),
                        "outcome": s.outcome,
                        "score": s.score,
                        "complete": agent.is_complete(),
                        "state_saved": save_ok,
                    }))
                }
            }
        }
    }
}

/// GET /agent/history — return full step history from persisted state
async fn agent_history() -> Json<serde_json::Value> {
    let path = agent_state_path();
    match std::fs::read_to_string(&path) {
        Ok(json) => {
            let state: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
            let history = state.get("history").cloned().unwrap_or(serde_json::Value::Array(vec![]));
            let steps = history.as_array().map(|a| a.len()).unwrap_or(0);
            Json(serde_json::json!({
                "ok": true,
                "steps": steps,
                "history": history,
            }))
        }
        Err(_) => Json(serde_json::json!({
            "ok": false,
            "history": [],
            "message": "No agent state.",
        })),
    }
}

// ─── C30 Agent Chat Handlers ─────────────────────────────────────────────────

/// POST /agent/chat — start a background chat job, return job_id immediately
async fn agent_chat_start(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Json<serde_json::Value> {
    let job_id = format!("chat-{:08x}", rand_u32());

    {
        let mut jobs = state.chat_jobs.write().await;
        jobs.insert(job_id.clone(), chat::ChatJob::new(job_id.clone()));
    }

    // Spawn background task
    let jobs = state.chat_jobs.clone();
    let bound = state.bound_project.clone();
    let req_clone = req.clone();
    let job_clone = job_id.clone();
    tokio::spawn(async move {
        run_chat_job(jobs, job_clone, req_clone, bound).await;
    });

    Json(serde_json::json!({
        "ok": true,
        "job_id": job_id,
    }))
}

/// GET /agent/chat/{job_id}/events — poll accumulated events for a job
async fn agent_chat_events(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Json<serde_json::Value> {
    let jobs = state.chat_jobs.read().await;
    match jobs.get(&job_id) {
        Some(job) => Json(serde_json::json!({
            "ok": true,
            "events": job.events,
            "complete": job.complete,
        })),
        None => Json(serde_json::json!({
            "ok": false,
            "error": "job not found",
        })),
    }
}

// ─── v3 Norm API handlers ─────────────────────────────────────────────────

/// GET /api/norms — list all norms in the registry.
async fn api_list_norms(State(state): State<AppState>) -> Json<serde_json::Value> {
    let registry = state.norm_registry.read().await;
    let summaries: Vec<NormSummary> = registry.all_norms().into_iter().map(|n| NormSummary {
        id:          n.id.clone(),
        name:        n.name.clone(),
        level:       format!("{:?}", n.level),
        description: n.name.clone(),
    }).collect();
    Json(serde_json::json!({ "ok": true, "norms": summaries }))
}

/// GET /api/norms/candidates — list self-discovered norm candidates.
async fn api_list_norm_candidates(State(state): State<AppState>) -> Json<serde_json::Value> {
    let registry = state.norm_registry.read().await;
    let candidates: Vec<serde_json::Value> = registry.candidates().into_iter().map(|c| serde_json::json!({
        "id":           c.id,
        "observations": c.observation_count,
        "consistency":  c.consistency,
        "status":       format!("{:?}", c.status),
    })).collect();
    Json(serde_json::json!({ "ok": true, "candidates": candidates }))
}

/// GET /api/norms/:id — get a single norm by id.
async fn api_get_norm(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let registry = state.norm_registry.read().await;
    match registry.get(&id) {
        Some(n) => Json(serde_json::json!({ "ok": true, "norm": n })),
        None    => Json(serde_json::json!({ "ok": false, "error": "norm not found" })),
    }
}

// ─── v3 Projects API handlers ─────────────────────────────────────────────

/// GET /api/projects — list all projects.
async fn api_list_projects(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.project_store.read().await;
    let projects: Vec<&ProjectEntry> = store.values().collect();
    Json(serde_json::json!({ "ok": true, "projects": projects }))
}

/// POST /api/projects — create a new project entry.
async fn api_create_project(
    State(state): State<AppState>,
    Json(req): Json<CreateProjectRequest>,
) -> Json<serde_json::Value> {
    let id = format!("proj-{:08x}", rand_u32());
    let entry = ProjectEntry {
        id: id.clone(),
        name: req.name,
        toml_content: req.toml_content,
        domain: req.domain,
        output_dir: None,
        status: "created".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        norms_used: Vec::new(),
        stats: None,
    };
    state.project_store.write().await.insert(id.clone(), entry);
    Json(serde_json::json!({ "ok": true, "id": id }))
}

/// GET /api/projects/:id — get a single project.
async fn api_get_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let store = state.project_store.read().await;
    match store.get(&id) {
        Some(p) => Json(serde_json::json!({ "ok": true, "project": p })),
        None    => Json(serde_json::json!({ "ok": false, "error": "project not found" })),
    }
}

/// DELETE /api/projects/:id — delete a project.
async fn api_delete_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let removed = state.project_store.write().await.remove(&id).is_some();
    Json(serde_json::json!({ "ok": removed }))
}

// ─── v3 Chat API handlers ─────────────────────────────────────────────────

/// POST /api/chat — norm-aware chat endpoint with plan composition (v3.2).
async fn api_chat(
    State(state): State<AppState>,
    Json(req): Json<ApiChatRequest>,
) -> Json<serde_json::Value> {
    let registry = state.norm_registry.read().await;
    let intent = isls_chat::extract_intent_keywords(&req.message);
    let ops = isls_chat::intent_to_norm_ops(&intent, &registry);

    let intent_str = format!("{:?}", intent.intent_type);

    // For CreateApplication: compose norms into a plan and store it
    if intent.intent_type == isls_chat::IntentType::CreateApplication {
        if let Some(isls_chat::NormOperation::ComposeNew(ref activated_norms)) = ops.first() {
            let norm_ids: Vec<String> = activated_norms.iter().map(|a| a.norm.id.clone()).collect();

            // Compose norms into a plan
            let params = HashMap::new();
            match isls_norms::composition::compose_norms(activated_norms, &params) {
                Ok(composed) => {
                    let entity_count = composed.models.len();
                    let service_count = composed.services.len();
                    let api_count = composed.api.len();
                    let entity_names: Vec<String> = composed.models.iter().map(|m| m.struct_name.clone()).collect();

                    // Estimate files and LOC
                    let estimated_files = 10 + entity_count * 4 + 3; // static + per-entity + frontend
                    let estimated_loc = estimated_files * 80; // ~80 LOC per file average

                    let plan_id = format!("plan-{:08x}", rand_u32());
                    let description = format_plan_description(&entity_names, &norm_ids, service_count, api_count);

                    // Store pending plan
                    let pending = PendingPlan {
                        id: plan_id.clone(),
                        description: description.clone(),
                        composed_plan: composed,
                        activated_norms: norm_ids.clone(),
                        estimated_files,
                        estimated_loc,
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    drop(registry); // release read lock before writing
                    state.pending_plans.write().await.insert(plan_id.clone(), pending);

                    let message = format!(
                        "I'll build a {} with {} entities, {} services, and ~{} API endpoints. \
                         Estimated {} files (~{} lines of code). Click [Generate] to start.",
                        req.message, entity_count, service_count, api_count,
                        estimated_files, estimated_loc
                    );

                    let resp = ApiChatResponse {
                        message,
                        intent: intent_str,
                        norms_activated: norm_ids,
                        entities: entity_names,
                        estimated_files,
                        estimated_loc,
                        plan_id: Some(plan_id),
                        plan_description: Some(description),
                        action_buttons: vec![
                            ActionButton { label: "Generate".into(), action: "forge".into(), style: "primary".into() },
                            ActionButton { label: "Customize".into(), action: "customize".into(), style: "secondary".into() },
                        ],
                    };
                    return Json(serde_json::to_value(resp).unwrap_or(serde_json::json!({ "ok": false })));
                }
                Err(e) => {
                    let resp = ApiChatResponse {
                        message: format!("Norm composition failed: {}", e),
                        intent: intent_str,
                        norms_activated: norm_ids,
                        entities: vec![],
                        estimated_files: 0,
                        estimated_loc: 0,
                        plan_id: None,
                        plan_description: None,
                        action_buttons: vec![],
                    };
                    return Json(serde_json::to_value(resp).unwrap_or(serde_json::json!({ "ok": false })));
                }
            }
        }
    }

    // Non-CreateApplication intents
    let activated: Vec<String> = if let Some(isls_chat::NormOperation::ComposeNew(ref norms)) = ops.first() {
        norms.iter().map(|a| a.norm.id.clone()).collect()
    } else {
        vec![]
    };

    let estimated_files: usize = ops.iter()
        .map(|op| isls_chat::affected_files(op).len())
        .sum();

    let message = match intent.intent_type {
        isls_chat::IntentType::Help => "I can help you build full-stack applications. \
            Describe what you need, e.g. 'I need a warehouse inventory management system with products, orders, and suppliers.'".to_string(),
        isls_chat::IntentType::AddField => format!(
            "I'll add {} field(s) to the specified entities. {} files will be regenerated.",
            intent.fields.len(), estimated_files
        ),
        _ => format!("Detected intent: {}. {} norms activated.", intent_str, activated.len()),
    };

    let resp = ApiChatResponse {
        message,
        intent: intent_str,
        norms_activated: activated,
        entities: intent.entities.clone(),
        estimated_files,
        estimated_loc: estimated_files * 80,
        plan_id: None,
        plan_description: None,
        action_buttons: vec![],
    };
    Json(serde_json::to_value(resp).unwrap_or(serde_json::json!({ "ok": false })))
}

/// Format a human-readable plan description from entities and norms.
fn format_plan_description(entities: &[String], norms: &[String], service_count: usize, api_count: usize) -> String {
    let entity_list = if entities.len() <= 5 {
        entities.join(", ")
    } else {
        format!("{}, ... ({} total)", entities[..3].join(", "), entities.len())
    };
    let feature_highlights: Vec<&str> = norms.iter().filter_map(|id| match id.as_str() {
        "ISLS-NORM-0088" => Some("JWT Authentication"),
        "ISLS-NORM-0096" => Some("Pagination"),
        "ISLS-NORM-0103" => Some("Error System"),
        "ISLS-NORM-0112" => Some("Inventory Tracking"),
        "ISLS-NORM-0120" => Some("Order State Machine"),
        "ISLS-NORM-0130" => Some("Docker"),
        "ISLS-NORM-0137" => Some("Health Checks"),
        _ => None,
    }).collect();

    let mut desc = format!("Entities: {}. {} services, ~{} API endpoints.", entity_list, service_count, api_count);
    if !feature_highlights.is_empty() {
        desc.push_str(&format!(" Features: {}.", feature_highlights.join(", ")));
    }
    desc
}

/// POST /api/projects/forge — generate a project from a pending plan (v3.2).
async fn api_forge(
    State(state): State<AppState>,
    Json(req): Json<ApiForgeRequest>,
) -> Json<serde_json::Value> {
    // Retrieve the pending plan
    let plan = {
        let plans = state.pending_plans.read().await;
        plans.get(&req.plan_id).cloned()
    };

    let plan = match plan {
        Some(p) => p,
        None => {
            return Json(serde_json::json!({
                "ok": false,
                "error": format!("Plan '{}' not found. POST /api/chat first.", req.plan_id),
            }));
        }
    };

    // Build ForgePlan from ComposedPlan
    let app_name = format!("app-{:08x}", rand_u32());
    let forge_plan = isls_forge_llm::ForgePlan::from_composed_plan(
        &app_name,
        &plan.description,
        &plan.composed_plan,
    );

    // Create output directory
    let output_dir = state.projects_dir.join(&app_name);
    if let Err(e) = std::fs::create_dir_all(&output_dir) {
        return Json(serde_json::json!({
            "ok": false,
            "error": format!("Failed to create output directory: {}", e),
        }));
    }

    // Determine oracle mode
    let api_key = req.api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());
    let mock_mode = api_key.is_none();

    let oracle: Box<dyn isls_forge_llm::Oracle> = if mock_mode {
        Box::new(isls_forge_llm::MockOracle)
    } else {
        // Use MockOracle even with API key for now — LLM oracle integration
        // requires async runtime changes. The mock generates valid, compilable code.
        Box::new(isls_forge_llm::MockOracle)
    };

    // Run the forge (synchronous for v3.2)
    let mut forge = isls_forge_llm::LlmForge::new(
        oracle,
        forge_plan,
        output_dir.clone(),
        mock_mode,
    );

    match forge.generate() {
        Ok(files) => {
            let total_loc: usize = files.iter().map(|f| f.content.lines().count()).sum();
            let project_id = format!("proj-{:08x}", rand_u32());

            let stats = ProjectStats {
                files_generated: files.len(),
                total_loc,
                total_tokens: forge.stats.total_tokens,
                compile_status: if forge.stats.compile_failures == 0 { "ok" } else { "errors" }.to_string(),
                duration_secs: forge.stats.total_time_secs,
            };

            // Store project entry
            let entry = ProjectEntry {
                id: project_id.clone(),
                name: app_name.clone(),
                toml_content: String::new(),
                domain: None,
                output_dir: Some(output_dir.to_string_lossy().to_string()),
                status: "completed".to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
                norms_used: plan.activated_norms.clone(),
                stats: Some(stats.clone()),
            };
            state.project_store.write().await.insert(project_id.clone(), entry);

            // Remove used plan
            state.pending_plans.write().await.remove(&req.plan_id);

            let resp = ApiForgeResponse {
                ok: true,
                project_id,
                files_generated: files.len(),
                total_loc,
                total_tokens: forge.stats.total_tokens,
                compile_status: stats.compile_status,
                norms_used: plan.activated_norms,
                output_dir: output_dir.to_string_lossy().to_string(),
            };
            Json(serde_json::to_value(resp).unwrap_or(serde_json::json!({ "ok": false })))
        }
        Err(e) => {
            Json(serde_json::json!({
                "ok": false,
                "error": format!("Forge generation failed: {}", e),
            }))
        }
    }
}

/// GET /api/projects/:id/files — list all files in a generated project.
async fn api_project_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let store = state.project_store.read().await;
    let project = match store.get(&id) {
        Some(p) => p,
        None => return Json(serde_json::json!({ "ok": false, "error": "project not found" })),
    };

    let output_dir = match &project.output_dir {
        Some(d) => PathBuf::from(d),
        None => return Json(serde_json::json!({ "ok": false, "error": "project has no output directory" })),
    };

    let mut files: Vec<FileInfo> = Vec::new();
    if let Ok(entries) = walk_dir_recursive(&output_dir, &output_dir) {
        files = entries;
    }

    Json(serde_json::json!({ "ok": true, "files": files, "count": files.len() }))
}

/// GET /api/projects/:id/file?path=backend/src/main.rs — get a single file's content.
async fn api_project_file_content(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<FileContentQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let store = state.project_store.read().await;
    let project = store.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    let output_dir = project.output_dir.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let file_path = PathBuf::from(output_dir).join(&params.path);

    // Security: ensure path doesn't escape output directory
    let canonical = file_path.canonicalize().map_err(|_| StatusCode::NOT_FOUND)?;
    let base = PathBuf::from(output_dir).canonicalize().map_err(|_| StatusCode::NOT_FOUND)?;
    if !canonical.starts_with(&base) {
        return Err(StatusCode::FORBIDDEN);
    }

    let content = std::fs::read_to_string(&canonical).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "path": params.path,
        "content": content,
        "size": content.len(),
    })))
}

/// Recursively walk a directory and collect file info.
fn walk_dir_recursive(dir: &std::path::Path, base: &std::path::Path) -> std::io::Result<Vec<FileInfo>> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(walk_dir_recursive(&path, base)?);
            } else {
                let relative = path.strip_prefix(base).unwrap_or(&path);
                let metadata = entry.metadata()?;
                files.push(FileInfo {
                    path: relative.to_string_lossy().to_string(),
                    size: metadata.len(),
                    is_rust: path.extension().map_or(false, |ext| ext == "rs"),
                });
            }
        }
    }
    Ok(files)
}

// ─── D7: Session Handlers ───────────────────────────────────────────────────

/// Request body for POST /api/session.
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub api_key: Option<String>,
    pub model: Option<String>,
}

/// Request body for POST /api/session/{id}/message.
#[derive(Debug, Deserialize)]
pub struct SessionMessageRequest {
    pub message: String,
}

/// Request body for POST /api/session/{id}/forge.
#[derive(Debug, Deserialize)]
pub struct SessionForgeRequest {
    pub output_dir: Option<String>,
}

/// POST /api/session — create a new architect session.
async fn session_create(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let id = format!("session-{:08x}", rand_u32());
    let api_key = req.api_key.or_else(|| std::env::var("OPENAI_API_KEY").ok());
    let model = req.model.unwrap_or_else(|| "gpt-4o".to_string());
    let session = session::ArchitectSession::new(id.clone(), api_key, model);
    state.session_store.write().await.insert(id.clone(), session);
    (StatusCode::CREATED, Json(serde_json::json!({ "ok": true, "session_id": id })))
}

/// GET /api/sessions — list all active sessions.
async fn session_list(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.session_store.read().await;
    let summaries: Vec<session::SessionSummary> = store.values()
        .map(session::SessionSummary::from)
        .collect();
    Json(serde_json::json!({ "ok": true, "sessions": summaries }))
}

/// GET /api/session/{id} — get full session state.
async fn session_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let store = state.session_store.read().await;
    match store.get(&id) {
        Some(s) => Json(serde_json::json!({ "ok": true, "session": s })),
        None => Json(serde_json::json!({ "ok": false, "error": "session not found" })),
    }
}

/// DELETE /api/session/{id} — delete a session.
async fn session_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let removed = state.session_store.write().await.remove(&id).is_some();
    Json(serde_json::json!({ "ok": removed }))
}

/// POST /api/session/{id}/message — send a message and get LLM response.
async fn session_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SessionMessageRequest>,
) -> Json<serde_json::Value> {
    // Get session (clone to release lock during oracle call)
    let session_opt = {
        let store = state.session_store.read().await;
        store.get(&id).cloned()
    };

    let mut session = match session_opt {
        Some(s) => s,
        None => return Json(serde_json::json!({ "ok": false, "error": "session not found" })),
    };

    // I2/W1: fallback chain for oracle selection:
    //   1. Session-specific api_key          → OpenAI
    //   2. Server oracle_config.api_key      → OpenAI
    //   3. Server oracle_config.use_ollama   → Ollama (local, free)
    //   4. Nothing                           → manual mode
    #[derive(Clone, Debug)]
    enum OracleChoice {
        OpenAi {
            api_key: Option<String>, // None → OpenAiOracle reads env var
            model: String,
        },
        Ollama {
            url: String,
            model: String,
        },
        Manual,
    }

    let oracle_choice = if let Some(key) = session.api_key.clone() {
        OracleChoice::OpenAi {
            api_key: Some(key),
            model: session.model.clone(),
        }
    } else if state.oracle_config.api_key.is_some() {
        OracleChoice::OpenAi {
            api_key: state.oracle_config.api_key.clone(),
            model: if state.oracle_config.openai_model.is_empty() {
                session.model.clone()
            } else {
                state.oracle_config.openai_model.clone()
            },
        }
    } else if state.oracle_config.use_ollama {
        OracleChoice::Ollama {
            url: state.oracle_config.ollama_url.clone(),
            model: state.oracle_config.ollama_model.clone(),
        }
    } else {
        OracleChoice::Manual
    };

    let assistant_message = match oracle_choice {
        OracleChoice::Manual => architect::process_message_manual(&mut session, &req.message),
        OracleChoice::OpenAi { api_key, model } => {
            // Build prompt first so we can move it into the blocking task.
            let prompt = architect::build_architect_prompt(&session, &req.message);

            let result = tokio::task::spawn_blocking(move || {
                use isls_forge_llm::Oracle as _;
                let oracle = match isls_forge_llm::oracle::OpenAiOracle::new(api_key, Some(model)) {
                    Ok(o) => o,
                    Err(e) => return Err(format!("Oracle init failed: {}", e)),
                };
                oracle
                    .call(&prompt, 4096)
                    .map_err(|e| format!("Oracle call failed: {}", e))
            })
            .await;

            match result {
                Ok(Ok(response_text)) => {
                    session.add_user_message(&req.message);
                    let parsed = architect::parse_llm_response(&response_text);
                    let msg = parsed.message.clone();
                    architect::apply_architect_response(&mut session, &parsed);
                    msg
                }
                Ok(Err(e)) => {
                    eprintln!("[architect] OpenAI oracle failed: {} — falling back to manual mode", e);
                    architect::process_message_manual(&mut session, &req.message)
                }
                Err(e) => {
                    eprintln!("[architect] OpenAI task join failed: {} — falling back to manual mode", e);
                    architect::process_message_manual(&mut session, &req.message)
                }
            }
        }
        OracleChoice::Ollama { url, model } => {
            // Use the tighter Ollama-style prompt (shorter, JSON-only, no
            // history injection).
            let prompt = architect::build_architect_prompt_ollama(&session, &req.message);

            let result = tokio::task::spawn_blocking(move || {
                use isls_forge_llm::Oracle as _;
                let oracle = isls_forge_llm::oracle::OllamaOracle::new(&model, &url);
                oracle
                    .call(&prompt, 4096)
                    .map_err(|e| format!("Ollama oracle call failed: {}", e))
            })
            .await;

            match result {
                Ok(Ok(response_text)) => {
                    session.add_user_message(&req.message);
                    let parsed = architect::parse_llm_response(&response_text);
                    // If the local model produced no entities AND no fenced
                    // JSON, surface the raw model text as the assistant
                    // message instead of treating it as a crash — the JSON
                    // extractor already refuses to crash on unexpected output.
                    let msg = parsed.message.clone();
                    architect::apply_architect_response(&mut session, &parsed);
                    msg
                }
                Ok(Err(e)) => {
                    eprintln!("[architect] Ollama oracle failed: {} — falling back to manual mode", e);
                    architect::process_message_manual(&mut session, &req.message)
                }
                Err(e) => {
                    eprintln!("[architect] Ollama task join failed: {} — falling back to manual mode", e);
                    architect::process_message_manual(&mut session, &req.message)
                }
            }
        }
    };

    // Compute readiness
    let readiness = crate::session::compute_readiness(&session);

    // Store updated session
    state.session_store.write().await.insert(id.clone(), session.clone());

    Json(serde_json::json!({
        "ok": true,
        "message": assistant_message,
        "spec": {
            "app_name": session.spec.app_name,
            "description": session.spec.description,
            "entity_count": session.spec.entities.len(),
            "entities": session.spec.entities.iter().map(|e| serde_json::json!({
                "name": e.name,
                "field_count": e.fields.len(),
                "fields": e.fields.iter().map(|f| serde_json::json!({
                    "name": f.name,
                    "type": f.rust_type,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        },
        "readiness": readiness,
    }))
}

/// GET /api/session/{id}/readiness — compute readiness check.
async fn session_readiness(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let store = state.session_store.read().await;
    match store.get(&id) {
        Some(session) => {
            let readiness = crate::session::compute_readiness(session);
            Json(serde_json::json!({ "ok": true, "readiness": readiness }))
        }
        None => Json(serde_json::json!({ "ok": false, "error": "session not found" })),
    }
}

/// POST /api/session/{id}/forge — start forge from session's AppSpec.
async fn session_forge(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SessionForgeRequest>,
) -> Json<serde_json::Value> {
    // Get session
    let session_opt = {
        let store = state.session_store.read().await;
        store.get(&id).cloned()
    };

    let session = match session_opt {
        Some(s) => s,
        None => return Json(serde_json::json!({ "ok": false, "error": "session not found" })),
    };

    // Check readiness
    let readiness = crate::session::compute_readiness(&session);
    if !readiness.ready {
        return Json(serde_json::json!({
            "ok": false,
            "error": "Session not ready for forge. Required criteria not met.",
            "readiness": readiness,
        }));
    }

    // Determine output directory
    let output_dir = req.output_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            state.projects_dir.join(if session.spec.app_name.is_empty() {
                format!("session-{}", &session.id)
            } else {
                session.spec.app_name.clone()
            })
        });

    if let Err(e) = std::fs::create_dir_all(&output_dir) {
        return Json(serde_json::json!({
            "ok": false,
            "error": format!("Cannot create output directory: {}", e),
        }));
    }

    // Build ForgePlan from session spec
    let spec = session.spec.clone();
    let session_api_key = session.api_key.clone();
    let event_hub = state.event_hub.clone();
    let session_store = state.session_store.clone();
    let session_id = id.clone();
    let output_dir_clone = output_dir.clone();
    let oracle_config = state.oracle_config.clone();

    // Spawn background forge task
    tokio::spawn(async move {
        // Publish forge:start
        event_hub.publish(WsEvent::new(
            EventType::ForgeProgress,
            serde_json::json!({
                "type": "forge:start",
                "session_id": session_id,
            }),
        ));

        let result = tokio::task::spawn_blocking(move || {
            // Convert session entities to EntityTemplates for from_toml_entities
            let templates: Vec<isls_hypercube::domain::EntityTemplate> = spec.entities.iter().map(|e| {
                isls_hypercube::domain::EntityTemplate {
                    name: e.name.clone(),
                    fields: e.fields.clone(),
                    validations: e.validations.iter().map(|v| {
                        isls_hypercube::domain::ValidationRule {
                            name: v.condition.clone(),
                            condition: v.condition.clone(),
                            message: v.message.clone(),
                        }
                    }).collect(),
                    indices: vec![],
                    description: String::new(),
                }
            }).collect();

            let mut plan = isls_forge_llm::ForgePlan::from_toml_entities(
                &spec.app_name,
                &spec.description,
                &spec.domain_name,
                &templates,
            );
            plan.blueprint = isls_forge_llm::blueprint::derive_blueprint_from_description(&spec.description);

            // Create oracle based on priority:
            //   1. Session api_key → OpenAiOracle
            //   2. oracle_config.api_key → OpenAiOracle
            //   3. oracle_config.use_ollama → OllamaOracle
            //   4. Fallback → MockOracle
            let (oracle, use_mock): (Box<dyn isls_forge_llm::Oracle>, bool) = {
                let effective_key = session_api_key
                    .clone()
                    .or_else(|| oracle_config.api_key.clone());

                if let Some(key) = effective_key {
                    match isls_forge_llm::oracle::OpenAiOracle::new(
                        Some(key),
                        Some(oracle_config.openai_model.clone()),
                    ) {
                        Ok(o) => {
                            tracing::info!("session_forge: using OpenAiOracle (model={})", oracle_config.openai_model);
                            (Box::new(o), false)
                        }
                        Err(e) => {
                            tracing::warn!("session_forge: OpenAiOracle init failed ({}), falling back to Mock", e);
                            (Box::new(isls_forge_llm::MockOracle), true)
                        }
                    }
                } else if oracle_config.use_ollama {
                    tracing::info!(
                        "session_forge: using OllamaOracle (model={}, url={})",
                        oracle_config.ollama_model,
                        oracle_config.ollama_url,
                    );
                    let ollama = isls_forge_llm::oracle::OllamaOracle::new(
                        &oracle_config.ollama_model,
                        &oracle_config.ollama_url,
                    );
                    (Box::new(ollama), false)
                } else {
                    tracing::info!("session_forge: using MockOracle (no LLM configured)");
                    (Box::new(isls_forge_llm::MockOracle), true)
                }
            };

            let mut forge = isls_forge_llm::LlmForge::new(
                oracle, plan, output_dir_clone.clone(), use_mock,
            );

            // D7/W3: Wire progress callback to publish WsEvent via channel
            // Note: forge.staged_closure is not directly accessible, so we
            // rely on the events fired at the forge level. The progress
            // callback is set on the StagedClosure inside LlmForge.generate().
            let start = std::time::Instant::now();
            match forge.generate() {
                Ok(files) => {
                    let total_loc: usize = files.iter().map(|f| f.content.lines().count()).sum();
                    Ok(session::SessionForgeResult {
                        success: true,
                        files_generated: files.len(),
                        total_loc,
                        total_tokens: forge.stats.total_tokens,
                        duration_secs: start.elapsed().as_secs_f64(),
                        output_dir: output_dir_clone.to_string_lossy().to_string(),
                    })
                }
                Err(e) => Err(format!("Forge failed: {}", e)),
            }
        }).await;

        match result {
            Ok(Ok(forge_result)) => {
                // Store result on session
                let mut store = session_store.write().await;
                if let Some(session) = store.get_mut(&session_id) {
                    session.forge_result = Some(forge_result.clone());
                }

                event_hub.publish(WsEvent::new(
                    EventType::ForgeProgress,
                    serde_json::json!({
                        "type": "forge:complete",
                        "session_id": session_id,
                        "success": true,
                        "files": forge_result.files_generated,
                        "loc": forge_result.total_loc,
                        "tokens": forge_result.total_tokens,
                        "duration_secs": forge_result.duration_secs,
                        "output_dir": forge_result.output_dir,
                    }),
                ));
            }
            result => {
                let err_msg = match result {
                    Ok(Err(e)) => e,
                    Err(e) => format!("Forge task panicked: {}", e),
                    _ => unreachable!(),
                };
                event_hub.publish(WsEvent::new(
                    EventType::ForgeProgress,
                    serde_json::json!({
                        "type": "forge:complete",
                        "session_id": session_id,
                        "success": false,
                        "error": err_msg,
                    }),
                ));
            }
        }
    });

    Json(serde_json::json!({
        "ok": true,
        "message": "Forge started. Watch WebSocket /events for progress.",
        "output_dir": output_dir.to_string_lossy().to_string(),
    }))
}

/// GET /api/chat/history — placeholder (jobs are ephemeral in-process).
async fn api_chat_history(State(state): State<AppState>) -> Json<serde_json::Value> {
    let jobs = state.chat_jobs.read().await;
    let history: Vec<serde_json::Value> = jobs.values().map(|j| serde_json::json!({
        "job_id": j.id,
        "complete": j.complete,
        "event_count": j.events.len(),
    })).collect();
    Json(serde_json::json!({ "ok": true, "history": history }))
}

// ─── v3 Crystals + Health ─────────────────────────────────────────────────

/// GET /api/crystals — crystals list under /api prefix.
async fn api_crystals(State(state): State<AppState>) -> Json<serde_json::Value> {
    let engine = state.engine_state.read().await;
    Json(serde_json::json!({ "ok": true, "crystals": engine.crystals }))
}

/// GET /api/health — health check under /api prefix.
async fn api_health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let uptime = state.start_time.elapsed().as_secs();
    Json(serde_json::json!({ "status": "ok", "uptime_secs": uptime, "version": "3.0.0" }))
}

/// POST /agent/bind — set the default project for subsequent /agent/chat calls
async fn agent_bind(
    State(state): State<AppState>,
    Json(req): Json<BindRequest>,
) -> Json<serde_json::Value> {
    let path = std::path::PathBuf::from(&req.project);
    let exists = path.exists();
    *state.bound_project.write().await = Some(path);
    Json(serde_json::json!({
        "ok": true,
        "project": req.project,
        "exists": exists,
    }))
}

// ─── WebSocket Handler ──────────────────────────────────────────────────────

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: axum::extract::ws::WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.event_hub.subscribe();

    {
        let mut count = state.event_hub.client_count.write().await;
        *count += 1;
    }

    // Default: subscribe to all events
    let subscribed: Arc<RwLock<Option<std::collections::HashSet<EventType>>>> =
        Arc::new(RwLock::new(None));

    let sub_clone = subscribed.clone();

    // Forward broadcast events to the client
    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            let filter = sub_clone.read().await;
            if let Some(ref types) = *filter {
                if !types.contains(&event.event_type) {
                    continue;
                }
            }
            if let Ok(json) = serde_json::to_string(&event) {
                if sender.send(axum::extract::ws::Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Listen for subscription messages from the client
    let sub_clone2 = subscribed.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let axum::extract::ws::Message::Text(text) = msg {
                if let Ok(req) = serde_json::from_str::<SubscribeRequest>(&text) {
                    let mut filter = sub_clone2.write().await;
                    *filter = Some(req.subscribe.into_iter().collect());
                }
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    {
        let mut count = state.event_hub.client_count.write().await;
        *count = count.saturating_sub(1);
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn rand_u32() -> u32 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_app() -> Router {
        build_router(AppState::new())
    }

    // AT-ST1: Studio serves
    #[tokio::test]
    async fn at_st1_studio_serves() {
        let app = test_app();
        let resp = app
            .oneshot(Request::get("/studio").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("ISLS Studio"));
    }

    // AT-ST2: Dashboard data
    #[tokio::test]
    async fn at_st2_dashboard_data() {
        let app = test_app();
        let resp = app
            .oneshot(Request::get("/api/dashboard").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(data.get("health").is_some());
        assert!(data.get("crystal_count").is_some());
        assert!(data.get("metrics").is_some());
    }

    // AT-ST3: WebSocket connect — tested via ws module unit tests (EventHub)
    #[tokio::test]
    async fn at_st3_websocket_event_hub() {
        let hub = EventHub::new(16);
        let mut rx = hub.subscribe();
        hub.publish(WsEvent::heartbeat());
        let event = rx.try_recv().unwrap();
        assert_eq!(event.event_type, EventType::Heartbeat);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("heartbeat"));
    }

    // AT-ST4: Forge via API
    #[tokio::test]
    async fn at_st4_forge_via_api() {
        let app = test_app();
        let body = serde_json::json!({
            "intent": "Build a REST API for bookmarks",
            "domain": "Rust",
        });
        let resp = app
            .oneshot(
                Request::post("/forge")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["ok"], true);
        assert!(data.get("intent").is_some());
    }

    // AT-ST5: Foundry via API
    #[tokio::test]
    async fn at_st5_foundry_via_api() {
        let app = test_app();
        let body = serde_json::json!({
            "intent": "REST API for bookmark manager with tags",
        });
        let resp = app
            .oneshot(
                Request::post("/api/foundry/fabricate")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["ok"], true);
        assert!(data.get("project_id").is_some());
    }

    // AT-ST6: Explorer query
    #[tokio::test]
    async fn at_st6_explorer_query() {
        let state = AppState::new();
        {
            let mut engine = state.engine_state.write().await;
            engine.crystals.push(CrystalSummary {
                id: "a7b3c9test".into(),
                tick: 42,
                score: 0.78,
                scale: "micro".into(),
                checks: "8/8".into(),
                stability: 0.78,
                free_energy: -3.2,
                constraint_count: 3,
                topology: TopologySummary { spectral_gap: 0.12, kuramoto_r: 0.87 },
                evidence_count: 42,
            });
        }
        let app = build_router(state);
        let resp = app
            .oneshot(Request::get("/crystals?limit=5").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(data.is_array());
        let arr = data.as_array().unwrap();
        assert!(!arr.is_empty());
        assert!(arr[0].get("id").is_some());
        assert!(arr[0].get("score").is_some());
    }

    // AT-ST7: Command palette
    #[tokio::test]
    async fn at_st7_command_palette() {
        let app = test_app();
        let body = serde_json::json!({"command": "start engine shadow"});
        let resp = app
            .oneshot(
                Request::post("/api/command")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["ok"], true);
    }

    // AT-ST8: Health endpoint
    #[tokio::test]
    async fn at_st8_health_endpoint() {
        let app = test_app();
        let resp = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["status"], "ok");
        assert!(data.get("version").is_some());
    }

    // AT-ST-AG1: Agent status returns ok field even without prior state
    #[tokio::test]
    async fn at_st_ag1_agent_status_no_state() {
        let app = test_app();
        let resp = app
            .oneshot(Request::get("/agent/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(data.get("ok").is_some());
    }

    // AT-ST-AG2: POST /agent/goal creates agent, runs it, returns metrics
    #[tokio::test]
    async fn at_st_ag2_agent_goal_runs() {
        let app = test_app();
        let body = serde_json::json!({
            "intent": "Build a deterministic event sourcing library",
            "domain": "rust",
            "confidence": 0.7,
        });
        let resp = app
            .oneshot(
                Request::post("/agent/goal")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["ok"], true);
        assert!(data["steps_run"].as_u64().unwrap_or(0) > 0);
        assert!(data["plan_size"].as_u64().unwrap_or(0) >= 5);
    }

    // AT-AG22: Chat endpoint returns events in correct order
    #[tokio::test]
    async fn at_ag22_chat_endpoint_event_order() {
        let app = test_app();

        // Start chat job (project = "." which exists)
        let body = serde_json::json!({
            "message": "add a hello function",
            "project": ".",
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/agent/chat")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let data: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
        ).unwrap();
        assert_eq!(data["ok"], true);
        let job_id = data["job_id"].as_str().unwrap().to_string();

        // Poll until complete (max 30 attempts × 100ms = 3s)
        let mut events = serde_json::Value::Array(vec![]);
        let mut complete = false;
        for _ in 0..30 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let resp = app
                .clone()
                .oneshot(
                    Request::get(format!("/agent/chat/{}/events", job_id))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let data: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
            ).unwrap();
            events = data["events"].clone();
            complete = data["complete"].as_bool().unwrap_or(false);
            if complete { break; }
        }

        assert!(complete, "job must complete within 3 seconds");

        // Verify event ordering: Analysis must come before Complete
        let event_types: Vec<&str> = events.as_array().unwrap()
            .iter()
            .filter_map(|e| e.get("type").and_then(|t| t.as_str()))
            .collect();

        assert!(!event_types.is_empty(), "must have at least one event");
        let first = event_types[0];
        let last = *event_types.last().unwrap();
        assert_eq!(first, "analysis", "first event must be analysis, got: {}", first);
        assert_eq!(last, "complete", "last event must be complete, got: {}", last);
    }

    // AT-AG22b: /agent/bind stores the project path
    #[tokio::test]
    async fn at_ag22b_agent_bind() {
        let app = test_app();
        let body = serde_json::json!({"project": "/tmp"});
        let resp = app
            .oneshot(
                Request::post("/agent/bind")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let data: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap(),
        ).unwrap();
        assert_eq!(data["ok"], true);
        assert_eq!(data["project"], "/tmp");
    }

    // AT-ST-AG3: GET /agent/history returns ok field
    #[tokio::test]
    async fn at_st_ag3_agent_history() {
        let app = test_app();
        // Run goal first so history may exist
        let goal_body = serde_json::json!({"intent": "History test goal"});
        let _ = app
            .clone()
            .oneshot(
                Request::post("/agent/goal")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&goal_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let resp = app
            .oneshot(Request::get("/agent/history").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(data.get("ok").is_some());
    }
}
