// isls-agent: conversation.rs — Multi-turn Conversation History (C30)
//
// Maintains a rolling window of user↔agent turns so the Oracle understands
// context across requests.  Persisted to ~/.isls/agent/conversation.json.

use std::path::Path;

use serde::{Deserialize, Serialize};

// ─── ConversationTurn ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConversationTurn {
    /// "user" or "agent"
    pub role: String,
    pub content: String,
    /// Unix timestamp in seconds
    pub timestamp: u64,
    /// Files modified during this turn (empty for user turns)
    pub files_changed: Vec<String>,
}

impl ConversationTurn {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            timestamp: unix_now(),
            files_changed: Vec::new(),
        }
    }

    pub fn agent(content: impl Into<String>, files_changed: Vec<String>) -> Self {
        Self {
            role: "agent".to_string(),
            content: content.into(),
            timestamp: unix_now(),
            files_changed,
        }
    }
}

// ─── Conversation ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Conversation {
    pub turns: Vec<ConversationTurn>,
    /// Maximum turns to keep (oldest are dropped when exceeded)
    pub max_turns: usize,
}

impl Conversation {
    pub fn new(max_turns: usize) -> Self {
        Self { turns: Vec::new(), max_turns }
    }

    /// Append a turn, dropping the oldest if the window is full.
    pub fn push(&mut self, turn: ConversationTurn) {
        self.turns.push(turn);
        if self.max_turns > 0 && self.turns.len() > self.max_turns {
            self.turns.remove(0);
        }
    }

    /// The last `n` turns (most recent), in chronological order.
    pub fn last_n(&self, n: usize) -> &[ConversationTurn] {
        let len = self.turns.len();
        if len <= n {
            &self.turns
        } else {
            &self.turns[len - n..]
        }
    }

    /// Format turns for inclusion in an Oracle prompt.
    pub fn prompt_fragment(&self, n: usize) -> String {
        self.last_n(n)
            .iter()
            .map(|t| format!("{}: {}", t.role.to_uppercase(), t.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ─── Persistence ────────────────────────────────────────────────────────

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir: {}", e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(path, json).map_err(|e| format!("write: {}", e))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| format!("read: {}", e))?;
        serde_json::from_str(&json).map_err(|e| format!("deserialize: {}", e))
    }

    pub fn load_or_default(path: &Path, max_turns: usize) -> Self {
        Self::load(path).unwrap_or_else(|_| Self::new(max_turns))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ─── Tests (AT-AG18) ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "isls_conv_test_{:016x}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64
        ))
    }

    // AT-AG18: Conversation persistence — two turns, verify second prompt includes first turn
    #[test]
    fn at_ag18_conversation_persistence() {
        let path = tmp_path();
        let mut conv = Conversation::new(20);

        conv.push(ConversationTurn::user("add a search endpoint"));
        conv.push(ConversationTurn::agent("Done — modified router.rs and database.rs", vec!["router.rs".into(), "database.rs".into()]));

        conv.save(&path).expect("save");

        let loaded = Conversation::load(&path).expect("load");
        assert_eq!(loaded.turns.len(), 2, "two turns persisted");
        assert_eq!(loaded.turns[0].role, "user");
        assert_eq!(loaded.turns[1].role, "agent");
        assert_eq!(loaded.turns[1].files_changed, vec!["router.rs", "database.rs"]);

        // Second turn prompt should include first turn context
        let fragment = loaded.prompt_fragment(5);
        assert!(fragment.contains("search endpoint"), "first turn in fragment");
        assert!(fragment.contains("router.rs"), "agent response in fragment");

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn at_ag18b_max_turns_window() {
        let mut conv = Conversation::new(3);
        for i in 0..5 {
            conv.push(ConversationTurn::user(format!("message {}", i)));
        }
        // Only last 3 messages should be kept
        assert_eq!(conv.turns.len(), 3, "max_turns window enforced");
        assert_eq!(conv.turns[0].content, "message 2");
        assert_eq!(conv.turns[2].content, "message 4");
    }

    #[test]
    fn at_ag18c_load_or_default_when_missing() {
        let missing = std::path::PathBuf::from("/tmp/isls_no_such_file.json");
        let conv = Conversation::load_or_default(&missing, 20);
        assert_eq!(conv.turns.len(), 0, "empty default when file missing");
    }
}
