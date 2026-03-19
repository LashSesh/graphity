// isls-agent: prompt.rs — C30 Workspace-Aware Oracle Prompt Builder
//
// Builds an Oracle prompt that includes the ACTUAL CODE from the operator's
// project so the Oracle can make surgical modifications.

use crate::{
    conversation::ConversationTurn,
    workspace::{AgentWorkspace, ModuleInfo},
};

// ─── WorkspacePrompt ─────────────────────────────────────────────────────────

/// The two-part prompt handed to the Oracle (or gateway's Oracle engine).
#[derive(Clone, Debug)]
pub struct WorkspacePrompt {
    pub system: String,
    pub user: String,
}

// ─── PatternHint ─────────────────────────────────────────────────────────────

/// Lightweight pattern hint from memory (avoids isls-oracle dependency).
#[derive(Clone, Debug)]
pub struct PatternHint {
    pub domain: String,
    pub quality_score: f64,
    pub summary: String,
}

// ─── Builder ─────────────────────────────────────────────────────────────────

/// Build an Oracle prompt that includes:
///   1. Project summary
///   2. Relevant file contents (read from disk, smart-truncated)
///   3. Known patterns that match (optional)
///   4. Conversation history (last 5 turns)
///
/// The system prompt instructs the Oracle to output ONLY the complete
/// modified file — no markdown fences, no explanation.
pub fn build_workspace_prompt(
    user_message: &str,
    workspace: &AgentWorkspace,
    relevant_files: &[&ModuleInfo],
    memory_matches: &[PatternHint],
    conversation: &[ConversationTurn],
) -> WorkspacePrompt {
    let mut context_parts: Vec<String> = Vec::new();

    // 1. Project summary
    context_parts.push(format!("PROJECT: {}", workspace.summary()));

    // 2. Relevant file contents
    let max_per_file = 2000usize;
    let max_total = 16_000usize;
    let mut used = 0usize;
    for module in relevant_files {
        let full_path = workspace.root.join(&module.path);
        let content = std::fs::read_to_string(&full_path).unwrap_or_default();
        let truncated = crate::workspace::smart_truncate(&content, max_per_file);
        let chunk = format!("FILE: {}\n```rust\n{}\n```", module.path, truncated);
        if used + chunk.len() > max_total {
            break;
        }
        used += chunk.len();
        context_parts.push(chunk);
    }

    // 3. Known patterns
    if !memory_matches.is_empty() {
        context_parts.push("KNOWN PATTERNS:".to_string());
        for p in memory_matches.iter().take(3) {
            context_parts.push(format!(
                "- {} (quality: {:.2}): {}",
                p.domain, p.quality_score, p.summary
            ));
        }
    }

    // 4. Conversation history (last 5 turns)
    if !conversation.is_empty() {
        context_parts.push("CONVERSATION:".to_string());
        let start = conversation.len().saturating_sub(5);
        for turn in &conversation[start..] {
            context_parts.push(format!("{}: {}", turn.role.to_uppercase(), turn.content));
        }
    }

    let context = context_parts.join("\n\n");

    let system = format!(
        "You are a Rust development agent. You modify EXISTING code in the operator's project.\n\
         Rules:\n\
         - Output ONLY the complete modified file. No markdown fences. No explanations.\n\
         - Preserve all existing code that is not related to the change.\n\
         - Add proper imports if needed.\n\
         - Follow the existing code style.\n\
         - If creating a new file, output the complete file content.\n\n\
         {context}"
    );

    WorkspacePrompt { system, user: user_message.to_string() }
}

// ─── Tests (AT-AG15) ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{AgentWorkspace, ModuleInfo};
    use std::path::PathBuf;

    // AT-AG15: Prompt with context — verify Oracle prompt contains file contents
    #[test]
    fn at_ag15_prompt_contains_file_contents() {
        // Create a real temp workspace with a known file
        let dir = std::env::temp_dir().join(format!(
            "isls_prompt_{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let file_content = "pub struct BookmarkRepository { pool: String }\n\
                            pub fn list_all() -> Vec<String> { vec![] }\n";
        std::fs::write(dir.join("src/lib.rs"), file_content).unwrap();

        let ws = AgentWorkspace::analyze(&dir).expect("analyze");
        let relevant: Vec<&ModuleInfo> = ws.modules.iter().collect();
        let hints = vec![PatternHint {
            domain: "rust".into(),
            quality_score: 0.92,
            summary: "REST CRUD pattern".into(),
        }];
        let conv = vec![
            ConversationTurn::user("add a search endpoint"),
            ConversationTurn::agent("Done", vec!["src/lib.rs".into()]),
        ];

        let prompt = build_workspace_prompt(
            "also add pagination to list_all",
            &ws,
            &relevant,
            &hints,
            &conv,
        );

        // System prompt must contain the file contents from disk
        assert!(
            prompt.system.contains("BookmarkRepository"),
            "prompt should contain struct name from file"
        );
        assert!(
            prompt.system.contains("list_all"),
            "prompt should contain function name from file"
        );
        assert!(
            prompt.system.contains("KNOWN PATTERNS"),
            "prompt should contain pattern hints"
        );
        assert!(
            prompt.system.contains("CONVERSATION"),
            "prompt should contain conversation history"
        );
        assert!(
            prompt.system.contains("search endpoint"),
            "prompt should contain previous user turn"
        );
        // User message is separate
        assert_eq!(prompt.user, "also add pagination to list_all");

        // The system prompt must enforce no-markdown-fence output
        assert!(
            prompt.system.contains("No markdown fences"),
            "system prompt must instruct no fences"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Additional: prompt with no files still has project summary
    #[test]
    fn at_ag15b_prompt_summary_always_present() {
        let dir = std::env::temp_dir().join(format!(
            "isls_prompt2_{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"mini\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let ws = AgentWorkspace::analyze(&dir).expect("analyze");
        let prompt = build_workspace_prompt("do something", &ws, &[], &[], &[]);
        assert!(prompt.system.contains("PROJECT:"), "project summary always present");
        assert!(prompt.system.contains("Rust"), "crate type in summary");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
