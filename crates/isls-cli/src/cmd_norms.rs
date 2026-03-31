// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! `isls norms` subcommand handlers for D4 CLI observability.

use isls_norms::NormRegistry;

/// List all norms (builtin + auto-discovered).
/// If `auto_only` is true, only show auto-discovered norms.
pub fn cmd_norms_list(auto_only: bool) {
    let registry = NormRegistry::new();
    let all = registry.all_norms();

    let (builtin_count, auto_count) = all.iter().fold((0, 0), |(b, a), n| {
        if n.id.starts_with("ISLS-NORM-AUTO-") { (b, a + 1) } else { (b + 1, a) }
    });

    println!("ISLS Norm Catalog ({} builtin, {} auto-discovered)", builtin_count, auto_count);
    println!("---");

    let mut norms: Vec<_> = all.into_iter().collect();
    norms.sort_by(|a, b| a.id.cmp(&b.id));

    for norm in norms {
        let is_auto = norm.id.starts_with("ISLS-NORM-AUTO-");
        if auto_only && !is_auto {
            continue;
        }

        let origin = if is_auto { "auto" } else { "builtin" };
        let layers = format_layers(norm);

        println!(
            "{}  {}  {}  {:?}  [{}]",
            norm.id, norm.name, origin, norm.level, layers
        );

        if is_auto {
            println!(
                "    domains: {}, observations: {}, builtin: {}",
                norm.evidence.domains_used.join(", "),
                norm.evidence.usage_count,
                norm.evidence.builtin
            );
        }
    }
}

/// Show full details for a specific norm.
pub fn cmd_norms_inspect(norm_id: &str) {
    let registry = NormRegistry::new();
    match registry.get(norm_id) {
        Some(norm) => {
            println!("Norm: {}", norm.id);
            println!("Name: {}", norm.name);
            println!("Level: {:?}", norm.level);
            println!("Version: {}", norm.version);
            println!();

            // Triggers
            if !norm.triggers.is_empty() {
                println!("Triggers:");
                for t in &norm.triggers {
                    if !t.keywords.is_empty() {
                        println!("  keywords: {}", t.keywords.join(", "));
                    }
                    if !t.concepts.is_empty() {
                        println!("  concepts: {}", t.concepts.join(", "));
                    }
                    println!("  min_confidence: {}", t.min_confidence);
                }
                println!();
            }

            // Layers summary
            println!("Layers: [{}]", format_layers(norm));
            println!(
                "  database: {} artifact(s), model: {} artifact(s), query: {} artifact(s)",
                norm.layers.database.len(),
                norm.layers.model.len(),
                norm.layers.query.len()
            );
            println!(
                "  service: {} artifact(s), api: {} artifact(s), frontend: {} artifact(s)",
                norm.layers.service.len(),
                norm.layers.api.len(),
                norm.layers.frontend.len()
            );
            println!(
                "  test: {} artifact(s), config: {} artifact(s)",
                norm.layers.test.len(),
                norm.layers.config.len()
            );
            println!();

            // Parameters
            if !norm.parameters.is_empty() {
                println!("Parameters:");
                for p in &norm.parameters {
                    println!("  {} ({:?}) — {}", p.name, p.param_type, p.description);
                }
                println!();
            }

            // Evidence
            println!("Evidence:");
            println!("  usage_count: {}", norm.evidence.usage_count);
            println!("  domains_used: {}", norm.evidence.domains_used.join(", "));
            println!("  builtin: {}", norm.evidence.builtin);
            if !norm.evidence.signature.is_empty() {
                println!("  signature: {}", norm.evidence.signature);
            }
        }
        None => {
            eprintln!("[ERROR] Norm '{}' not found", norm_id);
            std::process::exit(1);
        }
    }
}

/// List the candidate pool.
pub fn cmd_norms_candidates() {
    let registry = NormRegistry::new();
    let candidates = registry.candidates();

    if candidates.is_empty() {
        println!("No candidates in the learning pool.");
        println!("Generate applications with `isls forge-chat` to accumulate observations.");
        return;
    }

    println!("ISLS Candidate Pool ({} candidates)", candidates.len());
    println!("---");

    let mut cands: Vec<_> = candidates.into_iter().collect();
    cands.sort_by(|a, b| a.id.cmp(&b.id));

    for c in cands {
        println!(
            "{}  status: {:?}  observations: {}  domains: {}  consistency: {:.2}  layers: {}",
            c.id,
            c.status,
            c.observation_count,
            c.domains.join(", "),
            c.consistency,
            c.consistent_layers.len()
        );
    }
}

