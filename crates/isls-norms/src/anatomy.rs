// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Anatomical norm catalog: 10 body layers with 30+ organs.
//!
//! Each organ maps to [`ResoniteClass`] variants from spectroscopy and
//! carries scrape keywords for targeted discovery.  Coverage is computed
//! by matching norms and candidates against each organ's resonite classes.

use serde::Serialize;

use crate::fitness::FitnessStore;
use crate::spectroscopy::ResoniteClass;
use crate::types::Norm;
use crate::learning::NormCandidate;

// ─── Public types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AnatomyLayer {
    pub name: &'static str,
    pub description: &'static str,
    pub organs: Vec<OrganResult>,
    pub coverage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrganResult {
    pub name: &'static str,
    pub keywords: Vec<&'static str>,
    pub norms: Vec<OrganNorm>,
    pub candidates: Vec<OrganCandidate>,
    pub coverage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrganNorm {
    pub id: String,
    pub name: String,
    pub source: &'static str,
    pub fitness: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrganCandidate {
    pub id: String,
    pub observations: usize,
    pub domains: usize,
}

// ─── Organ definition (static) ──────────────────────────────────────────────

struct OrganDef {
    name: &'static str,
    classes: &'static [OrganClass],
    keywords: &'static [&'static str],
}

/// Either a known ResoniteClass variant or a Custom(name) match.
enum OrganClass {
    Known(ResoniteClass),
    Named(&'static str),
}

// ─── The 10 anatomical layers ───────────────────────────────────────────────

struct LayerDef {
    name: &'static str,
    description: &'static str,
    organs: &'static [OrganDef],
}

const ANATOMY: &[LayerDef] = &[
    // 1. Skelett
    LayerDef {
        name: "Skelett",
        description: "Server + Routing",
        organs: &[
            OrganDef {
                name: "HTTP Server",
                classes: &[OrganClass::Named("HttpServer"), OrganClass::Named("Router")],
                keywords: &["rust actix web server", "rust axum router handler"],
            },
            OrganDef {
                name: "Middleware",
                classes: &[OrganClass::Named("Middleware")],
                keywords: &["rust http server middleware", "rust tower service layer"],
            },
            OrganDef {
                name: "CLI Interface",
                classes: &[OrganClass::Known(ResoniteClass::CliInterface)],
                keywords: &["rust clap cli argument", "rust cli command parser"],
            },
        ],
    },
    // 2. Nervensystem
    LayerDef {
        name: "Nervensystem",
        description: "Daten + Persistenz",
        organs: &[
            OrganDef {
                name: "Database ORM",
                classes: &[OrganClass::Named("DatabaseOrm"), OrganClass::Known(ResoniteClass::Migration)],
                keywords: &["rust sqlx postgres query", "rust diesel orm database"],
            },
            OrganDef {
                name: "Connection Pool",
                classes: &[OrganClass::Known(ResoniteClass::ConnectionPool)],
                keywords: &["rust connection pool deadpool bb8", "rust pgpool database connection"],
            },
            OrganDef {
                name: "Migrations",
                classes: &[OrganClass::Known(ResoniteClass::Migration)],
                keywords: &["rust database migration sqlx", "rust schema migration refinery"],
            },
            OrganDef {
                name: "Cache",
                classes: &[OrganClass::Known(ResoniteClass::Caching)],
                keywords: &["rust cache lru redis", "rust memoization cache layer"],
            },
        ],
    },
    // 3. Immunsystem
    LayerDef {
        name: "Immunsystem",
        description: "Auth + Sicherheit",
        organs: &[
            OrganDef {
                name: "Authentication",
                classes: &[OrganClass::Known(ResoniteClass::Authentication)],
                keywords: &["rust jwt authentication login", "rust oauth2 session token"],
            },
            OrganDef {
                name: "Authorization",
                classes: &[OrganClass::Known(ResoniteClass::Authorization)],
                keywords: &["rust authorization rbac permission", "rust role guard middleware"],
            },
            OrganDef {
                name: "Input Validation",
                classes: &[OrganClass::Named("InputValidation")],
                keywords: &["rust input validation sanitize", "rust validator derive macro"],
            },
            OrganDef {
                name: "Rate Limiting",
                classes: &[OrganClass::Known(ResoniteClass::RateLimiting)],
                keywords: &["rust rate limiter governor", "rust throttle middleware"],
            },
            OrganDef {
                name: "CORS",
                classes: &[OrganClass::Named("Cors")],
                keywords: &["rust cors middleware origin", "rust actix cors tower"],
            },
        ],
    },
    // 4. Verdauung
    LayerDef {
        name: "Verdauung",
        description: "Serialisierung + Verarbeitung",
        organs: &[
            OrganDef {
                name: "Serialization",
                classes: &[OrganClass::Named("Serialization")],
                keywords: &["rust serde json serialization", "rust protobuf serialization"],
            },
            OrganDef {
                name: "Error Handling",
                classes: &[OrganClass::Named("ErrorHandling")],
                keywords: &["rust error handling thiserror anyhow", "rust error type response"],
            },
            OrganDef {
                name: "Configuration",
                classes: &[OrganClass::Known(ResoniteClass::Configuration)],
                keywords: &["rust config environment variable", "rust figment configuration toml"],
            },
            OrganDef {
                name: "Logging",
                classes: &[OrganClass::Known(ResoniteClass::Logging)],
                keywords: &["rust tracing structured logging", "rust log subscriber format"],
            },
        ],
    },
    // 5. Kreislauf
    LayerDef {
        name: "Kreislauf",
        description: "Kommunikation + Events",
        organs: &[
            OrganDef {
                name: "WebSocket",
                classes: &[OrganClass::Known(ResoniteClass::RealtimeWebSocket)],
                keywords: &["rust websocket realtime tokio", "rust axum websocket upgrade"],
            },
            OrganDef {
                name: "Event Bus",
                classes: &[OrganClass::Known(ResoniteClass::EventBus)],
                keywords: &["rust event bus pubsub broadcast", "rust tokio broadcast channel"],
            },
            OrganDef {
                name: "Message Queue",
                classes: &[OrganClass::Known(ResoniteClass::MessageQueue)],
                keywords: &["rust message queue rabbitmq nats", "rust async queue worker"],
            },
            OrganDef {
                name: "Background Jobs",
                classes: &[OrganClass::Known(ResoniteClass::Scheduling)],
                keywords: &["rust background job cron scheduler", "rust tokio spawn task worker"],
            },
            OrganDef {
                name: "gRPC",
                classes: &[OrganClass::Known(ResoniteClass::GrpcService)],
                keywords: &["rust grpc tonic protobuf service", "rust grpc server client"],
            },
        ],
    },
    // 6. Haut
    LayerDef {
        name: "Haut",
        description: "API-Oberfl\u{00e4}che + Interface",
        organs: &[
            OrganDef {
                name: "REST CRUD",
                classes: &[OrganClass::Known(ResoniteClass::CrudEntity)],
                keywords: &["rust crud rest api handler", "rust actix resource endpoint"],
            },
            OrganDef {
                name: "Pagination",
                classes: &[OrganClass::Known(ResoniteClass::Pagination)],
                keywords: &["rust pagination offset cursor", "rust paginated response query"],
            },
            OrganDef {
                name: "Search",
                classes: &[OrganClass::Known(ResoniteClass::Search)],
                keywords: &["rust fulltext search filter", "rust search query builder"],
            },
            OrganDef {
                name: "File Upload",
                classes: &[OrganClass::Known(ResoniteClass::FileUpload)],
                keywords: &["rust multipart file upload", "rust file upload handler stream"],
            },
            OrganDef {
                name: "Health Check",
                classes: &[OrganClass::Known(ResoniteClass::HealthCheck)],
                keywords: &["rust health check liveness probe", "rust readiness endpoint"],
            },
        ],
    },
    // 7. Knochen (Frontend)
    LayerDef {
        name: "Knochen (Frontend)",
        description: "Komponenten-Architektur",
        organs: &[
            OrganDef {
                name: "Component System",
                classes: &[OrganClass::Named("Component")],
                keywords: &["react component props state", "vue component composition api"],
            },
            OrganDef {
                name: "State Management",
                classes: &[OrganClass::Named("StateManagement")],
                keywords: &["javascript state management redux zustand", "vue pinia store state"],
            },
            OrganDef {
                name: "Client Routing",
                classes: &[OrganClass::Named("ClientRouter")],
                keywords: &["react router navigation", "vue router navigation guard"],
            },
            OrganDef {
                name: "Hooks/Composables",
                classes: &[OrganClass::Named("Hooks")],
                keywords: &["react hooks usestate useeffect", "vue composable reactive ref"],
            },
        ],
    },
    // 8. Muskeln (Frontend)
    LayerDef {
        name: "Muskeln (Frontend)",
        description: "Interaktion + Formulare",
        organs: &[
            OrganDef {
                name: "Form Handling",
                classes: &[OrganClass::Named("FormHandling")],
                keywords: &["react form validation submit", "javascript form multi step wizard"],
            },
            OrganDef {
                name: "DataTable",
                classes: &[OrganClass::Named("DataTable")],
                keywords: &["react table sorting filtering", "javascript datatable pagination sort"],
            },
            OrganDef {
                name: "Modal/Dialog",
                classes: &[OrganClass::Named("Modal")],
                keywords: &["react modal dialog overlay", "javascript modal drawer popover"],
            },
            OrganDef {
                name: "Drag & Drop",
                classes: &[OrganClass::Named("DragDrop")],
                keywords: &["react drag drop sortable", "javascript drag drop kanban"],
            },
            OrganDef {
                name: "Keyboard",
                classes: &[OrganClass::Named("Keyboard")],
                keywords: &["javascript keyboard shortcut hotkey"],
            },
        ],
    },
    // 9. Sinnesorgane (Frontend)
    LayerDef {
        name: "Sinnesorgane (Frontend)",
        description: "Visualisierung + Charts",
        organs: &[
            OrganDef {
                name: "Charts",
                classes: &[OrganClass::Known(ResoniteClass::DataVisualization)],
                keywords: &["react chart recharts victory", "javascript chart d3 plotly"],
            },
            OrganDef {
                name: "Dashboard Layout",
                classes: &[OrganClass::Named("Dashboard")],
                keywords: &["react dashboard widget layout", "javascript dashboard grid layout"],
            },
            OrganDef {
                name: "Realtime Client",
                classes: &[OrganClass::Named("RealtimeClient")],
                keywords: &["javascript websocket realtime client"],
            },
        ],
    },
    // 10. Kleidung (Frontend)
    LayerDef {
        name: "Kleidung (Frontend)",
        description: "Design-System + Theming",
        organs: &[
            OrganDef {
                name: "CSS Framework",
                classes: &[OrganClass::Named("CssFramework")],
                keywords: &["typescript tailwind css utility", "css design system tokens"],
            },
            OrganDef {
                name: "Theme System",
                classes: &[OrganClass::Named("ThemeSystem")],
                keywords: &["react theme dark light mode", "css responsive mobile first"],
            },
            OrganDef {
                name: "Animation",
                classes: &[OrganClass::Named("Animation")],
                keywords: &["react animation framer motion", "css animation keyframe transition"],
            },
            OrganDef {
                name: "UI Primitives",
                classes: &[OrganClass::Named("UiPrimitives")],
                keywords: &["typescript radix ui primitive", "typescript shadcn ui component"],
            },
        ],
    },
];

// ─── Coverage computation ───────────────────────────────────────────────────

/// Compute the full anatomy with coverage from the live norm + candidate data.
pub fn compute_anatomy(
    norms: &[&Norm],
    candidates: &[&NormCandidate],
    fitness_store: &FitnessStore,
) -> Vec<AnatomyLayer> {
    ANATOMY
        .iter()
        .map(|layer| {
            let organ_results: Vec<OrganResult> = layer
                .organs
                .iter()
                .map(|organ| compute_organ(organ, norms, candidates, fitness_store))
                .collect();

            let coverage = if organ_results.is_empty() {
                0.0
            } else {
                organ_results.iter().map(|o| o.coverage).sum::<f64>() / organ_results.len() as f64
            };

            AnatomyLayer {
                name: layer.name,
                description: layer.description,
                organs: organ_results,
                coverage,
            }
        })
        .collect()
}

/// Compute total coverage across all layers.
pub fn total_coverage(layers: &[AnatomyLayer]) -> f64 {
    if layers.is_empty() {
        return 0.0;
    }
    layers.iter().map(|l| l.coverage).sum::<f64>() / layers.len() as f64
}

fn compute_organ(
    organ: &OrganDef,
    norms: &[&Norm],
    candidates: &[&NormCandidate],
    fitness_store: &FitnessStore,
) -> OrganResult {
    // Match norms: check if any trigger keyword overlaps with organ keywords
    // or if the norm name/id matches the organ's resonite classes.
    let matching_norms: Vec<OrganNorm> = norms
        .iter()
        .filter(|n| norm_matches_organ(n, organ))
        .map(|n| {
            let fitness = fitness_store.get_fitness(&n.id);
            let source = if n.evidence.builtin {
                "builtin"
            } else if n.id.contains("AUTO") {
                "auto"
            } else if n.id.contains("INJECT") {
                "injected"
            } else {
                "auto"
            };
            OrganNorm {
                id: n.id.clone(),
                name: n.name.clone(),
                source,
                fitness,
            }
        })
        .collect();

    // Match candidates: check by consistent_layers overlap or keyword overlap
    let matching_candidates: Vec<OrganCandidate> = candidates
        .iter()
        .filter(|c| candidate_matches_organ(c, organ))
        .map(|c| OrganCandidate {
            id: c.id.clone(),
            observations: c.observation_count,
            domains: c.domains.len(),
        })
        .collect();

    // Coverage: 1.0 if norm with fitness > 0.8, 0.5 if any norm, 0.15 if only candidates
    let coverage = if matching_norms.iter().any(|n| n.fitness > 0.8) {
        1.0
    } else if !matching_norms.is_empty() {
        0.5
    } else if !matching_candidates.is_empty() {
        0.15
    } else {
        0.0
    };

    OrganResult {
        name: organ.name,
        keywords: organ.keywords.to_vec(),
        norms: matching_norms,
        candidates: matching_candidates,
        coverage,
    }
}

/// Check if a norm matches an organ by keyword overlap.
fn norm_matches_organ(norm: &Norm, organ: &OrganDef) -> bool {
    let norm_keywords: Vec<String> = norm
        .triggers
        .iter()
        .flat_map(|t| t.keywords.iter().chain(t.concepts.iter()))
        .map(|k| k.to_lowercase())
        .collect();

    let norm_name_lower = norm.name.to_lowercase();

    // Check if organ class names match the norm name
    for class in organ.classes {
        match class {
            OrganClass::Known(rc) => {
                let class_name = format!("{:?}", rc).to_lowercase();
                if norm_name_lower.contains(&class_name) {
                    return true;
                }
                // Also check keywords for class-related terms
                for nk in &norm_keywords {
                    if nk.contains(&class_name) {
                        return true;
                    }
                }
            }
            OrganClass::Named(name) => {
                let name_lower = name.to_lowercase();
                if norm_name_lower.contains(&name_lower) {
                    return true;
                }
                for nk in &norm_keywords {
                    if nk.contains(&name_lower) {
                        return true;
                    }
                }
            }
        }
    }

    // Check keyword overlap between organ keywords and norm trigger keywords
    for ok in organ.keywords {
        let ok_words: Vec<&str> = ok.split_whitespace().collect();
        for nk in &norm_keywords {
            let matches = ok_words.iter().filter(|w| nk.contains(&w.to_lowercase())).count();
            if matches >= 2 {
                return true;
            }
        }
    }

    false
}

/// Check if a candidate matches an organ by domain/keyword overlap.
fn candidate_matches_organ(candidate: &NormCandidate, organ: &OrganDef) -> bool {
    // Check domain names against organ class names and keywords
    let cand_domains: Vec<String> = candidate
        .domains
        .iter()
        .map(|d| d.to_lowercase())
        .collect();

    for class in organ.classes {
        let class_str = match class {
            OrganClass::Known(rc) => format!("{:?}", rc).to_lowercase(),
            OrganClass::Named(name) => name.to_lowercase(),
        };
        for domain in &cand_domains {
            if domain.contains(&class_str) {
                return true;
            }
        }
    }

    // Check organ keyword fragments against candidate domains
    for ok in organ.keywords {
        let ok_words: Vec<&str> = ok.split_whitespace().collect();
        for domain in &cand_domains {
            let matches = ok_words.iter().filter(|w| domain.contains(&w.to_lowercase())).count();
            if matches >= 2 {
                return true;
            }
        }
    }

    false
}
