// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Chameleon Pipeline — deterministic prompt projector.
//!
//! Transforms one base prompt into n focused variants using four canonical
//! lenses that target the four most common failure modes in ISLS code
//! generation (identified from D1–D7 debug cycles):
//!
//! 1. Wrong imports (Rule 11, ProvidedSymbol mismatch)
//! 2. Incomplete implementations (`todo!()` stubs)
//! 3. Missing error handling (`.unwrap()` on queries)
//! 4. Wrong naming (Rule 3 violations)
//!
//! Each lens attacks one failure mode. The consensus filters out answers
//! that fail on any of the four. Deterministic: same prompt → same projections.

const LENS_CORRECTNESS: &str = "\
PRIORITY: Type correctness. Every `use` import MUST resolve. \
Every function signature MUST match the AVAILABLE IMPORTS section exactly. \
If an import path is not listed, do NOT use it. Prefer explicit types over inference.";

const LENS_COMPLETENESS: &str = "\
PRIORITY: Complete implementation. Every CRUD function must have a full body. \
No `todo!()`, no `unimplemented!()`, no stub returns. \
Every SQL query must include all fields from the model struct.";

const LENS_ROBUSTNESS: &str = "\
PRIORITY: Error handling. Every `sqlx::query` call must \
`.map_err(|e| AppError::InternalError(e.to_string()))`. \
Not-found must return `AppError::NotFound`. \
Duplicate key must return `AppError::Conflict`. \
Never `.unwrap()` on database results.";

const LENS_CONSISTENCY: &str = "\
PRIORITY: Naming conventions. Functions: `get_{entity}`, `list_{entities}`, \
`create_{entity}`, `update_{entity}`, `delete_{entity}`. \
Parameters: `pool: &PgPool`, then payload, then id. \
Match the style of the PROVIDED SYMBOLS exactly.";

const LENSES: [&str; 4] = [
    LENS_CORRECTNESS,
    LENS_COMPLETENESS,
    LENS_ROBUSTNESS,
    LENS_CONSISTENCY,
];

const LENS_NAMES: [&str; 4] = [
    "Correctness",
    "Completeness",
    "Robustness",
    "Consistency",
];

/// Return the canonical lens name for the given index (mod 4).
pub fn lens_name(k: usize) -> &'static str {
    LENS_NAMES[k % LENS_NAMES.len()]
}

/// Project a base prompt into n focused variants using the four canonical lenses.
///
/// Each projection appends a focus directive that steers the LLM toward one
/// specific quality dimension. For n > 4, lenses cycle. For n = 1, returns
/// one prompt with the Correctness lens (graceful degradation).
///
/// Deterministic: same input → same output. Zero LLM cost.
pub fn project(base_prompt: &str, n: usize) -> Vec<String> {
    (0..n)
        .map(|k| {
            let lens = LENSES[k % LENSES.len()];
            format!(
                "{base}\n\n\
                 ## FOCUS DIRECTIVE (Thronengel {idx}/{total})\n\
                 {lens}\n\n\
                 Produce ONLY the Rust source code. \
                 No markdown fences. No explanations.",
                base = base_prompt,
                idx = k + 1,
                total = n,
                lens = lens,
            )
        })
        .collect()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_returns_n_strings() {
        let projections = project("Generate CRUD for Product", 4);
        assert_eq!(projections.len(), 4);
    }

    #[test]
    fn each_projection_contains_base_prompt() {
        let base = "Generate CRUD for Product";
        let projections = project(base, 4);
        for p in &projections {
            assert!(p.starts_with(base), "projection must start with base prompt");
        }
    }

    #[test]
    fn each_projection_has_unique_lens() {
        let projections = project("Generate CRUD", 4);
        assert!(projections[0].contains("Type correctness"));
        assert!(projections[1].contains("Complete implementation"));
        assert!(projections[2].contains("Error handling"));
        assert!(projections[3].contains("Naming conventions"));
    }

    #[test]
    fn projections_contain_focus_directive_header() {
        let projections = project("test", 4);
        assert!(projections[0].contains("Thronengel 1/4"));
        assert!(projections[1].contains("Thronengel 2/4"));
        assert!(projections[2].contains("Thronengel 3/4"));
        assert!(projections[3].contains("Thronengel 4/4"));
    }

    #[test]
    fn single_projection_uses_correctness_lens() {
        let projections = project("test", 1);
        assert_eq!(projections.len(), 1);
        assert!(projections[0].contains("Type correctness"));
        assert!(projections[0].contains("Thronengel 1/1"));
    }

    #[test]
    fn lenses_cycle_for_n_greater_than_4() {
        let projections = project("test", 6);
        assert_eq!(projections.len(), 6);
        // k=4 wraps to Correctness, k=5 wraps to Completeness
        assert!(projections[4].contains("Type correctness"));
        assert!(projections[5].contains("Complete implementation"));
    }

    #[test]
    fn projections_end_with_no_markdown_directive() {
        let projections = project("test", 4);
        for p in &projections {
            assert!(p.contains("No markdown fences"));
        }
    }

    #[test]
    fn lens_name_returns_correct_names() {
        assert_eq!(lens_name(0), "Correctness");
        assert_eq!(lens_name(1), "Completeness");
        assert_eq!(lens_name(2), "Robustness");
        assert_eq!(lens_name(3), "Consistency");
        assert_eq!(lens_name(4), "Correctness"); // cycles
    }

    #[test]
    fn deterministic_projections() {
        let a = project("determinism test", 4);
        let b = project("determinism test", 4);
        assert_eq!(a, b, "same input must produce same output");
    }
}
