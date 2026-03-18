// isls-harness/src/report.rs
// HTML report generator, JSON export, status line

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::metrics::{AlertLevel, MetricSnapshot};
use crate::iterate::IterationItem;

// ─── System Overview ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemOverview {
    pub version: String,
    pub uptime_secs: u64,
    pub entity_count: usize,
    pub edge_count: usize,
    pub crystal_count: usize,
    pub storage_bytes: u64,
    pub generated_at: DateTime<Utc>,
}

// ─── Full Report ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FullReport {
    pub overview: SystemOverview,
    pub latest_metrics: MetricSnapshot,
    pub alerts: Vec<crate::metrics::Alert>,
    pub iteration_items: Vec<IterationItem>,
    pub health: AlertLevel,
    pub history_len: usize,
    /// Pre-rendered HTML fragment for the Validation Summary section (empty = omit)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub validation_html: String,
    /// Generative pipeline benchmark results (B16–B24, empty = omit section)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generative_bench_results: Vec<crate::bench::BenchResult>,
}

// ─── Report Generator ─────────────────────────────────────────────────────────

pub struct ReportGenerator;

impl ReportGenerator {
    /// Generate one-line status string (must complete within 1 second)
    pub fn status_line(
        overview: &SystemOverview,
        snap: &MetricSnapshot,
        health: &AlertLevel,
    ) -> String {
        let uptime_str = format_uptime(overview.uptime_secs);
        let health_str = match health {
            AlertLevel::Green => "GREEN",
            AlertLevel::Yellow => "YELLOW",
            AlertLevel::Red => "RED",
        };
        format!(
            "ISLS v{} | UP {} | Entities: {} | Edges: {} | Crystals: {}\n\
             L0: {} ({:.0} obs/s) | L1: {} ({:+} edges/h) | L2: {} ({} active constraints)\n\
             L3: {} ({:.1} crystals/day, MCI={:.2}) | L4: {} ({:.0} mutations pending)\n\
             Replay: {} | Free Energy: {:.2} | Gate sel.: {:.2} | Storage: {:.1} GB\n\
             Health: {}",
            overview.version,
            uptime_str,
            overview.entity_count,
            overview.edge_count,
            overview.crystal_count,
            if snap.m1_ingestion_rate > 0.0 { "OK" } else { "WARN" },
            snap.m1_ingestion_rate,
            if snap.m2_graph_growth >= 0 { "OK" } else { "WARN" },
            snap.m2_graph_growth,
            if snap.m3_active_constraints > 0 { "OK" } else { "WARN" },
            snap.m3_active_constraints,
            if snap.m4_crystal_rate >= 0.0 { "OK" } else { "WARN" },
            snap.m4_crystal_rate,
            snap.m10_dual_consensus_mci,
            if snap.m5_mutation_rate >= 0.0 { "OK" } else { "WARN" },
            snap.m5_mutation_rate,
            if snap.m6_replay_fidelity >= 1.0 { "PASS" } else { "FAIL" },
            snap.m8_lattice_stability,
            snap.m9_gate_selectivity,
            overview.storage_bytes as f64 / 1e9,
            health_str,
        )
    }

    /// Generate JSON report
    pub fn json(report: &FullReport) -> String {
        serde_json::to_string_pretty(report).unwrap_or_default()
    }

