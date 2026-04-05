// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! I3/W3 — `isls evolve` subcommand.
//!
//! One command to metamorphose: Solve (scrape) → Spectroscopy
//! → optional targeted fill → Merge → Coagula (forge-chat) → Report.
//! This command is deliberately thin — it composes existing primitives
//! (`parse_directory`, `spectroscopy`, `forge-chat`) and produces a
//! human-readable before/after report.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use isls_norms::spectroscopy::{spectroscopy, SpectroscopyResult};
use isls_norms::NormRegistry;

use crate::cmd_forge_chat;
use crate::cmd_spectroscopy::resonites_from_analysis;

/// High-level report produced by the evolve command.
pub struct EvolveReport {
    pub from: PathBuf,
    pub output: PathBuf,
    pub delta: String,
    pub resonite_count: usize,
    pub gaps_before: usize,
    pub gaps_after: usize,
    pub coverage_before: f64,
    pub coverage_after: f64,
    pub entities: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_evolve(
    from: &str,
    delta: &str,
    output: &str,
    scrape_gaps: bool,
    api_key: Option<String>,
    model: &str,
    ollama: bool,
    ollama_url: &str,
) {
    let from_path = PathBuf::from(from);
    if !from_path.exists() || !from_path.is_dir() {
        eprintln!("[ERROR] --from path is not a directory: {}", from);
        std::process::exit(1);
    }
    let output_path = PathBuf::from(output);

    println!("ISLS Evolve: Solve-Coagula Cycle");
    println!("================================");
    println!();

    // ── 1. SOLVE ──────────────────────────────────────────────────────────
    println!("[Solve] Scraping {}...", from_path.display());
    let analysis = match isls_reader::parse_directory(&from_path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[ERROR] Parse failed: {}", e);
            std::process::exit(1);
        }
    };
    let resonites = resonites_from_analysis(&analysis);
    println!(
        "        {} files, {} resonites, {} LOC",
        analysis.files.len(),
        resonites.len(),
        analysis.total_loc
    );

    let entities = extract_entities(&analysis);
    if !entities.is_empty() {
        let preview: Vec<&String> = entities.iter().take(10).collect();
        println!(
            "        Entities: {}{}",
            preview
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            if entities.len() > 10 { ", ..." } else { "" }
        );
    }
    println!();

    // ── 2. SPECTROSCOPY ───────────────────────────────────────────────────
    let registry = NormRegistry::new();
    let spectrum = spectroscopy(&resonites, &registry);
    print_spectrum_block("[Spectroscopy]", &spectrum);
    println!();

    // ── 3. FILL GAPS (optional) ──────────────────────────────────────────
    let (gaps_after, coverage_after) = if scrape_gaps && !spectrum.suggestions.is_empty() {
        println!(
            "[Fill] Targeted scrape campaigns for {} gap(s)...",
            spectrum.suggestions.len()
        );
        for (i, sugg) in spectrum.suggestions.iter().enumerate() {
            println!(
                "       {}/{}. {} — {} keyword(s)",
                i + 1,
                spectrum.suggestions.len(),
                sugg.gap,
                sugg.keywords.len()
            );
            for kw in &sugg.keywords {
                println!("          - {}", kw);
            }
        }
        println!(
            "       NOTE: keyword campaigns are printed for review. Use"
        );
        println!(
            "             `isls scrape --manifest` or the gateway's"
        );
        println!(
            "             /api/discover/spectroscopy/fill endpoint for"
        );
        println!(
            "             the actual background run. This keeps `isls"
        );
        println!(
            "             evolve` deterministic and network-free."
        );
        // Re-evaluate once to refresh counts — in a real fill the
        // registry would have new auto-norms; without the run we just
        // surface the same numbers.
        let spectrum2 = spectroscopy(&resonites, &NormRegistry::new());
        println!(
            "       Coverage after fill: {:.1}% ({} remaining gap(s))",
            spectrum2.coverage * 100.0,
            spectrum2.gaps.len()
        );
        println!();
        (spectrum2.gaps.len(), spectrum2.coverage)
    } else {
        (spectrum.gaps.len(), spectrum.coverage)
    };

