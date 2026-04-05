// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! I4/harpoon: Cascading targeted scrape — CLI front-end.
//!
//! `isls harpoon --seed <path_or_url> [--depth 3] [--repos-per-keyword 5]
//!               [--stop-at-coverage 0.9] [--domain <name>]`
//!
//! Multi-depth cascade:
//!   seed → extract keywords → scrape repos → new keywords → … (up to depth)
//!
//! Only `isls harpoon` cascades. Regular `isls scrape` writes auto-keywords to
//! `suggested_keywords.txt`; harpoon reads them for its next depth level.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use isls_code_topo::bridge;
use isls_norms::NormRegistry;

// ─── repos-per-keyword schedule by cascade depth ─────────────────────────────

const RPK_SCHEDULE: [usize; 3] = [5, 3, 2];
const MAX_REPOS: usize = 200;

// ─── Public entry point ───────────────────────────────────────────────────────

pub struct HarpoonOpts {
    pub seed: String,
    pub depth: usize,
    pub repos_per_keyword: usize,
    pub stop_at_coverage: f64,
    pub domain: Option<String>,
}

pub fn cmd_harpoon(opts: HarpoonOpts) {
    let depth = opts.depth.clamp(1, 3);
    let rpk0 = opts.repos_per_keyword.min(10);
    let stop_cov = opts.stop_at_coverage.clamp(0.0, 1.0);

    let domain = opts.domain.unwrap_or_else(|| {
        if opts.seed.starts_with("http") {
            bridge::domain_from_url(&opts.seed)
        } else {
            bridge::domain_from_path(Path::new(&opts.seed))
        }
    });

    println!("╔═══════════════════════════════════════════════════════╗");
    println!("║     ISLS I4 — Harpoon Cascading Targeted Scrape      ║");
    println!("╚═══════════════════════════════════════════════════════╝");
    println!();
    println!("[Harpoon] Seed:               {}", opts.seed);
    println!("[Harpoon] Max depth:          {}", depth);
    println!("[Harpoon] Repos/kw (d0):      {}", rpk0);
    println!("[Harpoon] Stop at coverage:   {:.0}%", stop_cov * 100.0);
    println!("[Harpoon] Domain:             {}", domain);
    println!();

    // ── Stage 0: analyse seed ────────────────────────────────────────────────
    println!("[Stage 0] Analysing seed…");
    let (seed_keywords, seed_stats) = match analyse_seed(&opts.seed, &domain) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[ERROR] Failed to analyse seed: {}", e);
            std::process::exit(1);
        }
    };
    println!(
        "[Stage 0] Seed: {} files, {} artifacts, {} auto-keywords",
        seed_stats.files, seed_stats.artifacts, seed_keywords.len()
    );
    if seed_keywords.is_empty() {
        println!("[Harpoon] No keywords could be extracted from the seed. Stopping.");
        return;
    }
    println!("[Stage 0] Keywords: {}", seed_keywords.join(", "));
    println!();

    let mut current_keywords = seed_keywords;
    let mut total_scraped: usize = 0;
    let mut total_failed: usize = 0;
    let mut all_keywords_used: BTreeSet<String> = BTreeSet::new();

    // ── Cascade loop ─────────────────────────────────────────────────────────
    for d in 0..depth {
        let rpk = if d == 0 { rpk0 } else if d < RPK_SCHEDULE.len() { RPK_SCHEDULE[d] } else { 1 };

        println!(
            "━━━ Depth {} / {} — {} keywords, {}/kw ━━━",
            d + 1,
            depth,
            current_keywords.len(),
            rpk
        );

        for kw in &current_keywords {
            all_keywords_used.insert(kw.clone());
        }

        let mut next_keywords: Vec<String> = Vec::new();

        for keyword in &current_keywords {
            if total_scraped >= MAX_REPOS {
                println!("[Harpoon] Reached max repos ({}). Stopping.", MAX_REPOS);
                break;
            }

            println!("[Search] GitHub: \"{}\" ({} repos)", keyword, rpk);
            let repos = match search_github(keyword, rpk) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[WARN] Search failed for '{}': {}", keyword, e);
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            for (clone_url, full_name) in &repos {
                if total_scraped >= MAX_REPOS {
                    break;
                }
                let repo_domain = bridge::domain_from_url(clone_url);
                print!("  [Scrape] {} … ", full_name);
                match scrape_url(clone_url, &repo_domain) {
                    Ok((auto_kws, stats)) => {
                        total_scraped += 1;
                        next_keywords.extend(auto_kws.iter().cloned());
                        println!(
                            "OK ({} files, {} artifacts, {} new-kw)",
                            stats.files, stats.artifacts, auto_kws.len()
                        );
                    }
                    Err(e) => {
                        total_failed += 1;
                        println!("FAIL ({})", e);
                    }
                }
            }

            // honour GitHub rate limit between searches
            std::thread::sleep(Duration::from_secs(2));
        }

        // Deduplicate new keywords; skip ones we already used
        next_keywords.sort();
        next_keywords.dedup();
        next_keywords.retain(|k| !all_keywords_used.contains(k));

        // Persist suggested keywords from this stage
        let suggested_path = suggested_keywords_path();
        append_keywords_to_file(&suggested_path, &next_keywords);

        // Compute coverage
        let coverage = compute_coverage();

        println!(
            "[Depth {}] scraped={}, failed={}, suggested_kw={}, coverage={:.1}%",
            d + 1,
            total_scraped,
            total_failed,
            next_keywords.len(),
            coverage * 100.0
        );

        if coverage >= stop_cov {
            println!(
                "[Harpoon] Coverage {:.1}% >= threshold {:.1}%. Stopping.",
                coverage * 100.0,
                stop_cov * 100.0
            );
            break;
        }
        if next_keywords.is_empty() || d + 1 >= depth {
            break;
        }

        current_keywords = next_keywords;
        println!();
    }

    println!();
    println!("╔═══════════════════════════════════════════════════════╗");
    println!("║     Harpoon Complete                                  ║");
    println!("╚═══════════════════════════════════════════════════════╝");
    println!("Repos scraped:    {}", total_scraped);
    println!("Repos failed:     {}", total_failed);
    println!("Keywords used:    {}", all_keywords_used.len());
    println!("Final coverage:   {:.1}%", compute_coverage() * 100.0);
    println!();
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

