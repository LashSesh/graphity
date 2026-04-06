// isls-gateway/src/targets.rs — MC1: Mission Control API handlers
//
// CRUD for target systems, coverage computation, convergence data,
// auto-steer state, and forge trigger per target.

use axum::{
    extract::{Json, Path, Query, State},
    response::Json as AxumJson,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::Ordering;

use crate::AppState;

// ─── Request types ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateTargetRequest {
    pub name: String,
    pub description: String,
    pub priority: Option<u8>,
}

#[derive(Deserialize)]
pub struct UpdateTargetRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub priority: Option<u8>,
    pub required_classes: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct AutoSteerRequest {
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct ConvergenceQuery {
    pub hours: Option<u64>,
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn rand_id() -> String {
    format!("target-{:08x}", {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut h);
        std::thread::current().id().hash(&mut h);
        (h.finish() & 0xFFFF_FFFF) as u32
    })
}

fn parse_resonite_class(s: &str) -> isls_norms::spectroscopy::ResoniteClass {
    use isls_norms::spectroscopy::ResoniteClass;
    match s {
        "CrudEntity" => ResoniteClass::CrudEntity,
        "Authentication" => ResoniteClass::Authentication,
        "Authorization" => ResoniteClass::Authorization,
        "Pagination" => ResoniteClass::Pagination,
        "Search" => ResoniteClass::Search,
        "FileUpload" => ResoniteClass::FileUpload,
        "Notification" => ResoniteClass::Notification,
        "StateMachine" => ResoniteClass::StateMachine,
        "Workflow" => ResoniteClass::Workflow,
        "Caching" => ResoniteClass::Caching,
        "RateLimiting" => ResoniteClass::RateLimiting,
        "EventBus" => ResoniteClass::EventBus,
        "RealtimeWebSocket" => ResoniteClass::RealtimeWebSocket,
        "GraphQLApi" => ResoniteClass::GraphQLApi,
        "ExportImport" => ResoniteClass::ExportImport,
        "Scheduling" => ResoniteClass::Scheduling,
        "HealthCheck" => ResoniteClass::HealthCheck,
        "Logging" => ResoniteClass::Logging,
        "Metrics" => ResoniteClass::Metrics,
        "Docker" => ResoniteClass::Docker,
        "Migration" => ResoniteClass::Migration,
        "Configuration" => ResoniteClass::Configuration,
        "DataVisualization" => ResoniteClass::DataVisualization,
        "MessageQueue" => ResoniteClass::MessageQueue,
        other => ResoniteClass::Custom(other.to_string()),
    }
}

/// Recompute coverage for all targets against current norms.
fn refresh_coverages(targets: &mut Vec<isls_norms::targets::TargetSystem>, state: &AppState) {
    let registry = state.norm_registry.try_read();
    let norms: Vec<isls_norms::Norm> = match &registry {
        Ok(reg) => reg.all_norms().into_iter().cloned().collect(),
        Err(_) => return,
    };
    let fitness_store = isls_norms::fitness::FitnessStore::load();
    let fitness: HashMap<String, f64> = norms.iter()
        .map(|n| (n.id.clone(), fitness_store.get_fitness(&n.id)))
        .collect();

    for target in targets.iter_mut() {
        isls_norms::targets::compute_target_coverage(target, &norms, &fitness);
    }
}

// ─── Handlers ───────────────────────────────────────────────────────────────

/// GET /api/targets — list all targets with live coverage.
pub async fn api_list_targets(
    State(state): State<AppState>,
) -> AxumJson<serde_json::Value> {
    let mut targets = state.targets.read().await.clone();
    // Refresh coverages synchronously (cheap: keyword matching only)
    {
        let registry = state.norm_registry.try_read();
        if let Ok(reg) = &registry {
            let norms: Vec<isls_norms::Norm> = reg.all_norms().into_iter().cloned().collect();
            let fitness_store = isls_norms::fitness::FitnessStore::load();
            let fitness: HashMap<String, f64> = norms.iter()
                .map(|n| (n.id.clone(), fitness_store.get_fitness(&n.id)))
                .collect();
            for target in targets.iter_mut() {
                isls_norms::targets::compute_target_coverage(target, &norms, &fitness);
            }
        }
    }
    AxumJson(serde_json::json!({ "ok": true, "targets": targets }))
}

/// POST /api/targets — create a new target system.
pub async fn api_create_target(
    State(state): State<AppState>,
    Json(req): Json<CreateTargetRequest>,
) -> AxumJson<serde_json::Value> {
    let mut targets = state.targets.write().await;
    if targets.len() >= isls_norms::targets::MAX_TARGETS {
        return AxumJson(serde_json::json!({
            "ok": false, "error": format!("Maximum {} targets reached", isls_norms::targets::MAX_TARGETS)
        }));
    }

    let required_classes = isls_norms::targets::extract_requirements_from_description(&req.description);
    let id = rand_id();
    let mut target = isls_norms::targets::TargetSystem {
        id: id.clone(),
        name: req.name,
        description: req.description,
        required_classes,
        priority: req.priority.unwrap_or(3).min(5).max(1),
        coverage: 0.0,
        missing: vec![],
        status: isls_norms::targets::TargetStatus::NotReady,
    };

    // Compute initial coverage
    {
        let registry = state.norm_registry.try_read();
        if let Ok(reg) = &registry {
            let norms: Vec<isls_norms::Norm> = reg.all_norms().into_iter().cloned().collect();
            let fitness_store = isls_norms::fitness::FitnessStore::load();
            let fitness: HashMap<String, f64> = norms.iter()
                .map(|n| (n.id.clone(), fitness_store.get_fitness(&n.id)))
                .collect();
            isls_norms::targets::compute_target_coverage(&mut target, &norms, &fitness);
        }
    }

    targets.push(target.clone());
    let _ = isls_norms::targets::save_targets(&targets);

    AxumJson(serde_json::json!({ "ok": true, "target": target }))
}

/// PUT /api/targets/:id — update a target.
pub async fn api_update_target(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateTargetRequest>,
) -> AxumJson<serde_json::Value> {
    let mut targets = state.targets.write().await;
    let target = match targets.iter_mut().find(|t| t.id == id) {
        Some(t) => t,
        None => return AxumJson(serde_json::json!({ "ok": false, "error": "target not found" })),
    };

    if let Some(name) = req.name {
        target.name = name;
    }
    if let Some(desc) = req.description {
        target.description = desc.clone();
        // Re-extract requirements from new description
        target.required_classes = isls_norms::targets::extract_requirements_from_description(&desc);
    }
    if let Some(prio) = req.priority {
        target.priority = prio.min(5).max(1);
    }
    if let Some(classes) = req.required_classes {
        target.required_classes = classes.iter().map(|s| parse_resonite_class(s)).collect();
    }

    // Recompute coverage
    {
        let registry = state.norm_registry.try_read();
        if let Ok(reg) = &registry {
            let norms: Vec<isls_norms::Norm> = reg.all_norms().into_iter().cloned().collect();
            let fitness_store = isls_norms::fitness::FitnessStore::load();
            let fitness: HashMap<String, f64> = norms.iter()
                .map(|n| (n.id.clone(), fitness_store.get_fitness(&n.id)))
                .collect();
            isls_norms::targets::compute_target_coverage(target, &norms, &fitness);
        }
    }

    let t = target.clone();
    let _ = isls_norms::targets::save_targets(&targets);
    AxumJson(serde_json::json!({ "ok": true, "target": t }))
}

/// DELETE /api/targets/:id
pub async fn api_delete_target(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AxumJson<serde_json::Value> {
    let mut targets = state.targets.write().await;
    let before = targets.len();
    targets.retain(|t| t.id != id);
    if targets.len() == before {
        return AxumJson(serde_json::json!({ "ok": false, "error": "target not found" }));
    }
    let _ = isls_norms::targets::save_targets(&targets);
    AxumJson(serde_json::json!({ "ok": true }))
}

/// GET /api/targets/:id/coverage
pub async fn api_target_coverage(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AxumJson<serde_json::Value> {
    let mut targets = state.targets.write().await;
    refresh_coverages(&mut targets, &state);
    match targets.iter().find(|t| t.id == id) {
        Some(t) => AxumJson(serde_json::json!({
            "ok": true,
            "coverage": t.coverage,
            "missing": t.missing,
            "status": t.status,
        })),
        None => AxumJson(serde_json::json!({ "ok": false, "error": "target not found" })),
    }
}

/// POST /api/targets/:id/forge — start a forge run with target description.
pub async fn api_target_forge(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AxumJson<serde_json::Value> {
    let targets = state.targets.read().await;
    let target = match targets.iter().find(|t| t.id == id) {
        Some(t) => t.clone(),
        None => return AxumJson(serde_json::json!({ "ok": false, "error": "target not found" })),
    };

    if target.coverage < 0.5 {
        return AxumJson(serde_json::json!({
            "ok": false,
            "error": format!(
                "Coverage too low ({:.0}%). Missing: {}",
                target.coverage * 100.0,
                target.missing.iter().map(|r| r.as_str()).collect::<Vec<_>>().join(", ")
            )
        }));
    }

    // Create a session with the target description and trigger forge
    // For now, return the description and instructions for manual forge
    AxumJson(serde_json::json!({
        "ok": true,
        "target_id": target.id,
        "name": target.name,
        "description": target.description,
        "coverage": target.coverage,
        "message": "Use Erschaffen mode with this description to forge the target system."
    }))
}

/// GET /api/targets/convergence?hours=24
pub async fn api_targets_convergence(
    State(_state): State<AppState>,
    Query(q): Query<ConvergenceQuery>,
) -> AxumJson<serde_json::Value> {
    let hours = q.hours.unwrap_or(24).min(720);
    let entries = crate::timeseries::read_timeseries_entries(hours);

    let mut convergence: Vec<serde_json::Value> = Vec::new();
    for entry in &entries {
        if let Some(coverages) = &entry.target_coverages {
            convergence.push(serde_json::json!({
                "timestamp": entry.timestamp,
                "coverages": coverages,
            }));
        }
    }

    AxumJson(serde_json::json!({ "ok": true, "convergence": convergence, "count": convergence.len() }))
}

/// POST /api/targets/auto-steer — toggle auto-steer.
pub async fn api_set_auto_steer(
    State(state): State<AppState>,
    Json(req): Json<AutoSteerRequest>,
) -> AxumJson<serde_json::Value> {
    state.auto_steer_enabled.store(req.enabled, Ordering::Relaxed);
    AxumJson(serde_json::json!({ "ok": true, "enabled": req.enabled }))
}

/// GET /api/targets/auto-steer — get auto-steer state.
pub async fn api_get_auto_steer(
    State(state): State<AppState>,
) -> AxumJson<serde_json::Value> {
    let enabled = state.auto_steer_enabled.load(Ordering::Relaxed);
    let targets = state.targets.read().await;
    let deficit = isls_norms::targets::highest_priority_deficit(&targets);

    let (current_target, current_class) = match deficit {
        Some(t) => (
            Some(t.name.clone()),
            t.missing.first().map(|r| r.as_str()),
        ),
        None => (None, None),
    };

    AxumJson(serde_json::json!({
        "ok": true,
        "enabled": enabled,
        "current_target": current_target,
        "current_class": current_class,
    }))
}
