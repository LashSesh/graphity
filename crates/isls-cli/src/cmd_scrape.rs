// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! D5: Repository scraping — local, git, and manifest batch modes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use isls_reader::{self, WorkspaceAnalysis};
use isls_code_topo::{compute_code_topology, CodeTopology, bridge};
use isls_norms::NormRegistry;

// ─── Scrape Options ──────────────────────────────────────────────────────────

pub struct ScrapeOpts {
    pub path: Option<String>,
    pub url: Option<String>,
    pub manifest: Option<String>,
    pub domain: Option<String>,
    pub max_size_mb: u64,
    pub timeout_secs: u64,
}

// ─── TempCloneGuard (RAII) ───────────────────────────────────────────────────

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

// ─── Scrape Report ───────────────────────────────────────────────────────────

struct ScrapeReport {
    source: String,
    domain: String,
    language_breakdown: BTreeMap<String, usize>,
    files_parsed: usize,
    artifact_count: usize,
    struct_count: usize,
    fn_count: usize,
    table_count: usize,
    skipped: usize,
    layer_counts: BTreeMap<String, usize>,
    topology: CodeTopology,
    candidates_before: usize,
    candidates_after: usize,
    new_candidates: usize,
    promoted: usize,
}

impl ScrapeReport {
    fn print(&self) {
        println!("ISLS Scrape Report");
        println!("---");
        println!("Source:         {}", self.source);
        println!("Domain:         {}", self.domain);

        let langs: Vec<String> = self.language_breakdown.iter()
            .map(|(k, v)| format!("{} ({} LOC)", k, v))
            .collect();
        println!("Languages:      {}", langs.join(", "));
        println!("Files parsed:   {}", self.files_parsed);
        println!("Artifacts:      {} ({} struct, {} fn, {} table)",
            self.artifact_count, self.struct_count, self.fn_count, self.table_count);
        println!("Skipped:        {} (unmapped directories)", self.skipped);

        let layers: Vec<String> = self.layer_counts.iter()
            .map(|(k, v)| format!("{}({})", k, v))
            .collect();
        println!("Layers:         {}", layers.join(", "));

        println!("---");
        println!("Topology:");
        println!("  Nodes: {}, Edges: {}, Connectivity: {:.3}",
            self.topology.node_count, self.topology.edge_count, self.topology.connectivity);
        if !self.topology.struct_names.is_empty() {
            let structs: Vec<&str> = self.topology.struct_names.iter()
                .take(10).map(|s| s.as_str()).collect();
            let suffix = if self.topology.struct_names.len() > 10 { ", ..." } else { "" };
            println!("  Structs: {}{}", structs.join(", "), suffix);
        }
        if !self.topology.function_signatures.is_empty() {
            let fns: Vec<&str> = self.topology.function_signatures.iter()
                .take(10).map(|s| s.as_str()).collect();
            let suffix = if self.topology.function_signatures.len() > 10 { ", ..." } else { "" };
            println!("  Functions: {}{}", fns.join(", "), suffix);
        }

        println!("---");
        println!("Norm System:");
        println!("  Candidates updated: {}", self.candidates_after.saturating_sub(self.candidates_before) + self.new_candidates);
        println!("  New candidates: {}", self.new_candidates);
        println!("  Promoted: {}", self.promoted);
    }
}

// ─── Core Scraping Pipeline ──────────────────────────────────────────────────

fn scrape_directory(
    dir: &Path,
    domain: &str,
    source_label: &str,
) -> Result<ScrapeReport, String> {
    // 1. Parse directory
    let analysis: WorkspaceAnalysis = isls_reader::parse_directory(dir)
        .map_err(|e| format!("Parse error: {}", e))?;

    // 2. Convert to artifacts
    let (artifacts, skipped) = bridge::observations_from_code(&analysis.files);

    if artifacts.is_empty() {
        eprintln!("[WARN] No architectural patterns found. The project may use non-standard directory layout.");
    }

    // 3. Compute topology for report
    let topology = compute_code_topology(&analysis.files);

    // 4. Count by type and layer
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

    // 5. Generate run_id
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let run_id = format!("scrape_{}_{}", domain, timestamp);

    // 6. Load NormRegistry, observe, save
    let mut registry = NormRegistry::new();
    if let Err(e) = registry.load() {
        eprintln!("[WARN] Could not load norms.json: {}", e);
    }
    let candidates_before = registry.candidates().len();
    let norms_before = registry.all_norms().len();

    registry.observe_and_learn(&artifacts, domain, &run_id);

    let candidates_after = registry.candidates().len();
    let norms_after = registry.all_norms().len();

    if let Err(e) = registry.save() {
        eprintln!("[WARN] Could not save norms.json: {}", e);
    }

    Ok(ScrapeReport {
        source: source_label.to_string(),
        domain: domain.to_string(),
        language_breakdown: topology.language_breakdown.clone(),
        files_parsed: analysis.files.len(),
        artifact_count: artifacts.len(),
        struct_count,
        fn_count,
        table_count,
        skipped,
        layer_counts,
        topology,
        candidates_before,
        candidates_after,
        new_candidates: candidates_after.saturating_sub(candidates_before),
        promoted: norms_after.saturating_sub(norms_before),
    })
}

