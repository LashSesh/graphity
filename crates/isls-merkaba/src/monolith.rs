// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Monolith Gate — Mandorla selection and Swarm-Coagula repair.
//!
//! If D_total ≥ Θ: return the candidate with the highest D_k (Mandorla).
//! If D_total < Θ: Swarm-Coagula — take the best candidate, identify
//! disagreements, ask the oracle to fix them, re-evaluate.

use isls_forge_llm::oracle::{self, Oracle};

use crate::ophanim;

/// Mandorla selection result.
#[derive(Clone, Debug)]
pub struct SelectionResult {
    /// The selected code.
    pub code: String,
    /// Index of the selected candidate (or n if from Coagula).
    pub selected_index: usize,
    /// Whether Swarm-Coagula was triggered.
    pub coagula_triggered: bool,
    /// Number of Coagula rounds executed.
    pub coagula_rounds: u32,
}

/// Select the best candidate or trigger Swarm-Coagula.
///
/// - If `dtotal >= threshold`: return the Mandorla (highest D_k candidate).
/// - If `dtotal < threshold`: attempt Swarm-Coagula repair up to `max_coagula` rounds.
/// - If Coagula exhausts: return best candidate anyway (graceful degradation).
pub fn select(
    candidates: &[String],
    dk: &[f64],
    dtotal: f64,
    threshold: f64,
    max_coagula: u32,
    inner_oracle: &dyn Oracle,
    original_prompt: &str,
    max_tokens: u32,
) -> oracle::Result<SelectionResult> {
    if candidates.is_empty() {
        return Err(isls_forge_llm::forge::ForgeLlmError::Oracle(
            "no candidates to select from".into(),
        ));
    }

    // Find the Mandorla: candidate with highest D_k
    let best_idx = dk
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);

    // If D_total exceeds threshold, emit the Mandorla directly
    if dtotal >= threshold {
        tracing::info!(
            dtotal = dtotal,
            threshold = threshold,
            mandorla = best_idx,
            "Monolith: EMIT (Mandorla: T{})",
            best_idx + 1,
        );
        return Ok(SelectionResult {
            code: candidates[best_idx].clone(),
            selected_index: best_idx,
            coagula_triggered: false,
            coagula_rounds: 0,
        });
    }

    // ── Swarm-Coagula ────────────────────────────────────────────────────────
    tracing::warn!(
        dtotal = dtotal,
        threshold = threshold,
        "Monolith: Swarm-Coagula triggered (D_total < Θ)"
    );

    let base_code = &candidates[best_idx];
    let features = ophanim::extract_all_features(candidates);
    let base_features = &features[best_idx];

    // Find missing functions: present in ≥2 candidates but absent from base
    let missing = find_missing_functions(base_features, &features);

    if missing.is_empty() {
        // No actionable disagreements — return best as-is
        tracing::info!("Swarm-Coagula: no missing functions identified, returning Mandorla");
        return Ok(SelectionResult {
            code: base_code.clone(),
            selected_index: best_idx,
            coagula_triggered: true,
            coagula_rounds: 0,
        });
    }

    let mut current_best = base_code.clone();
    let mut rounds = 0;

    for round in 0..max_coagula {
        rounds = round + 1;
        tracing::info!(round = rounds, max = max_coagula, "Swarm-Coagula round");

        let coagula_prompt = build_coagula_prompt(original_prompt, &current_best, &missing);

        match inner_oracle.call(&coagula_prompt, max_tokens) {
            Ok(fixed) => {
                // Re-evaluate: does the fixed version have higher resonance?
                let mut eval_candidates: Vec<String> = candidates.to_vec();
                eval_candidates.push(fixed.clone());
                let (new_dk, new_pairwise) = ophanim::compute_resonance(&eval_candidates);
                let new_omega = crate::konus::compute_omega(&new_pairwise, eval_candidates.len());
                let new_dtotal = crate::konus::compute_dtotal(&new_dk, new_omega);

                if new_dtotal >= threshold {
                    // Fixed version achieved coherence
                    let fixed_dk = new_dk.last().copied().unwrap_or(0.0);
                    let best_new_idx = new_dk
                        .iter()
                        .enumerate()
                        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|(i, _)| i)
                        .unwrap_or(0);

                    tracing::info!(
                        new_dtotal = new_dtotal,
                        fixed_dk = fixed_dk,
                        "Swarm-Coagula: coherence achieved after round {}",
                        rounds
                    );

                    return Ok(SelectionResult {
                        code: eval_candidates[best_new_idx].clone(),
                        selected_index: best_new_idx,
                        coagula_triggered: true,
                        coagula_rounds: rounds,
                    });
                }

                // Not yet coherent — use fixed as new base for next round
                current_best = fixed;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Swarm-Coagula: inner oracle call failed");
                break;
            }
        }
    }

    // Coagula exhausted — return best candidate (graceful degradation, Rule 10)
    tracing::warn!(
        rounds = rounds,
        "Swarm-Coagula exhausted, returning best candidate (graceful degradation)"
    );
    Ok(SelectionResult {
        code: current_best,
        selected_index: best_idx,
        coagula_triggered: true,
        coagula_rounds: rounds,
    })
}

