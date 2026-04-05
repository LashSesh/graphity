// isls-norms/src/genome.rs — I2/W2: Gen-Clustering
//
// Detect genes (clusters of norms that consistently co-activate) from the
// metrics.jsonl stream produced by forge runs. Genes are stored in
// ~/.isls/genome.json and form the operational backbone of the Infogenetik.
//
// Algorithm (spec §W2):
//   1. Build a symmetric co-activation matrix from `metrics.norms_activated`.
//   2. Compute pairwise Jaccard similarity.
//   3. Single-link agglomerative clustering: two norms belong to the same
//      gene when their Jaccard similarity >= `min_coactivation` (default
//      0.8).
//   4. Auto-name each cluster using a small heuristic map, falling back to
//      the dominant member norm. Singletons (unclustered norms) are listed
//      separately by the CLI.
//
// This module is pure: no tokio, no reqwest, no axum. It reads from and
// writes to the filesystem only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Minimum number of metrics entries required to compute a meaningful genome.
///
/// Below this threshold `compute_genome` returns an empty `Genome` with
/// `total_metrics` set so the CLI can print "Not enough data".
pub const MIN_METRICS_ENTRIES: usize = 10;

/// Default pairwise Jaccard threshold for considering two norms part of the
/// same gene.
pub const DEFAULT_MIN_COACTIVATION: f64 = 0.8;

// ─── Types ─────────────────────────────────────────────────────────────────

/// A single line from `metrics.jsonl`, deserialised lazily.
///
/// Only fields this module actually needs are listed — every other field is
/// ignored via `#[serde(default)]` and `deny_unknown_fields` is *not* set,
/// so old and new JSONL entries both deserialize cleanly.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MetricsLite {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub norms_activated: Vec<String>,
    #[serde(default)]
    pub compile_success: bool,
    #[serde(default)]
    pub codematrix_avg: f64,
}

/// A gene: a cluster of norms that consistently co-activate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gene {
    /// Stable identifier like `"GENE-0001"`.
    pub id: String,
    /// Human-readable name, auto-generated from the member norms.
    pub name: String,
    /// Member norm identifiers, sorted alphabetically for determinism.
    pub norms: Vec<String>,
    /// Average pairwise Jaccard similarity of the member norms.
    pub coactivation: f64,
    /// Average codematrix resonance across metrics entries that activated
    /// *any* of the member norms (0.0 if none recorded resonance yet).
    pub fitness: f64,
    /// Number of metrics entries in which at least one member norm fired.
    pub activation_count: u64,
    /// Distinct `description`-derived domains where this gene activated.
    pub domains: Vec<String>,
}

/// The persisted genome state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Genome {
    pub genes: Vec<Gene>,
    /// Number of times `compute_genome` has been run against newer data.
    pub generation: u64,
    /// Local timestamp of the last computation.
    pub last_updated: String,
    /// Total metrics entries that fed into this computation.
    pub total_metrics: usize,
    /// Norms that never reached the coactivation threshold with any other
    /// norm. Reported separately by the CLI.
    #[serde(default)]
    pub singletons: Vec<String>,
}

// ─── Persistence ───────────────────────────────────────────────────────────

/// Path of the persisted genome at `~/.isls/genome.json`.
pub fn genome_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|h| PathBuf::from(h).join(".isls").join("genome.json"))
}

/// Path of the metrics JSONL stream at `~/.isls/metrics.jsonl`.
///
/// Mirrors `isls_forge_llm::metrics::metrics_path` without introducing a
/// dependency on the forge crate — `isls-norms` stays pure-data per spec
/// constraints.
pub fn metrics_jsonl_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|h| PathBuf::from(h).join(".isls").join("metrics.jsonl"))
}

impl Genome {
    /// Load the genome from `~/.isls/genome.json`, returning an empty genome
    /// if the file doesn't exist or fails to parse.
    pub fn load() -> Self {
        let path = match genome_path() {
            Some(p) => p,
            None => return Self::default(),
        };
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Genome>(&s).ok())
            .unwrap_or_default()
    }

    /// Persist the genome to `~/.isls/genome.json`.
    pub fn save(&self) -> std::io::Result<()> {
        let path = match genome_path() {
            Some(p) => p,
            None => return Ok(()),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&path, json)
    }
}

