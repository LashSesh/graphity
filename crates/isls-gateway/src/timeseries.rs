// isls-gateway/src/timeseries.rs — I5/W1: Zeitreihen-Persistenz
//
// Periodically snapshots the ISLS system state into ~/.isls/timeseries.jsonl
// (append-only, one JSON object per line, 96 entries/day ≈ 10 MB/year).
//
// API: GET /api/timeseries?hours=24|168|720

use std::io::{BufRead, Write};
use std::path::PathBuf;

use axum::{
    extract::{Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};

use crate::AppState;

// ─── Data Types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeseriesEntry {
    pub timestamp: String, // ISO 8601

    // Norm-System
    pub norms_builtin: usize,
    pub norms_auto: usize,
    pub norms_injected: usize,
    pub norms_total: usize,
    pub candidates_total: usize,
    pub candidates_observing: usize,
    pub candidates_promoted: usize,
    pub observations_total: usize,
    pub domains_total: usize,

    // Fitness
    pub fitness_mean: f64,
    pub fitness_min: f64,
    pub fitness_max: f64,
    pub fitness_count: usize,

    // SGB (aus letzter Generierung, falls vorhanden)
    pub sgb: Option<f64>,
    pub last_compile_success: Option<bool>,
    pub last_codematrix_resonance: Option<f64>,

    // Scraping
    pub repos_scraped_total: usize,
    pub keywords_active: usize,
    pub keywords_suggested: usize,

    // Genom
    pub genes_count: usize,
    pub cross_language_norms: usize,

    // MC1: Target system coverages
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_coverages: Option<std::collections::HashMap<String, f64>>,
}

// ─── Persistence ─────────────────────────────────────────────────────────────

pub fn timeseries_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|h| PathBuf::from(h).join(".isls").join("timeseries.jsonl"))
}

pub fn append_timeseries_entry(entry: &TimeseriesEntry) -> std::io::Result<()> {
    let path = match timeseries_path() {
        Some(p) => p,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot determine home directory",
            ))
        }
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let json = serde_json::to_string(entry)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    writeln!(file, "{}", json)?;
    Ok(())
}

pub fn read_timeseries_entries(hours: u64) -> Vec<TimeseriesEntry> {
    let path = match timeseries_path() {
        Some(p) => p,
        None => return vec![],
    };
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };
    let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours as i64);
    let reader = std::io::BufReader::new(file);
    reader
        .lines()
        .filter_map(|line| line.ok())
        .filter_map(|line| serde_json::from_str::<TimeseriesEntry>(&line).ok())
        .filter(|entry| {
            chrono::DateTime::parse_from_rfc3339(&entry.timestamp)
                .map(|t| t > cutoff)
                .unwrap_or(false)
        })
        .collect()
}

// ─── Snapshot Collection ─────────────────────────────────────────────────────

