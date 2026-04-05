// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! `isls norms` subcommand handlers for D4 CLI observability.

use isls_norms::{injection::INJECT_PREFIX, NormRegistry};

/// Return the source tag (`"builtin"` / `"auto"` / `"injected"`) for a norm.
fn norm_source_tag(id: &str) -> &'static str {
    if id.starts_with("ISLS-NORM-AUTO-") {
        "auto"
    } else if id.starts_with(INJECT_PREFIX) {
        "injected"
    } else {
        "builtin"
    }
}

/// List all norms (builtin + auto-discovered + injected).
///
/// * `auto_only` restricts the output to auto-discovered norms (legacy flag).
/// * `source_filter` restricts to norms of a particular origin
///   (`"builtin" | "auto" | "injected"`), which is what I3/W2 exposes.
pub fn cmd_norms_list(auto_only: bool, source_filter: Option<&str>) {
    let registry = NormRegistry::new();
    let all = registry.all_norms();

    let mut builtin_count = 0usize;
    let mut auto_count = 0usize;
    let mut injected_count = 0usize;
    for n in &all {
        match norm_source_tag(&n.id) {
            "auto" => auto_count += 1,
            "injected" => injected_count += 1,
            _ => builtin_count += 1,
        }
    }

    println!(
        "ISLS Norm Catalog ({} builtin, {} auto-discovered, {} injected)",
        builtin_count, auto_count, injected_count
    );
    println!("---");

    let mut norms: Vec<_> = all.into_iter().collect();
    norms.sort_by(|a, b| a.id.cmp(&b.id));

    for norm in norms {
        let origin = norm_source_tag(&norm.id);
        let is_auto = origin == "auto";
        if auto_only && !is_auto {
            continue;
        }
        if let Some(filter) = source_filter {
            if !filter.eq_ignore_ascii_case(origin) {
                continue;
            }
        }
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
    let persistence_path = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().map(std::path::PathBuf::from)
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

/// Show norm fitness report.
pub fn cmd_norms_fitness() {
    let store = isls_norms::fitness::FitnessStore::load();
    let entries = store.sorted_entries();

    println!("ISLS Norm Fitness Report");
    println!("---");

    if entries.is_empty() {
        println!("No fitness data yet. Run a generation to start tracking.");
        return;
    }

    for entry in &entries {
        let ratio = if entry.activation_count > 0 {
            format!("{}/{}", entry.success_count, entry.activation_count)
        } else {
            "n/a".to_string()
        };
        println!(
            "{:<35} {:.2}  ({} success)",
            entry.norm_id, entry.fitness, ratio,
        );
    }

    println!("---");
    println!("Total tracked: {}", entries.len());

    // Show weakest
    let weak: Vec<_> = entries.iter().filter(|e| e.fitness < 0.5 && e.activation_count >= 2).collect();
    if !weak.is_empty() {
        println!("\nWeakest:");
        for e in weak {
            println!(
                "  {:<35} {:.2}  ({}/{}) -- needs attention",
                e.norm_id, e.fitness, e.success_count, e.activation_count,
            );
        }
    }
}

/// I2/W2: Compute and display the ISLS Genome (gene clusters).
///
/// Reads `~/.isls/metrics.jsonl`, runs Jaccard + single-link clustering,
/// persists the result to `~/.isls/genome.json`, and prints it. When fewer
/// than 10 metrics entries exist, prints "Not enough data" and exits.
pub fn cmd_norms_genome() {
    use isls_norms::genome::{load_metrics_lite, compute_genome, DEFAULT_MIN_COACTIVATION, MIN_METRICS_ENTRIES};

    let metrics = load_metrics_lite();
    let genome = compute_genome(&metrics, DEFAULT_MIN_COACTIVATION);

    if metrics.len() < MIN_METRICS_ENTRIES {
        println!(
            "ISLS Genome — Not enough data ({} / {} metrics entries required)",
            metrics.len(),
            MIN_METRICS_ENTRIES
        );
        println!("Generate more applications with `isls forge-chat` or the Studio to accumulate entries.");
        return;
    }

    // Persist before printing so the CLI is observable end-to-end.
    if let Err(e) = genome.save() {
        eprintln!("[WARN] Could not save genome.json: {}", e);
    }

    println!(
        "ISLS Genome (Generation {}, {} metrics entries)",
        genome.generation, genome.total_metrics
    );
    println!("---");

    if genome.genes.is_empty() {
        println!("No gene clusters detected yet — norms are not co-activating consistently.");
    } else {
        for gene in &genome.genes {
            println!(
                "{}  \"{}\"  coact={:.2}  fitness={:.2}",
                gene.id, gene.name, gene.coactivation, gene.fitness
            );
            for norm in &gene.norms {
                println!("  {}", norm);
            }
            if !gene.domains.is_empty() {
                let shown: Vec<&String> = gene.domains.iter().take(5).collect();
                let more = gene.domains.len().saturating_sub(shown.len());
                let suffix = if more > 0 {
                    format!(", ... ({})", gene.domains.len())
                } else {
                    String::new()
                };
                println!(
                    "  Domains: {}{}",
                    shown
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    suffix
                );
            }
            println!("  Activations: {}", gene.activation_count);
            println!();
        }
    }

    if !genome.singletons.is_empty() {
        println!("Singletons (not yet clustered):");
        for s in &genome.singletons {
            println!("  {}", s);
        }
    }
}

/// Delete ~/.isls/norms.json (reset auto-discovered norms).
pub fn cmd_norms_reset() {
    let persistence_path = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().map(std::path::PathBuf::from)
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

// ─── I3/W2 Norm Injection ───────────────────────────────────────────────────

/// Inject a norm blueprint JSON file into the registry.
pub fn cmd_norms_inject(file: &str) {
    let path = std::path::Path::new(file);
    if !path.exists() {
        eprintln!("[ERROR] Blueprint file not found: {}", file);
        std::process::exit(1);
    }
    let blueprint = match isls_norms::injection::load_blueprint(path) {
        Ok(bp) => bp,
        Err(e) => {
            eprintln!("[ERROR] Invalid blueprint: {}", e);
            std::process::exit(1);
        }
    };

    let mut registry = NormRegistry::new();
    let _ = registry.load();

    match isls_norms::injection::inject_norm(&mut registry, blueprint.clone()) {
        Ok(id) => {
            if let Err(e) = registry.save() {
                eprintln!("[WARN] Could not persist norms.json: {}", e);
            }
            // Initialise fitness at the neutral midpoint (0.5) — the
            // fitness system will grow or shrink it through normal usage.
            let mut fitness = isls_norms::fitness::FitnessStore::load();
            {
                let entry = fitness.get_or_create(&id);
                entry.fitness = 0.5;
            }
            let _ = fitness.save();

            println!("[OK] Injected {} \"{}\"", id, blueprint.name);
            println!("     Fitness: 0.50 (neutral, will be validated by usage)");
            if !blueprint.activation_keywords.is_empty() {
                println!(
                    "     Keywords: {}",
                    blueprint.activation_keywords.join(", ")
                );
            }
        }
        Err(e) => {
            eprintln!("[ERROR] {}", e);
            std::process::exit(1);
        }
    }
}

/// Remove an injected norm by id. Builtins / auto-discovered norms are
/// rejected — only `ISLS-NORM-INJECT-*` IDs are accepted.
pub fn cmd_norms_remove(id: &str) {
    if !id.starts_with(isls_norms::injection::INJECT_PREFIX) {
        eprintln!(
            "[ERROR] Only injected norms (ID prefix {}) can be removed.",
            isls_norms::injection::INJECT_PREFIX
        );
        std::process::exit(1);
    }

    let mut registry = NormRegistry::new();
    let _ = registry.load();

    if isls_norms::injection::remove_injected(&mut registry, id) {
        if let Err(e) = registry.save() {
            eprintln!("[WARN] Could not persist norms.json: {}", e);
        }
        println!("[OK] Removed {}. Fitness data preserved.", id);
    } else {
        eprintln!("[ERROR] No injected norm with id '{}' found.", id);
        std::process::exit(1);
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
