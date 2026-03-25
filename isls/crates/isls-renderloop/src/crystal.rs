// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Universal Crystal Architecture for ISLS v2.1.
//!
//! Crystals represent validated structural and implementation knowledge at four
//! abstraction levels. Level 0 (Universal) crystals apply to any software system.
//! Higher levels become progressively more domain-specific.

use std::path::Path;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::RenderloopError;

pub type Result<T> = std::result::Result<T, RenderloopError>;

// ─── CrystalLevel ────────────────────────────────────────────────────────────

/// Abstraction level of a crystal.
///
/// Search order during matching: Universal → Architectural → Structural → DomainSpecific.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CrystalLevel {
    /// Level 0: Applies to any software system (e.g. CRUD, belongs_to).
    Universal,
    /// Level 1: Applies to a class of applications (e.g. REST+auth services).
    Architectural,
    /// Level 2: Cross-domain but specific pattern (e.g. inventory tracking).
    Structural,
    /// Level 3: Single domain only (e.g. warehouse order processing).
    DomainSpecific,
}

// ─── StructuralKnowledge ─────────────────────────────────────────────────────

/// Describes the structural shape of a crystal: what components it requires and
/// how they relate.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StructuralKnowledge {
    /// Component specifications (structs, functions, endpoints, migrations).
    pub components: Vec<ComponentSpec>,
    /// Relationships between components.
    pub relationships: Vec<RelSpec>,
    /// Invariants that must hold for this crystal to apply.
    pub constraints: Vec<String>,
}

/// A single component described by a crystal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentSpec {
    /// Component kind: "struct", "function", "endpoint", "migration".
    pub kind: String,
    /// Name pattern with `{entity}` placeholder, e.g. `"{entity}_service"`.
    pub name_pattern: String,
    /// Parameters for this component.
    pub parameters: Vec<ParamSpec>,
    /// Return type (if applicable).
    pub returns: Option<String>,
    /// Human-readable description.
    pub description: String,
}

/// A single parameter in a component specification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParamSpec {
    /// Parameter name.
    pub name: String,
    /// Rust type.
    pub rust_type: String,
}

/// A relationship between two named components.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelSpec {
    /// Source component name.
    pub from: String,
    /// Target component name.
    pub to: String,
    /// Relationship description.
    pub description: String,
}

// ─── ImplementationKnowledge ─────────────────────────────────────────────────

/// Implementation-level knowledge accumulated from LLM outputs across domains.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImplementationKnowledge {
    /// Common implementation patterns found in function bodies.
    pub body_patterns: Vec<String>,
    /// Common error cases that must be handled.
    pub error_cases: Vec<String>,
    /// Edge cases discovered through testing.
    pub edge_cases: Vec<String>,
    /// Performance considerations.
    pub perf_notes: Vec<String>,
    /// Security considerations.
    pub security_notes: Vec<String>,
    /// Test scenarios that should exist.
    pub test_scenarios: Vec<String>,
}

// ─── CrystalStats ────────────────────────────────────────────────────────────

/// Usage statistics for a crystal, updated after every render run.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CrystalStats {
    /// Total number of times this crystal was applied.
    pub times_used: u64,
    /// Number of times application succeeded (code compiled and tests passed).
    pub times_succeeded: u64,
    /// Application domains in which this crystal has been used.
    pub domains_used_in: Vec<String>,
    /// ISO-8601 timestamp of last use.
    pub last_used: String,
    /// Estimated average tokens saved by reusing this crystal.
    pub avg_tokens_saved: f64,
}

// ─── Crystal ─────────────────────────────────────────────────────────────────