// ─── Git Cloning ─────────────────────────────────────────────────────────────

fn check_git_available() -> Result<(), String> {
    match std::process::Command::new("git").arg("--version").status() {
        Ok(s) if s.success() => Ok(()),
        _ => Err("git is required for URL scraping. Install git or use --path for local directories.".to_string()),
    }
}

fn dir_size_mb(path: &Path) -> u64 {
    fn walk(dir: &Path, total: &mut u64) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, total);
                } else if let Ok(meta) = p.metadata() {
                    *total += meta.len();
                }
            }
        }
    }
    let mut total = 0u64;
    walk(path, &mut total);
    total / (1024 * 1024)
}

fn git_clone_and_scrape(
    url: &str,
    domain: &str,
    max_size_mb: u64,
    timeout_secs: u64,
) -> Result<ScrapeReport, String> {
    check_git_available()?;

    // Create temp directory
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    };
    let temp_dir = std::env::temp_dir().join(format!("isls-scrape-{}", &hash[..8]));

    // Ensure clean state
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    let guard = TempCloneGuard { path: temp_dir.clone() };

    // Clone with timeout
    println!("[Clone] {} → {}", url, temp_dir.display());
    let child = std::process::Command::new("git")
        .args(["clone", "--depth", "1", "--single-branch", url])
        .arg(&temp_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start git clone: {}", e))?;

    let output = wait_with_timeout(child, Duration::from_secs(timeout_secs))
        .map_err(|e| format!("Clone failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {}", stderr.trim()));
    }

    // Check size
    let size = dir_size_mb(&temp_dir);
    if size > max_size_mb {
        return Err(format!(
            "Repository too large: {} MB (limit: {} MB). Use --max-size-mb to increase.",
            size, max_size_mb
        ));
    }
    println!("[Clone] Complete ({} MB)", size);

    let result = scrape_directory(&temp_dir, domain, url);

    // guard drops here → temp dir cleaned up
    drop(guard);

    result
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child.stdout.take().map(|mut s| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut s, &mut buf).ok();
                    buf
                }).unwrap_or_default();
                let stderr = child.stderr.take().map(|mut s| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut s, &mut buf).ok();
                    buf
                }).unwrap_or_default();
                return Ok(std::process::Output { status, stdout, stderr });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(format!("Clone timed out after {} seconds", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Error waiting for process: {}", e)),
        }
    }
}

// ─── Manifest Batch Scraping ─────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct Manifest {
    repo: Vec<ManifestEntry>,
}

#[derive(serde::Deserialize)]
struct ManifestEntry {
    url: Option<String>,
    path: Option<String>,
    domain: Option<String>,
    max_size_mb: Option<u64>,
}

struct BatchResult {
    domain: String,
    status: BatchStatus,
}

enum BatchStatus {
    Pass { language: String, loc: usize, artifacts: usize },
    Fail { reason: String },
}

