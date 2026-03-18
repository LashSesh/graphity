// ── C27 §3: Toolchain Executor ──────────────────────────────────────
//
// Shells out to real cargo commands for compile-test-fix verification.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// Result of a single toolchain invocation.
#[derive(Debug, Clone)]
pub struct ToolchainResult {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

/// Executes cargo / rustfmt commands against a project directory.
#[derive(Debug, Clone)]
pub struct ToolchainExecutor {
    pub cargo_path: String,
    pub rustfmt_path: String,
    pub timeout_seconds: u64,
}

impl Default for ToolchainExecutor {
    fn default() -> Self {
        Self {
            cargo_path: "cargo".into(),
            rustfmt_path: "rustfmt".into(),
            timeout_seconds: 120,
        }
    }
}

impl ToolchainExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check whether `cargo` is available on the system.
    pub fn cargo_available(&self) -> bool {
        Command::new(&self.cargo_path)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn cargo_check(&self, dir: &Path) -> ToolchainResult {
        self.run_cargo(&["check", "--message-format=short"], dir)
    }

    pub fn cargo_build(&self, dir: &Path) -> ToolchainResult {
        self.run_cargo(&["build"], dir)
    }

    pub fn cargo_test(&self, dir: &Path) -> ToolchainResult {
        self.run_cargo(&["test"], dir)
    }

    pub fn cargo_clippy(&self, dir: &Path) -> ToolchainResult {
        self.run_cargo(&["clippy", "--", "-D", "clippy::correctness"], dir)
    }

    pub fn cargo_fmt_check(&self, dir: &Path) -> ToolchainResult {
        self.run_cargo(&["fmt", "--check"], dir)
    }

    pub fn cargo_fmt(&self, dir: &Path) -> ToolchainResult {
        self.run_cargo(&["fmt"], dir)
    }

    pub fn cargo_doc(&self, dir: &Path) -> ToolchainResult {
        self.run_cargo(&["doc", "--no-deps"], dir)
    }

    // ── internal ────────────────────────────────────────────────────

    fn run_cargo(&self, args: &[&str], dir: &Path) -> ToolchainResult {
        let start = Instant::now();
        let output = Command::new(&self.cargo_path)
            .args(args)
            .current_dir(dir)
            .output();

        let elapsed = start.elapsed().as_millis() as u64;

        match output {
            Ok(o) => ToolchainResult {
                success: o.status.success(),
                exit_code: o.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
                duration_ms: elapsed,
            },
            Err(e) => ToolchainResult {
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("failed to spawn cargo: {e}"),
                duration_ms: elapsed,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_executor() {
        let e = ToolchainExecutor::new();
        assert_eq!(e.cargo_path, "cargo");
        assert_eq!(e.timeout_seconds, 120);
    }

    #[test]
    fn toolchain_result_failed_spawn() {
        let e = ToolchainExecutor {
            cargo_path: "/nonexistent/cargo".into(),
            ..Default::default()
        };
        let r = e.cargo_check(Path::new("/tmp"));
        assert!(!r.success);
        assert!(r.stderr.contains("failed to spawn"));
    }
}