struct ScrapeStats {
    files: usize,
    artifacts: usize,
}

/// Analyse a seed (local path or git URL) and return (auto_keywords, stats).
fn analyse_seed(seed: &str, domain: &str) -> Result<(Vec<String>, ScrapeStats), String> {
    if seed.starts_with("http") {
        scrape_url(seed, domain)
    } else {
        let p = PathBuf::from(seed);
        if !p.is_dir() {
            return Err(format!("'{}' is not a directory", seed));
        }
        scrape_local(&p, domain)
    }
}

/// Clone and scrape a git URL; returns (auto_keywords, stats).
fn scrape_url(url: &str, domain: &str) -> Result<(Vec<String>, ScrapeStats), String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    let temp_dir = std::env::temp_dir().join(format!("isls-harpoon-{:x}", hasher.finish()));
    if temp_dir.exists() {
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // Shallow clone
    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", "--single-branch", url])
        .arg(&temp_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("git clone failed: {}", e))?;

    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&temp_dir);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("clone: {}", stderr.trim()));
    }

    let result = scrape_local(&temp_dir, domain);
    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

/// Parse a local directory, feed norms, return (auto_keywords, stats).
fn scrape_local(dir: &Path, domain: &str) -> Result<(Vec<String>, ScrapeStats), String> {
    let analysis = isls_reader::parse_directory(dir)
        .map_err(|e| format!("parse error: {}", e))?;

    let topology = isls_code_topo::compute_code_topology(&analysis.files);
    let (artifacts, _) = bridge::observations_from_code(&analysis.files);

    // Feed norms
    if !artifacts.is_empty() {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let run_id = format!("harpoon_{}_{}", domain, timestamp);
        let mut registry = NormRegistry::new();
        let _ = registry.load();
        registry.observe_and_learn(&artifacts, domain, &run_id);
        let _ = registry.save();
    }

    // Extract keywords
    let mut all_imports: Vec<String> = Vec::new();
    for obs in &analysis.files {
        all_imports.extend(obs.imports.iter().cloned());
    }
    all_imports.sort();
    all_imports.dedup();

    let auto_kws = isls_norms::spectroscopy::extract_keywords_from_analysis(
        &topology.struct_names,
        &all_imports,
        &topology.primary_language,
        5,
    );

    Ok((
        auto_kws,
        ScrapeStats {
            files: analysis.files.len(),
            artifacts: artifacts.len(),
        },
    ))
}

/// Search GitHub for repos matching `keyword`, return (clone_url, full_name) pairs.
fn search_github(keyword: &str, max: usize) -> Result<Vec<(String, String)>, String> {
    let url = format!(
        "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page={}",
        url_encode(keyword),
        max.min(30),
    );

    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "isls-cli/1.0")
        .header("Accept", "application/vnd.github.v3+json")
        .timeout(Duration::from_secs(15))
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if resp.status().as_u16() == 403 {
        return Err("GitHub rate limit — waiting".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("GitHub returned {}", resp.status()));
    }

    let body: serde_json::Value = resp.json().map_err(|e| format!("JSON: {}", e))?;
    let repos = body["items"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|item| {
            let clone_url = item["clone_url"].as_str()?.to_string();
            let full_name = item["full_name"].as_str().unwrap_or("unknown").to_string();
            if clone_url.is_empty() { None } else { Some((clone_url, full_name)) }
        })
        .collect();

    Ok(repos)
}

fn url_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "+".to_string(),
            c if c.is_ascii_alphanumeric() || "-_.~".contains(c) => c.to_string(),
            c => format!("%{:02X}", c as u32),
        })
        .collect()
}

fn suggested_keywords_path() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".isls").join("suggested_keywords.txt"))
        .unwrap_or_else(|_| PathBuf::from("/tmp/isls/suggested_keywords.txt"))
}

fn append_keywords_to_file(path: &Path, keywords: &[String]) {
    if keywords.is_empty() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing_raw = std::fs::read_to_string(path).unwrap_or_default();
    let existing: BTreeSet<String> = existing_raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.trim().to_lowercase())
        .collect();

    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        for kw in keywords {
            if !existing.contains(&kw.to_lowercase()) {
                let _ = writeln!(f, "{}", kw);
            }
        }
    }
}

fn compute_coverage() -> f64 {
    let mut registry = NormRegistry::new();
    let _ = registry.load();
    // 34 = number of entries in universal_target_resonites()
    let universe = 34usize;
    let covered = registry.all_norms().len();
    (covered as f64 / universe as f64).min(1.0)
}
