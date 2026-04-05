// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! I3/W1 — Constraint Spectroscopy.
//!
//! Analyse a target system (given as a set of resonites), classify its
//! components into high-level architectural classes, compare them against
//! the available norm registry, and produce a list of gaps together with
//! concrete scrape-keyword suggestions.
//!
//! The module is deliberately keyword-based and deterministic — no LLM,
//! no network, no IO.  Resonites are expressed via a lightweight
//! [`Resonite`] enum local to this module so that `isls-norms` remains
//! a pure data crate with no dependency on `isls-reader` or
//! `isls-forge-llm`.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::types::Norm;
use crate::NormRegistry;

// ─── Resonite (lightweight) ──────────────────────────────────────────────────

/// Kind of type declaration recognised by the spectroscopy classifier.
#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum ResoniteTypeKind {
    Struct,
    Enum,
    Trait,
}

/// A lightweight "atomic observable" used by the spectroscopy module.
///
/// This mirrors `isls_forge_llm::codematrix::Resonite` but keeps the crate
/// boundary clean — the full Codematrix type lives in `isls-forge-llm`
/// while this module only needs name/kind information for classification.
#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum Resonite {
    /// A function or method.
    Fn {
        name: String,
        arity: usize,
    },
    /// A struct / enum / trait definition.
    Type {
        name: String,
        kind: ResoniteTypeKind,
    },
    /// An import / use statement.
    Import { path: String },
    /// A layer artefact (e.g. model, query, service …).
    Layer { depth: u8, artifact: String },
    /// A relation (foreign key, contains, uses …).
    Relation {
        source: String,
        target: String,
        kind: String,
    },
}

impl Resonite {
    /// Human-readable label for reports.
    pub fn label(&self) -> String {
        match self {
            Resonite::Fn { name, .. } => format!("fn {}", name),
            Resonite::Type { name, kind } => format!("{:?} {}", kind, name),
            Resonite::Import { path } => format!("use {}", path),
            Resonite::Layer { depth, artifact } => {
                format!("layer{}:{}", depth, artifact)
            }
            Resonite::Relation { source, target, kind } => {
                format!("rel[{}] {}→{}", kind, source, target)
            }
        }
    }
}

// ─── Resonite classification ────────────────────────────────────────────────

/// High-level architectural class of a resonite.
#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum ResoniteClass {
    CrudEntity,
    Authentication,
    Authorization,
    EventBus,
    MessageQueue,
    ConnectionPool,
    Caching,
    RateLimiting,
    CircuitBreaker,
    RetryPattern,
    Pagination,
    Search,
    FileUpload,
    ExportImport,
    Scheduling,
    Notification,
    Logging,
    Metrics,
    HealthCheck,
    Configuration,
    Migration,
    Testing,
    Docker,
    StateMachine,
    Workflow,
    IndicatorPipeline,
    SignalProcessing,
    RiskManagement,
    OrderExecution,
    DataVisualization,
    RealtimeWebSocket,
    GraphQLApi,
    GrpcService,
    CliInterface,
    PluginSystem,
    Custom(String),
}

impl ResoniteClass {
    /// Stable short name used in API responses / CLI output.
    pub fn as_str(&self) -> String {
        match self {
            ResoniteClass::Custom(s) => format!("Custom({})", s),
            other => format!("{:?}", other),
        }
    }
}

/// Classify a single resonite into one of [`ResoniteClass`].
///
/// The classifier is keyword-based and returns `None` for resonites that
/// do not match any known class.
pub fn classify_resonite(r: &Resonite) -> Option<ResoniteClass> {
    match r {
        Resonite::Fn { name, .. } => classify_fn_name(name),
        Resonite::Type { name, .. } => classify_type_name(name),
        Resonite::Import { path } => classify_import_path(path),
        Resonite::Layer { artifact, .. } => classify_layer(artifact),
        Resonite::Relation { .. } => None,
    }
}

