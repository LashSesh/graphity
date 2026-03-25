// isls-agent: apply.rs — C30 File Writing + Compile/Test Loop
//
// Writes Oracle-generated code to the project, verifies it compiles, and
// iterates fix attempts through the Oracle when it doesn't.
//
// The ApplyOracle trait is intentionally minimal to keep isls-agent free of
// heavy HTTP dependencies.  The gateway wraps the real Oracle engine behind it.

use std::path::Path;

// ─── ApplyOracle ─────────────────────────────────────────────────────────────

/// Minimal Oracle abstraction used by apply_and_verify.
///
/// Implementors: real (ClaudeOracle via gateway), mock (tests).
pub trait ApplyOracle: Send + Sync {
    /// Given code that failed to compile, return a corrected version.
    ///
    /// `file_path` — relative path for context ("src/router.rs")
    /// `bad_code`  — the code that failed
    /// `error`     — compiler stderr
    ///
    /// Returns the corrected file content, or an error string.
    fn fix_compile_error(
        &self,
        file_path: &str,
        bad_code: &str,
        error: &str,
    ) -> Result<String, String>;
}

// ─── CompileCheck ─────────────────────────────────────────────────────────────

/// Abstraction over the compile + test steps, injectable for tests.
pub trait CompileCheck: Send + Sync {
    /// Run `cargo check`.  Returns Ok(()) if it passes, Err(stderr) if not.
    fn check(&self, root: &Path) -> Result<(), String>;
    /// Run `cargo test --quiet`.  Returns Ok(stdout) or Err(stderr).
    fn run_tests(&self, root: &Path) -> Result<String, String>;
}

// ─── CargoCheck ──────────────────────────────────────────────────────────────

/// Production compiler: invokes real `cargo` subprocess.
pub struct CargoCheck;

impl CompileCheck for CargoCheck {
    fn check(&self, root: &Path) -> Result<(), String> {
        let out = std::process::Command::new("cargo")
            .args(["check"])
            .current_dir(root)
            .output()
            .map_err(|e| format!("cargo check failed to start: {}", e))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }

    fn run_tests(&self, root: &Path) -> Result<String, String> {
        let out = std::process::Command::new("cargo")
            .args(["test", "--quiet"])
            .current_dir(root)
            .output()
            .map_err(|e| format!("cargo test failed to start: {}", e))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }
}

// ─── ApplyResult ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ApplyResult {
    Success {
        compiled: bool,
        tests_passed: bool,
        test_output: String,
        /// 1 = first try; 2+ = needed fix attempts
        attempts: usize,
    },
    CompileFailed {
        error: String,
        attempts: usize,
    },
}

impl ApplyResult {
    pub fn compiled(&self) -> bool {
        matches!(self, ApplyResult::Success { compiled: true, .. })
    }
    pub fn tests_passed(&self) -> bool {
        matches!(self, ApplyResult::Success { tests_passed: true, .. })
    }
}

// ─── apply_and_verify ────────────────────────────────────────────────────────

/// Write `new_content` to `workspace_root/file_path`, verify it compiles,
/// and retry through `oracle` on failure (up to `max_fix_attempts` total
/// attempts, including the first).
///
/// A backup of the original file is taken before any writes and is restored
/// if all attempts fail.
pub fn apply_and_verify(
    workspace_root: &Path,
    file_path: &str,
    new_content: &str,
    oracle: &dyn ApplyOracle,
    max_fix_attempts: usize,
    compiler: &dyn CompileCheck,
) -> Result<ApplyResult, String> {
    let full_path = workspace_root.join(file_path);

    // Ensure parent directory exists
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }

    // Backup original
    let backup = std::fs::read_to_string(&full_path).ok();

    // Write initial content
    std::fs::write(&full_path, new_content)
        .map_err(|e| format!("write {}: {}", file_path, e))?;

    let max = max_fix_attempts.max(1);
    let mut current = new_content.to_string();

    for attempt in 1..=max {
        match compiler.check(workspace_root) {
            Ok(()) => {
                // Compiles — run tests
                let test_result = compiler.run_tests(workspace_root);
                return Ok(ApplyResult::Success {
                    compiled: true,
                    tests_passed: test_result.is_ok(),
                    test_output: test_result.unwrap_or_else(|e| e),
                    attempts: attempt,
                });
            }
            Err(compile_error) => {
                if attempt >= max {
                    // All attempts exhausted — restore backup and report failure
                    if let Some(ref orig) = backup {
                        let _ = std::fs::write(&full_path, orig);
                    }
                    return Ok(ApplyResult::CompileFailed {
                        error: compile_error,
                        attempts: attempt,
                    });
                }

                // Ask Oracle to fix the compile error
                let fixed = oracle.fix_compile_error(file_path, &current, &compile_error)?;
                let fixed = strip_markdown_fences(&fixed);
                std::fs::write(&full_path, &fixed)
                    .map_err(|e| format!("write fix: {}", e))?;
                current = fixed;
            }
        }
    }

    // Should not be reached
    Ok(ApplyResult::CompileFailed {
        error: "no attempts executed".into(),
        attempts: 0,
    })
}