pub fn collect_timeseries_snapshot(state: &AppState) -> TimeseriesEntry {
    use isls_norms::learning::CandidateStatus;
    use isls_norms::INJECT_PREFIX;

    // ── Norm registry (try_read to avoid blocking) ────────────────
    let (
        norms_builtin,
        norms_auto,
        norms_injected,
        norms_total,
        candidates_total,
        candidates_observing,
        candidates_promoted,
        observations_total,
        domains_total,
        cross_language_norms,
    ) = match state.norm_registry.try_read() {
        Ok(reg) => {
            let all = reg.all_norms();
            let auto = all
                .iter()
                .filter(|n| n.id.starts_with("ISLS-NORM-AUTO-"))
                .count();
            let injected = all
                .iter()
                .filter(|n| n.id.starts_with(INJECT_PREFIX))
                .count();
            let builtin = all.len().saturating_sub(auto + injected);
            let total = all.len();

            let cands = reg.candidates();
            let cands_total = cands.len();
            let cands_observing = cands
                .iter()
                .filter(|c| c.status == CandidateStatus::Observing)
                .count();
            let cands_promoted = cands
                .iter()
                .filter(|c| c.status == CandidateStatus::Promoted)
                .count();
            let obs_total: usize = cands.iter().map(|c| c.observation_count).sum();

            let mut domain_set = std::collections::HashSet::new();
            for n in all.iter() {
                for d in &n.evidence.domains_used {
                    domain_set.insert(d.as_str().to_string());
                }
            }
            let xl = cands.iter().filter(|c| c.cross_language).count();

            (
                builtin,
                auto,
                injected,
                total,
                cands_total,
                cands_observing,
                cands_promoted,
                obs_total,
                domain_set.len(),
                xl,
            )
        }
        Err(_) => (0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
    };

    // ── Fitness from metrics.jsonl ─────────────────────────────────
    let all_metrics = isls_forge_llm::metrics::load_metrics();
    let fitnesses: Vec<f64> = all_metrics
        .iter()
        .map(|m| m.codematrix_avg)
        .filter(|&f| f > 0.0)
        .collect();
    let fitness_count = fitnesses.len();
    let fitness_mean = if fitness_count > 0 {
        fitnesses.iter().sum::<f64>() / fitness_count as f64
    } else {
        0.0
    };
    let fitness_min = if fitness_count > 0 {
        fitnesses.iter().cloned().fold(f64::MAX, f64::min)
    } else {
        0.0
    };
    let fitness_max = if fitness_count > 0 {
        fitnesses.iter().cloned().fold(f64::MIN, f64::max)
    } else {
        0.0
    };

    // ── SGB from last generation metrics ──────────────────────────
    let last_m = all_metrics.last();
    let sgb = last_m.map(|m| {
        if m.file_count > 0 {
            m.structural_files as f64 / m.file_count as f64
        } else {
            0.0
        }
    });
    let last_compile_success = last_m.map(|m| m.compile_success);
    let last_codematrix_resonance = last_m.map(|m| m.codematrix_avg);

    // ── Keywords ──────────────────────────────────────────────────
    let keywords_active = count_keyword_lines(&state.scrape_keywords_path);
    let keywords_suggested = count_keyword_lines(&state.suggested_keywords_path);

    // ── Repos scraped total (from scrape history ring-buffer) ─────
    let repos_scraped_total = state
        .scrape_history
        .try_lock()
        .map(|h| h.iter().map(|e| e.repos.len()).sum())
        .unwrap_or(0);

    // ── Genes from genome ─────────────────────────────────────────
    let genes_count = {
        use isls_norms::genome::{compute_genome, load_metrics_lite, DEFAULT_MIN_COACTIVATION};
        let ml = load_metrics_lite();
        let genome = compute_genome(&ml, DEFAULT_MIN_COACTIVATION);
        genome.genes.len()
    };

    // ── MC1: Target coverages ────────────────────────────────────
    let target_coverages = {
        let targets = state.targets.try_read();
        match targets {
            Ok(tgts) if !tgts.is_empty() => {
                let mut coverages = std::collections::HashMap::new();
                // Recompute using norms from registry
                if let Ok(reg) = state.norm_registry.try_read() {
                    let norms: Vec<isls_norms::Norm> = reg.all_norms().into_iter().cloned().collect();
                    let fitness_store = isls_norms::fitness::FitnessStore::load();
                    let fitness: std::collections::HashMap<String, f64> = norms.iter()
                        .map(|n| (n.id.clone(), fitness_store.get_fitness(&n.id)))
                        .collect();
                    for t in tgts.iter() {
                        let mut t_clone = t.clone();
                        isls_norms::targets::compute_target_coverage(&mut t_clone, &norms, &fitness);
                        coverages.insert(t.id.clone(), t_clone.coverage);
                    }
                }
                Some(coverages)
            }
            _ => None,
        }
    };

    TimeseriesEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        norms_builtin,
        norms_auto,
        norms_injected,
        norms_total,
        candidates_total,
        candidates_observing,
        candidates_promoted,
        observations_total,
        domains_total,
        fitness_mean,
        fitness_min,
        fitness_max,
        fitness_count,
        sgb,
        last_compile_success,
        last_codematrix_resonance,
        repos_scraped_total,
        keywords_active,
        keywords_suggested,
        genes_count,
        cross_language_norms,
        target_coverages,
    }
}

fn count_keyword_lines(path: &std::path::Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
        .count()
}

// ─── Background Recorder Task ────────────────────────────────────────────────

pub async fn timeseries_recorder(state: AppState) {
    // Snapshot every 15 minutes (900 seconds).
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(900));
    loop {
        interval.tick().await;
        let entry = collect_timeseries_snapshot(&state);
        match append_timeseries_entry(&entry) {
            Ok(()) => tracing::debug!("[Timeseries] Snapshot written (norms={})", entry.norms_total),
            Err(e) => tracing::warn!("[Timeseries] Failed to write snapshot: {}", e),
        }
    }
}

// ─── API Handler ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TimeseriesQuery {
    pub hours: Option<u64>,
}

#[derive(Serialize)]
pub struct TimeseriesResponse {
    pub entries: Vec<TimeseriesEntry>,
    pub count: usize,
}

/// GET /api/timeseries?hours=24
pub async fn api_timeseries(
    Query(q): Query<TimeseriesQuery>,
    State(_state): State<AppState>,
) -> Json<TimeseriesResponse> {
    let hours = q.hours.unwrap_or(24).min(8760); // max 1 year
    let entries = read_timeseries_entries(hours);
    let count = entries.len();
    Json(TimeseriesResponse { entries, count })
}
