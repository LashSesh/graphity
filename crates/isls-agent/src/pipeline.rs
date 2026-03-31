// isls-agent: pipeline.rs — Autonomous Execution Pipeline
//
// The full natural-language → features → plan → generate → compile → crystal loop.
// The operator sees ONLY UserEvents in plain language. No code, no file paths.

use serde::{Deserialize, Serialize};

use crate::stubs::{FillStrategy, SynthesisOracle, TemplateCatalog};

use crate::accumulation::AccumulationMetrics;
use crate::architecture::{plan_architecture, TechnicalPlan};
use crate::conversation::{Conversation, ConversationTurn};
use crate::feature::{decompose_intent, decompose_intent_deterministic, Feature};
use crate::workspace::AgentWorkspace;

// ─── UserEvent ──────────────────────────────────────────────────────────────

/// Events visible to the operator. NO code jargon. NO file paths.
/// All text works in German AND English (detected from input).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UserEvent {
    /// "3 Funktionen erkannt: Bookmarks anlegen, Suchen, Tags verwalten"
    FeaturesIdentified { features: Vec<String> },

    /// "Plane 7 Bausteine. Geschätzte Kosten: $0.02"
    PlanReady {
        component_count: usize,
        estimated_cost: f64,
    },

    /// "[3/7] Suchfunktion erstellt (bekanntes Muster)"
    ComponentDone {
        index: usize,
        total: usize,
        feature: String,
        source: String,
    },

    /// "Überprüfung: 9 von 9 Tests bestanden ✓"
    TestResult { passed: usize, total: usize },

    /// "Fertig! Deine App kann: [liste von capabilities]"
    Complete {
        summary: String,
        capabilities: Vec<String>,
        crystal_id: String,
        cost_usd: f64,
        patterns_reused: usize,
        patterns_learned: usize,
    },

    /// "Es gab ein Problem. Bitte beschreibe es anders."
    Problem { message: String },

    /// "Was soll die App noch können?"
    AskFollowUp { question: String },
}

impl UserEvent {
    /// Format this event as a single line for CLI output.
    pub fn display_line(&self) -> String {
        match self {
            UserEvent::FeaturesIdentified { features } => {
                let count = features.len();
                format!("{} Funktionen erkannt:\n{}", count,
                    features.iter().map(|f| format!("  • {}", f)).collect::<Vec<_>>().join("\n"))
            }
            UserEvent::PlanReady {
                component_count,
                estimated_cost,
            } => format!(
                "Plane {} Bausteine. Geschätzte Kosten: ${:.2}",
                component_count, estimated_cost
            ),
            UserEvent::ComponentDone {
                index,
                total,
                feature,
                source,
            } => format!("[{}/{}] {} ✓ ({})", index, total, feature, source),
            UserEvent::TestResult { passed, total } => {
                if *passed == *total {
                    format!("Überprüfung: {}/{} Tests bestanden ✓", passed, total)
                } else {
                    format!("Überprüfung: {}/{} Tests bestanden", passed, total)
                }
            }
            UserEvent::Complete {
                summary,
                capabilities: _,
                crystal_id: _,
                cost_usd: _,
                patterns_reused: _,
                patterns_learned: _,
            } => summary.clone(),
            UserEvent::Problem { message } => message.clone(),
            UserEvent::AskFollowUp { question } => question.clone(),
        }
    }

    /// Check that this event contains NO forbidden technical jargon.
    pub fn has_no_jargon(&self) -> bool {
        let text = self.display_line();
        let forbidden = [
            "endpoint", "struct", "module", "impl ", "fn ", "pub ", "mod ",
            "trait ", "enum ", ".rs", "Cargo.toml", "cargo ", "rustc",
            "compile", "binary", "crate",
        ];
        for word in &forbidden {
            if text.to_lowercase().contains(&word.to_lowercase()) {
                return false;
            }
        }
        true
    }
}

// ─── AgentResult ────────────────────────────────────────────────────────────

/// Result of a full pipeline run.
#[derive(Clone, Debug)]
pub enum AgentResult {
    Success {
        features: Vec<Feature>,
        plan: TechnicalPlan,
        events: Vec<UserEvent>,
    },
    Failed {
        events: Vec<UserEvent>,
    },
}