    /// Generate self-contained HTML report with inline CSS and SVG sparklines
    pub fn html(report: &FullReport) -> String {
        let snap = &report.latest_metrics;
        let health_color = match report.health {
            AlertLevel::Green => "#059669",
            AlertLevel::Yellow => "#D97706",
            AlertLevel::Red => "#DC2626",
        };
        let health_str = match report.health {
            AlertLevel::Green => "GREEN",
            AlertLevel::Yellow => "YELLOW",
            AlertLevel::Red => "RED",
        };

        // Build metric rows
        let layer_rows = format!(
            "{}\n{}\n{}\n{}\n{}",
            metric_row("M1", "L0 Ingestion Rate", &format!("{:.1} obs/s", snap.m1_ingestion_rate), snap.m1_ingestion_rate > 0.0),
            metric_row("M2", "L1 Graph Growth", &format!("{:+} nodes+edges", snap.m2_graph_growth), snap.m2_graph_growth >= 0),
            metric_row("M3", "L2 Active Constraints", &format!("{}", snap.m3_active_constraints), snap.m3_active_constraints >= 1),
            metric_row("M4", "L3 Crystal Rate", &format!("{:.1}/24h", snap.m4_crystal_rate), snap.m4_crystal_rate >= 0.0),
            metric_row("M5", "L4 Mutation Rate", &format!("{:.0}/24h", snap.m5_mutation_rate), snap.m5_mutation_rate >= 0.0),
        );

        let quality_rows = format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            metric_row("M6", "Replay Fidelity", &format!("{:.1}%", snap.m6_replay_fidelity * 100.0), snap.m6_replay_fidelity >= 1.0),
            metric_row("M7", "Convergence Rate", &format!("{:.4}", snap.m7_convergence_rate), true),
            metric_row("M8", "Lattice Stability (F̄)", &format!("{:.3}", snap.m8_lattice_stability), snap.m8_lattice_stability < 0.0),
            metric_row("M9", "Gate Selectivity", &format!("{:.3}", snap.m9_gate_selectivity), snap.m9_gate_selectivity > 0.01 && snap.m9_gate_selectivity <= 0.30),
            metric_row("M10", "Dual Consensus MCI", &format!("{:.3}", snap.m10_dual_consensus_mci), snap.m10_dual_consensus_mci >= 0.90),
            metric_row("M11", "PoR Latency", &format!("{:.2}s", snap.m11_por_latency_secs), snap.m11_por_latency_secs >= 0.0),
            metric_row("M12", "Evidence Integrity", &format!("{:.1}%", snap.m12_evidence_integrity * 100.0), snap.m12_evidence_integrity >= 1.0),
            metric_row("M13", "Operator Version Drift", &format!("{}", snap.m13_operator_version_drift), snap.m13_operator_version_drift == 0),
            metric_row("M14", "Storage Efficiency", &format!("{:.1} MB/asset", snap.m14_storage_efficiency_bytes as f64 / 1_048_576.0), snap.m14_storage_efficiency_bytes < 3 * 1024 * 1024),
        );

        let perf_rows = format!(
            "{}\n{}\n{}\n{}\n{}",
            metric_row("M15", "Macro-Step Latency", &format!("{:.2}s", snap.m15_macro_step_latency_secs), snap.m15_macro_step_latency_secs < 60.0),
            metric_row("M16", "Memory Footprint", &format!("{:.1} GB", snap.m16_memory_footprint_bytes as f64 / 1e9), snap.m16_memory_footprint_bytes < 4 * 1024 * 1024 * 1024),
            metric_row("M17", "Extraction Throughput", &format!("{:.0} cand/s", snap.m17_extraction_throughput), snap.m17_extraction_throughput > 100.0),
            metric_row("M18", "Archive Growth Rate", &format!("{:.1} MB/day", snap.m18_archive_growth_bytes_per_day as f64 / 1e6), true),
            metric_row("M19", "Carrier Migration Latency", &format!("{:.2}s", snap.m19_carrier_migration_latency_secs), snap.m19_carrier_migration_latency_secs < 5.0),
        );

        let empirical_rows = format!(
            "{}\n{}\n{}\n{}\n{}",
            metric_row("M20", "Constraint Hit Rate", &format!("{:.1}%", snap.m20_constraint_hit_rate * 100.0), snap.m20_constraint_hit_rate > 0.6),
            metric_row("M21", "Crystal Predictive Value", &format!("{:.1}%", snap.m21_crystal_predictive_value * 100.0), snap.m21_crystal_predictive_value > 0.5),
            metric_row("M22", "Signal Lead Time", &format!("{:.0}s", snap.m22_signal_lead_time_secs), snap.m22_signal_lead_time_secs > 0.0),
            metric_row("M23", "Basket Quality Lift", &format!("{:.3}", snap.m23_basket_quality_lift), snap.m23_basket_quality_lift > 0.0),
            metric_row("M24", "Coverage Growth", &format!("{} entities", snap.m24_coverage_growth), true),
        );

        // Alerts section
        let alerts_html = if report.alerts.is_empty() {
            "<p style='color:#059669'>No active alerts.</p>".to_string()
        } else {
            let rows: String = report.alerts.iter().map(|a| {
                format!("<tr><td>{}</td><td>{}</td><td>{:.4}</td><td>{}</td></tr>",
                    a.metric_id, a.metric_name, a.current_value, a.message)
            }).collect();
            format!("<table><thead><tr><th>ID</th><th>Metric</th><th>Value</th><th>Message</th></tr></thead><tbody>{}</tbody></table>", rows)
        };

        // Iteration guidance
        let iteration_html = if report.iteration_items.is_empty() {
            "<p style='color:#059669'>No action items. System is healthy.</p>".to_string()
        } else {
            let rows: String = report.iteration_items.iter().map(|item| {
                let prio_color = match item.priority {
                    crate::iterate::Priority::P0 => "#DC2626",
                    crate::iterate::Priority::P1 => "#D97706",
                    crate::iterate::Priority::P2 => "#0369A1",
                };
                format!(
                    "<tr><td style='color:{};font-weight:bold'>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    prio_color, item.priority, item.metric_id, item.symptom, item.diagnosis, item.action
                )
            }).collect();
            format!("<table><thead><tr><th>Priority</th><th>Metric</th><th>Symptom</th><th>Diagnosis</th><th>Action</th></tr></thead><tbody>{}</tbody></table>", rows)
        };

