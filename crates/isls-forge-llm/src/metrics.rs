// isls-forge-llm/src/metrics.rs — D7/W4: Generation Metrics
//
// Logs metrics for every generation (CLI and Cockpit) to enable
// the Gen 2 vs Gen 1 comparison (Generationsspirale).
//
// Storage: ~/.isls/metrics.jsonl (one JSON object per line, append-only).

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ─── Metric Types ───────────────────────────────────────────────────────────

/// Metrics recorded for each generation run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerationMetrics {
    pub id: String,
    pub timestamp: String,
    pub source: GenerationSource,
    pub description: String,
    pub entity_count: usize,
    pub file_count: usize,
    pub structural_files: usize,
    pub llm_files: usize,
    pub total_tokens: u64,
    pub compile_success: bool,
    pub coagula_cycles: u32,
    pub duration_secs: f64,
    pub norms_activated: Vec<String>,
    pub conversation_turns: usize,
    // I1: Infogenetik extensions
    #[serde(default)]
    pub contraction_ratios: Vec<f64>,
    #[serde(default)]
    pub was_contractive: bool,
    #[serde(default)]
    pub mikro_gate_pass_rate: f64,
    #[serde(default)]
    pub meso_gate_pass_rate: f64,
    // I2/W4: Codematrix-enhanced fitness.
    /// Average Codematrix resonance (geometric mean of R/F/T/S/E) across all
    /// generated files in this run. 0.0 when no files were gated. Used as
    /// the continuous reward signal for fitness updates. Persisted with
    /// `#[serde(default)]` so old JSONL entries still deserialize.
    #[serde(default)]
    pub codematrix_avg: f64,
    // I5/W5: SGB tracking.
    /// Structural-Generative Boundary: structural_files / file_count.
    /// Represents the fraction of generated files produced structurally
    /// (norm-driven) vs LLM-generated. Approaches 1.0 as the system
    /// converges. Persisted with `#[serde(default)]` for JSONL compat.
    #[serde(default)]
    pub sgb: f64,
}

/// Source of a generation run.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GenerationSource {
    Cli,
    Cockpit,
}

impl std::fmt::Display for GenerationSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerationSource::Cli => write!(f, "cli"),
            GenerationSource::Cockpit => write!(f, "cockpit"),
        }
    }
}

// ─── Persistence ────────────────────────────────────────────────────────────

/// Returns the path to ~/.isls/metrics.jsonl
pub fn metrics_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|h| PathBuf::from(h).join(".isls").join("metrics.jsonl"))
}

/// Append a metrics entry to the JSONL file.
pub fn append_metrics(metrics: &GenerationMetrics) -> std::io::Result<()> {
    let path = match metrics_path() {
        Some(p) => p,
        None => return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "cannot determine home directory")),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let json = serde_json::to_string(metrics)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    writeln!(file, "{}", json)?;
    Ok(())
}

/// Load all metrics from the JSONL file.
pub fn load_metrics() -> Vec<GenerationMetrics> {
    let path = match metrics_path() {
        Some(p) => p,
        None => return vec![],
    };
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };
    let reader = std::io::BufReader::new(file);
    reader.lines()
        .filter_map(|line| line.ok())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

/// Load the last N metrics entries.
pub fn load_last_n(n: usize) -> Vec<GenerationMetrics> {
    let all = load_metrics();
    let start = all.len().saturating_sub(n);
    all[start..].to_vec()
}

// ─── Comparison ─────────────────────────────────────────────────────────────

/// Aggregated stats for one generation source.
#[derive(Debug, Default)]
pub struct SourceStats {
    pub count: usize,
    pub avg_entities: f64,
    pub avg_tokens: f64,
    pub compile_success_rate: f64,
    pub avg_coagula: f64,
    pub avg_duration: f64,
    pub avg_turns: f64,
}

/// Comparison table: CLI vs Cockpit.
#[derive(Debug)]
pub struct ComparisonTable {
    pub cli: SourceStats,
    pub cockpit: SourceStats,
}

/// Build a comparison table from all metrics.
pub fn compare_metrics(all: &[GenerationMetrics]) -> ComparisonTable {
    let cli: Vec<&GenerationMetrics> = all.iter().filter(|m| m.source == GenerationSource::Cli).collect();
    let cockpit: Vec<&GenerationMetrics> = all.iter().filter(|m| m.source == GenerationSource::Cockpit).collect();

    ComparisonTable {
        cli: compute_stats(&cli),
        cockpit: compute_stats(&cockpit),
    }
}

fn compute_stats(metrics: &[&GenerationMetrics]) -> SourceStats {
    if metrics.is_empty() {
        return SourceStats::default();
    }
    let n = metrics.len() as f64;
    SourceStats {
        count: metrics.len(),
        avg_entities: metrics.iter().map(|m| m.entity_count as f64).sum::<f64>() / n,
        avg_tokens: metrics.iter().map(|m| m.total_tokens as f64).sum::<f64>() / n,
        compile_success_rate: metrics.iter().filter(|m| m.compile_success).count() as f64 / n * 100.0,
        avg_coagula: metrics.iter().map(|m| m.coagula_cycles as f64).sum::<f64>() / n,
        avg_duration: metrics.iter().map(|m| m.duration_secs).sum::<f64>() / n,
        avg_turns: metrics.iter().map(|m| m.conversation_turns as f64).sum::<f64>() / n,
    }
}

// ─── Formatting ─────────────────────────────────────────────────────────────