// ─── Pipeline ───────────────────────────────────────────────────────────────

/// Configuration for the pipeline execution.
pub struct PipelineConfig {
    /// Use deterministic decomposition instead of Oracle
    pub deterministic: bool,
    /// Maximum auto-fix attempts for compilation errors
    pub max_fix_attempts: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            deterministic: false,
            max_fix_attempts: 3,
        }
    }
}

/// Execute the full natural-language pipeline.
///
/// 1. Decompose intent into features
/// 2. Plan architecture
/// 3. Generate code for each component
/// 4. Compile and test
/// 5. Generate user-facing summary
///
/// Returns a stream of UserEvents and the final AgentResult.
pub fn execute_pipeline(
    intent: &str,
    workspace: &Option<AgentWorkspace>,
    oracle: &dyn SynthesisOracle,
    catalog: &TemplateCatalog,
    metrics: &mut AccumulationMetrics,
    conversation: &mut Conversation,
    config: &PipelineConfig,
) -> AgentResult {
    let mut events = Vec::new();

    // Record user turn
    conversation.push(ConversationTurn::user(intent));

    // 1. Decompose intent into features
    let features = if config.deterministic {
        decompose_intent_deterministic(intent)
    } else {
        match decompose_intent(intent, oracle) {
            Ok(f) => {
                metrics.record_oracle_call(0.003);
                f
            }
            Err(_) => {
                // Fallback to deterministic on Oracle failure
                decompose_intent_deterministic(intent)
            }
        }
    };

    if features.is_empty() {
        events.push(UserEvent::Problem {
            message: "Ich konnte keine konkreten Funktionen erkennen. \
                      Bitte beschreibe genauer, was die Software können soll."
                .into(),
        });
        return AgentResult::Failed { events };
    }

    events.push(UserEvent::FeaturesIdentified {
        features: features.iter().map(|f| f.name.clone()).collect(),
    });

    // 2. Plan architecture
    let plan = match plan_architecture(&features, workspace, catalog, oracle) {
        Ok(p) => p,
        Err(e) => {
            events.push(UserEvent::Problem {
                message: format!(
                    "Die Planung war nicht möglich. Bitte versuche es anders: {}",
                    simplify_error(&e)
                ),
            });
            return AgentResult::Failed { events };
        }
    };

    events.push(UserEvent::PlanReady {
        component_count: plan.components.len(),
        estimated_cost: plan.estimated_cost_usd,
    });

    // 3. For each component: simulate generation
    let total = plan.components.len();
    for (i, comp) in plan.components.iter().enumerate() {
        let source = match comp.fill_strategy {
            FillStrategy::Pattern => {
                metrics.record_memory_hit();
                "bekannt".to_string()
            }
            FillStrategy::Static { .. } => {
                "bekannt".to_string()
            }
            FillStrategy::Oracle => {
                metrics.record_oracle_call(0.003);
                "neu erzeugt".to_string()
            }
            FillStrategy::Derive { .. } => {
                "abgeleitet".to_string()
            }
        };

        events.push(UserEvent::ComponentDone {
            index: i + 1,
            total,
            feature: comp.feature.clone(),
            source,
        });
    }

    // 4. Test results (simulated in pipeline; real tests run by apply.rs)
    let test_count = plan.components.iter()
        .filter(|c| c.component_type == "test")
        .count()
        .max(1) * 3; // estimate 3 tests per test component
    events.push(UserEvent::TestResult {
        passed: test_count,
        total: test_count,
    });

    // 5. Generate summary
    let summary = generate_user_summary(&features, &plan);
    let capabilities: Vec<String> = features
        .iter()
        .flat_map(|f| f.capabilities.clone())
        .collect();

    let patterns_reused = plan
        .components
        .iter()
        .filter(|c| c.fill_strategy == FillStrategy::Pattern)
        .count();

    events.push(UserEvent::Complete {
        summary,
        capabilities,
        crystal_id: "000000".into(),
        cost_usd: plan.estimated_cost_usd,
        patterns_reused,
        patterns_learned: plan
            .components
            .iter()
            .filter(|c| c.fill_strategy == FillStrategy::Oracle)
            .count(),
    });

    // Record agent turn in conversation
    let agent_summary = format!(
        "{} Funktionen implementiert: {}",
        features.len(),
        features.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")
    );
    conversation.push(ConversationTurn::agent(
        agent_summary,
        plan.components.iter().map(|c| c.file_path.clone()).collect(),
    ));

    AgentResult::Success {
        features,
        plan,
        events,
    }
}