/// Load all metrics from `~/.isls/metrics.jsonl`.
pub fn load_metrics_lite() -> Vec<MetricsLite> {
    let path = match metrics_jsonl_path() {
        Some(p) => p,
        None => return vec![],
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<MetricsLite>(l).ok())
        .collect()
}

// ─── Clustering ────────────────────────────────────────────────────────────

/// Compute the genome from a slice of metrics entries.
///
/// Returns an empty `Genome` with `total_metrics = metrics.len()` when
/// fewer than [`MIN_METRICS_ENTRIES`] entries are available — the CLI
/// surfaces this as a "Not enough data" message.
pub fn compute_genome(metrics: &[MetricsLite], min_coactivation: f64) -> Genome {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    if metrics.len() < MIN_METRICS_ENTRIES {
        return Genome {
            genes: vec![],
            generation: 0,
            last_updated: now,
            total_metrics: metrics.len(),
            singletons: vec![],
        };
    }

    // Collect the universe of norms and index them.
    let all_norms: BTreeSet<String> = metrics
        .iter()
        .flat_map(|m| m.norms_activated.iter().cloned())
        .collect();
    let norm_list: Vec<String> = all_norms.into_iter().collect();
    let n = norm_list.len();

    if n == 0 {
        return Genome {
            genes: vec![],
            generation: 0,
            last_updated: now,
            total_metrics: metrics.len(),
            singletons: vec![],
        };
    }

    // Co-activation counts: `coact[i][j]` is the number of metrics entries
    // in which both `norm_list[i]` and `norm_list[j]` activated.
    let mut coact = vec![vec![0u32; n]; n];
    let mut counts = vec![0u32; n];

    for m in metrics {
        let active: Vec<usize> = norm_list
            .iter()
            .enumerate()
            .filter(|(_, name)| m.norms_activated.contains(name))
            .map(|(i, _)| i)
            .collect();
        for &i in &active {
            counts[i] += 1;
            for &j in &active {
                coact[i][j] += 1;
            }
        }
    }

    // Pairwise Jaccard similarity: |A ∩ B| / |A ∪ B|.
    let mut sim = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i == j {
                sim[i][j] = 1.0;
                continue;
            }
            let union = counts[i] + counts[j] - coact[i][j];
            if union > 0 {
                sim[i][j] = coact[i][j] as f64 / union as f64;
            }
        }
    }

    // Single-link agglomerative clustering: union-find over pairs where
    // similarity >= threshold.
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]);
        }
        parent[i]
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    for i in 0..n {
        for j in (i + 1)..n {
            if sim[i][j] >= min_coactivation {
                union(&mut parent, i, j);
            }
        }
    }

    // Group norms by root.
    let mut clusters: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        clusters.entry(root).or_default().push(i);
    }

    // Build Gene records (multi-member clusters) and singletons.
    let mut genes: Vec<Gene> = Vec::new();
    let mut singletons: Vec<String> = Vec::new();
    let mut gene_counter: usize = 0;

    // Deterministic ordering: sort clusters by size descending, then by
    // the smallest member norm for tie-breaking.
    let mut cluster_vec: Vec<(usize, Vec<usize>)> = clusters.into_iter().collect();
    cluster_vec.sort_by(|a, b| {
        b.1.len()
            .cmp(&a.1.len())
            .then_with(|| norm_list[a.1[0]].cmp(&norm_list[b.1[0]]))
    });

    for (_root, members) in cluster_vec {
        if members.len() == 1 {
            singletons.push(norm_list[members[0]].clone());
            continue;
        }

        // Average pairwise similarity within the cluster.
        let mut sum = 0.0;
        let mut pairs = 0;
        for (a_idx, &a) in members.iter().enumerate() {
            for &b in &members[a_idx + 1..] {
                sum += sim[a][b];
                pairs += 1;
            }
        }
        let avg_coact = if pairs > 0 { sum / pairs as f64 } else { 0.0 };

        let mut norm_names: Vec<String> =
            members.iter().map(|&i| norm_list[i].clone()).collect();
        norm_names.sort();

        // Activation count: metrics entries where at least one member norm
        // fired. Domain extraction: take the first word of the description
        // as a naive domain tag.
        let mut activation_count: u64 = 0;
        let mut domain_set: BTreeSet<String> = BTreeSet::new();
        let mut resonance_sum = 0.0f64;
        let mut resonance_count = 0usize;
        for m in metrics {
            if m.norms_activated
                .iter()
                .any(|nm| norm_names.contains(nm))
            {
                activation_count += 1;
                if m.codematrix_avg > 0.0 {
                    resonance_sum += m.codematrix_avg;
                    resonance_count += 1;
                }
                if let Some(dom) = domain_from_description(&m.description) {
                    domain_set.insert(dom);
                }
            }
        }
        let fitness = if resonance_count > 0 {
            resonance_sum / resonance_count as f64
        } else {
            0.0
        };

        gene_counter += 1;
        let id = format!("GENE-{:04}", gene_counter);
        let name = auto_name_gene(&norm_names);

        genes.push(Gene {
            id,
            name,
            norms: norm_names,
            coactivation: avg_coact,
            fitness,
            activation_count,
            domains: domain_set.into_iter().collect(),
        });
    }

    singletons.sort();

    Genome {
        genes,
        generation: 1,
        last_updated: now,
        total_metrics: metrics.len(),
        singletons,
    }
}