fn classify_fn_name(name: &str) -> Option<ResoniteClass> {
    let n = name.to_lowercase();
    // CRUD verbs
    if n.starts_with("get_")
        || n.starts_with("list_")
        || n.starts_with("create_")
        || n.starts_with("update_")
        || n.starts_with("delete_")
    {
        return Some(ResoniteClass::CrudEntity);
    }
    // Auth
    if n.contains("login") || n.contains("logout") || n.contains("token")
        || n.contains("authenticate") || n.starts_with("auth_")
    {
        return Some(ResoniteClass::Authentication);
    }
    if n.contains("permission") || n.contains("authorize") || n.contains("role_check") {
        return Some(ResoniteClass::Authorization);
    }
    // Event / pub-sub
    if n.contains("emit") || n.contains("publish") || n.contains("subscribe")
        || n.contains("dispatch") || n.contains("on_event") || n.contains("handle_event")
    {
        return Some(ResoniteClass::EventBus);
    }
    if n.contains("enqueue") || n.contains("dequeue") || n.contains("job_queue") {
        return Some(ResoniteClass::MessageQueue);
    }
    if n.contains("cache") || n.contains("invalidate") || n.contains("memoize") {
        return Some(ResoniteClass::Caching);
    }
    if n.contains("pool") || n.contains("acquire_conn") || n.contains("release_conn") {
        return Some(ResoniteClass::ConnectionPool);
    }
    if n.contains("rate_limit") || n.contains("throttle") {
        return Some(ResoniteClass::RateLimiting);
    }
    if n.contains("circuit_break") || n.contains("trip_breaker") {
        return Some(ResoniteClass::CircuitBreaker);
    }
    if n.contains("retry") || n.contains("backoff") {
        return Some(ResoniteClass::RetryPattern);
    }
    if n.contains("paginate") || n.contains("page_of") {
        return Some(ResoniteClass::Pagination);
    }
    if n.contains("search") || n.contains("query_fulltext") {
        return Some(ResoniteClass::Search);
    }
    if n.contains("upload") || n.contains("multipart") {
        return Some(ResoniteClass::FileUpload);
    }
    if n.contains("export") || n.contains("import_csv") || n.contains("to_csv") {
        return Some(ResoniteClass::ExportImport);
    }
    if n.contains("schedule") || n.contains("cron") || n.contains("tick_") {
        return Some(ResoniteClass::Scheduling);
    }
    if n.contains("notify") || n.contains("send_email") || n.contains("send_sms") {
        return Some(ResoniteClass::Notification);
    }
    if n.contains("log_") || n.contains("tracing_") {
        return Some(ResoniteClass::Logging);
    }
    if n.contains("metric") || n.contains("prometheus") {
        return Some(ResoniteClass::Metrics);
    }
    if n.contains("health_check") || n == "healthz" || n == "readyz" {
        return Some(ResoniteClass::HealthCheck);
    }
    if n.contains("load_config") || n.contains("read_env") {
        return Some(ResoniteClass::Configuration);
    }
    if n.contains("migrate") {
        return Some(ResoniteClass::Migration);
    }
    if n.starts_with("test_") || n.contains("_test") {
        return Some(ResoniteClass::Testing);
    }
    if n.contains("state_transition") || n.contains("next_state") {
        return Some(ResoniteClass::StateMachine);
    }
    if n.contains("workflow") || n.contains("step_") {
        return Some(ResoniteClass::Workflow);
    }
    if n.contains("indicator") || n.contains("rsi") || n.contains("macd") || n.contains("ema") {
        return Some(ResoniteClass::IndicatorPipeline);
    }
    if n.contains("signal") || n.contains("fft") || n.contains("filter_signal") {
        return Some(ResoniteClass::SignalProcessing);
    }
    if n.contains("risk") || n.contains("kelly") || n.contains("var_") || n.contains("drawdown") {
        return Some(ResoniteClass::RiskManagement);
    }
    if n.contains("execute_order") || n.contains("place_order") || n.contains("cancel_order") {
        return Some(ResoniteClass::OrderExecution);
    }
    if n.contains("chart") || n.contains("plot") || n.contains("render_graph") {
        return Some(ResoniteClass::DataVisualization);
    }
    if n.contains("websocket") || n.contains("ws_") || n.contains("broadcast") {
        return Some(ResoniteClass::RealtimeWebSocket);
    }
    if n.contains("graphql") || n.contains("resolver") {
        return Some(ResoniteClass::GraphQLApi);
    }
    if n.contains("grpc") || n.contains("tonic_") {
        return Some(ResoniteClass::GrpcService);
    }
    if n.contains("cli_") || n.contains("parse_args") {
        return Some(ResoniteClass::CliInterface);
    }
    if n.contains("plugin") || n.contains("register_module") {
        return Some(ResoniteClass::PluginSystem);
    }
    None
}

