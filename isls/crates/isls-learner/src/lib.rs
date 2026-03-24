// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Cross-language pattern accumulation for ISLS — ported from Barbara codex-learn.
//!
//! Accumulates structural patterns from code observations across sessions.
//! Provides similarity-based retrieval of previously seen patterns.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use isls_reader::CodeObservation;
use isls_code_topo::{compute_code_topology, topology_similarity, CodeTopology};

// ─── LearnedPattern ──────────────────────────────────────────────────────────

/// A structural pattern that has been learned from past code observations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LearnedPattern {
    /// Stable content-addressed identifier.
    pub id: String,
    /// Human-readable label (e.g. file stem or module name).
    pub label: String,
    /// Primary language of the observations.
    pub language: String,
    /// The extracted topology.
    pub topology: CodeTopology,
    /// How many times this pattern was observed.
    pub occurrences: usize,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f64,
}

// ─── LibraryStats ────────────────────────────────────────────────────────────

/// Summary statistics for the pattern library.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LibraryStats {
    pub pattern_count: usize,
    pub total_occurrences: usize,
    pub languages: Vec<String>,
    pub avg_confidence: f64,
}

// ─── PatternLibrary ──────────────────────────────────────────────────────────

/// In-memory accumulator for cross-language structural patterns.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PatternLibrary {
    patterns: Vec<LearnedPattern>,
}

impl PatternLibrary {
    /// Create an empty pattern library.
    pub fn new() -> Self {
        Self { patterns: Vec::new() }
    }

    /// Ingest a set of code observations and merge them into the library.
    ///
    /// If a similar pattern already exists (similarity ≥ 0.88), the occurrence
    /// count is incremented. Otherwise a new pattern is created.
    pub fn add_pattern(&mut self, label: &str, obs: &[CodeObservation]) {
        if obs.is_empty() { return; }

        let topo = compute_code_topology(obs);
        let lang = obs.iter()
            .max_by_key(|o| o.loc)
            .map(|o| o.language.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        // Check for existing similar pattern
        let threshold = 0.88_f64;
        if let Some(existing) = self.patterns.iter_mut()
            .filter(|p| p.language == lang)
            .find(|p| topology_similarity(&p.topology, &topo) >= threshold)
        {
            existing.occurrences += 1;
            existing.confidence = (existing.confidence * 0.9 + 0.1).min(1.0);
            return;
        }

        // New pattern
        let id = make_id(&topo, &lang);
        self.patterns.push(LearnedPattern {
            id,
            label: label.to_string(),
            language: lang,
            topology: topo,
            occurrences: 1,
            confidence: 0.5,
        });
    }

    /// Find patterns similar to the given topology above a similarity threshold.
    pub fn find_similar(&self, topo: &CodeTopology, threshold: f64) -> Vec<&LearnedPattern> {
        let mut results: Vec<(f64, &LearnedPattern)> = self.patterns.iter()
            .map(|p| (topology_similarity(&p.topology, topo), p))
            .filter(|(sim, _)| *sim >= threshold)
            .collect();
        results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        results.into_iter().map(|(_, p)| p).collect()
    }

    /// Return summary statistics.
    pub fn stats(&self) -> LibraryStats {
        if self.patterns.is_empty() {
            return LibraryStats::default();
        }
        let total_occurrences = self.patterns.iter().map(|p| p.occurrences).sum();
        let avg_confidence = self.patterns.iter().map(|p| p.confidence).sum::<f64>()
            / self.patterns.len() as f64;
        let mut languages: Vec<String> = self.patterns.iter().map(|p| p.language.clone()).collect();
        languages.sort();
        languages.dedup();
        LibraryStats {
            pattern_count: self.patterns.len(),
            total_occurrences,
            languages,
            avg_confidence,
        }
    }

    /// All patterns in the library.
    pub fn patterns(&self) -> &[LearnedPattern] {
        &self.patterns
    }

    /// Number of patterns in the library.
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    /// True if the library contains no patterns.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }
}

fn make_id(topo: &CodeTopology, lang: &str) -> String {
    let mut h = Sha256::new();
    h.update(lang.as_bytes());
    for sig in &topo.function_signatures {
        h.update(sig.as_bytes());
    }
    for name in &topo.struct_names {
        h.update(name.as_bytes());
    }
    let bytes = h.finalize();
    bytes.iter().take(8).map(|b| format!("{:02x}", b)).collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_reader::{parse_string, Language};

    #[test]
    fn add_and_retrieve_pattern() {
        let mut lib = PatternLibrary::new();
        let obs = parse_string(
            "pub fn list() {} pub fn create() {} pub struct Product {}",
            Language::Rust,
        ).unwrap();
        lib.add_pattern("product_service", &[obs.clone()]);
        assert_eq!(lib.len(), 1);

        let topo = compute_code_topology(&[obs]);
        let matches = lib.find_similar(&topo, 0.5);
        assert!(!matches.is_empty());
    }

    #[test]
    fn deduplicates_similar_patterns() {
        let mut lib = PatternLibrary::new();
        let src = "pub fn list() {} pub fn create() {}";
        let obs1 = parse_string(src, Language::Rust).unwrap();
        let obs2 = parse_string(src, Language::Rust).unwrap();
        lib.add_pattern("a", &[obs1]);
        lib.add_pattern("b", &[obs2]);
        // Should be deduped into 1 pattern
        assert_eq!(lib.len(), 1);
        assert_eq!(lib.stats().total_occurrences, 2);
    }
}
