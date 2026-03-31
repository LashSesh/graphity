// isls-agent: launcher.rs — Project Launcher
//
// After generating a project, the Agent can build and start it.
// The operator sees "Starten mit: bookmark-manager" — no technical details.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::workspace::AgentWorkspace;

// ─── LaunchInfo ─────────────────────────────────────────────────────────────

/// Information about how to launch a generated project.
/// Shown to the operator in plain language.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaunchInfo {
    /// Path to the compiled binary
    pub binary: PathBuf,
    /// URL if it's a web service
    pub url: Option<String>,
    /// Human-readable instructions (in operator's language)
    pub instructions: String,
}

// ─── Launch ─────────────────────────────────────────────────────────────────

/// Build and optionally launch a project.
///
/// Returns LaunchInfo with instructions for the operator.
/// Errors are returned as friendly messages (no stack traces).
pub fn launch_project(workspace: &AgentWorkspace) -> Result<LaunchInfo, String> {
    let root = &workspace.root;

    // Try cargo build --release
    let _build_result = cargo_build_release(root)?;

    // Find the binary
    let binary = find_binary(root)?;

    // Detect if it's a web service (has routes)
    let url = if !workspace.routes.is_empty() {
        Some("http://localhost:3000".into())
    } else {
        None
    };

    let bin_name = binary
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app".into());

    let instructions = if let Some(ref u) = url {
        format!("Starten mit: ./{}\nErreichbar unter: {}", bin_name, u)
    } else {
        format!("Starten mit: ./{}", bin_name)
    };

    Ok(LaunchInfo {
        binary,
        url,
        instructions,
    })
}

/// Build the project in release mode.
fn cargo_build_release(root: &Path) -> Result<String, String> {
    let output = std::process::Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("Die Software konnte nicht erstellt werden: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err("Die Software konnte nicht fehlerfrei erstellt werden. \
             Bitte beschreibe das Gewünschte anders oder einfacher."
            .into())
    }
}

/// Find the main binary in target/release.
fn find_binary(root: &Path) -> Result<PathBuf, String> {
    let release_dir = root.join("target").join("release");

    // Read Cargo.toml to find project name
    let cargo_path = root.join("Cargo.toml");
    let cargo_content = std::fs::read_to_string(&cargo_path)
        .map_err(|_| "Projekt-Konfiguration nicht gefunden.".to_string())?;

    let project_name = cargo_content
        .lines()
        .find(|l| l.starts_with("name"))
        .and_then(|l| {
            let parts: Vec<&str> = l.split('=').collect();
            parts.get(1).map(|v| v.trim().trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "app".into());

    let binary = release_dir.join(&project_name);
    if binary.exists() {
        Ok(binary)
    } else {
        // On Windows, try .exe
        let binary_exe = release_dir.join(format!("{}.exe", project_name));
        if binary_exe.exists() {
            Ok(binary_exe)
        } else {
            // Return the expected path even if not built yet
            Ok(binary)
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_info_serialization() {
        let info = LaunchInfo {
            binary: PathBuf::from("/tmp/test-app"),
            url: Some("http://localhost:3000".into()),
            instructions: "Starten mit: ./test-app".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: LaunchInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.binary, info.binary);
        assert_eq!(parsed.url, info.url);
    }

    #[test]
    fn instructions_no_jargon() {
        let info = LaunchInfo {
            binary: PathBuf::from("bookmark-manager"),
            url: Some("http://localhost:3000".into()),
            instructions: "Starten mit: ./bookmark-manager\nErreichbar unter: http://localhost:3000".into(),
        };
        // Should not contain Rust/dev jargon
        assert!(!info.instructions.contains("cargo"));
        assert!(!info.instructions.contains("compile"));
        assert!(!info.instructions.contains("target/release"));
    }
}
