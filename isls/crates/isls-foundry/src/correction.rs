// ── C27 §2: Correction Prompts ──────────────────────────────────────
//
// Structured error feedback for the Oracle correction loop.

use serde::{Deserialize, Serialize};

use crate::workspace::WorkspaceContext;

/// Classification of a build / test error (Def 2.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorClass {
    /// Parse error, missing semicolons, brackets.
    Syntax,
    /// Type mismatch, wrong generics, missing From impl.
    Type,
    /// Missing use statements, wrong module paths.
    Import,
    /// Missing trait implementation, wrong bounds.
    Trait,
    /// Borrow checker errors.
    Lifetime,
    /// Assertion failure, panic in test.
    Test,
    /// Test passes but wrong output.
    Logic,
    /// Catch-all for unclassified errors.
    Unknown,
}

impl ErrorClass {
    /// Classify a `rustc` or `cargo test` error from its stderr text.
    pub fn classify(stderr: &str) -> Self {
        if stderr.contains("expected `;`")
            || stderr.contains("unexpected token")
            || stderr.contains("unclosed delimiter")
            || stderr.contains("expected one of")
        {
            return Self::Syntax;
        }
        if stderr.contains("mismatched types")
            || stderr.contains("expected type")
            || stderr.contains("E0308")
            || stderr.contains("E0277")
        {
            return Self::Type;
        }
        if stderr.contains("unresolved import")
            || stderr.contains("could not find")
            || stderr.contains("E0432")
            || stderr.contains("E0433")
        {
            return Self::Import;
        }
        if stderr.contains("not implemented")
            || stderr.contains("the trait bound")
            || stderr.contains("E0046")
        {
            return Self::Trait;
        }
        if stderr.contains("borrow")
            || stderr.contains("lifetime")
            || stderr.contains("E0505")
            || stderr.contains("E0502")
            || stderr.contains("E0597")
        {
            return Self::Lifetime;
        }
        if stderr.contains("assertion `left == right` failed")
            || stderr.contains("assertion failed")
            || stderr.contains("panicked at")
        {
            return Self::Test;
        }
        Self::Unknown
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Syntax => "syntax",
            Self::Type => "type",
            Self::Import => "import",
            Self::Trait => "trait",
            Self::Lifetime => "lifetime",
            Self::Test => "test",
            Self::Logic => "logic",
            Self::Unknown => "unknown",
        }
    }
}

/// A structured correction prompt sent to the Oracle when
/// compilation or tests fail (Def 2.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionPrompt {
    pub original_code: String,
    pub error_output: String,
    pub error_file: String,
    pub error_line: Option<usize>,
    pub error_class: ErrorClass,
    pub attempt: usize,
    pub max_attempts: usize,
    pub context: WorkspaceContext,
}

impl CorrectionPrompt {
    /// Build the system + user prompt pair for the Oracle.
    pub fn to_oracle_prompt(&self) -> (String, String) {
        let system = "You previously generated code that failed to compile. \
            Fix the error. Return ONLY the corrected complete file. \
            Do not explain. Do not apologize. Just fix it."
            .to_string();

        let mut user = format!(
            "File: {}\nError (attempt {}/{}, class={}):\n{}\n\nOriginal code:\n{}\n",
            self.error_file,
            self.attempt,
            self.max_attempts,
            self.error_class.as_str(),
            self.error_output,
            self.original_code,
        );

        if !self.context.summary.is_empty() {
            user.push_str(&format!("\nContext:\n{}\n", self.context.summary));
        }
        for t in &self.context.relevant_types {
            user.push_str(&format!(
                "  existing type: {} ({:?}) in {}\n",
                t.name, t.kind, t.module,
            ));
        }
        for t in &self.context.relevant_traits {
            user.push_str(&format!("  existing trait: {} in {}\n", t.name, t.module));
        }

        (system, user)
    }

    /// Extract the file path from a cargo error line like
    /// `  --> src/lib.rs:42:5`.
    pub fn extract_error_location(stderr: &str) -> (String, Option<usize>) {
        for line in stderr.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("--> ") {
                let parts: Vec<&str> = rest.splitn(3, ':').collect();
                let file = parts.first().unwrap_or(&"").to_string();
                let line_no = parts.get(1).and_then(|s| s.parse::<usize>().ok());
                return (file, line_no);
            }
        }
        (String::new(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_type_error() {
        let stderr = r#"error[E0277]: the trait bound `User: Serialize` is not satisfied"#;
        assert_eq!(ErrorClass::classify(stderr), ErrorClass::Type);
    }

    #[test]
    fn classify_import_error() {
        let stderr = r#"error[E0432]: unresolved import `crate::models`"#;
        assert_eq!(ErrorClass::classify(stderr), ErrorClass::Import);
    }

    #[test]
    fn classify_test_error() {
        let stderr = "thread 'test' panicked at 'assertion `left == right` failed'";
        assert_eq!(ErrorClass::classify(stderr), ErrorClass::Test);
    }

    #[test]
    fn classify_lifetime_error() {
        let stderr = "error[E0502]: cannot borrow `x` as mutable";
        assert_eq!(ErrorClass::classify(stderr), ErrorClass::Lifetime);
    }

    #[test]
    fn extract_location() {
        let stderr = "error[E0599]: no method\n  --> src/handlers.rs:42:5\n  |";
        let (file, line) = CorrectionPrompt::extract_error_location(stderr);
        assert_eq!(file, "src/handlers.rs");
        assert_eq!(line, Some(42));
    }

    #[test]
    fn to_oracle_prompt_includes_context() {
        let prompt = CorrectionPrompt {
            original_code: "fn main() {}".into(),
            error_output: "error: missing semicolon".into(),
            error_file: "src/main.rs".into(),
            error_line: Some(1),
            error_class: ErrorClass::Syntax,
            attempt: 1,
            max_attempts: 5,
            context: WorkspaceContext {
                summary: "demo (Bin), 1 modules".into(),
                relevant_types: Vec::new(),
                relevant_traits: Vec::new(),
                relevant_imports: Vec::new(),
                existing_patterns: Vec::new(),
                file_being_modified: None,
            },
        };
        let (sys, usr) = prompt.to_oracle_prompt();
        assert!(sys.contains("Fix the error"));
        assert!(usr.contains("src/main.rs"));
        assert!(usr.contains("attempt 1/5"));
    }
}
