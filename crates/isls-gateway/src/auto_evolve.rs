// isls-gateway/src/auto_evolve.rs — I5/W4: Autonomer Evolve-Zyklus
//
// Optionaler Background-Task der alle 6 Stunden eine Standard-App generiert,
// das Ergebnis scrapt (Selbstbeobachtung) und dann die Ausgabe löscht.
//
// Standardmäßig DEAKTIVIERT. Aktivierung über --auto-evolve CLI-Flag oder
// POST /api/auto-evolve/toggle im Studio (Sensorium-Modus).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::{extract::State, response::Json};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::AppState;

// ─── History Entry ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoEvolveRun {
    pub timestamp: String,
    pub description: String,
    pub files_generated: usize,
    pub duration_secs: f64,
    pub success: bool,
    pub error: Option<String>,
}

pub type AutoEvolveStore = Arc<RwLock<AutoEvolveState>>;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AutoEvolveState {
    pub active: bool,
    pub history: Vec<AutoEvolveRun>,    // last 20 runs
    pub next_run_at: Option<String>,    // ISO 8601
}

pub fn new_auto_evolve_store() -> AutoEvolveStore {
    Arc::new(RwLock::new(AutoEvolveState::default()))
}

// ─── Description Pool ────────────────────────────────────────────────────────

const DESCRIPTIONS: &[&str] = &[
    "Pet shop with animals, owners, and appointments",
    "Hotel booking system with rooms, guests, and reservations",
    "Blog platform with posts, authors, comments, and tags",
    "Task manager with projects, tasks, and team members",
    "Inventory system with products, warehouses, and orders",
];

// ─── Background Task ─────────────────────────────────────────────────────────

pub async fn auto_evolve_cycle(state: AppState, enabled: Arc<AtomicBool>) {
    let cycle_secs = 6 * 3600u64; // 6 hours
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(cycle_secs));
    let mut rng_idx: usize = 0;

    loop {
        interval.tick().await;

        if !enabled.load(Ordering::Relaxed) {
            // Update next_run_at even when disabled to keep the UI accurate
            continue;
        }

        // Pick description round-robin
        let desc = DESCRIPTIONS[rng_idx % DESCRIPTIONS.len()];
        rng_idx = rng_idx.wrapping_add(1);

        tracing::info!("[AutoEvolve] Starting: {}", desc);

        let output = std::env::temp_dir().join(format!(
            "auto-evolve-{}",
            chrono::Utc::now().timestamp()
        ));
        let _ = std::fs::create_dir_all(&output);

        let start = std::time::Instant::now();
        let oracle_config = state.oracle_config.clone();
        let desc_owned = desc.to_string();
        let output_clone = output.clone();
        let norm_registry = state.norm_registry.clone();

        let result = tokio::task::spawn_blocking(move || {
            run_single_forge(&desc_owned, &output_clone, &oracle_config, &norm_registry)
        })
        .await;

        let duration_secs = start.elapsed().as_secs_f64();

        let run = match result {
            Ok(Ok(files)) => {
                tracing::info!(
                    "[AutoEvolve] Success: {} files, {:.0}s — \"{}\"",
                    files,
                    duration_secs,
                    desc
                );
                AutoEvolveRun {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    description: desc.to_string(),
                    files_generated: files,
                    duration_secs,
                    success: true,
                    error: None,
                }
            }
            Ok(Err(e)) => {
                tracing::warn!("[AutoEvolve] Failed: {} — \"{}\"", e, desc);
                AutoEvolveRun {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    description: desc.to_string(),
                    files_generated: 0,
                    duration_secs,
                    success: false,
                    error: Some(e),
                }
            }
            Err(e) => {
                tracing::warn!("[AutoEvolve] Task panic: {}", e);
                AutoEvolveRun {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    description: desc.to_string(),
                    files_generated: 0,
                    duration_secs,
                    success: false,
                    error: Some(format!("task panic: {}", e)),
                }
            }
        };

        // Store run in history
        {
            let mut store = state.auto_evolve.write().await;
            store.history.push(run);
            // Keep only last 20 runs
            let excess = store.history.len().saturating_sub(20);
            if excess > 0 {
                store.history.drain(0..excess);
            }
            // Update next run
            let next = chrono::Utc::now() + chrono::Duration::seconds(cycle_secs as i64);
            store.next_run_at = Some(next.to_rfc3339());
        }

        // Cleanup generated output (already scraped inside run_single_forge)
        let _ = std::fs::remove_dir_all(&output);
    }
}