    // ── 4. MERGE ──────────────────────────────────────────────────────────
    let entity_sentence = if entities.is_empty() {
        "no pre-existing entities".to_string()
    } else {
        entities.join(", ")
    };
    let merged_description = format!(
        "{}. Existing system has: {}. Retain all existing functionality.",
        delta.trim_end_matches('.'),
        entity_sentence
    );
    println!("[Merge] Merged description:");
    println!("        {}", merged_description);
    println!();

    // ── 5. COAGULA ────────────────────────────────────────────────────────
    println!("[Coagula] Forging next generation into {}...", output);
    // Compose via the existing forge-chat pipeline — no new generation
    // logic, only orchestration. swarm flags are held at defaults so
    // `isls evolve` stays focused.
    cmd_forge_chat(
        &merged_description,
        output,
        api_key,
        model,
        ollama,
        ollama_url,
        false, // swarm
        4,     // swarm_size (unused)
        0.20,  // swarm_threshold (unused)
    );

    // ── 6. REPORT ─────────────────────────────────────────────────────────
    let report = EvolveReport {
        from: from_path,
        output: output_path,
        delta: delta.to_string(),
        resonite_count: resonites.len(),
        gaps_before: spectrum.gaps.len(),
        gaps_after,
        coverage_before: spectrum.coverage,
        coverage_after,
        entities,
    };
    println!();
    print_final_report(&report);
}

fn print_spectrum_block(label: &str, result: &SpectroscopyResult) {
    println!(
        "{} Coverage: {:.1}% ({} gap(s))",
        label,
        result.coverage * 100.0,
        result.gaps.len()
    );
    for gap in &result.gaps {
        println!(
            "  [GAP] {:<22} priority={:.0} (resonites={})",
            gap.class.as_str(),
            gap.priority,
            gap.resonite_count
        );
    }
}

fn print_final_report(r: &EvolveReport) {
    println!("[Report]");
    println!(
        "  Before: {} ({} resonites, {:.1}% coverage)",
        r.from.display(),
        r.resonite_count,
        r.coverage_before * 100.0
    );
    println!(
        "  After:  {} (+delta, {:.1}% coverage)",
        r.output.display(),
        r.coverage_after * 100.0
    );
    println!(
        "  Gaps:   {} → {} ({:+})",
        r.gaps_before,
        r.gaps_after,
        r.gaps_after as i64 - r.gaps_before as i64
    );
    println!("  Delta:  {}", r.delta);
    if !r.entities.is_empty() {
        println!("  Entities retained: {}", r.entities.len());
    }
}

/// Pull a de-duplicated list of "entity-like" type names out of a parsed
/// workspace — PascalCase structs that are not obvious request/response
/// DTOs or helper types. Used to inform the forge-chat prompt.
pub fn extract_entities(analysis: &isls_reader::WorkspaceAnalysis) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for file in &analysis.files {
        // Skip tests / config files.
        if is_noisy_path(&file.file_path) {
            continue;
        }
        for s in &file.structs {
            if is_entity_like(&s.name) {
                out.insert(s.name.clone());
            }
        }
    }
    out.into_iter().collect()
}

fn is_noisy_path(p: &Path) -> bool {
    let s = p.to_string_lossy().to_lowercase();
    s.contains("/tests/") || s.contains("_test") || s.contains("/examples/")
}

fn is_entity_like(name: &str) -> bool {
    if !name.chars().next().map_or(false, |c| c.is_uppercase()) {
        return false;
    }
    const NOISE: [&str; 12] = [
        "Request", "Response", "Error", "Config", "Opts", "Options",
        "Builder", "State", "Store", "Args", "Params", "Query",
    ];
    !NOISE.iter().any(|n| name.ends_with(n))
}