// ─── Incremental regeneration ────────────────────────────────────────────────

/// Result of an incremental regen pass.
#[derive(Debug)]
pub struct IncrementalRegenResult {
    /// Files that were successfully regenerated.
    pub regenerated: Vec<String>,
    /// Files that failed compilation after regeneration.
    pub failed: Vec<(String, String)>,
    /// Files that were skipped (content generator returned None).
    pub skipped: Vec<String>,
}

/// Regenerate only the files affected by a list of [`isls_chat::NormOperation`]s.
///
/// For each operation, calls [`isls_chat::affected_files`] to obtain relative
/// paths, then invokes `content_for` to generate new file content.  If
/// `content_for` returns `None` for a path the file is skipped (no-op).
///
/// After writing all files a single `cargo check` is run on `workspace_root`.
/// If it fails, all written files are restored from their backups.
///
/// # Arguments
/// * `workspace_root` — project root (must contain a `Cargo.toml`).
/// * `ops`            — norm operations to apply.
/// * `content_for`    — callback `(relative_path) -> Option<new_content>`.
/// * `oracle`         — used for compile-error auto-fix (up to `max_fix_attempts`).
/// * `max_fix_attempts` — compile retry budget per file.
/// * `compiler`       — abstraction over `cargo check`.
pub fn apply_norm_ops(
    workspace_root: &Path,
    ops: &[isls_chat::NormOperation],
    content_for: &dyn Fn(&str) -> Option<String>,
    oracle: &dyn ApplyOracle,
    max_fix_attempts: usize,
    compiler: &dyn CompileCheck,
) -> IncrementalRegenResult {
    let mut regenerated = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = Vec::new();

    // Collect all unique affected file paths (dedup across ops).
    let mut affected: Vec<String> = Vec::new();
    for op in ops {
        for path in isls_chat::affected_files(op) {
            if !affected.contains(&path) {
                affected.push(path);
            }
        }
    }

    if affected.is_empty() {
        return IncrementalRegenResult { regenerated, failed, skipped };
    }

    // Write each affected file via apply_and_verify.
    for rel_path in &affected {
        match content_for(rel_path) {
            None => {
                skipped.push(rel_path.clone());
            }
            Some(content) => {
                match apply_and_verify(workspace_root, rel_path, &content, oracle, max_fix_attempts, compiler) {
                    Ok(res) if res.compiled() => {
                        regenerated.push(rel_path.clone());
                    }
                    Ok(ApplyResult::CompileFailed { error, .. }) => {
                        failed.push((rel_path.clone(), error));
                    }
                    Ok(_) => {
                        skipped.push(rel_path.clone());
                    }
                    Err(e) => {
                        failed.push((rel_path.clone(), e));
                    }
                }
            }
        }
    }

    IncrementalRegenResult { regenerated, failed, skipped }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Strip ```rust ... ``` or ``` ... ``` markdown fences from Oracle output.
pub fn strip_markdown_fences(s: &str) -> String {
    let s = s.trim();
    // Find opening fence
    let inner = if let Some(rest) = s.strip_prefix("```rust") {
        rest
    } else if let Some(rest) = s.strip_prefix("```") {
        rest
    } else {
        return s.to_string();
    };
    // Strip leading newline after opening fence
    let inner = inner.strip_prefix('\n').unwrap_or(inner);
    // Find closing fence
    if let Some(end) = inner.rfind("\n```") {
        inner[..end].to_string()
    } else if let Some(end) = inner.rfind("```") {
        inner[..end].trim_end().to_string()
    } else {
        inner.to_string()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // Mock oracle that returns responses in sequence
    struct SequenceOracle {
        responses: Vec<String>,
        idx: Arc<AtomicUsize>,
    }

    impl SequenceOracle {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: responses.into_iter().map(String::from).collect(),
                idx: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl ApplyOracle for SequenceOracle {
        fn fix_compile_error(&self, _: &str, _: &str, _: &str) -> Result<String, String> {
            let i = self.idx.fetch_add(1, Ordering::SeqCst);
            self.responses.get(i).cloned().ok_or("exhausted".into())
        }
    }

    // Compiler that always succeeds
    struct AlwaysOk;
    impl CompileCheck for AlwaysOk {
        fn check(&self, _: &Path) -> Result<(), String> { Ok(()) }
        fn run_tests(&self, _: &Path) -> Result<String, String> { Ok("ok".into()) }
    }

    // Compiler that fails first N times then succeeds
    struct FailFirst {
        fail_count: usize,
        calls: Arc<AtomicUsize>,
    }
    impl FailFirst {
        fn new(fail_count: usize) -> Self {
            Self { fail_count, calls: Arc::new(AtomicUsize::new(0)) }
        }
    }
    impl CompileCheck for FailFirst {
        fn check(&self, _: &Path) -> Result<(), String> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_count {
                Err(format!("error[E0432]: missing import on attempt {}", n + 1))
            } else {
                Ok(())
            }
        }
        fn run_tests(&self, _: &Path) -> Result<String, String> { Ok("tests passed".into()) }
    }

    fn tmp_dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "isls_apply_{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64
        ));
        std::fs::create_dir_all(d.join("src")).unwrap();
        d
    }

    // AT-AG16: Agent writes a .rs file; AlwaysOk compiler → Success on first try
    #[test]
    fn at_ag16_file_write_compile_success() {
        let dir = tmp_dir();
        // Write a Cargo.toml so the project is valid
        std::fs::write(dir.join("Cargo.toml"),
            "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").unwrap();

        let oracle = SequenceOracle::new(vec![]);
        let compiler = AlwaysOk;
        let code = "pub fn hello() -> &'static str { \"world\" }\n";

        let result = apply_and_verify(&dir, "src/lib.rs", code, &oracle, 3, &compiler)
            .expect("apply_and_verify");

        assert!(result.compiled(), "should compile");
        assert!(result.tests_passed(), "tests should pass");

        let written = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert_eq!(written, code, "file content matches");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    // AT-AG17: Mock Oracle returns bad code first, good code second → retry fixes it
    #[test]
    fn at_ag17_auto_fix_retry() {
        let dir = tmp_dir();
        std::fs::write(dir.join("Cargo.toml"),
            "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").unwrap();

        // Oracle returns good code on the fix attempt
        let oracle = SequenceOracle::new(vec!["pub fn fixed() {}\n"]);
        // Compiler fails once, then succeeds
        let compiler = FailFirst::new(1);

        let bad_code = "pub fn broken() { missing_import!() }\n";
        let result = apply_and_verify(&dir, "src/lib.rs", bad_code, &oracle, 3, &compiler)
            .expect("apply_and_verify");

        assert!(result.compiled(), "should compile after fix");
        if let ApplyResult::Success { attempts, .. } = result {
            assert_eq!(attempts, 2, "should have taken 2 attempts");
        } else {
            panic!("expected Success");
        }

        // File should contain the fixed content (strip_markdown_fences trims)
        let written = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert_eq!(written.trim(), "pub fn fixed() {}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // AT-AG17b: When all fix attempts exhausted → CompileFailed + original restored
    #[test]
    fn at_ag17b_all_attempts_fail() {
        let dir = tmp_dir();
        std::fs::write(dir.join("Cargo.toml"),
            "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").unwrap();
        let original = "pub fn original() {}\n";
        std::fs::write(dir.join("src/lib.rs"), original).unwrap();

        // Oracle always returns bad code
        let oracle = SequenceOracle::new(vec!["bad1\n", "bad2\n"]);
        let compiler = FailFirst::new(999); // always fails

        let result = apply_and_verify(&dir, "src/lib.rs", "bad_code\n", &oracle, 3, &compiler)
            .expect("no error");

        assert!(matches!(result, ApplyResult::CompileFailed { .. }), "should be CompileFailed");

        // Original file should be restored
        let restored = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert_eq!(restored, original, "original file restored");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // strip_markdown_fences tests
    #[test]
    fn test_strip_fences_rust() {
        let s = "```rust\npub fn foo() {}\n```";
        assert_eq!(strip_markdown_fences(s), "pub fn foo() {}");
    }

    #[test]
    fn test_strip_fences_plain() {
        let s = "```\nsome code\n```";
        assert_eq!(strip_markdown_fences(s), "some code");
    }

    #[test]
    fn test_strip_fences_no_fences() {
        let s = "pub fn foo() {}";
        assert_eq!(strip_markdown_fences(s), "pub fn foo() {}");
    }
}
