// isls-norms/src/targets.rs — MC1: Mission Control Target Systems
//
// Operators define target systems (applications they want the system to
// be able to forge). Each target has required ResoniteClasses, a live
// coverage score, and a status (Ready/Partial/NotReady).

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::spectroscopy::ResoniteClass;
use crate::types::Norm;

// ─── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSystem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub required_classes: Vec<ResoniteClass>,
    pub priority: u8,
    pub coverage: f64,
    pub missing: Vec<ResoniteClass>,
    pub status: TargetStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetStatus {
    Ready,
    Partial,
    NotReady,
}

/// Maximum number of target systems allowed.
pub const MAX_TARGETS: usize = 20;

// ─── Requirement extraction from description ────────────────────────────────

pub fn extract_requirements_from_description(desc: &str) -> Vec<ResoniteClass> {
    let d = desc.to_lowercase();
    let mut reqs = vec![ResoniteClass::CrudEntity, ResoniteClass::Authentication];

    if d.contains("websocket") || d.contains("realtime") || d.contains("echtzeit") || d.contains("live") {
        reqs.push(ResoniteClass::RealtimeWebSocket);
    }
    if d.contains("chart") || d.contains("graph") || d.contains("visuali") {
        reqs.push(ResoniteClass::DataVisualization);
    }
    if d.contains("upload") || d.contains("datei") || d.contains("file") || d.contains("image") || d.contains("bild") {
        reqs.push(ResoniteClass::FileUpload);
    }
    if d.contains("email") || d.contains("notification") || d.contains("benachrichtigung") {
        reqs.push(ResoniteClass::Notification);
    }
    if d.contains("status") || d.contains("workflow") || d.contains("kanban") || d.contains("state machine") {
        reqs.push(ResoniteClass::StateMachine);
    }
    if d.contains("cache") || d.contains("redis") {
        reqs.push(ResoniteClass::Caching);
    }
    if d.contains("rate limit") || d.contains("throttl") {
        reqs.push(ResoniteClass::RateLimiting);
    }
    if d.contains("search") || d.contains("suche") || d.contains("volltext") {
        reqs.push(ResoniteClass::Search);
    }
    if d.contains("cart") || d.contains("warenkorb") || d.contains("checkout") {
        reqs.push(ResoniteClass::Custom("Cart".into()));
    }
    if d.contains("payment") || d.contains("zahlung") || d.contains("stripe") {
        reqs.push(ResoniteClass::Custom("Payment".into()));
    }
    if d.contains("schedule") || d.contains("cron") || d.contains("timer") {
        reqs.push(ResoniteClass::Scheduling);
    }
    if d.contains("export") || d.contains("csv") || d.contains("pdf") {
        reqs.push(ResoniteClass::ExportImport);
    }
    if d.contains("chat") || d.contains("messaging") || d.contains("nachricht") {
        reqs.push(ResoniteClass::MessageQueue);
    }
    if d.contains("indicator") || d.contains("trading") || d.contains("signal") {
        reqs.push(ResoniteClass::Custom("IndicatorPipeline".into()));
    }
    if d.contains("risk") || d.contains("risiko") || d.contains("portfolio") {
        reqs.push(ResoniteClass::Custom("RiskCalculation".into()));
    }
    if d.contains("tag") || d.contains("label") || d.contains("kategorie") {
        reqs.push(ResoniteClass::Custom("Tagging".into()));
    }
    if d.contains("proxy") || d.contains("gateway") {
        reqs.push(ResoniteClass::Custom("ProxyPattern".into()));
    }
    if d.contains("api version") || d.contains("versionierung") {
        reqs.push(ResoniteClass::Custom("APIVersioning".into()));
    }
    if d.contains("pagination") || d.contains("seite") || d.contains("blätter") {
        reqs.push(ResoniteClass::Pagination);
    }
    if d.contains("docker") || d.contains("container") || d.contains("deploy") {
        reqs.push(ResoniteClass::Docker);
    }

    reqs.sort();
    reqs.dedup();
    reqs
}

// ─── Coverage computation ───────────────────────────────────────────────────