/// Print summary statistics.
pub fn cmd_norms_stats() {
    let registry = NormRegistry::new();
    let all = registry.all_norms();
    let candidates = registry.candidates();

    let builtin_count = all.iter().filter(|n| !n.id.starts_with("ISLS-NORM-AUTO-")).count();
    let auto_count = all.iter().filter(|n| n.id.starts_with("ISLS-NORM-AUTO-")).count();

    let molecule_count = all.iter().filter(|n| matches!(n.level, isls_norms::NormLevel::Molecule)).count();
    let organism_count = all.iter().filter(|n| matches!(n.level, isls_norms::NormLevel::Organism)).count();
    let atom_count = all.iter().filter(|n| matches!(n.level, isls_norms::NormLevel::Atom)).count();

    let observing = candidates.iter().filter(|c| c.status == isls_norms::CandidateStatus::Observing).count();
    let promoted = candidates.iter().filter(|c| c.status == isls_norms::CandidateStatus::Promoted).count();

    let total_observations: usize = candidates.iter().map(|c| c.observation_count).sum();
    let all_domains: std::collections::HashSet<&str> = candidates
        .iter()
        .flat_map(|c| c.domains.iter().map(|d| d.as_str()))
        .collect();

    let wirings = registry.all_wirings().len();

    // Check persistence file
    let persistence_path = std::env::var("HOME").ok().map(std::path::PathBuf::from)
        .map(|h| h.join(".isls/norms.json"))
        .unwrap_or_default();
    let persistence_info = if persistence_path.exists() {
        let size = std::fs::metadata(&persistence_path)
            .map(|m| m.len())
            .unwrap_or(0);
        format!("{} ({} bytes)", persistence_path.display(), size)
    } else {
        format!("{} (not created yet)", persistence_path.display())
    };

    println!("ISLS Norm Statistics");
    println!("---");
    println!(
        "Builtin norms:       {} ({} molecule, {} organism, {} atom)",
        builtin_count, molecule_count, organism_count, atom_count
    );
    println!("Auto-discovered:     {}", auto_count);
    println!(
        "Candidates:          {} ({} observing, {} promoted)",
        candidates.len(), observing, promoted
    );
    println!(
        "Total observations:  {} across {} domains",
        total_observations,
        all_domains.len()
    );
    println!("Wirings:             {}", wirings);
    println!("Persistence:         {}", persistence_info);
}

/// Delete ~/.isls/norms.json (reset auto-discovered norms).
pub fn cmd_norms_reset() {
    let persistence_path = std::env::var("HOME").ok().map(std::path::PathBuf::from)
        .map(|h| h.join(".isls/norms.json"))
        .unwrap_or_default();

    if !persistence_path.exists() {
        println!("No norms.json found — nothing to reset.");
        return;
    }

    // Print warning and ask for confirmation via stdin
    println!(
        "This will delete {} and reset all auto-discovered norms and candidates.",
        persistence_path.display()
    );
    println!("Builtin norms are not affected.");
    print!("Continue? [y/N] ");

    // Flush stdout so prompt appears
    use std::io::Write;
    let _ = std::io::stdout().flush();

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_ok() {
        if input.trim().eq_ignore_ascii_case("y") {
            match std::fs::remove_file(&persistence_path) {
                Ok(()) => println!("Deleted {}. Auto-norms reset.", persistence_path.display()),
                Err(e) => eprintln!("[ERROR] Failed to delete: {}", e),
            }
        } else {
            println!("Aborted.");
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn format_layers(norm: &isls_norms::Norm) -> String {
    let mut layers = Vec::new();
    if !norm.layers.database.is_empty() { layers.push("Db"); }
    if !norm.layers.model.is_empty()    { layers.push("Model"); }
    if !norm.layers.query.is_empty()    { layers.push("Query"); }
    if !norm.layers.service.is_empty()  { layers.push("Svc"); }
    if !norm.layers.api.is_empty()      { layers.push("Api"); }
    if !norm.layers.frontend.is_empty() { layers.push("Fe"); }
    if !norm.layers.test.is_empty()     { layers.push("Test"); }
    if !norm.layers.config.is_empty()   { layers.push("Cfg"); }
    layers.join(",")
}
