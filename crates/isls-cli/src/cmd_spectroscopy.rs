// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! I3/W1 — `isls spectroscopy` subcommand.
//!
//! Parses a local project via `isls_reader`, extracts a set of lightweight
//! resonites and feeds them into `isls_norms::spectroscopy`. Optionally
//! triggers a targeted scrape for each detected gap (`--scrape`).

use std::path::{Path, PathBuf};

use isls_norms::spectroscopy::{
    spectroscopy, Resonite, ResoniteTypeKind, SpectroscopyResult,
};
use isls_norms::NormRegistry;
use isls_reader::{CodeObservation, Language, WorkspaceAnalysis};

// ─── Public entry ───────────────────────────────────────────────────────────

/// Run constraint spectroscopy on a local path.
pub fn cmd_spectroscopy(path: Option<&str>, scrape: bool) {
    let path_str = path.unwrap_or(".");
    let path = PathBuf::from(path_str);
    if !path.exists() || !path.is_dir() {
        eprintln!("[ERROR] Path is not a directory: {}", path_str);
        std::process::exit(1);
    }

    println!("ISLS Constraint Spectroscopy");
    println!("---");
    println!("Target: {}", path_str);

    let analysis = match isls_reader::parse_directory(&path) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[ERROR] Parse failed: {}", e);
            std::process::exit(1);
        }
    };

    let resonites = resonites_from_analysis(&analysis);
    let registry = NormRegistry::new();
    let result = spectroscopy(&resonites, &registry);

    print_report(&analysis, &resonites, &result);

    if scrape {
        eprintln!();
        eprintln!("[Scrape] --scrape requested.");
        if result.suggestions.is_empty() {
            eprintln!("         No scrape suggestions available — nothing to fill.");
        } else {
            eprintln!(
                "         Run `isls scrape --manifest <file>` with the suggested"
            );
            eprintln!(
                "         keywords, or invoke the gateway endpoint"
            );
            eprintln!(
                "         POST /api/discover/spectroscopy/fill to drive a"
            );
            eprintln!(
                "         background mass-scrape for the gaps listed above."
            );
        }
    }
}

// ─── Conversion ─────────────────────────────────────────────────────────────

/// Convert a parsed [`WorkspaceAnalysis`] into a flat list of resonites
/// usable by [`spectroscopy`].
pub fn resonites_from_analysis(analysis: &WorkspaceAnalysis) -> Vec<Resonite> {
    let mut out = Vec::new();
    for file in &analysis.files {
        resonites_from_file(file, &mut out);
    }
    out
}

fn resonites_from_file(obs: &CodeObservation, out: &mut Vec<Resonite>) {
    // Functions
    for f in &obs.functions {
        out.push(Resonite::Fn {
            name: f.name.clone(),
            arity: f.params.len(),
        });
    }
    // Types
    for s in &obs.structs {
        let kind = infer_type_kind(&s.derives);
        out.push(Resonite::Type {
            name: s.name.clone(),
            kind,
        });
    }
    // Imports
    for i in &obs.imports {
        out.push(Resonite::Import { path: i.clone() });
    }
    // Inferred layer artefact from the file path.
    if let Some(artifact) = infer_layer_artifact(&obs.file_path, &obs.language) {
        let depth = infer_layer_depth(&obs.file_path);
        out.push(Resonite::Layer { depth, artifact });
    }
}

fn infer_type_kind(derives: &[String]) -> ResoniteTypeKind {
    if derives.iter().any(|d| d.to_lowercase().contains("enum")) {
        ResoniteTypeKind::Enum
    } else {
        ResoniteTypeKind::Struct
    }
}

fn infer_layer_artifact(path: &Path, _lang: &Language) -> Option<String> {
    let lower = path.to_string_lossy().to_lowercase();
    for (needle, label) in [
        ("/models/", "model"),
        ("/model/", "model"),
        ("/queries/", "query"),
        ("/database/", "query"),
        ("/services/", "service"),
        ("/service/", "service"),
        ("/api/", "api"),
        ("/handlers/", "api"),
        ("/routes/", "api"),
        ("/frontend/", "frontend"),
        ("/pages/", "frontend"),
        ("/components/", "frontend"),
        ("/tests/", "test"),
        ("/migration", "migration"),
        ("/config", "config"),
        ("/auth", "auth"),
    ] {
        if lower.contains(needle) {
            return Some(label.to_string());
        }
    }
    None
}

fn infer_layer_depth(path: &Path) -> u8 {
    let lower = path.to_string_lossy().to_lowercase();
    if lower.contains("/tests/") {
        return 9;
    }
    if lower.contains("/frontend/") || lower.contains("/pages/") {
        return 8;
    }
    if lower.contains("/api/") || lower.contains("/handlers/") || lower.contains("/routes/") {
        return 6;
    }
    if lower.contains("/services/") || lower.contains("/service/") {
        return 5;
    }
    if lower.contains("/database/") || lower.contains("/queries/") {
        return 4;
    }
    if lower.contains("/models/") || lower.contains("/model/") {
        return 3;
    }
    if lower.contains("/auth") {
        return 2;
    }
    if lower.contains("config") || lower.contains("pagination") {
        return 1;
    }
    0
}

// ─── Reporting ──────────────────────────────────────────────────────────────

fn print_report(
    analysis: &WorkspaceAnalysis,
    resonites: &[Resonite],
    result: &SpectroscopyResult,
) {
    let fn_count = resonites.iter().filter(|r| matches!(r, Resonite::Fn { .. })).count();
    let type_count = resonites.iter().filter(|r| matches!(r, Resonite::Type { .. })).count();
    let import_count = resonites.iter().filter(|r| matches!(r, Resonite::Import { .. })).count();
    let layer_count = resonites.iter().filter(|r| matches!(r, Resonite::Layer { .. })).count();

    println!(
        "Resonites: {} ({} Fn, {} Type, {} Import, {} Layer)",
        resonites.len(),
        fn_count,
        type_count,
        import_count,
        layer_count
    );
    println!(
        "Files parsed: {} ({} total LOC)",
        analysis.files.len(),
        analysis.total_loc
    );
    println!(
        "Spectrum: {} classes identified",
        result.target_spectrum.len()
    );
    println!();

    let total = result.target_spectrum.len();
    let covered = result.covered.len();
    println!(
        "Coverage: {}/{} ({:.1}%)",
        covered,
        total,
        result.coverage * 100.0
    );

    for class in &result.covered {
        println!("  [OK]  {}", class.as_str());
    }
    for gap in &result.gaps {
        println!(
            "  [GAP] {:<22} ({} resonites, {} layer(s))",
            gap.class.as_str(),
            gap.resonite_count,
            gap.layers_affected.len()
        );
    }

    if !result.suggestions.is_empty() {
        println!();
        println!("Suggested scrape campaigns:");
        for (i, sugg) in result.suggestions.iter().enumerate() {
            if let Some(first) = sugg.keywords.first() {
                println!(
                    "  {}. \"{}\" (gap: {})",
                    i + 1,
                    first,
                    sugg.gap
                );
            }
        }
    }
}