fn classify_type_name(name: &str) -> Option<ResoniteClass> {
    let n = name.to_lowercase();
    if n.contains("event") && !n.contains("eventual") {
        return Some(ResoniteClass::EventBus);
    }
    if n.contains("handler") {
        return Some(ResoniteClass::EventBus);
    }
    if n.contains("queue") {
        return Some(ResoniteClass::MessageQueue);
    }
    if n.contains("cache") {
        return Some(ResoniteClass::Caching);
    }
    if n.contains("pool") {
        return Some(ResoniteClass::ConnectionPool);
    }
    if n.contains("ratelimit") || n.contains("throttle") {
        return Some(ResoniteClass::RateLimiting);
    }
    if n.contains("circuitbreaker") {
        return Some(ResoniteClass::CircuitBreaker);
    }
    if n.contains("statemachine") || n.contains("fsm") {
        return Some(ResoniteClass::StateMachine);
    }
    if n.contains("workflow") {
        return Some(ResoniteClass::Workflow);
    }
    if n.contains("indicator") {
        return Some(ResoniteClass::IndicatorPipeline);
    }
    if n.contains("signal") {
        return Some(ResoniteClass::SignalProcessing);
    }
    if n.contains("risk") {
        return Some(ResoniteClass::RiskManagement);
    }
    if n.contains("order") {
        return Some(ResoniteClass::OrderExecution);
    }
    if n.contains("chart") {
        return Some(ResoniteClass::DataVisualization);
    }
    if n.contains("websocket") || n.contains("ws") {
        return Some(ResoniteClass::RealtimeWebSocket);
    }
    if n.contains("graphql") {
        return Some(ResoniteClass::GraphQLApi);
    }
    if n.contains("grpc") {
        return Some(ResoniteClass::GrpcService);
    }
    if n.contains("config") {
        return Some(ResoniteClass::Configuration);
    }
    if n.contains("migration") {
        return Some(ResoniteClass::Migration);
    }
    if n.contains("plugin") {
        return Some(ResoniteClass::PluginSystem);
    }
    None
}

fn classify_import_path(path: &str) -> Option<ResoniteClass> {
    let p = path.to_lowercase();
    if p.contains("tokio::sync::broadcast") || p.contains("tokio::sync::mpsc") {
        return Some(ResoniteClass::EventBus);
    }
    if p.contains("deadpool") || p.contains("bb8") || p.contains("r2d2") || p.contains("sqlx::pool") {
        return Some(ResoniteClass::ConnectionPool);
    }
    if p.contains("moka") || p.contains("lru") || p.contains("cached") {
        return Some(ResoniteClass::Caching);
    }
    if p.contains("tower::limit") || p.contains("governor") {
        return Some(ResoniteClass::RateLimiting);
    }
    if p.contains("axum::extract::ws") || p.contains("tokio_tungstenite") {
        return Some(ResoniteClass::RealtimeWebSocket);
    }
    if p.contains("async_graphql") || p.contains("juniper") {
        return Some(ResoniteClass::GraphQLApi);
    }
    if p.contains("tonic") || p.contains("prost") {
        return Some(ResoniteClass::GrpcService);
    }
    if p.contains("tracing") {
        return Some(ResoniteClass::Logging);
    }
    if p.contains("prometheus") || p.contains("metrics_exporter") {
        return Some(ResoniteClass::Metrics);
    }
    if p.contains("reqwest_retry") || p.contains("backoff") {
        return Some(ResoniteClass::RetryPattern);
    }
    None
}

fn classify_layer(artifact: &str) -> Option<ResoniteClass> {
    let a = artifact.to_lowercase();
    if a.contains("migration") {
        Some(ResoniteClass::Migration)
    } else if a.contains("test") {
        Some(ResoniteClass::Testing)
    } else if a.contains("config") {
        Some(ResoniteClass::Configuration)
    } else if a.contains("auth") {
        Some(ResoniteClass::Authentication)
    } else {
        None
    }
}

// ─── SpectroscopyResult ─────────────────────────────────────────────────────

/// A missing norm-class in the target system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormGap {
    pub class: ResoniteClass,
    /// Number of resonites in the target that belong to this class.
    pub resonite_count: usize,
    /// Layer depths the resonites were observed on.
    pub layers_affected: Vec<u8>,
    /// Priority score: `resonite_count * (1 + distinct_layers)`.
    pub priority: f64,
}

