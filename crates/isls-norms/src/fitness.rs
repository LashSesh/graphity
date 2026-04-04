// isls-norms/src/fitness.rs — I1: Norm Fitness Tracking
//
// Every norm gets a fitness value φ ∈ [0,1] based on generation outcomes.
// φ_{t+1} = α·φ_t + (1-α)·r_t, where r_t = 1 (success) or 0 (failure).
// Fitness-weighted activation: score_adjusted = score_keyword · φ.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Exponential smoothing factor. Higher α = more memory, slower adaptation.
const ALPHA: f64 = 0.9;
/// Default fitness for norms with no history.
const DEFAULT_FITNESS: f64 = 0.5;

// ═══════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormFitness {
    pub norm_id: String,
    pub fitness: f64,
    pub activation_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub last_activated: String,
}

impl NormFitness {
    pub fn new(norm_id: &str) -> Self {
        Self {
            norm_id: norm_id.to_string(),
            fitness: DEFAULT_FITNESS,
            activation_count: 0,
            success_count: 0,
            failure_count: 0,
            last_activated: String::new(),
        }
    }

    /// Update fitness after a generation outcome.
    pub fn update(&mut self, success: bool) {
        let r = if success { 1.0 } else { 0.0 };
        self.fitness = ALPHA * self.fitness + (1.0 - ALPHA) * r;
        self.fitness = self.fitness.clamp(0.0, 1.0);
        self.activation_count += 1;
        if success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }
        self.last_activated = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    }
}

// ═══════════════════════════════════════════════════════════════════
// FitnessStore
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FitnessStore {
    pub entries: HashMap<String, NormFitness>,
}

impl FitnessStore {
    pub fn new() -> Self {
        Self { entries: HashMap::new() }
    }

    /// Get fitness for a norm. Returns DEFAULT_FITNESS if not tracked.
    pub fn get_fitness(&self, norm_id: &str) -> f64 {
        self.entries.get(norm_id).map(|e| e.fitness).unwrap_or(DEFAULT_FITNESS)
    }

    /// Get full fitness entry (or create a new default one).
    pub fn get_or_create(&mut self, norm_id: &str) -> &mut NormFitness {
        self.entries.entry(norm_id.to_string()).or_insert_with(|| NormFitness::new(norm_id))
    }

    /// Update fitness for a set of norms that were activated in a generation.
    pub fn update_fitness(&mut self, norm_ids: &[String], success: bool) {
        for id in norm_ids {
            self.get_or_create(id).update(success);
        }
    }

    /// Get all entries sorted by fitness (descending).
    pub fn sorted_entries(&self) -> Vec<&NormFitness> {
        let mut entries: Vec<&NormFitness> = self.entries.values().collect();
        entries.sort_by(|a, b| b.fitness.partial_cmp(&a.fitness).unwrap_or(std::cmp::Ordering::Equal));
        entries
    }

    // ── Persistence ────────────────────────────────────────────────

    fn persistence_path() -> Option<PathBuf> {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(|h| std::path::PathBuf::from(h).join(".isls").join("fitness.json"))
    }

    /// Load fitness store from ~/.isls/fitness.json.
    pub fn load() -> Self {
        let path = match Self::persistence_path() {
            Some(p) => p,
            None => return Self::new(),
        };
        if !path.exists() {
            return Self::new();
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::new(),
        }
    }

    /// Save fitness store to ~/.isls/fitness.json.
    pub fn save(&self) -> std::io::Result<()> {
        let path = match Self::persistence_path() {
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

/// Apply fitness weighting to a confidence score.
pub fn fitness_weighted_score(keyword_score: f64, fitness: f64) -> f64 {
    keyword_score * fitness
}

// ═══════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_norm_fitness_default() {
        let nf = NormFitness::new("ISLS-NORM-0042");
        assert_eq!(nf.fitness, 0.5);
        assert_eq!(nf.activation_count, 0);
    }

    #[test]
    fn test_fitness_update_success() {
        let mut nf = NormFitness::new("ISLS-NORM-0042");
        nf.update(true);
        // φ = 0.9 * 0.5 + 0.1 * 1.0 = 0.55
        assert!((nf.fitness - 0.55).abs() < 1e-10);
        assert_eq!(nf.success_count, 1);
        assert_eq!(nf.activation_count, 1);
    }

    #[test]
    fn test_fitness_update_failure() {
        let mut nf = NormFitness::new("ISLS-NORM-0042");
        nf.update(false);
        // φ = 0.9 * 0.5 + 0.1 * 0.0 = 0.45
        assert!((nf.fitness - 0.45).abs() < 1e-10);
        assert_eq!(nf.failure_count, 1);
    }

    #[test]
    fn test_fitness_convergence_after_many_successes() {
        let mut nf = NormFitness::new("test");
        for _ in 0..20 {
            nf.update(true);
        }
        assert!(nf.fitness > 0.85, "After 20 successes, fitness should be >0.85: {}", nf.fitness);
    }

    #[test]
    fn test_fitness_store_update() {
        let mut store = FitnessStore::new();
        store.update_fitness(&["A".into(), "B".into()], true);
        assert!(store.get_fitness("A") > 0.5);
        assert!(store.get_fitness("B") > 0.5);
        assert_eq!(store.get_fitness("C"), 0.5); // default
    }

    #[test]
    fn test_fitness_weighted_score() {
        assert!((fitness_weighted_score(0.8, 0.9) - 0.72).abs() < 1e-10);
        assert!((fitness_weighted_score(0.8, 0.1) - 0.08).abs() < 1e-10);
    }
}