        // Sparkline SVG (simple bar chart from last 30 history points for M8)
        let sparkline_svg = "<svg width='200' height='30' viewBox='0 0 200 30'>\
            <text x='0' y='20' font-size='10' fill='gray'>sparklines available in live mode</text>\
            </svg>";

        // Section 11: Generative Pipeline Benchmarks (B16–B24)
        let generative_section = if report.generative_bench_results.is_empty() {
            String::new()
        } else {
            let rows: String = report.generative_bench_results.iter().map(|r| {
                format!("<tr><td>{}</td><td>{}</td><td>{:.3}</td><td>{}</td></tr>",
                    r.bench_id, r.metric_name, r.metric_value, r.metric_unit)
            }).collect();
            format!(
                "<h2>11. Generative Pipeline Benchmarks (B16\u{2013}B24)</h2>\n\
                 <table><thead><tr><th>ID</th><th>Metric</th><th>Value</th><th>Unit</th></tr></thead>\
                 <tbody>{rows}</tbody></table>"
            )
        };

        format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>ISLS Dashboard — {generated_at}</title>
<style>
* {{ box-sizing: border-box; margin: 0; padding: 0; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', monospace; background: #0F172A; color: #E2E8F0; padding: 20px; }}
h1 {{ color: #38BDF8; font-size: 1.5em; margin-bottom: 4px; }}
h2 {{ color: #0EA5E9; font-size: 1.1em; margin: 16px 0 8px; border-bottom: 1px solid #1E3A5F; padding-bottom: 4px; }}
.health-badge {{ display: inline-block; background: {health_color}; color: white; padding: 4px 12px; border-radius: 4px; font-weight: bold; font-size: 1.2em; margin: 8px 0; }}
.overview {{ background: #1E293B; border-radius: 8px; padding: 12px; margin-bottom: 16px; display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 8px; }}
.stat {{ text-align: center; }}
.stat-value {{ font-size: 1.4em; font-weight: bold; color: #38BDF8; }}
.stat-label {{ font-size: 0.75em; color: #94A3B8; }}
table {{ width: 100%; border-collapse: collapse; background: #1E293B; border-radius: 8px; overflow: hidden; margin-bottom: 16px; }}
th {{ background: #0F172A; color: #94A3B8; font-size: 0.75em; text-align: left; padding: 8px 12px; }}
td {{ padding: 6px 12px; font-size: 0.85em; border-bottom: 1px solid #0F172A; }}
.ok {{ color: #059669; }}
.warn {{ color: #DC2626; }}
.section {{ background: #1E293B; border-radius: 8px; padding: 16px; margin-bottom: 16px; }}
footer {{ color: #475569; font-size: 0.75em; text-align: center; margin-top: 24px; }}
</style>
</head>
<body>
<h1>ISLS Validation Dashboard</h1>
<div class="health-badge">{health_str}</div>
<p style="color:#94A3B8;font-size:0.8em">Generated: {generated_at} | Tick: {tick}</p>

<div class="overview">
  <div class="stat"><div class="stat-value">{entity_count}</div><div class="stat-label">Entities</div></div>
  <div class="stat"><div class="stat-value">{edge_count}</div><div class="stat-label">Edges</div></div>
  <div class="stat"><div class="stat-value">{crystal_count}</div><div class="stat-label">Crystals</div></div>
  <div class="stat"><div class="stat-value">{uptime}</div><div class="stat-label">Uptime</div></div>
  <div class="stat"><div class="stat-value">{storage}</div><div class="stat-label">Storage</div></div>
</div>

<h2>1. Layer Health (M1–M5)</h2>
<table>
<thead><tr><th>ID</th><th>Metric</th><th>Value</th><th>Status</th></tr></thead>
<tbody>{layer_rows}</tbody>
</table>

<h2>2. Core Quality (M6–M14)</h2>
<table>
<thead><tr><th>ID</th><th>Metric</th><th>Value</th><th>Status</th></tr></thead>
<tbody>{quality_rows}</tbody>
</table>

<h2>3. Performance (M15–M19)</h2>
<table>
<thead><tr><th>ID</th><th>Metric</th><th>Value</th><th>Status</th></tr></thead>
<tbody>{perf_rows}</tbody>
</table>

<h2>4. Empirical Domain (M20–M24)</h2>
<table>
<thead><tr><th>ID</th><th>Metric</th><th>Value</th><th>Status</th></tr></thead>
<tbody>{empirical_rows}</tbody>
</table>

<h2>5. Active Alerts</h2>
<div class="section">{alerts_html}</div>

<h2>6. Action Items (Iteration Guidance)</h2>
<div class="section">{iteration_html}</div>

<h2>7. Sparklines</h2>
<div class="section">{sparkline_svg}</div>

{validation_section}

{generative_section}

<footer>Generated by ISLS v1.0.0 — deterministic, append-only, replay-verified</footer>
</body>
</html>"#,
            generated_at = report.overview.generated_at.format("%Y-%m-%d %H:%M:%S UTC"),
            health_color = health_color,
            health_str = health_str,
            tick = snap.tick,
            entity_count = report.overview.entity_count,
            edge_count = report.overview.edge_count,
            crystal_count = report.overview.crystal_count,
            uptime = format_uptime(report.overview.uptime_secs),
            storage = format!("{:.1} GB", report.overview.storage_bytes as f64 / 1e9),
            layer_rows = layer_rows,
            quality_rows = quality_rows,
            perf_rows = perf_rows,
            empirical_rows = empirical_rows,
            alerts_html = alerts_html,
            iteration_html = iteration_html,
            sparkline_svg = sparkline_svg,
            validation_section = if report.validation_html.is_empty() {
                String::new()
            } else {
                format!("<h2>8. Validation Summary</h2><div class=\"section\">{}</div>",
                    report.validation_html)
            },
            generative_section = generative_section,
        )
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

fn metric_row(id: &str, name: &str, value: &str, healthy: bool) -> String {
    let status = if healthy {
        "<span class='ok'>OK</span>"
    } else {
        "<span class='warn'>ALERT</span>"
    };
    format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>", id, name, value, status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MetricSnapshot;

    fn make_report() -> FullReport {
        FullReport {
            overview: SystemOverview {
                version: "1.0.0".to_string(),
                uptime_secs: 3600,
                entity_count: 100,
                edge_count: 500,
                crystal_count: 5,
                storage_bytes: 1_000_000,
                generated_at: Utc::now(),
            },
            latest_metrics: MetricSnapshot::default(),
            alerts: vec![],
            iteration_items: vec![],
            health: AlertLevel::Green,
            history_len: 10,
            validation_html: String::new(),
            generative_bench_results: vec![],
        }
    }

    #[test]
    fn test_status_line_format() {
        let report = make_report();
        let line = ReportGenerator::status_line(
            &report.overview,
            &report.latest_metrics,
            &report.health,
        );
        assert!(line.contains("ISLS v1.0.0"));
        assert!(line.contains("GREEN"));
        assert!(line.contains("Entities:"));
        assert!(line.contains("Crystals:"));
        assert!(line.contains("Health:"));
    }

    #[test]
    fn test_json_report_is_valid_json() {
        let report = make_report();
        let json = ReportGenerator::json(&report);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed.get("overview").is_some());
        assert!(parsed.get("health").is_some());
    }

    #[test]
    fn test_html_report_contains_sections() {
        let report = make_report();
        let html = ReportGenerator::html(&report);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Layer Health"));
        assert!(html.contains("Core Quality"));
        assert!(html.contains("Performance"));
        assert!(html.contains("Empirical Domain"));
        assert!(html.contains("Active Alerts"));
        assert!(html.contains("Action Items"));
        assert!(html.contains("GREEN"));
    }

    #[test]
    fn test_html_has_no_external_deps() {
        let report = make_report();
        let html = ReportGenerator::html(&report);
        // No external URLs in src= or href= (only inline styles)
        assert!(!html.contains("https://"), "HTML should have no external URLs");
        assert!(!html.contains("http://"), "HTML should have no external HTTP URLs");
    }

    #[test]
    fn test_html_contains_all_24_metrics() {
        let report = make_report();
        let html = ReportGenerator::html(&report);
        for i in 1..=24 {
            let mid = format!(">M{}<", i);
            assert!(html.contains(&mid), "Missing metric M{} in HTML report", i);
        }
    }

    #[test]
    fn test_format_uptime() {
        assert_eq!(format_uptime(0), "0m");
        assert_eq!(format_uptime(3600), "1h 0m");
        assert_eq!(format_uptime(86400), "1d 0h");
        assert_eq!(format_uptime(90061), "1d 1h");
    }

    #[test]
    fn test_status_line_within_1_second() {
        use std::time::Instant;
        let report = make_report();
        let start = Instant::now();
        let _ = ReportGenerator::status_line(&report.overview, &report.latest_metrics, &report.health);
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 1000, "status_line took {}ms, must be < 1000ms", elapsed.as_millis());
    }
}