/// Classify which ResoniteClasses a norm covers, from keywords/concepts.
fn classify_norm_resonites(norm: &Norm) -> BTreeSet<String> {
    let all_keywords: Vec<String> = norm.triggers.iter()
        .flat_map(|t| t.keywords.iter().chain(t.concepts.iter()))
        .cloned()
        .collect();
    let joined = all_keywords.join(" ").to_lowercase();

    let mut classes = BTreeSet::new();
    let mappings: &[(&str, &str)] = &[
        ("crud", "CrudEntity"), ("entity", "CrudEntity"),
        ("auth", "Authentication"), ("jwt", "Authentication"), ("login", "Authentication"),
        ("pagination", "Pagination"), ("page", "Pagination"),
        ("search", "Search"),
        ("file", "FileUpload"), ("upload", "FileUpload"),
        ("state machine", "StateMachine"), ("status", "StateMachine"), ("workflow", "StateMachine"),
        ("notification", "Notification"), ("email", "Notification"),
        ("websocket", "RealtimeWebSocket"), ("realtime", "RealtimeWebSocket"),
        ("cache", "Caching"),
        ("rate limit", "RateLimiting"),
        ("graphql", "GraphQLApi"),
        ("export", "ExportImport"), ("import", "ExportImport"),
        ("schedule", "Scheduling"), ("cron", "Scheduling"),
        ("health", "HealthCheck"),
        ("log", "Logging"),
        ("metric", "Metrics"),
        ("docker", "Docker"),
        ("chart", "DataVisualization"), ("dashboard", "DataVisualization"), ("visuali", "DataVisualization"),
        ("indicator", "Custom(IndicatorPipeline)"), ("trading", "Custom(IndicatorPipeline)"),
        ("risk", "Custom(RiskCalculation)"),
        ("cart", "Custom(Cart)"), ("warenkorb", "Custom(Cart)"),
        ("payment", "Custom(Payment)"), ("zahlung", "Custom(Payment)"),
        ("tag", "Custom(Tagging)"), ("label", "Custom(Tagging)"),
        ("proxy", "Custom(ProxyPattern)"), ("gateway", "Custom(ProxyPattern)"),
        ("config", "Configuration"), ("error", "Configuration"),
        ("migration", "Migration"),
    ];

    for (keyword, class) in mappings {
        if joined.contains(keyword) {
            classes.insert(class.to_string());
        }
    }
    classes
}

/// Compute the coverage of a target system against the available norms.
pub fn compute_target_coverage(
    target: &mut TargetSystem,
    norms: &[Norm],
    fitness: &std::collections::HashMap<String, f64>,
) {
    let mut covered = Vec::new();
    let mut missing = Vec::new();

    for rc in &target.required_classes {
        let class_str = rc.as_str();
        let is_covered = norms.iter().any(|n| {
            let phi = fitness.get(&n.id).copied().unwrap_or(0.5);
            phi > 0.3 && classify_norm_resonites(n).iter().any(|c| *c == class_str)
        });
        if is_covered {
            covered.push(rc.clone());
        } else {
            missing.push(rc.clone());
        }
    }

    let total = target.required_classes.len().max(1);
    target.coverage = covered.len() as f64 / total as f64;
    target.missing = missing;
    target.status = if target.coverage >= 1.0 {
        TargetStatus::Ready
    } else if target.coverage >= 0.5 {
        TargetStatus::Partial
    } else {
        TargetStatus::NotReady
    };
}

// ─── Auto-Steer: find keywords for missing classes ──────────────────────────

/// Given missing ResoniteClasses, suggest scrape keywords to fill gaps.
pub fn keywords_for_missing_classes(missing: &[ResoniteClass]) -> Vec<String> {
    let mut keywords = Vec::new();
    for rc in missing {
        let kws: Vec<&str> = match rc {
            ResoniteClass::RealtimeWebSocket => vec!["rust websocket actix-web tokio-tungstenite"],
            ResoniteClass::DataVisualization => vec!["rust chart plotters dashboard visualization"],
            ResoniteClass::FileUpload => vec!["rust file upload multipart axum actix"],
            ResoniteClass::Notification => vec!["rust email notification lettre smtp"],
            ResoniteClass::StateMachine => vec!["rust state machine workflow finite automaton"],
            ResoniteClass::Caching => vec!["rust redis cache moka in-memory"],
            ResoniteClass::RateLimiting => vec!["rust rate limiter tower middleware"],
            ResoniteClass::Search => vec!["rust full-text search tantivy meilisearch"],
            ResoniteClass::Scheduling => vec!["rust cron scheduler background job"],
            ResoniteClass::ExportImport => vec!["rust csv export pdf generator"],
            ResoniteClass::MessageQueue => vec!["rust message queue amqp nats"],
            ResoniteClass::Pagination => vec!["rust pagination offset cursor keyset"],
            ResoniteClass::Docker => vec!["rust dockerfile container deploy"],
            ResoniteClass::GraphQLApi => vec!["rust graphql async-graphql juniper"],
            ResoniteClass::HealthCheck => vec!["rust health check readiness liveness"],
            ResoniteClass::Logging => vec!["rust tracing logging structured"],
            ResoniteClass::Metrics => vec!["rust prometheus metrics opentelemetry"],
            ResoniteClass::Custom(s) => match s.as_str() {
                "IndicatorPipeline" => vec!["rust trading indicator technical analysis signal"],
                "RiskCalculation" => vec!["rust risk calculation portfolio monte carlo"],
                "Cart" => vec!["rust shopping cart e-commerce basket"],
                "Payment" => vec!["rust payment stripe checkout billing"],
                "Tagging" => vec!["rust tagging label category taxonomy"],
                "ProxyPattern" => vec!["rust reverse proxy gateway load balancer"],
                "APIVersioning" => vec!["rust api versioning middleware header"],
                _ => vec![],
            },
            _ => vec![],
        };
        for kw in kws {
            keywords.push(kw.to_string());
        }
    }
    keywords
}