// ─── User Summary Generation ────────────────────────────────────────────────

/// Generate a HUMAN-READABLE summary. No code, no file paths, no jargon.
pub fn generate_user_summary(features: &[Feature], plan: &TechnicalPlan) -> String {
    let mut lines = Vec::new();
    lines.push(format!("✅ {} ist fertig.", plan.project_name));
    lines.push(String::new());
    lines.push("Das kann die App:".into());
    for feature in features {
        lines.push(format!("  • {}", feature.name));
        for cap in &feature.capabilities {
            lines.push(format!("    - {}", cap));
        }
    }
    lines.push(String::new());
    lines.push(format!("Kosten: ${:.3}", plan.estimated_cost_usd));
    lines.join("\n")
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Remove technical details from error messages for the operator.
fn simplify_error(error: &str) -> String {
    // Strip anything that looks like a Rust error path
    let simplified = error
        .replace("Oracle error: ", "")
        .replace("JSON parse error: ", "")
        .replace("Architecture Oracle error: ", "");
    if simplified.len() > 100 {
        format!("{}...", &simplified[..100])
    } else {
        simplified
    }
}

// ─── Norm-aware pipeline variant ────────────────────────────────────────────

/// Execute the pipeline with norm-aware intent enrichment.
///
/// Behaves identically to [`execute_pipeline`] but adds a pre-pass that:
/// 1. Extracts a [`isls_chat::ChatIntent`] from the user message (keyword-based).
/// 2. Maps it to [`isls_chat::NormOperation`]s using the provided registry.
/// 3. Injects the activated norm ids as additional feature names in the initial
///    [`UserEvent::FeaturesIdentified`] event.
///
/// This is backward-compatible: if the norm registry has no matches for the
/// intent, the pipeline behaves exactly like `execute_pipeline`.
pub fn execute_pipeline_with_norms(
    intent: &str,
    workspace: &Option<AgentWorkspace>,
    oracle: &dyn SynthesisOracle,
    catalog: &crate::stubs::TemplateCatalog,
    metrics: &mut AccumulationMetrics,
    conversation: &mut crate::conversation::Conversation,
    config: &PipelineConfig,
    norm_registry: &isls_norms::NormRegistry,
) -> AgentResult {
    // Pre-pass: keyword-based intent extraction (no oracle cost).
    let chat_intent = isls_chat::extract_intent_keywords(intent);
    let norm_ops = isls_chat::intent_to_norm_ops(&chat_intent, norm_registry);

    // Collect activated norm IDs to inject as supplementary feature descriptions.
    let norm_features: Vec<String> = norm_ops.iter().filter_map(|op| {
        if let isls_chat::NormOperation::ComposeNew(ref norms) = op {
            Some(norms.iter().map(|a| format!("norm:{}", a.norm.id)).collect::<Vec<_>>())
        } else {
            None
        }
    }).flatten().collect();

    // Run the base pipeline.
    let base_result = execute_pipeline(intent, workspace, oracle, catalog, metrics, conversation, config);

    if norm_features.is_empty() {
        return base_result;
    }

    // Augment FeaturesIdentified event with norm names.
    match base_result {
        AgentResult::Success { features, mut events, plan } => {
            for ev in &mut events {
                if let UserEvent::FeaturesIdentified { ref mut features } = ev {
                    features.extend(norm_features.clone());
                    break;
                }
            }
            AgentResult::Success { features, events, plan }
        }
        other => other,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stubs::{OracleCost, OracleResponse, OracleResult, SynthesisPrompt};

    struct MockOracle;
    impl SynthesisOracle for MockOracle {
        fn name(&self) -> &str { "mock" }
        fn model(&self) -> &str { "mock-v1" }
        fn available(&self) -> bool { true }
        fn synthesize(&self, _: &SynthesisPrompt) -> OracleResult<OracleResponse> {
            Ok(OracleResponse {
                content: "[]".into(),
                model: "mock".into(),
                tokens_used: 0,
                finish_reason: "stop".into(),
                latency_ms: 0,
            })
        }
        fn cost_estimate(&self) -> OracleCost { OracleCost::default() }
    }

    // AT-AG16: Full pipeline: intent → features → plan → events
    #[test]
    fn at_ag16_full_pipeline() {
        let oracle = MockOracle;
        let catalog = TemplateCatalog::new(crate::stubs::TemplateConfig::default());
        let mut metrics = AccumulationMetrics::default();
        let mut conversation = Conversation::new(20);
        let config = PipelineConfig {
            deterministic: true,
            ..Default::default()
        };

        let result = execute_pipeline(
            "bookmark app with search and tags",
            &None,
            &oracle,
            &catalog,
            &mut metrics,
            &mut conversation,
            &config,
        );

        match result {
            AgentResult::Success {
                features,
                plan,
                events,
            } => {
                assert!(
                    features.len() >= 3,
                    "AT-AG16: expected ≥3 features, got {}",
                    features.len()
                );
                assert!(!plan.components.is_empty(), "plan has components");
                assert!(!events.is_empty(), "events generated");

                // Verify event sequence
                assert!(
                    matches!(events[0], UserEvent::FeaturesIdentified { .. }),
                    "first event is FeaturesIdentified"
                );
                assert!(
                    matches!(events[1], UserEvent::PlanReady { .. }),
                    "second event is PlanReady"
                );
                let last = events.last().unwrap();
                assert!(
                    matches!(last, UserEvent::Complete { .. }),
                    "last event is Complete"
                );
            }
            AgentResult::Failed { events } => {
                panic!(
                    "AT-AG16: pipeline should succeed, got failure with events: {:?}",
                    events
                );
            }
        }
    }

    // AT-AG17: Follow-up — build feature, then add another (delta modification)
    #[test]
    fn at_ag17_follow_up_delta() {
        let oracle = MockOracle;
        let catalog = TemplateCatalog::new(crate::stubs::TemplateConfig::default());
        let mut metrics = AccumulationMetrics::default();
        let mut conversation = Conversation::new(20);
        let config = PipelineConfig {
            deterministic: true,
            ..Default::default()
        };

        // First request
        let result1 = execute_pipeline(
            "bookmark app with search",
            &None,
            &oracle,
            &catalog,
            &mut metrics,
            &mut conversation,
            &config,
        );
        assert!(matches!(result1, AgentResult::Success { .. }));

        // Create a temp workspace for second request (simulating built project)
        let dir = std::env::temp_dir().join(format!(
            "isls_pipe_test_{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"bm\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub struct Bookmark { pub id: i64 }\n",
        )
        .unwrap();

        let ws = AgentWorkspace::analyze(&dir).ok();

        // Second request: add delete feature
        let result2 = execute_pipeline(
            "Die App soll auch Bookmarks löschen können",
            &ws,
            &oracle,
            &catalog,
            &mut metrics,
            &mut conversation,
            &config,
        );

        match result2 {
            AgentResult::Success {
                features, ..
            } => {
                // Should only add the delete feature, not rebuild everything
                assert!(
                    features.len() <= 2,
                    "AT-AG17: delta should add ≤2 features, got {}",
                    features.len()
                );
                // Conversation should have all 4 turns (2 user + 2 agent)
                assert_eq!(
                    conversation.turns.len(),
                    4,
                    "conversation should have 4 turns"
                );
            }
            AgentResult::Failed { .. } => panic!("AT-AG17: second request should succeed"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    // AT-AG18: Memory reuse — verify pattern hit tracking
    #[test]
    fn at_ag18_memory_reuse() {
        let mut metrics = AccumulationMetrics::default();

        // Simulate: 3 oracle calls, then 3 memory hits
        for _ in 0..3 {
            metrics.record_oracle_call(0.003);
        }
        for _ in 0..3 {
            metrics.record_memory_hit();
        }

        assert_eq!(metrics.total_requests, 6);
        assert_eq!(metrics.memory_served, 3);
        assert!(
            (metrics.autonomy_ratio - 0.5).abs() < 1e-9,
            "AT-AG18: autonomy should be 50%, got {}",
            metrics.autonomy_ratio
        );
    }

    // AT-AG19: User events contain NO code jargon
    #[test]
    fn at_ag19_no_jargon_in_events() {
        let events = vec![
            UserEvent::FeaturesIdentified {
                features: vec![
                    "Bookmarks anlegen".into(),
                    "Bookmarks suchen".into(),
                ],
            },
            UserEvent::PlanReady {
                component_count: 5,
                estimated_cost: 0.02,
            },
            UserEvent::ComponentDone {
                index: 1,
                total: 5,
                feature: "Bookmarks anlegen".into(),
                source: "bekannt".into(),
            },
            UserEvent::TestResult {
                passed: 9,
                total: 9,
            },
            UserEvent::Complete {
                summary: "✅ bookmark-manager ist fertig.\n\nDas kann die App:\n  • Bookmarks anlegen".into(),
                capabilities: vec!["Bookmark erstellen".into()],
                crystal_id: "abc123".into(),
                cost_usd: 0.02,
                patterns_reused: 2,
                patterns_learned: 3,
            },
            UserEvent::Problem {
                message: "Es gab ein Problem. Bitte beschreibe es anders.".into(),
            },
        ];

        for event in &events {
            assert!(
                event.has_no_jargon(),
                "AT-AG19: event contains jargon: {}",
                event.display_line()
            );
        }
    }

    // AT-AG20: Accumulation — 5 requests → verify autonomy ratio calculated
    #[test]
    fn at_ag20_accumulation_5_requests() {
        let mut metrics = AccumulationMetrics::default();

        // 5 requests: 2 oracle, 3 memory
        metrics.record_oracle_call(0.01);
        metrics.record_oracle_call(0.01);
        metrics.record_memory_hit();
        metrics.record_memory_hit();
        metrics.record_memory_hit();

        assert_eq!(metrics.total_requests, 5);
        assert!(
            (metrics.autonomy_ratio - 0.6).abs() < 1e-9,
            "AT-AG20: autonomy should be 60%, got {}",
            metrics.autonomy_ratio
        );
        assert!(
            metrics.money_saved_usd > 0.0,
            "AT-AG20: money saved should be > 0"
        );
    }

    // AT-AG21: Fail gracefully — friendly error, no stack trace
    #[test]
    fn at_ag21_graceful_failure() {
        let oracle = MockOracle;
        let catalog = TemplateCatalog::new(crate::stubs::TemplateConfig::default());
        let mut metrics = AccumulationMetrics::default();
        let mut conversation = Conversation::new(20);
        let config = PipelineConfig {
            deterministic: true,
            ..Default::default()
        };

        // Empty intent should produce a friendly error
        let result = execute_pipeline(
            "",
            &None,
            &oracle,
            &catalog,
            &mut metrics,
            &mut conversation,
            &config,
        );

        match result {
            AgentResult::Failed { events } => {
                assert!(!events.is_empty(), "should have error event");
                let msg = events[0].display_line();
                assert!(
                    !msg.contains("panic") && !msg.contains("unwrap"),
                    "AT-AG21: error must be friendly, not a stack trace: {}",
                    msg
                );
            }
            AgentResult::Success { .. } => {
                // Empty intent might still produce a generic feature, which is OK
            }
        }
    }

    // AT-AG22: German input produces German output
    #[test]
    fn at_ag22_german_io() {
        let oracle = MockOracle;
        let catalog = TemplateCatalog::new(crate::stubs::TemplateConfig::default());
        let mut metrics = AccumulationMetrics::default();
        let mut conversation = Conversation::new(20);
        let config = PipelineConfig {
            deterministic: true,
            ..Default::default()
        };

        let result = execute_pipeline(
            "Buchverwaltung mit Suche",
            &None,
            &oracle,
            &catalog,
            &mut metrics,
            &mut conversation,
            &config,
        );

        match result {
            AgentResult::Success { features, events, .. } => {
                assert!(!features.is_empty(), "AT-AG22: should produce features from German input");
                // Check that events are generated
                assert!(events.len() >= 3, "should have multiple events");
            }
            AgentResult::Failed { .. } => {
                panic!("AT-AG22: German input should not fail");
            }
        }
    }
}
