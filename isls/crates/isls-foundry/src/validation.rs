// ── C27 §5: Foundry Validation Pipeline ─────────────────────────────
//
// Extended validation that augments the 8-gate cascade with
// compilation, testing and code quality checks.

use serde::{Deserialize, Serialize};

/// Complete validation result for a fabricated project (Def 5.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundryValidation {
    // Foundry-specific gates
    pub compiles: bool,
    pub tests_pass: bool,
    pub test_count: usize,
    pub warnings: usize,
    pub formatted: bool,
    pub docs_build: bool,

    // Metrics
    pub loc: usize,
    pub test_coverage_estimate: f64,
    pub compilation_attempts: usize,
    pub oracle_tokens_used: usize,
}

impl FoundryValidation {
    /// A fabrication passes minimum quality (Req 5.1) if it compiles,
    /// tests pass, and at least one test exists.
    pub fn passes_minimum(&self) -> bool {
        self.compiles && self.tests_pass && self.test_count > 0
    }

    /// Fully clean: compiles, tests, formatted, no warnings.
    pub fn fully_clean(&self) -> bool {
        self.passes_minimum() && self.formatted && self.warnings == 0
    }
}

impl Default for FoundryValidation {
    fn default() -> Self {
        Self {
            compiles: false,
            tests_pass: false,
            test_count: 0,
            warnings: 0,
            formatted: false,
            docs_build: false,
            loc: 0,
            test_coverage_estimate: 0.0,
            compilation_attempts: 0,
            oracle_tokens_used: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_does_not_pass() {
        let v = FoundryValidation::default();
        assert!(!v.passes_minimum());
        assert!(!v.fully_clean());
    }

    #[test]
    fn minimum_pass() {
        let v = FoundryValidation {
            compiles: true,
            tests_pass: true,
            test_count: 3,
            ..Default::default()
        };
        assert!(v.passes_minimum());
        assert!(!v.fully_clean());
    }

    #[test]
    fn fully_clean_pass() {
        let v = FoundryValidation {
            compiles: true,
            tests_pass: true,
            test_count: 5,
            warnings: 0,
            formatted: true,
            docs_build: true,
            loc: 200,
            test_coverage_estimate: 0.8,
            compilation_attempts: 1,
            oracle_tokens_used: 1000,
        };
        assert!(v.passes_minimum());
        assert!(v.fully_clean());
    }
}