/// A validated, content-addressed knowledge crystal.
///
/// Crystals are the primary knowledge unit in ISLS v2.1. Each crystal captures
/// both structural (what to generate) and implementation (how to generate it)
/// knowledge at a specific abstraction level.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Crystal {
    /// Content-addressed identifier (SHA-256 of level + pattern_name).
    pub id: String,
    /// Abstraction level.
    pub level: CrystalLevel,
    /// Canonical pattern name, e.g. `"crud_entity"`, `"belongs_to"`.
    pub pattern_name: String,
    /// Human-readable description.
    pub description: String,
    /// Confidence score in `[0.0, 1.0]`. Universal crystals never drop below 0.9.
    pub confidence: f64,
    /// Structural knowledge: components, relationships, constraints.
    pub structure: StructuralKnowledge,
    /// Implementation knowledge accumulated from LLM outputs.
    pub implementation: ImplementationKnowledge,
    /// Usage statistics.
    pub stats: CrystalStats,
    /// Evidence chain: SHA-256 hashes of artifacts that validated this crystal.
    pub evidence: Vec<String>,
}

impl Crystal {
    /// Create a new universal crystal with the given name and description.
    pub fn universal(pattern_name: impl Into<String>, description: impl Into<String>) -> Self {
        let pattern_name = pattern_name.into();
        let description = description.into();
        let id = make_crystal_id(CrystalLevel::Universal, &pattern_name);
        Crystal {
            id,
            level: CrystalLevel::Universal,
            pattern_name,
            description,
            confidence: 0.95,
            structure: StructuralKnowledge::default(),
            implementation: ImplementationKnowledge::default(),
            stats: CrystalStats::default(),
            evidence: vec![],
        }
    }

    /// Returns true if this crystal's pattern_name matches the given query string.
    ///
    /// Matching is case-insensitive substring match.
    pub fn matches_pattern(&self, query: &str) -> bool {
        self.pattern_name.to_lowercase().contains(&query.to_lowercase())
    }
}

fn make_crystal_id(level: CrystalLevel, pattern_name: &str) -> String {
    let level_str = match level {
        CrystalLevel::Universal => "universal",
        CrystalLevel::Architectural => "architectural",
        CrystalLevel::Structural => "structural",
        CrystalLevel::DomainSpecific => "domain",
    };
    let mut hasher = Sha256::new();
    hasher.update(level_str.as_bytes());
    hasher.update(b":");
    hasher.update(pattern_name.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ─── CrystalRegistry ─────────────────────────────────────────────────────────

/// Registry of all known crystals, including the 15 built-in universal crystals.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrystalRegistry {
    crystals: Vec<Crystal>,
}

impl CrystalRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        CrystalRegistry { crystals: vec![] }
    }

    /// Create a registry pre-loaded with all 15 built-in universal crystals.
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        reg.crystals = builtin_crystals();
        reg
    }

    /// All crystals in the registry (immutable slice).
    pub fn crystals(&self) -> &[Crystal] {
        &self.crystals
    }

    /// Number of crystals in the registry.
    pub fn len(&self) -> usize {
        self.crystals.len()
    }

    /// Returns true if the registry contains no crystals.
    pub fn is_empty(&self) -> bool {
        self.crystals.is_empty()
    }

    /// Find a crystal by exact pattern name.
    pub fn get(&self, pattern_name: &str) -> Option<&Crystal> {
        self.crystals.iter().find(|c| c.pattern_name == pattern_name)
    }

    /// Find all crystals whose pattern_name matches `query` and whose confidence
    /// is at or above `min_confidence`.
    ///
    /// Results are sorted Universal → Architectural → Structural → DomainSpecific,
    /// then by confidence descending within each level.
    pub fn find_matches(&self, query: &str, min_confidence: f64) -> Vec<&Crystal> {
        let mut matches: Vec<&Crystal> = self.crystals.iter()
            .filter(|c| c.confidence >= min_confidence && c.matches_pattern(query))
            .collect();
        matches.sort_by(|a, b| {
            a.level.cmp(&b.level)
                .then(b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal))
        });
        matches
    }

    /// Update usage statistics for a crystal after a render run.
    pub fn update_stats(&mut self, pattern_name: &str, succeeded: bool, domain: &str) {
        if let Some(c) = self.crystals.iter_mut().find(|c| c.pattern_name == pattern_name) {
            c.stats.times_used += 1;
            if succeeded {
                c.stats.times_succeeded += 1;
            }
            let domain_owned = domain.to_string();
            if !c.stats.domains_used_in.contains(&domain_owned) {
                c.stats.domains_used_in.push(domain_owned);
            }
            c.stats.last_used = chrono_now();
        }
    }

    /// Append a new crystal or replace an existing one with the same pattern_name.
    pub fn upsert(&mut self, crystal: Crystal) {
        if let Some(existing) = self.crystals.iter_mut().find(|c| c.pattern_name == crystal.pattern_name) {
            *existing = crystal;
        } else {
            self.crystals.push(crystal);
        }
    }

    /// Persist the registry to a JSON file.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a registry from a JSON file.
    ///
    /// If the file does not exist, returns a fresh registry with builtins.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::with_builtins());
        }
        let content = std::fs::read_to_string(path)?;
        let reg: CrystalRegistry = serde_json::from_str(&content)?;
        Ok(reg)
    }
}