fn scrape_manifest(
    manifest_path: &Path,
    default_max_size_mb: u64,
    timeout_secs: u64,
) -> Result<(), String> {
    let content = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("Cannot read manifest: {}", e))?;
    let manifest: Manifest = toml::from_str(&content)
        .map_err(|e| format!("Invalid manifest TOML: {}", e))?;

    let total = manifest.repo.len();
    println!("ISLS Batch Scrape Report");
    println!("---");
    println!("Manifest:  {} ({} repos)", manifest_path.display(), total);
    println!();

    let mut results: Vec<BatchResult> = Vec::new();
    let mut total_loc = 0usize;
    let mut total_artifacts = 0usize;
    let mut language_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut pass_count = 0usize;

    // Load registry once for candidate tracking
    let mut registry = NormRegistry::new();
    if let Err(e) = registry.load() {
        eprintln!("[WARN] Could not load norms.json: {}", e);
    }
    let candidates_before = registry.candidates().len();
    let norms_before = registry.all_norms().len();

    for (i, entry) in manifest.repo.iter().enumerate() {
        let domain = entry.domain.clone().unwrap_or_else(|| {
            if let Some(ref url) = entry.url {
                bridge::domain_from_url(url)
            } else if let Some(ref path) = entry.path {
                bridge::domain_from_path(Path::new(path))
            } else {
                format!("repo-{}", i)
            }
        });

        print!("  [{}/{}] Scraping {}...", i + 1, total, domain);

        let max_size = entry.max_size_mb.unwrap_or(default_max_size_mb);

        let result = if let Some(ref url) = entry.url {
            git_clone_and_scrape(url, &domain, max_size, timeout_secs)
        } else if let Some(ref path) = entry.path {
            let p = Path::new(path);
            if !p.is_dir() {
                Err(format!("Path not found or not a directory: {}", path))
            } else {
                scrape_directory(p, &domain, path)
            }
        } else {
            Err("Entry must have either 'url' or 'path'".to_string())
        };

        match result {
            Ok(report) => {
                let primary_lang = report.language_breakdown.iter()
                    .max_by_key(|(_, v)| *v)
                    .map(|(k, _)| k.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                let loc: usize = report.language_breakdown.values().sum();
                total_loc += loc;
                total_artifacts += report.artifact_count;
                for (lang, _) in &report.language_breakdown {
                    *language_counts.entry(lang.clone()).or_insert(0) += 1;
                }
                pass_count += 1;
                println!(" [PASS]");
                results.push(BatchResult {
                    domain: domain.clone(),
                    status: BatchStatus::Pass {
                        language: primary_lang,
                        loc,
                        artifacts: report.artifact_count,
                    },
                });
            }
            Err(reason) => {
                println!(" [FAIL]");
                results.push(BatchResult {
                    domain: domain.clone(),
                    status: BatchStatus::Fail { reason },
                });
            }
        }
    }

    // Reload registry to get final counts
    let mut registry_final = NormRegistry::new();
    let _ = registry_final.load();
    let candidates_after = registry_final.candidates().len();
    let norms_after = registry_final.all_norms().len();

    // Print batch summary
    println!();
    for r in &results {
        match &r.status {
            BatchStatus::Pass { language, loc, artifacts } => {
                println!("  [PASS] {:<20} {:<10} {} LOC   {} artifacts",
                    r.domain, language, loc, artifacts);
            }
            BatchStatus::Fail { reason } => {
                println!("  [FAIL] {:<20} {}", r.domain, reason);
            }
        }
    }

    println!();
    println!("Summary:");
    println!("  Scraped:     {}/{}", pass_count, total);
    println!("  Total LOC:   {}", total_loc);
    println!("  Total artifacts: {}", total_artifacts);
    let langs: Vec<String> = language_counts.iter()
        .map(|(k, v)| format!("{}({})", k, v))
        .collect();
    println!("  Languages:   {}", langs.join(", "));
    println!();
    println!("Norm System:");
    println!("  Candidates before: {}", candidates_before);
    println!("  Candidates after:  {}", candidates_after);
    println!("  New auto-norms:    {}", norms_after.saturating_sub(norms_before));

    Ok(())
}

// ─── Public Entry Point ──────────────────────────────────────────────────────

pub fn cmd_scrape(opts: ScrapeOpts) {
    if let Some(ref manifest_path) = opts.manifest {
        let path = Path::new(manifest_path);
        if !path.exists() {
            eprintln!("[ERROR] Manifest file not found: {}", manifest_path);
            std::process::exit(1);
        }
        match scrape_manifest(path, opts.max_size_mb, opts.timeout_secs) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("[ERROR] Manifest scrape failed: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    if let Some(ref url) = opts.url {
        let domain = opts.domain.clone()
            .unwrap_or_else(|| bridge::domain_from_url(url));
        println!("╔══════════════════════════════════════════════════════╗");
        println!("║     ISLS D5 — Repository Scraping (Git)               ║");
        println!("╚══════════════════════════════════════════════════════╝");
        println!();

        match git_clone_and_scrape(url, &domain, opts.max_size_mb, opts.timeout_secs) {
            Ok(report) => report.print(),
            Err(e) => {
                eprintln!("[ERROR] {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    if let Some(ref path_str) = opts.path {
        let path = Path::new(path_str);
        if !path.exists() {
            eprintln!("[ERROR] Path does not exist: {}", path_str);
            std::process::exit(1);
        }
        if !path.is_dir() {
            eprintln!("[ERROR] Path is not a directory: {}", path_str);
            eprintln!("Usage: isls scrape --path <directory>");
            std::process::exit(1);
        }

        let domain = opts.domain.clone()
            .unwrap_or_else(|| bridge::domain_from_path(path));

        println!("╔══════════════════════════════════════════════════════╗");
        println!("║     ISLS D5 — Repository Scraping (Local)             ║");
        println!("╚══════════════════════════════════════════════════════╝");
        println!();

        match scrape_directory(path, &domain, path_str) {
            Ok(report) => report.print(),
            Err(e) => {
                eprintln!("[ERROR] {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    eprintln!("[ERROR] One of --path, --url, or --manifest is required.");
    eprintln!("Usage:");
    eprintln!("  isls scrape --path ./some-repo");
    eprintln!("  isls scrape --url https://github.com/user/repo.git");
    eprintln!("  isls scrape --manifest repos.toml");
    std::process::exit(1);
}