/// A concrete scrape-keyword recommendation for a gap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeSuggestion {
    pub gap: String,
    pub keywords: Vec<String>,
    /// Heuristic number of repositories to fetch per keyword.
    pub estimated_repos: usize,
}

/// Result of running [`spectroscopy`] on a target / norm set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectroscopyResult {
    /// Every class detected in the target (deduplicated).
    pub target_spectrum: Vec<ResoniteClass>,
    /// Classes already covered by at least one existing norm.
    pub covered: Vec<ResoniteClass>,
    /// Classes in the target but missing from the norm registry.
    pub gaps: Vec<NormGap>,
    /// Suggested scrape campaigns for each gap.
    pub suggestions: Vec<ScrapeSuggestion>,
    /// `|covered| / |target_spectrum|` — `1.0` when the spectrum is empty.
    pub coverage: f64,
    /// Total number of classified resonites.
    pub classified_resonites: usize,
}

// ─── Analysis ───────────────────────────────────────────────────────────────

/// Analyse a target set of resonites against the norm registry.
///
/// Returns the full [`SpectroscopyResult`] with gaps, coverage and
/// scrape-keyword suggestions.
pub fn spectroscopy(resonites: &[Resonite], registry: &NormRegistry) -> SpectroscopyResult {
    // 1. Count classes in the target.
    let mut counts: HashMap<ResoniteClass, usize> = HashMap::new();
    let mut layers: HashMap<ResoniteClass, Vec<u8>> = HashMap::new();
    let mut classified = 0usize;
    for r in resonites {
        if let Some(class) = classify_resonite(r) {
            classified += 1;
            *counts.entry(class.clone()).or_insert(0) += 1;
            if let Resonite::Layer { depth, .. } = r {
                layers.entry(class).or_default().push(*depth);
            }
        }
    }

    // 2. Determine which classes the norm registry already covers.
    let covered_classes = registry_covered_classes(registry);

    // 3. Build the spectrum (deterministic order).
    let mut target_spectrum: Vec<ResoniteClass> = counts.keys().cloned().collect();
    target_spectrum.sort_by(|a, b| a.as_str().cmp(&b.as_str()));

    let mut covered: Vec<ResoniteClass> = Vec::new();
    let mut gaps: Vec<NormGap> = Vec::new();
    for class in &target_spectrum {
        if covered_classes.contains_key(class) {
            covered.push(class.clone());
        } else {
            let count = *counts.get(class).unwrap_or(&0);
            let mut class_layers: Vec<u8> =
                layers.get(class).cloned().unwrap_or_default();
            class_layers.sort_unstable();
            class_layers.dedup();
            let distinct = class_layers.len() as f64;
            let priority = count as f64 * (1.0 + distinct);
            gaps.push(NormGap {
                class: class.clone(),
                resonite_count: count,
                layers_affected: class_layers,
                priority,
            });
        }
    }
    // Sort gaps by descending priority for nicer reports.
    gaps.sort_by(|a, b| {
        b.priority
            .partial_cmp(&a.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 4. Build suggestions.
    let suggestions: Vec<ScrapeSuggestion> = gaps
        .iter()
        .map(|g| {
            let keywords = suggest_keywords_for_class(&g.class);
            ScrapeSuggestion {
                gap: g.class.as_str(),
                keywords,
                estimated_repos: 5,
            }
        })
        .filter(|s| !s.keywords.is_empty())
        .collect();

    let coverage = if target_spectrum.is_empty() {
        1.0
    } else {
        covered.len() as f64 / target_spectrum.len() as f64
    };

    SpectroscopyResult {
        target_spectrum,
        covered,
        gaps,
        suggestions,
        coverage,
        classified_resonites: classified,
    }
}

/// Return the set of classes that the norm registry already covers.
///
/// A class is considered covered when at least one norm has a trigger
/// keyword that maps to it.
pub fn registry_covered_classes(registry: &NormRegistry) -> BTreeMap<ResoniteClass, String> {
    let mut out: BTreeMap<ResoniteClass, String> = BTreeMap::new();
    for norm in registry.all_norms() {
        for class in norm_covered_classes(norm) {
            out.entry(class).or_insert_with(|| norm.id.clone());
        }
    }
    out
}

fn norm_covered_classes(norm: &Norm) -> Vec<ResoniteClass> {
    let mut haystack = norm.name.to_lowercase();
    haystack.push(' ');
    for t in &norm.triggers {
        for kw in &t.keywords {
            haystack.push_str(&kw.to_lowercase());
            haystack.push(' ');
        }
        for c in &t.concepts {
            haystack.push_str(&c.to_lowercase());
            haystack.push(' ');
        }
    }
    let mut out = Vec::new();
    for (class, needles) in class_keyword_index() {
        if needles.iter().any(|n| haystack.contains(n)) {
            out.push(class);
        }
    }
    out
}

/// Keyword → class map used for both suggestion generation and
/// norm-coverage detection. Each class is associated with ≥1 needle.
fn class_keyword_index() -> Vec<(ResoniteClass, Vec<&'static str>)> {
    vec![
        (ResoniteClass::CrudEntity, vec!["crud", "entity"]),
        (ResoniteClass::Authentication, vec!["auth", "login", "jwt", "token"]),
        (ResoniteClass::Authorization, vec!["authorization", "permission", "role"]),
        (ResoniteClass::EventBus, vec!["event", "pub-sub", "publish", "subscribe", "dispatch"]),
        (ResoniteClass::MessageQueue, vec!["queue", "mq", "broker"]),
        (ResoniteClass::ConnectionPool, vec!["pool", "deadpool", "bb8", "r2d2"]),
        (ResoniteClass::Caching, vec!["cache", "lru", "redis", "memoize"]),
        (ResoniteClass::RateLimiting, vec!["rate limit", "throttle", "governor"]),
        (ResoniteClass::CircuitBreaker, vec!["circuit breaker", "resilience"]),
        (ResoniteClass::RetryPattern, vec!["retry", "backoff"]),
        (ResoniteClass::Pagination, vec!["pagination", "paginate", "offset", "cursor"]),
        (ResoniteClass::Search, vec!["search", "fulltext", "tantivy"]),
        (ResoniteClass::FileUpload, vec!["upload", "multipart"]),
        (ResoniteClass::ExportImport, vec!["export", "csv", "import"]),
        (ResoniteClass::Scheduling, vec!["schedule", "cron"]),
        (ResoniteClass::Notification, vec!["notification", "notify", "email", "sms"]),
        (ResoniteClass::Logging, vec!["logging", "log", "tracing"]),
        (ResoniteClass::Metrics, vec!["metrics", "prometheus"]),
        (ResoniteClass::HealthCheck, vec!["health", "healthz", "readyz"]),
        (ResoniteClass::Configuration, vec!["config", "dotenv"]),
        (ResoniteClass::Migration, vec!["migration", "migrate"]),
        (ResoniteClass::Testing, vec!["test"]),
        (ResoniteClass::Docker, vec!["docker", "container"]),
        (ResoniteClass::StateMachine, vec!["state machine", "fsm"]),
        (ResoniteClass::Workflow, vec!["workflow", "pipeline"]),
        (ResoniteClass::IndicatorPipeline, vec!["indicator", "rsi", "macd", "ema"]),
        (ResoniteClass::SignalProcessing, vec!["signal", "fft", "filter"]),
        (ResoniteClass::RiskManagement, vec!["risk", "kelly", "var", "drawdown"]),
        (ResoniteClass::OrderExecution, vec!["order execution", "place order", "cancel order"]),
        (ResoniteClass::DataVisualization, vec!["chart", "plot", "visualization"]),
        (ResoniteClass::RealtimeWebSocket, vec!["websocket", "realtime", "ws"]),
        (ResoniteClass::GraphQLApi, vec!["graphql"]),
        (ResoniteClass::GrpcService, vec!["grpc", "tonic", "protobuf"]),
        (ResoniteClass::CliInterface, vec!["cli", "clap", "argparse"]),
        (ResoniteClass::PluginSystem, vec!["plugin"]),
    ]
}

// ─── Keyword suggestions ────────────────────────────────────────────────────

/// Generate scrape keywords for a given [`NormGap`].
///
/// Thin wrapper over [`suggest_keywords_for_class`] that preserves the
/// signature from the I3 specification.
pub fn suggest_keywords(gap: &NormGap) -> Vec<String> {
    suggest_keywords_for_class(&gap.class)
}

/// Produce a set of GitHub search keywords suitable for filling the given
/// class gap.
pub fn suggest_keywords_for_class(class: &ResoniteClass) -> Vec<String> {
    let raw: &[&str] = match class {
        ResoniteClass::EventBus => &[
            "rust event bus async",
            "rust event driven architecture",
            "rust publish subscribe pattern",
        ],
        ResoniteClass::MessageQueue => &[
            "rust job queue worker",
            "rust message broker",
        ],
        ResoniteClass::Caching => &[
            "rust cache lru",
            "rust caching redis",
            "rust memoization",
        ],
        ResoniteClass::ConnectionPool => &[
            "rust connection pool database",
            "rust deadpool bb8 r2d2",
        ],
        ResoniteClass::RateLimiting => &[
            "rust rate limit tower",
            "rust rate limiter governor",
        ],
        ResoniteClass::CircuitBreaker => &[
            "rust circuit breaker resilience",
        ],
        ResoniteClass::RetryPattern => &[
            "rust retry backoff",
        ],
        ResoniteClass::Pagination => &[
            "rust pagination cursor offset",
        ],
        ResoniteClass::Search => &[
            "rust fulltext search tantivy",
        ],
        ResoniteClass::FileUpload => &[
            "rust file upload multipart",
        ],
        ResoniteClass::ExportImport => &[
            "rust csv export report",
        ],
        ResoniteClass::Scheduling => &[
            "rust scheduler cron job",
        ],
        ResoniteClass::Notification => &[
            "rust email smtp lettre",
            "rust push notification",
        ],
        ResoniteClass::Logging => &[
            "rust logging tracing structured",
        ],
        ResoniteClass::Metrics => &[
            "rust metrics prometheus export",
        ],
        ResoniteClass::HealthCheck => &[
            "rust health check endpoint",
        ],
        ResoniteClass::Configuration => &[
            "rust config toml dotenv",
        ],
        ResoniteClass::Migration => &[
            "rust sqlx migration",
        ],
        ResoniteClass::Testing => &[
            "rust integration test harness",
        ],
        ResoniteClass::StateMachine => &[
            "rust state machine fsm",
        ],
        ResoniteClass::Workflow => &[
            "rust workflow engine",
        ],
        ResoniteClass::IndicatorPipeline => &[
            "rust technical indicators trading",
            "rust financial analysis ta",
        ],
        ResoniteClass::SignalProcessing => &[
            "rust signal processing fft",
        ],
        ResoniteClass::RiskManagement => &[
            "rust risk management kelly criterion",
            "rust portfolio risk var",
        ],
        ResoniteClass::OrderExecution => &[
            "rust order execution trading",
        ],
        ResoniteClass::DataVisualization => &[
            "rust charting plotters",
        ],
        ResoniteClass::RealtimeWebSocket => &[
            "rust websocket realtime axum",
            "rust tokio tungstenite",
        ],
        ResoniteClass::GraphQLApi => &[
            "rust graphql async-graphql",
        ],
        ResoniteClass::GrpcService => &[
            "rust grpc tonic protobuf",
        ],
        ResoniteClass::CliInterface => &[
            "rust cli clap",
        ],
        ResoniteClass::PluginSystem => &[
            "rust plugin system dynamic",
        ],
        ResoniteClass::Authentication => &[
            "rust authentication jwt",
        ],
        ResoniteClass::Authorization => &[
            "rust authorization rbac",
        ],
        ResoniteClass::CrudEntity => &[
            "rust axum crud entity",
        ],
        ResoniteClass::Docker => &[
            "rust dockerfile multistage",
        ],
        ResoniteClass::Custom(_) => &[],
    };
    raw.iter().map(|s| s.to_string()).collect()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_fn_crud() {
        let r = Resonite::Fn {
            name: "list_products".into(),
            arity: 1,
        };
        assert_eq!(classify_resonite(&r), Some(ResoniteClass::CrudEntity));
    }

    #[test]
    fn classify_fn_event_bus() {
        let r = Resonite::Fn {
            name: "emit_trade_event".into(),
            arity: 2,
        };
        assert_eq!(classify_resonite(&r), Some(ResoniteClass::EventBus));
    }

    #[test]
    fn classify_type_cache() {
        let r = Resonite::Type {
            name: "LruCache".into(),
            kind: ResoniteTypeKind::Struct,
        };
        assert_eq!(classify_resonite(&r), Some(ResoniteClass::Caching));
    }

    #[test]
    fn spectroscopy_identifies_gap() {
        // Use an empty registry so builtin keyword coverage doesn't mask
        // the gaps we're trying to detect.
        let reg = NormRegistry::empty_without_persistence();
        let resonites = vec![
            Resonite::Fn { name: "emit_trade".into(), arity: 1 },
            Resonite::Fn { name: "subscribe_trades".into(), arity: 1 },
            Resonite::Type {
                name: "RsiIndicator".into(),
                kind: ResoniteTypeKind::Struct,
            },
        ];
        let result = spectroscopy(&resonites, &reg);
        let gap_classes: Vec<String> =
            result.gaps.iter().map(|g| g.class.as_str()).collect();
        assert!(gap_classes.contains(&"EventBus".to_string()));
        assert!(gap_classes.contains(&"IndicatorPipeline".to_string()));
        assert!(!result.suggestions.is_empty());
    }

    #[test]
    fn suggest_keywords_event_bus() {
        let kws = suggest_keywords_for_class(&ResoniteClass::EventBus);
        assert!(!kws.is_empty());
        assert!(kws.iter().any(|k| k.contains("event bus")));
    }

    #[test]
    fn empty_resonites_full_coverage() {
        let reg = NormRegistry::new_without_persistence();
        let result = spectroscopy(&[], &reg);
        assert_eq!(result.coverage, 1.0);
        assert!(result.gaps.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════
// I4/bonus: Auto-keyword extraction from scraped repos
// ═══════════════════════════════════════════════════════════════════

/// Words that produce low-quality keywords.  Keywords whose content words
/// (everything after the language prefix) consist entirely of these terms
/// are discarded.
const KEYWORD_BLACKLIST: &[&str] = &[
    "utils", "util", "lib", "core", "common", "helper",
    "helpers", "misc", "main", "app", "config", "mod",
    "test", "tests", "bench", "example", "examples",
    "internal", "private", "public", "types", "error",
    "errors", "base", "shared", "new", "old", "tmp",
    "temp", "default", "impl", "init", "setup",
];

/// Rust / stdlib import prefixes — not external crates.
const RUST_STDLIB_PREFIXES: &[&str] = &[
    "std::", "core::", "alloc::", "crate::", "super::", "self::",
];

/// Split a PascalCase (or camelCase) identifier into lower-case words.
///
/// `"ConnectionPool"` → `["connection", "pool"]`
/// `"HTTPClient"`     → `["h", "t", "t", "p", "client"]` (best-effort)
pub fn split_pascal_case(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in name.chars() {
        if ch.is_uppercase() && !current.is_empty() {
            words.push(current.to_lowercase());
            current.clear();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current.to_lowercase());
    }
    words
}

/// Returns `true` when the keyword is useful (not just a language prefix +
/// blacklisted filler word).
pub fn is_useful_keyword(keyword: &str) -> bool {
    let parts: Vec<&str> = keyword.split_whitespace().collect();
    // Must have at least "lang word" (2 parts).
    if parts.len() < 2 {
        return false;
    }
    // Every content word (after the language prefix) must not be blacklisted.
    let content = &parts[1..];
    !content.iter().all(|w| KEYWORD_BLACKLIST.contains(w))
}

/// Extract up to `max_keywords` search keywords from a scraped code corpus.
///
/// # Arguments
/// * `struct_names`  — type / class names found in the repo (from topology).
/// * `import_paths`  — raw import strings from all files:
///   - Python / TypeScript / Go: external imports are prefixed with `"ext:"`.
///   - Rust: external crates look like `"tokio"` or `"tokio::sync::..."`.
/// * `language`     — primary language of the repo (`"rust"`, `"python"`, etc.).
/// * `max_keywords` — cap on the number of returned keywords (spec default: 5).
///
/// Keywords have the form `"<lang> <concept>"`, e.g. `"rust connection pool"`.
pub fn extract_keywords_from_analysis(
    struct_names: &[String],
    import_paths: &[String],
    language: &str,
    max_keywords: usize,
) -> Vec<String> {
    use std::collections::BTreeSet;

    if language.is_empty() || language == "unknown" {
        return vec![];
    }

    let mut candidates: BTreeSet<String> = BTreeSet::new();

    // ── Type names → keywords ─────────────────────────────────────────
    for name in struct_names {
        let words = split_pascal_case(name);
        if words.len() >= 2 {
            let keyword = format!("{} {}", language, words.join(" "));
            if is_useful_keyword(&keyword) {
                candidates.insert(keyword);
            }
        }
    }

    // ── External imports → keywords ───────────────────────────────────
    for path in import_paths {
        // Python / TypeScript / Go: "ext:requests", "ext:github.com/pkg/errors"
        let crate_name = if let Some(ext) = path.strip_prefix("ext:") {
            // Go convention: "github.com/pkg/errors" → take last path component ("errors").
            // npm convention: "express/Request" → take first component ("express").
            let first_segment = ext.split('/').next().unwrap_or(ext);
            if first_segment.contains('.') {
                // Go-style host path → last component is the package
                ext.split('/').last().unwrap_or(ext).to_string()
            } else {
                // npm / Python style → package name is the first segment
                first_segment.to_string()
            }
        } else {
            // Rust: skip stdlib
            if RUST_STDLIB_PREFIXES.iter().any(|p| path.starts_with(p)) {
                continue;
            }
            // Rust external crate: "tokio::sync::mpsc" → "tokio"
            // "sqlx" → "sqlx"
            path.split("::").next().unwrap_or(path.as_str()).to_string()
        };

        let name = crate_name.trim();
        // Quality gates: length 3–30, at least one letter.
        if name.len() < 3 || name.len() > 30 || !name.chars().any(|c| c.is_alphabetic()) {
            continue;
        }
        let keyword = format!("{} {}", language, name.to_lowercase());
        if is_useful_keyword(&keyword) {
            candidates.insert(keyword);
        }
    }

    // ── Return top `max_keywords` in insertion (BTree) order ─────────
    candidates.into_iter().take(max_keywords).collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod auto_keywords_tests {
    use super::*;

    #[test]
    fn pascal_split_connection_pool() {
        let words = split_pascal_case("ConnectionPool");
        assert_eq!(words, vec!["connection", "pool"]);
    }

    #[test]
    fn pascal_split_event_handler() {
        let words = split_pascal_case("EventHandler");
        assert_eq!(words, vec!["event", "handler"]);
    }

    #[test]
    fn keyword_from_struct_name() {
        let kws = extract_keywords_from_analysis(
            &["ConnectionPool".to_string(), "EventBus".to_string()],
            &[],
            "rust",
            5,
        );
        assert!(kws.iter().any(|k| k == "rust connection pool"),
            "Expected 'rust connection pool', got {:?}", kws);
        assert!(kws.iter().any(|k| k == "rust event bus"),
            "Expected 'rust event bus', got {:?}", kws);
    }

    #[test]
    fn keyword_from_external_import() {
        let kws = extract_keywords_from_analysis(
            &[],
            &["ext:sqlx".to_string(), "ext:tokio".to_string()],
            "rust",
            5,
        );
        assert!(kws.iter().any(|k| k == "rust sqlx"),
            "Expected 'rust sqlx', got {:?}", kws);
        assert!(kws.iter().any(|k| k == "rust tokio"),
            "Expected 'rust tokio', got {:?}", kws);
    }

    #[test]
    fn blacklist_filters_utils() {
        let kws = extract_keywords_from_analysis(
            &["Utils".to_string(), "Helpers".to_string()],
            &[],
            "python",
            5,
        );
        // "python utils" and "python helpers" should be filtered out
        assert!(!kws.iter().any(|k| k.contains("utils")), "utils should be filtered: {:?}", kws);
    }

    #[test]
    fn max_keywords_respected() {
        let structs: Vec<String> = (0..20)
            .map(|i| format!("ServiceLayer{}", i))
            .collect();
        let kws = extract_keywords_from_analysis(&structs, &[], "go", 5);
        assert!(kws.len() <= 5, "Should not exceed max_keywords=5, got {}", kws.len());
    }

    #[test]
    fn stdlib_rust_skipped() {
        let kws = extract_keywords_from_analysis(
            &[],
            &["std::io".to_string(), "crate::models".to_string(), "core::fmt".to_string()],
            "rust",
            5,
        );
        assert!(kws.is_empty(), "Stdlib/crate imports should produce no keywords: {:?}", kws);
    }

    #[test]
    fn go_external_import_pkg() {
        let kws = extract_keywords_from_analysis(
            &[],
            &["ext:github.com/jackc/pgx".to_string()],
            "go",
            5,
        );
        assert!(kws.iter().any(|k| k == "go pgx"),
            "Expected 'go pgx', got {:?}", kws);
    }

    #[test]
    fn typescript_external_import() {
        let kws = extract_keywords_from_analysis(
            &[],
            &["ext:express/Request".to_string()],
            "typescript",
            5,
        );
        assert!(kws.iter().any(|k| k.contains("express")),
            "Expected express keyword, got {:?}", kws);
    }
}
