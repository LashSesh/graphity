// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! SwarmOracle — the Ophanim Swarm Oracle.
//!
//! Wraps ANY Oracle implementation. Calls it n times with n different
//! Chameleon projections. Applies resonance consensus. Returns the
//! Mandorla candidate.
//!
//! The inner oracle does not know it is inside a swarm.
//! The caller does not know a swarm was used.
//! Drop-in replacement — implements `Oracle`.

use isls_forge_llm::oracle::{self, Oracle};

use crate::{chameleon, konus, monolith, ophanim};

/// The Ophanim Swarm Oracle.
///
/// Wraps any `Box<dyn Oracle>`. Calls it n times with n different
/// Chameleon projections. Applies resonance consensus (Ophanim + Konus).
/// Returns the Mandorla candidate via the Monolith Gate.
pub struct SwarmOracle {
    /// The wrapped oracle (OpenAI, Ollama, Mock, anything).
    inner: Box<dyn Oracle>,
    /// Number of Thronengel per call (default: 4).
    pub swarm_size: usize,
    /// Resonance threshold Theta (default: 0.20).
    pub threshold: f64,
    /// Max Swarm-Coagula rounds (default: 2).
    pub max_coagula: u32,
}

impl SwarmOracle {
    /// Create a new Swarm Oracle wrapping an inner oracle.
    ///
    /// The inner oracle is opaque — SwarmOracle does not know or care
    /// what oracle it wraps.
    pub fn new(inner: Box<dyn Oracle>, swarm_size: usize) -> Self {
        Self {
            inner,
            swarm_size: swarm_size.max(1),
            threshold: konus::DEFAULT_THRESHOLD,
            max_coagula: 2,
        }
    }

    /// Set the resonance threshold Θ (builder pattern).
    pub fn with_threshold(mut self, theta: f64) -> Self {
        self.threshold = theta;
        self
    }

    /// Set the max Coagula rounds (builder pattern).
    pub fn with_max_coagula(mut self, max: u32) -> Self {
        self.max_coagula = max;
        self
    }
}

impl Oracle for SwarmOracle {
    fn call(&self, prompt: &str, max_tokens: u32) -> oracle::Result<String> {
        let start = std::time::Instant::now();
        let n = self.swarm_size;

        eprintln!(
            "[MERKABA] Swarm(n={}, oracle={}) starting...",
            n,
            self.inner.model_name()
        );

        // 1. Chameleon: prompt → n projections
        let projections = chameleon::project(prompt, n);

        // 2. Swarm: call inner oracle n times
        let mut candidates: Vec<String> = Vec::with_capacity(n);
        let mut per_tokens: Vec<u64> = Vec::with_capacity(n);
        for (k, pi) in projections.iter().enumerate() {
            match self.inner.call(pi, max_tokens) {
                Ok(resp) => {
                    let tokens = self.inner.count_tokens(&resp);
                    per_tokens.push(tokens);
                    candidates.push(resp);
                }
                Err(e) => {
                    tracing::warn!(
                        thronengel = k + 1,
                        error = %e,
                        "Thronengel failed, using empty candidate"
                    );
                    per_tokens.push(0);
                    candidates.push(String::new());
                }
            }
        }

        // 3. Ophanim: measure coherence
        let (dk, pairwise_sims) = ophanim::compute_resonance(&candidates);

        // 4. Konus: D_total
        let omega = konus::compute_omega(&pairwise_sims, n);
        let dtotal = konus::compute_dtotal(&dk, omega);

        // ── Logging ──────────────────────────────────────────────────────────
        let features: Vec<ophanim::CodeFeatures> = candidates
            .iter()
            .map(|c| ophanim::extract_features(c))
            .collect();

        for (k, feat) in features.iter().enumerate() {
            eprintln!(
                "  T{} ({:12}):  D={:.2}  fns={} structs={} imports={}",
                k + 1,
                chameleon::lens_name(k),
                dk.get(k).copied().unwrap_or(0.0),
                feat.functions.len(),
                feat.structs.len(),
                feat.imports.len(),
            );
        }

        let total_tokens: u64 = per_tokens.iter().sum();
        let avg_tokens = if per_tokens.is_empty() {
            0
        } else {
            total_tokens / per_tokens.len() as u64
        };

        // 5. Monolith: decide
        let result = monolith::select(
            &candidates,
            &dk,
            dtotal,
            self.threshold,
            self.max_coagula,
            self.inner.as_ref(),
            prompt,
            max_tokens,
        )?;

        let elapsed = start.elapsed().as_secs_f64();

        let decision = if result.coagula_triggered {
            format!("COAGULA ({} rounds)", result.coagula_rounds)
        } else {
            format!("EMIT (Mandorla: T{})", result.selected_index + 1)
        };

        eprintln!(
            "  Omega={:.2}  Dtotal={:.2}  Theta={:.2} -> {}",
            omega, dtotal, self.threshold, decision,
        );
        eprintln!(
            "  Tokens: {}x{}={}  Time: {:.1}s",
            n, avg_tokens, total_tokens, elapsed,
        );

        Ok(result.code)
    }

    fn call_json(&self, prompt: &str, max_tokens: u32) -> oracle::Result<serde_json::Value> {
        let text = self.call(prompt, max_tokens)?;
        serde_json::from_str(text.trim()).map_err(|e| {
            isls_forge_llm::forge::ForgeLlmError::Oracle(format!(
                "failed to parse swarm JSON response: {e}: {text}"
            ))
        })
    }

    fn model_name(&self) -> &str {
        // Can't return a formatted string as &str, so use inner model name
        // with a note. The logging above provides full swarm context.
        self.inner.model_name()
    }

    fn cost_per_1k_tokens(&self) -> f64 {
        self.inner.cost_per_1k_tokens() * self.swarm_size as f64
    }

    fn count_tokens(&self, text: &str) -> u64 {
        self.inner.count_tokens(text)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_forge_llm::oracle::MockOracle;

    #[test]
    fn swarm_with_mock_returns_mock_response() {
        let swarm = SwarmOracle::new(Box::new(MockOracle), 4);
        let result = swarm.call("Generate CRUD", 1024);
        assert!(result.is_ok(), "swarm with mock should succeed");
        let code = result.unwrap();
        // MockOracle returns "// [mock: no implementation generated]"
        // All 4 candidates are identical → trivial consensus
        assert!(!code.is_empty(), "result should not be empty");
    }

    #[test]
    fn swarm_size_1_works() {
        let swarm = SwarmOracle::new(Box::new(MockOracle), 1);
        let result = swarm.call("test", 512);
        assert!(result.is_ok(), "swarm_size=1 should work");
    }

    #[test]
    fn swarm_cost_multiplied_by_size() {
        let swarm = SwarmOracle::new(Box::new(MockOracle), 4);
        assert_eq!(swarm.cost_per_1k_tokens(), 0.0); // MockOracle cost is 0.0
    }

    #[test]
    fn swarm_threshold_builder() {
        let swarm = SwarmOracle::new(Box::new(MockOracle), 4)
            .with_threshold(0.30)
            .with_max_coagula(3);
        assert_eq!(swarm.threshold, 0.30);
        assert_eq!(swarm.max_coagula, 3);
    }

    #[test]
    fn swarm_model_name_from_inner() {
        let swarm = SwarmOracle::new(Box::new(MockOracle), 4);
        assert_eq!(swarm.model_name(), "mock");
    }

    #[test]
    fn swarm_zero_size_clamped_to_1() {
        let swarm = SwarmOracle::new(Box::new(MockOracle), 0);
        assert_eq!(swarm.swarm_size, 1);
    }
}