/// Find functions present in ≥2 candidates but absent from the base.
fn find_missing_functions(
    base: &ophanim::CodeFeatures,
    all: &[ophanim::CodeFeatures],
) -> Vec<String> {
    use std::collections::HashMap;

    // Count how many candidates have each function
    let mut fn_counts: HashMap<&str, usize> = HashMap::new();
    for features in all {
        for f in &features.functions {
            *fn_counts.entry(f.as_str()).or_insert(0) += 1;
        }
    }

    // Functions present in ≥2 candidates but absent from base
    fn_counts
        .iter()
        .filter(|(&f, &count)| count >= 2 && !base.functions.contains(f))
        .map(|(&f, _)| f.to_string())
        .collect()
}

/// Build a Coagula prompt from the base code and missing elements.
fn build_coagula_prompt(original_prompt: &str, base_code: &str, missing: &[String]) -> String {
    let missing_list = missing
        .iter()
        .map(|f| format!("- {}", f))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "{original_prompt}\n\n\
         ## COAGULA FIX REQUEST\n\
         The following code was selected as the best candidate but peer review \
         identified missing elements. Fix the code to include them.\n\n\
         ### Current code:\n\
         ```rust\n{base_code}\n```\n\n\
         ### Missing elements (expected by peer review):\n\
         {missing_list}\n\n\
         Produce ONLY the fixed Rust source code. No markdown fences. No explanations.",
    )
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_forge_llm::oracle::MockOracle;

    #[test]
    fn mandorla_picks_highest_dk() {
        let candidates = vec![
            "fn a() {}".to_string(),
            "fn b() {}".to_string(),
            "fn c() {}".to_string(),
        ];
        let dk = vec![0.3, 0.8, 0.5];
        let result = select(
            &candidates, &dk, 0.5, 0.20, 2,
            &MockOracle, "test", 1024,
        ).unwrap();
        assert_eq!(result.selected_index, 1, "should pick candidate with highest D_k");
        assert!(!result.coagula_triggered);
    }

    #[test]
    fn coagula_triggered_when_below_threshold() {
        let candidates = vec![
            "fn a() {}".to_string(),
            "fn b() {}".to_string(),
        ];
        let dk = vec![0.1, 0.15];
        let result = select(
            &candidates, &dk, 0.05, 0.50, 1,
            &MockOracle, "test", 1024,
        ).unwrap();
        assert!(result.coagula_triggered);
    }

    #[test]
    fn empty_candidates_returns_error() {
        let result = select(
            &[], &[], 0.0, 0.20, 2,
            &MockOracle, "test", 1024,
        );
        assert!(result.is_err());
    }

    #[test]
    fn coagula_prompt_contains_missing_elements() {
        let prompt = build_coagula_prompt(
            "Generate CRUD",
            "fn get_product() {}",
            &["list_products/1".to_string(), "create_product/2".to_string()],
        );
        assert!(prompt.contains("COAGULA FIX REQUEST"));
        assert!(prompt.contains("list_products/1"));
        assert!(prompt.contains("create_product/2"));
        assert!(prompt.contains("fn get_product()"));
    }
}