/// Synchronous forge + self-observation.
/// Returns number of generated files on success or an error string.
fn run_single_forge(
    description: &str,
    output_dir: &std::path::Path,
    oracle_config: &crate::OracleConfig,
    norm_registry: &std::sync::Arc<tokio::sync::RwLock<isls_norms::NormRegistry>>,
) -> Result<usize, String> {
    use isls_hypercube::domain::DomainRegistry;

    // Build a minimal ForgePlan from the description
    let domain_registry = DomainRegistry::new();
    let plan = match domain_registry.detect(description) {
        Some(domain) => {
            isls_forge_llm::ForgePlan::from_domain(
                &slug_from(description),
                description,
                domain,
            )
        }
        None => isls_forge_llm::ForgePlan::warehouse_default(&slug_from(description)),
    };

    // Build oracle
    let (oracle, use_mock): (Box<dyn isls_forge_llm::Oracle>, bool) = {
        let effective_key = oracle_config.api_key.clone();
        if let Some(key) = effective_key {
            match isls_forge_llm::oracle::OpenAiOracle::new(
                Some(key),
                Some(oracle_config.openai_model.clone()),
            ) {
                Ok(o) => (Box::new(o), false),
                Err(_) => (Box::new(isls_forge_llm::MockOracle), true),
            }
        } else if oracle_config.use_ollama {
            let o = isls_forge_llm::oracle::OllamaOracle::new(
                &oracle_config.ollama_model,
                &oracle_config.ollama_url,
            );
            (Box::new(o), false)
        } else {
            (Box::new(isls_forge_llm::MockOracle), true)
        }
    };

    let mut forge = isls_forge_llm::LlmForge::new(oracle, plan, output_dir.to_path_buf(), use_mock);
    let files = forge.generate().map_err(|e| e.to_string())?;
    let file_count = files.len();

    // Self-observation: scrape generated artifacts into norm registry
    let collector = isls_forge_llm::artifact_collector::ArtifactCollector::new(output_dir);
    let observed = collector.collect();
    if !observed.is_empty() {
        let domain = "auto-evolve";
        let run_id = format!("auto-evolve-{}", chrono::Utc::now().timestamp());
        // Use a blocking read on the shared norm registry
        if let Ok(mut reg) = norm_registry.try_write() {
            reg.observe_and_learn(&observed, domain, &run_id);
            let _ = reg.save();
            tracing::info!(
                "[AutoEvolve] Self-observation: {} artifacts fed to norm learning",
                observed.len()
            );
        }
    }

    Ok(file_count)
}

fn slug_from(description: &str) -> String {
    description
        .to_lowercase()
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join("-")
}

// ─── API Handlers ─────────────────────────────────────────────────────────────

/// POST /api/auto-evolve/toggle — activate or deactivate Auto-Evolve.
pub async fn api_auto_evolve_toggle(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut store = state.auto_evolve.write().await;
    store.active = !store.active;
    state
        .auto_evolve_enabled
        .store(store.active, Ordering::Relaxed);
    let is_active = store.active;
    if is_active {
        let next = chrono::Utc::now() + chrono::Duration::hours(6);
        store.next_run_at = Some(next.to_rfc3339());
    } else {
        store.next_run_at = None;
    }
    Json(serde_json::json!({ "ok": true, "active": is_active }))
}

/// GET /api/auto-evolve/status — return current Auto-Evolve state + history.
pub async fn api_auto_evolve_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let store = state.auto_evolve.read().await;
    let active = state.auto_evolve_enabled.load(Ordering::Relaxed);

    let total = store.history.len();
    let pass = store.history.iter().filter(|r| r.success).count();
    let fail = total - pass;

    let last = store.history.last();
    let last_run = last.as_ref().map(|r| {
        serde_json::json!({
            "timestamp": r.timestamp,
            "description": r.description,
            "files": r.files_generated,
            "duration_secs": r.duration_secs,
            "success": r.success,
        })
    });

    Json(serde_json::json!({
        "ok": true,
        "active": active,
        "next_run_at": store.next_run_at,
        "history_total": total,
        "history_pass": pass,
        "history_fail": fail,
        "last_run": last_run,
        "history": store.history,
    }))
}