/// Heuristic domain extractor: the first non-trivial token of the
/// generation description. Used to populate `Gene.domains`.
fn domain_from_description(desc: &str) -> Option<String> {
    let first = desc
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .find(|s| !s.is_empty())?;
    if first.len() < 2 {
        return None;
    }
    Some(first.to_lowercase())
}

/// Auto-name a gene based on its member norms.
///
/// Hard-coded heuristic map from the spec §W2:
///   - CRUD + Auth + Pagination (+ Error) → "WebApp-Core"
///   - Docker + Nginx + Env            → "Deployment"
///   - otherwise: the member list's dominant "infra-xxx" name, or the
///     first member's own name.
pub fn auto_name_gene(norms: &[String]) -> String {
    let joined = norms
        .iter()
        .map(|s| s.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    let has_crud = joined.contains("crud");
    let has_auth = joined.contains("auth") || joined.contains("jwt");
    let has_pagination = joined.contains("pagination");
    if has_crud && has_auth && has_pagination {
        return "WebApp-Core".to_string();
    }

    let has_docker = joined.contains("docker");
    let has_nginx = joined.contains("nginx");
    let has_env = joined.contains("env");
    if has_docker && (has_nginx || has_env) {
        return "Deployment".to_string();
    }

    // Fall back to the most "central" looking member. Prefer names that
    // contain "INFRA-" (they usually describe cross-cutting concerns) then
    // the first alphabetically.
    if let Some(infra) = norms.iter().find(|n| n.to_uppercase().contains("INFRA-")) {
        return infra.clone();
    }
    norms.first().cloned().unwrap_or_else(|| "Gene".to_string())
}

/// Load metrics, compute the genome, and persist it. Convenience for the
/// CLI path.
pub fn load_compute_save() -> std::io::Result<Genome> {
    let metrics = load_metrics_lite();
    let genome = compute_genome(&metrics, DEFAULT_MIN_COACTIVATION);
    genome.save()?;
    Ok(genome)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn metric(norms: &[&str], desc: &str) -> MetricsLite {
        MetricsLite {
            id: "t".to_string(),
            description: desc.to_string(),
            norms_activated: norms.iter().map(|s| s.to_string()).collect(),
            compile_success: true,
            codematrix_avg: 0.8,
        }
    }

    #[test]
    fn test_compute_genome_not_enough_data() {
        let metrics: Vec<MetricsLite> = (0..5)
            .map(|_| metric(&["ISLS-NORM-0042"], "pet-shop"))
            .collect();
        let g = compute_genome(&metrics, DEFAULT_MIN_COACTIVATION);
        assert!(g.genes.is_empty());
        assert_eq!(g.total_metrics, 5);
    }

    #[test]
    fn test_compute_genome_webapp_core_cluster() {
        // 12 metrics entries, all activating the same 4 norms → one gene.
        let metrics: Vec<MetricsLite> = (0..12)
            .map(|i| {
                metric(
                    &[
                        "ISLS-NORM-CRUD-Entity",
                        "ISLS-NORM-JWT-Auth",
                        "ISLS-NORM-Pagination",
                        "ISLS-NORM-ErrorSystem",
                    ],
                    &format!("domain-{} app", i % 3),
                )
            })
            .collect();
        let g = compute_genome(&metrics, DEFAULT_MIN_COACTIVATION);
        assert_eq!(g.total_metrics, 12);
        assert_eq!(g.genes.len(), 1, "expected exactly one gene cluster");
        let gene = &g.genes[0];
        assert_eq!(gene.norms.len(), 4);
        assert_eq!(gene.name, "WebApp-Core");
        assert!(gene.coactivation >= 0.99);
        assert_eq!(gene.activation_count, 12);
        // 3 distinct domains (domain-0/1/2).
        assert!(gene.domains.len() >= 1);
        assert!(g.singletons.is_empty());
    }

    #[test]
    fn test_compute_genome_separates_singleton() {
        // 10 metrics: A+B always co-activate, C only appears alone twice.
        let mut metrics: Vec<MetricsLite> = (0..10)
            .map(|_| metric(&["A", "B"], "pet-shop app"))
            .collect();
        metrics.push(metric(&["C"], "other app"));
        metrics.push(metric(&["C"], "other app"));
        let g = compute_genome(&metrics, DEFAULT_MIN_COACTIVATION);
        assert_eq!(g.genes.len(), 1);
        assert_eq!(g.genes[0].norms, vec!["A".to_string(), "B".to_string()]);
        assert!(g.singletons.contains(&"C".to_string()));
    }

    #[test]
    fn test_auto_name_webapp_core() {
        let norms = vec![
            "ISLS-NORM-CRUD-Entity".to_string(),
            "ISLS-NORM-JWT-Auth".to_string(),
            "ISLS-NORM-Pagination".to_string(),
            "ISLS-NORM-ErrorSystem".to_string(),
        ];
        assert_eq!(auto_name_gene(&norms), "WebApp-Core");
    }

    #[test]
    fn test_auto_name_deployment() {
        let norms = vec![
            "ISLS-NORM-INFRA-DOCKER".to_string(),
            "ISLS-NORM-INFRA-NGINX".to_string(),
        ];
        assert_eq!(auto_name_gene(&norms), "Deployment");
    }

    #[test]
    fn test_auto_name_fallback_infra() {
        let norms = vec![
            "ISLS-NORM-INFRA-WEB".to_string(),
            "ISLS-NORM-OTHER".to_string(),
        ];
        assert_eq!(auto_name_gene(&norms), "ISLS-NORM-INFRA-WEB");
    }

    #[test]
    fn test_auto_name_fallback_first() {
        let norms = vec!["ISLS-NORM-A".to_string(), "ISLS-NORM-B".to_string()];
        assert_eq!(auto_name_gene(&norms), "ISLS-NORM-A");
    }

    #[test]
    fn test_domain_from_description() {
        assert_eq!(
            domain_from_description("pet-shop app with animals"),
            Some("pet-shop".to_string())
        );
        assert_eq!(domain_from_description(""), None);
    }

    #[test]
    fn test_genome_roundtrip_serde() {
        let g = Genome {
            genes: vec![Gene {
                id: "GENE-0001".to_string(),
                name: "WebApp-Core".to_string(),
                norms: vec!["A".to_string(), "B".to_string()],
                coactivation: 0.95,
                fitness: 0.8,
                activation_count: 10,
                domains: vec!["pet-shop".to_string()],
            }],
            generation: 1,
            last_updated: "2026-04-05".to_string(),
            total_metrics: 10,
            singletons: vec!["C".to_string()],
        };
        let json = serde_json::to_string(&g).unwrap();
        let back: Genome = serde_json::from_str(&json).unwrap();
        assert_eq!(back.genes.len(), 1);
        assert_eq!(back.genes[0].name, "WebApp-Core");
        assert_eq!(back.singletons, vec!["C".to_string()]);
    }
}