impl Default for CrystalRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

fn chrono_now() -> String {
    // Simple RFC 3339 timestamp without external chrono dep in this module
    // Using std::time for a basic representation
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

// ─── Built-in Universal Crystals ─────────────────────────────────────────────

/// Returns the 15 built-in universal crystals defined by the ISLS v2.1 spec.
pub fn builtin_crystals() -> Vec<Crystal> {
    vec![
        make_universal(
            "belongs_to",
            "Entity A references Entity B via a foreign key. Generates FK field, \
             migration constraint, and service lookup helper.",
        ),
        make_universal(
            "has_many",
            "Entity A owns a collection of Entity B. Generates list query, \
             cascade rules, and service collection accessor.",
        ),
        make_universal(
            "crud_entity",
            "Entity with full Create/Read/Update/Delete lifecycle. Generates \
             model struct, SQL migrations, service methods, and REST endpoints.",
        ),
        make_universal(
            "state_machine",
            "Entity with discrete states and valid transition table. Generates \
             state enum, transition validator, and guard middleware.",
        ),
        make_universal(
            "pagination",
            "List endpoint with page/per_page/sort/search parameters. Generates \
             query builder, SQL LIMIT/OFFSET, and response envelope.",
        ),
        make_universal(
            "jwt_auth",
            "JWT authentication flow: login, token issue, refresh, revoke. \
             Generates auth middleware, token service, and protected route guards.",
        ),
        make_universal(
            "error_handling",
            "Typed error system mapping domain errors to HTTP status codes. \
             Generates error enum, From impls, and JSON error response formatter.",
        ),
        make_universal(
            "soft_delete",
            "Entity marked inactive instead of physically deleted. Generates \
             deleted_at field, filter middleware, and restore endpoint.",
        ),
        make_universal(
            "audit_trail",
            "Track who changed what and when on an entity. Generates audit \
             log table, service hooks, and audit query endpoints.",
        ),
        make_universal(
            "event_emitter",
            "Domain action produces typed domain events consumed asynchronously. \
             Generates event enum, publisher, and subscriber registration.",
        ),
        make_universal(
            "pipeline",
            "Data flows through ordered stages: validate → transform → persist → \
             respond. Generates stage trait, pipeline runner, and error short-circuit.",
        ),
        make_universal(
            "config_from_env",
            "Configuration loaded from environment variables at startup. \
             Generates Config struct, env readers, validation, and dotenv support.",
        ),
        make_universal(
            "health_check",
            "System health endpoint reporting database, cache, and service \
             liveness. Generates /health route and component probe trait.",
        ),
        make_universal(
            "rate_limiting",
            "API rate limiting per user or IP with configurable window and \
             quota. Generates middleware, token bucket, and 429 response.",
        ),
        make_universal(
            "background_job",
            "Async task processed outside the request cycle. Generates job \
             struct, queue interface, worker loop, and retry policy.",
        ),
    ]
}

fn make_universal(pattern_name: &str, description: &str) -> Crystal {
    Crystal::universal(pattern_name, description)
}
