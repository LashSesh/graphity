// isls-gateway/src/discover.rs — Entdecken-mode API handlers (S1c)
//
// GitHub search, X-ray topology, scrape into norms, mass-scrape
// background jobs, norm gap analysis, and norm genealogy.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path as AxumPath, Query, State};
use axum::response::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use isls_code_topo::{self, bridge, CodeTopology};
use isls_norms::NormRegistry;

use crate::ws::{EventType, WsEvent};
use crate::AppState;

// ═══════════════════════════════════════════════════════════════════
// Request / Response types
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub max_results: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct XrayRequest {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct ScrapeRequest {
    pub url: Option<String>,
    pub path: Option<String>,
    pub domain: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MassScrapeRequest {
    pub keywords: Vec<String>,
    pub results_per_keyword: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SimilarityQuery {
    pub domain: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════
// Mass-scrape job tracking
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize)]
pub struct MassScrapeJob {
    pub id: String,
    pub status: String,
    pub keywords_total: usize,
    pub keywords_done: usize,
    pub repos_scraped: usize,
    pub repos_failed: usize,
    pub new_candidates: usize,
    pub new_norms: usize,
    pub errors: Vec<String>,
}

pub type MassScrapeStore = Arc<RwLock<HashMap<String, MassScrapeJob>>>;

pub fn new_mass_scrape_store() -> MassScrapeStore {
    Arc::new(RwLock::new(HashMap::new()))
}

// ═══════════════════════════════════════════════════════════════════
// GitHub API client
// ═══════════════════════════════════════════════════════════════════

async fn github_search_repos(
    query: &str,
    max_results: usize,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page={}",
        urlencoded(query),
        max_results.min(30),
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "isls-gateway/1.0")
        .header("Accept", "application/vnd.github.v3+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {}", e))?;

    let status = resp.status();
    if status.as_u16() == 403 {
        return Err("GitHub API rate limit exceeded. Wait 60 seconds.".to_string());
    }
    if !status.is_success() {
        return Err(format!("GitHub API returned {}", status));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub response: {}", e))?;

    let repos: Vec<serde_json::Value> = body["items"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|item| {
            serde_json::json!({
                "full_name": item["full_name"],
                "description": item["description"],
                "html_url": item["html_url"],
                "clone_url": item["clone_url"],
                "stargazers_count": item["stargazers_count"],
                "language": item["language"],
                "updated_at": item["updated_at"],
            })
        })
        .collect();

    Ok(serde_json::json!({
        "total_count": body["total_count"],
        "repos": repos,
    }))
}

fn urlencoded(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "+".to_string(),
            c if c.is_ascii_alphanumeric() || "-_.~:".contains(c) => c.to_string(),
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════
// Git clone + analyze pipeline
// ═══════════════════════════════════════════════════════════════════

struct TempCloneGuard {
    path: PathBuf,
}

impl Drop for TempCloneGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn temp_clone_path(url: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir().join(format!("isls-discover-{:x}-{}", hasher.finish(), ts))
}

/// Clone a repo, parse it, compute topology. Optionally feed norms.
async fn clone_and_analyze(
    url: &str,
    domain: &str,
    registry: Option<Arc<RwLock<NormRegistry>>>,
) -> Result<serde_json::Value, String> {
    let clone_url = url.to_string();
    let domain_str = domain.to_string();

    tokio::task::spawn_blocking(move || {
        clone_and_analyze_sync(&clone_url, &domain_str, registry.is_some())
    })
    .await
    .map_err(|e| format!("Task panicked: {}", e))?
    .map(|(result, artifacts_for_norms)| {
        // If we have a registry + artifacts, feed norms on the async side
        // But since spawn_blocking already returned, we handle norms inline
        // Actually we handle norms inside the blocking task for simplicity
        result
    })
}

fn clone_and_analyze_sync(
    url: &str,
    domain: &str,
    feed_norms: bool,
) -> Result<(serde_json::Value, bool), String> {
    let temp_dir = temp_clone_path(url);
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
    let _guard = TempCloneGuard { path: temp_dir.clone() };

    // Shallow clone
    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", "--single-branch", url])
        .arg(&temp_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("git clone failed to start: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {}", stderr.trim()));
    }

    analyze_directory_sync(&temp_dir, domain, Some(url), feed_norms)
}

fn analyze_directory_sync(
    dir: &Path,
    domain: &str,
    source_url: Option<&str>,
    feed_norms: bool,
) -> Result<(serde_json::Value, bool), String> {
    // Parse
    let analysis = isls_reader::parse_directory(dir)
        .map_err(|e| format!("Parse error: {}", e))?;

    // Compute topology
    let topology = isls_code_topo::compute_code_topology(&analysis.files);

    // Bridge to artifacts
    let (artifacts, skipped) = bridge::observations_from_code(&analysis.files);

    let mut struct_count = 0usize;
    let mut fn_count = 0usize;
    let mut table_count = 0usize;
    let mut layer_counts: BTreeMap<String, usize> = BTreeMap::new();
    for a in &artifacts {
        match a.artifact_type.as_str() {
            "struct" => struct_count += 1,
            "fn" => fn_count += 1,
            "table" => table_count += 1,
            _ => {}
        }
        *layer_counts.entry(format!("{:?}", a.layer)).or_insert(0) += 1;
    }

    let mut norm_result = serde_json::json!(null);

    if feed_norms && !artifacts.is_empty() {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let run_id = format!("discover_{}_{}", domain, timestamp);

        let mut registry = NormRegistry::new();
        if let Err(e) = registry.load() {
            tracing::warn!("Could not load norms.json: {}", e);
        }
        let candidates_before = registry.candidates().len();
        let norms_before = registry.all_norms().len();

        registry.observe_and_learn(&artifacts, domain, &run_id);

        let candidates_after = registry.candidates().len();
        let norms_after = registry.all_norms().len();

        if let Err(e) = registry.save() {
            tracing::warn!("Could not save norms.json: {}", e);
        }

        norm_result = serde_json::json!({
            "new_candidates": candidates_after.saturating_sub(candidates_before),
            "new_norms": norms_after.saturating_sub(norms_before),
            "total_candidates": candidates_after,
            "total_norms": norms_after,
        });
    }

    let result = serde_json::json!({
        "source": source_url.unwrap_or("local"),
        "domain": domain,
        "files_parsed": analysis.files.len(),
        "total_loc": analysis.total_loc,
        "artifact_count": artifacts.len(),
        "struct_count": struct_count,
        "fn_count": fn_count,
        "table_count": table_count,
        "skipped": skipped,
        "layers": layer_counts,
        "topology": {
            "node_count": topology.node_count,
            "edge_count": topology.edge_count,
            "connectivity": topology.connectivity,
            "layers": topology.layers,
            "struct_names": topology.struct_names.iter().take(30).collect::<Vec<_>>(),
            "function_signatures": topology.function_signatures.iter().take(30).collect::<Vec<_>>(),
            "language_breakdown": topology.language_breakdown,
        },
        "norms": norm_result,
    });

    Ok((result, feed_norms))
}

// ═══════════════════════════════════════════════════════════════════
// Handlers
// ═══════════════════════════════════════════════════════════════════

/// POST /api/discover/search — GitHub keyword search
pub async fn discover_search(
    Json(req): Json<SearchRequest>,
) -> Json<serde_json::Value> {
    let max = req.max_results.unwrap_or(10).min(30);

    match github_search_repos(&req.query, max).await {
        Ok(data) => Json(serde_json::json!({ "ok": true, "data": data })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

/// POST /api/discover/xray — Read-only topology scan (no norm feed)
pub async fn discover_xray(
    Json(req): Json<XrayRequest>,
) -> Json<serde_json::Value> {
    let domain = bridge::domain_from_url(&req.url);

    match clone_and_analyze(&req.url, &domain, None).await {
        Ok(data) => Json(serde_json::json!({ "ok": true, "data": data })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

/// POST /api/discover/scrape — Scrape single repo into norms
pub async fn discover_scrape(
    State(state): State<AppState>,
    Json(req): Json<ScrapeRequest>,
) -> Json<serde_json::Value> {
    if req.url.is_none() && req.path.is_none() {
        return Json(serde_json::json!({ "ok": false, "error": "url or path required" }));
    }

    let domain = req.domain.clone().unwrap_or_else(|| {
        if let Some(ref u) = req.url {
            bridge::domain_from_url(u)
        } else if let Some(ref p) = req.path {
            bridge::domain_from_path(Path::new(p))
        } else {
            "unknown".to_string()
        }
    });

    if let Some(ref url) = req.url {
        match clone_and_analyze(url, &domain, Some(state.norm_registry.clone())).await {
            Ok(data) => Json(serde_json::json!({ "ok": true, "data": data })),
            Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
        }
    } else if let Some(ref path_str) = req.path {
        let p = PathBuf::from(path_str);
        if !p.is_dir() {
            return Json(serde_json::json!({ "ok": false, "error": "path is not a directory" }));
        }
        let domain_c = domain.clone();
        let result = tokio::task::spawn_blocking(move || {
            analyze_directory_sync(&p, &domain_c, None, true)
        })
        .await;

        match result {
            Ok(Ok((data, _))) => Json(serde_json::json!({ "ok": true, "data": data })),
            Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
            Err(e) => Json(serde_json::json!({ "ok": false, "error": format!("Task panicked: {}", e) })),
        }
    } else {
        Json(serde_json::json!({ "ok": false, "error": "url or path required" }))
    }
}

/// POST /api/discover/mass-scrape — Background mass-scrape job
pub async fn discover_mass_scrape(
    State(state): State<AppState>,
    Json(req): Json<MassScrapeRequest>,
) -> Json<serde_json::Value> {
    if req.keywords.is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "keywords list is empty" }));
    }

    let per_kw = req.results_per_keyword.unwrap_or(3).min(10);
    let job_id = format!("mscrape-{:08x}", rand_u32());
    let keywords = req.keywords.clone();

    let job = MassScrapeJob {
        id: job_id.clone(),
        status: "running".to_string(),
        keywords_total: keywords.len(),
        keywords_done: 0,
        repos_scraped: 0,
        repos_failed: 0,
        new_candidates: 0,
        new_norms: 0,
        errors: vec![],
    };

    state.mass_scrape_jobs.write().await.insert(job_id.clone(), job);

    let jobs = state.mass_scrape_jobs.clone();
    let event_hub = state.event_hub.clone();
    let job_id_ret = job_id.clone();

    tokio::spawn(async move {
        run_mass_scrape(jobs, event_hub, job_id, keywords, per_kw).await;
    });

    Json(serde_json::json!({
        "ok": true,
        "job_id": job_id_ret,
        "keywords": req.keywords.len(),
        "results_per_keyword": per_kw,
    }))
}

async fn run_mass_scrape(
    jobs: MassScrapeStore,
    event_hub: crate::ws::EventHub,
    job_id: String,
    keywords: Vec<String>,
    per_keyword: usize,
) {
    let total = keywords.len();

    for (ki, keyword) in keywords.iter().enumerate() {
        // Publish keyword event
        event_hub.publish(WsEvent::new(
            EventType::DiscoverKeyword,
            serde_json::json!({
                "type": "discover:keyword",
                "job_id": &job_id,
                "index": ki + 1,
                "total": total,
                "keyword": keyword,
            }),
        ));

        // GitHub search
        let repos = match github_search_repos(keyword, per_keyword).await {
            Ok(data) => {
                data["repos"].as_array().cloned().unwrap_or_default()
            }
            Err(e) => {
                let mut j = jobs.write().await;
                if let Some(job) = j.get_mut(&job_id) {
                    job.errors.push(format!("Search '{}': {}", keyword, e));
                }
                vec![]
            }
        };

        for repo in &repos {
            let clone_url = repo["clone_url"].as_str().unwrap_or("");
            let full_name = repo["full_name"].as_str().unwrap_or("unknown");

            if clone_url.is_empty() {
                continue;
            }

            let domain = bridge::domain_from_url(clone_url);
            let clone_url_owned = clone_url.to_string();
            let domain_owned = domain.clone();

            let result = tokio::task::spawn_blocking(move || {
                clone_and_analyze_sync(&clone_url_owned, &domain_owned, true)
            })
            .await;

            match result {
                Ok(Ok((data, _))) => {
                    let artifact_count = data["artifact_count"].as_u64().unwrap_or(0);
                    let new_cands = data["norms"]["new_candidates"].as_u64().unwrap_or(0);
                    let new_norms = data["norms"]["new_norms"].as_u64().unwrap_or(0);

                    event_hub.publish(WsEvent::new(
                        EventType::DiscoverRepo,
                        serde_json::json!({
                            "type": "discover:repo",
                            "job_id": &job_id,
                            "repo": full_name,
                            "artifacts": artifact_count,
                            "keyword": keyword,
                        }),
                    ));

                    let mut j = jobs.write().await;
                    if let Some(job) = j.get_mut(&job_id) {
                        job.repos_scraped += 1;
                        job.new_candidates += new_cands as usize;
                        job.new_norms += new_norms as usize;
                    }
                }
                _ => {
                    let mut j = jobs.write().await;
                    if let Some(job) = j.get_mut(&job_id) {
                        job.repos_failed += 1;
                        job.errors.push(format!("Scrape failed: {}", full_name));
                    }
                }
            }
        }

        // Update keyword progress
        {
            let mut j = jobs.write().await;
            if let Some(job) = j.get_mut(&job_id) {
                job.keywords_done = ki + 1;
            }
        }

        // Rate limit: 2s between GitHub API calls
        if ki + 1 < total {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    // Complete
    let final_job = {
        let mut j = jobs.write().await;
        if let Some(job) = j.get_mut(&job_id) {
            job.status = "complete".to_string();
            job.clone()
        } else {
            return;
        }
    };

    event_hub.publish(WsEvent::new(
        EventType::DiscoverComplete,
        serde_json::json!({
            "type": "discover:complete",
            "job_id": &job_id,
            "repos_scraped": final_job.repos_scraped,
            "repos_failed": final_job.repos_failed,
            "new_candidates": final_job.new_candidates,
            "new_norms": final_job.new_norms,
        }),
    ));
}

/// GET /api/discover/mass-scrape/{id}/status
pub async fn discover_mass_scrape_status(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Json<serde_json::Value> {
    let jobs = state.mass_scrape_jobs.read().await;
    match jobs.get(&id) {
        Some(job) => Json(serde_json::json!({ "ok": true, "job": job })),
        None => Json(serde_json::json!({ "ok": false, "error": "job not found" })),
    }
}

/// POST /api/discover/upload-keywords — Parse text body as keywords
pub async fn discover_upload_keywords(
    State(state): State<AppState>,
    body: String,
) -> Json<serde_json::Value> {
    let keywords: Vec<String> = body
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    if keywords.is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "no keywords found in body" }));
    }

    let req = MassScrapeRequest {
        keywords,
        results_per_keyword: Some(3),
    };

    discover_mass_scrape(State(state), Json(req)).await
}

/// GET /api/discover/gaps — Norm gap analysis
pub async fn discover_gaps(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let registry = state.norm_registry.read().await;
    let all_norms = registry.all_norms();
    let candidates = registry.candidates();

    // Collect all keywords from existing norms
    let mut covered_keywords: Vec<String> = Vec::new();
    for norm in &all_norms {
        covered_keywords.push(norm.name.to_lowercase());
        for trigger in &norm.triggers {
            covered_keywords.extend(trigger.keywords.iter().map(|k| k.to_lowercase()));
            covered_keywords.extend(trigger.concepts.iter().map(|c| c.to_lowercase()));
        }
    }
    let covered_str = covered_keywords.join(" ");

    // Common patterns to check coverage for
    let patterns = vec![
        ("Caching", "rust cache lru redis", vec!["cache", "lru", "redis", "caching"]),
        ("Background Jobs", "rust job queue worker", vec!["job", "queue", "worker", "background"]),
        ("WebSocket Realtime", "rust websocket realtime", vec!["websocket", "realtime", "ws"]),
        ("Rate Limiting", "rust rate limit tower", vec!["rate", "limit", "throttle"]),
        ("Circuit Breaker", "rust circuit breaker resilience", vec!["circuit", "breaker", "resilience"]),
        ("Retry Patterns", "rust retry backoff", vec!["retry", "backoff"]),
        ("Config Management", "rust config toml dotenv", vec!["config", "dotenv", "configuration"]),
        ("Logging/Tracing", "rust logging tracing structured", vec!["logging", "tracing", "structured"]),
        ("Metrics/Monitoring", "rust metrics prometheus export", vec!["metrics", "prometheus", "monitoring"]),
        ("GraphQL", "rust graphql async-graphql", vec!["graphql"]),
        ("gRPC", "rust grpc tonic protobuf", vec!["grpc", "tonic", "protobuf"]),
        ("File Upload", "rust file upload multipart", vec!["upload", "multipart"]),
        ("CSV/Export", "rust csv export report", vec!["csv", "export"]),
        ("Email", "rust email smtp lettre", vec!["email", "smtp"]),
        ("Scheduling/Cron", "rust scheduler cron job", vec!["scheduler", "cron"]),
        ("Full-text Search", "rust search fulltext tantivy", vec!["search", "fulltext", "tantivy"]),
        ("i18n", "rust i18n internationalization", vec!["i18n", "internationalization", "locale"]),
        ("Feature Flags", "rust feature flag toggle", vec!["feature", "flag", "toggle"]),
        ("Audit Logging", "rust audit log trail", vec!["audit", "trail"]),
        ("Health Check", "rust health check endpoint", vec!["health", "check"]),
    ];

    let mut gaps = Vec::new();
    let mut covered = Vec::new();

    for (area, suggested_query, check_keywords) in &patterns {
        let norm_count = check_keywords
            .iter()
            .filter(|kw| covered_str.contains(*kw))
            .count();

        if norm_count == 0 {
            gaps.push(serde_json::json!({
                "area": area,
                "norm_count": 0,
                "suggested_query": suggested_query,
            }));
        } else {
            covered.push(serde_json::json!({
                "area": area,
                "keyword_matches": norm_count,
            }));
        }
    }

    // Count norm types
    let builtin_count = all_norms.iter().filter(|n| n.evidence.builtin).count();
    let auto_count = all_norms.len() - builtin_count;

    Json(serde_json::json!({
        "ok": true,
        "total_norms": all_norms.len(),
        "builtin": builtin_count,
        "auto": auto_count,
        "candidates": candidates.len(),
        "gaps": gaps,
        "covered": covered,
    }))
}

/// GET /api/discover/genealogy/{norm_id} — Norm observation history
pub async fn discover_genealogy(
    State(state): State<AppState>,
    AxumPath(norm_id): AxumPath<String>,
) -> Json<serde_json::Value> {
    let registry = state.norm_registry.read().await;

    // Look up as a full norm first
    if let Some(norm) = registry.get(&norm_id) {
        let mut result = serde_json::json!({
            "ok": true,
            "id": norm.id,
            "name": norm.name,
            "level": format!("{:?}", norm.level),
            "evidence": {
                "builtin": norm.evidence.builtin,
                "usage_count": norm.evidence.usage_count,
                "domains_used": norm.evidence.domains_used,
                "signature": norm.evidence.signature,
            },
        });

        // For auto-norms, find matching candidate history
        if !norm.evidence.builtin {
            for cand in registry.candidates() {
                if cand.id == norm_id {
                    result["candidate"] = serde_json::json!({
                        "candidate_id": cand.id,
                        "observation_count": cand.observation_count,
                        "domains": cand.domains,
                        "consistency": cand.consistency,
                        "consistent_layers": cand.consistent_layers.iter()
                            .map(|l| format!("{:?}", l))
                            .collect::<Vec<_>>(),
                        "observations": cand.observations.iter().map(|o| serde_json::json!({
                            "entity": o.observed_on,
                            "domain": o.domain,
                            "run_id": o.run_id,
                            "layers": o.layers_present.iter()
                                .map(|l| format!("{:?}", l))
                                .collect::<Vec<_>>(),
                        })).collect::<Vec<serde_json::Value>>(),
                    });
                    break;
                }
            }
        }

        return Json(result);
    }

    // Look up as a candidate
    for cand in registry.candidates() {
        if cand.id == norm_id {
            return Json(serde_json::json!({
                "ok": true,
                "type": "candidate",
                "id": cand.id,
                "observation_count": cand.observation_count,
                "domains": cand.domains,
                "consistency": cand.consistency,
                "status": format!("{:?}", cand.status),
                "consistent_layers": cand.consistent_layers.iter()
                    .map(|l| format!("{:?}", l))
                    .collect::<Vec<_>>(),
                "observations": cand.observations.iter().map(|o| serde_json::json!({
                    "entity": o.observed_on,
                    "domain": o.domain,
                    "run_id": o.run_id,
                })).collect::<Vec<serde_json::Value>>(),
            }));
        }
    }

    Json(serde_json::json!({ "ok": false, "error": "norm or candidate not found" }))
}

/// GET /api/discover/similarity?domain=... — Similarity search
pub async fn discover_similarity(
    State(state): State<AppState>,
    Query(params): Query<SimilarityQuery>,
) -> Json<serde_json::Value> {
    // This is a placeholder — full similarity requires stored topologies
    // For now, return the norm catalog overview
    let registry = state.norm_registry.read().await;
    let norms: Vec<serde_json::Value> = registry
        .all_norms()
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id,
                "name": n.name,
                "level": format!("{:?}", n.level),
                "domains_used": n.evidence.domains_used,
            })
        })
        .collect();

    Json(serde_json::json!({
        "ok": true,
        "query_domain": params.domain,
        "norms": norms,
    }))
}

// ═══════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════

fn rand_u32() -> u32 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    (hasher.finish() & 0xFFFF_FFFF) as u32
}