/// Find the highest-priority target with lowest coverage.
pub fn highest_priority_deficit(targets: &[TargetSystem]) -> Option<&TargetSystem> {
    targets.iter()
        .filter(|t| t.status != TargetStatus::Ready)
        .min_by(|a, b| {
            a.priority.cmp(&b.priority)
                .then(a.coverage.partial_cmp(&b.coverage).unwrap_or(std::cmp::Ordering::Equal))
        })
}

// ─── Persistence ────────────────────────────────────────────────────────────

fn targets_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|h| PathBuf::from(h).join(".isls").join("targets.json"))
}

pub fn save_targets(targets: &[TargetSystem]) -> std::io::Result<()> {
    let p = match targets_path() {
        Some(p) => p,
        None => return Ok(()),
    };
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(targets).unwrap_or_default())?;
    Ok(())
}

pub fn load_targets() -> Vec<TargetSystem> {
    let p = match targets_path() {
        Some(p) => p,
        None => return vec![],
    };
    let content = match std::fs::read_to_string(&p) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    serde_json::from_str(&content).unwrap_or_default()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_requirements_trading() {
        let reqs = extract_requirements_from_description(
            "Trading Journal mit Charts, WebSocket-Feeds und Risiko-Analyse"
        );
        assert!(reqs.iter().any(|r| *r == ResoniteClass::RealtimeWebSocket),
            "Should detect WebSocket requirement");
        assert!(reqs.iter().any(|r| *r == ResoniteClass::DataVisualization),
            "Should detect DataVisualization from 'Charts'");
        assert!(reqs.iter().any(|r| matches!(r, ResoniteClass::Custom(s) if s == "RiskCalculation")),
            "Should detect RiskCalculation from 'Risiko'");
    }

    #[test]
    fn test_extract_requirements_ecommerce() {
        let reqs = extract_requirements_from_description(
            "E-Commerce Shop mit Warenkorb, Payment und Email-Benachrichtigungen"
        );
        assert!(reqs.iter().any(|r| matches!(r, ResoniteClass::Custom(s) if s == "Cart")),
            "Should detect Cart from 'Warenkorb'");
        assert!(reqs.iter().any(|r| matches!(r, ResoniteClass::Custom(s) if s == "Payment")),
            "Should detect Payment");
        assert!(reqs.iter().any(|r| *r == ResoniteClass::Notification),
            "Should detect Notification from 'Benachrichtigungen'");
    }

    #[test]
    fn test_coverage_computation() {
        let norms = crate::catalog::builtin_norms();
        let fitness = std::collections::HashMap::new();
        let mut target = TargetSystem {
            id: "test-1".into(),
            name: "Test".into(),
            description: "Test system".into(),
            required_classes: vec![ResoniteClass::CrudEntity, ResoniteClass::Authentication],
            priority: 1,
            coverage: 0.0,
            missing: vec![],
            status: TargetStatus::NotReady,
        };
        compute_target_coverage(&mut target, &norms, &fitness);
        // Builtins cover CRUD and Auth
        assert!(target.coverage > 0.0,
            "Coverage should be > 0 with builtin norms, got {}", target.coverage);
    }

    #[test]
    fn test_keywords_for_missing() {
        let missing = vec![ResoniteClass::RealtimeWebSocket, ResoniteClass::Caching];
        let kws = keywords_for_missing_classes(&missing);
        assert!(!kws.is_empty(), "Should produce keywords for missing classes");
        assert!(kws.iter().any(|k| k.contains("websocket")));
        assert!(kws.iter().any(|k| k.contains("redis") || k.contains("cache")));
    }

    #[test]
    fn test_highest_priority_deficit() {
        let targets = vec![
            TargetSystem {
                id: "a".into(), name: "A".into(), description: "".into(),
                required_classes: vec![], priority: 2, coverage: 0.3,
                missing: vec![], status: TargetStatus::NotReady,
            },
            TargetSystem {
                id: "b".into(), name: "B".into(), description: "".into(),
                required_classes: vec![], priority: 1, coverage: 0.8,
                missing: vec![], status: TargetStatus::Partial,
            },
        ];
        let best = highest_priority_deficit(&targets).unwrap();
        assert_eq!(best.id, "b", "Should pick highest priority (lowest number)");
    }
}