/// Format a summary of recent metrics.
pub fn format_summary(metrics: &[GenerationMetrics]) -> String {
    if metrics.is_empty() {
        return "No generation metrics recorded yet.\nRun 'isls forge-chat' or use the Cockpit to generate an app.".to_string();
    }
    let mut out = String::new();
    out.push_str(&format!("ISLS Generation Metrics — {} entries\n", metrics.len()));
    out.push_str("─────────────────────────────────────────────\n");
    for m in metrics.iter().rev().take(10) {
        out.push_str(&format!(
            "  {} | {} | {} entities | {} files | {} tokens | {:.1}s | {}\n",
            &m.timestamp[..10],
            m.source,
            m.entity_count,
            m.file_count,
            m.total_tokens,
            m.duration_secs,
            if m.compile_success { "✓" } else { "✗" },
        ));
    }
    out
}

/// Format a comparison table.
pub fn format_comparison(table: &ComparisonTable) -> String {
    let mut out = String::new();
    out.push_str("ISLS Generation Metrics — CLI vs Cockpit\n");
    out.push_str("─────────────────────────────────────────────────────\n");
    out.push_str(&format!("{:<22} {:>14} {:>16}\n", "", "CLI (Gen 1)", "Cockpit (Gen 2)"));
    out.push_str(&format!("{:<22} {:>14} {:>16}\n", "Generations:", table.cli.count, table.cockpit.count));
    out.push_str(&format!("{:<22} {:>14.1} {:>16.1}\n", "Avg entities:", table.cli.avg_entities, table.cockpit.avg_entities));
    out.push_str(&format!("{:<22} {:>14.0} {:>16.0}\n", "Avg tokens:", table.cli.avg_tokens, table.cockpit.avg_tokens));
    out.push_str(&format!("{:<22} {:>13.0}% {:>15.0}%\n", "Compile success:", table.cli.compile_success_rate, table.cockpit.compile_success_rate));
    out.push_str(&format!("{:<22} {:>14.2} {:>16.2}\n", "Avg coagula:", table.cli.avg_coagula, table.cockpit.avg_coagula));
    out.push_str(&format!("{:<22} {:>13.0}s {:>15.0}s\n", "Avg duration:", table.cli.avg_duration, table.cockpit.avg_duration));
    out.push_str(&format!("{:<22} {:>14.1} {:>16.1}\n", "Conversation turns:", table.cli.avg_turns, table.cockpit.avg_turns));
    out
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metrics(source: GenerationSource, entities: usize, success: bool) -> GenerationMetrics {
        let turns = if source == GenerationSource::Cockpit { 4 } else { 1 };
        GenerationMetrics {
            id: format!("test-{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)),
            timestamp: chrono::Utc::now().to_rfc3339(),
            source,
            description: "test app".to_string(),
            entity_count: entities,
            file_count: 20,
            structural_files: 10,
            llm_files: 10,
            total_tokens: 12000,
            compile_success: success,
            coagula_cycles: if success { 0 } else { 1 },
            duration_secs: 120.0,
            norms_activated: vec!["ISLS-NORM-0042".to_string()],
            conversation_turns: turns,
            contraction_ratios: vec![],
            was_contractive: true,
            mikro_gate_pass_rate: 1.0,
            meso_gate_pass_rate: 1.0,
            codematrix_avg: 0.0,
            sgb: 0.0,
        }
    }

    #[test]
    fn test_metrics_serialization() {
        let m = sample_metrics(GenerationSource::Cli, 5, true);
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"source\":\"cli\""));
        let parsed: GenerationMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.entity_count, 5);
        assert!(parsed.compile_success);
    }

    #[test]
    fn test_comparison_table() {
        let metrics = vec![
            sample_metrics(GenerationSource::Cli, 5, true),
            sample_metrics(GenerationSource::Cli, 3, false),
            sample_metrics(GenerationSource::Cockpit, 8, true),
            sample_metrics(GenerationSource::Cockpit, 7, true),
        ];
        let refs: Vec<&GenerationMetrics> = metrics.iter().collect();
        let table = compare_metrics(&metrics);
        assert_eq!(table.cli.count, 2);
        assert_eq!(table.cockpit.count, 2);
        assert!((table.cli.avg_entities - 4.0).abs() < 0.01);
        assert!((table.cockpit.avg_entities - 7.5).abs() < 0.01);
        assert!((table.cockpit.compile_success_rate - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_format_summary_empty() {
        let out = format_summary(&[]);
        assert!(out.contains("No generation metrics"));
    }

    #[test]
    fn test_format_comparison() {
        let metrics = vec![
            sample_metrics(GenerationSource::Cli, 5, true),
            sample_metrics(GenerationSource::Cockpit, 8, true),
        ];
        let table = compare_metrics(&metrics);
        let out = format_comparison(&table);
        assert!(out.contains("CLI (Gen 1)"));
        assert!(out.contains("Cockpit (Gen 2)"));
    }

    #[test]
    fn test_append_and_load() {
        // Use a temp dir to avoid polluting ~/.isls
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("metrics.jsonl");

        // Manually write and read
        let m = sample_metrics(GenerationSource::Cli, 5, true);
        let json = serde_json::to_string(&m).unwrap();
        std::fs::write(&path, format!("{}\n", json)).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let parsed: GenerationMetrics = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(parsed.entity_count, 5);
    }

    #[test]
    fn test_metrics_path() {
        // Should return Some path on any system with HOME set
        let path = metrics_path();
        // In CI, HOME might not be set, so we just test the function doesn't panic
        if let Some(p) = path {
            assert!(p.to_string_lossy().contains("metrics.jsonl"));
        }
    }

    #[test]
    fn test_source_display() {
        assert_eq!(format!("{}", GenerationSource::Cli), "cli");
        assert_eq!(format!("{}", GenerationSource::Cockpit), "cockpit");
    }
}
