//! Plain chat endpoint — pure Ollama conversation without entity extraction,
//! session state, or norm composition. Used by Studio's 💬 Chat mode.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::AppState;

const CHAT_SYSTEM_PROMPT: &str = "You are ISLS, an intelligent software \
architecture assistant. You help with questions about software design, Rust \
programming, architecture patterns, and technical decisions. Answer concisely \
and practically. You speak German and English.";

#[derive(Debug, Deserialize)]
pub struct ChatPlainRequest {
    pub message: String,
    #[serde(default)]
    pub history: Vec<ChatMessage>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String, // "user" | "assistant"
    pub content: String,
}

/// POST /api/chat/plain — stateless LLM conversation via local Ollama.
///
/// Unlike `/api/chat` (norm-aware v3.2) or `/api/session/{id}/message`
/// (architect with entity extraction), this endpoint just forwards the
/// prompt + history to the configured Ollama model and returns the raw
/// response. No session, no spec, no entities.
pub async fn api_chat_plain(
    State(state): State<AppState>,
    Json(req): Json<ChatPlainRequest>,
) -> Json<serde_json::Value> {
    if !state.oracle_config.use_ollama {
        return Json(serde_json::json!({
            "ok": false,
            "error": "plain chat requires --ollama (local model)"
        }));
    }

    let url = state.oracle_config.ollama_url.clone();
    let model = state.oracle_config.ollama_model.clone();

    // Build a flat prompt: system + last ≤10 history turns + user message.
    let mut prompt = String::from(CHAT_SYSTEM_PROMPT);
    prompt.push_str("\n\n");
    let start = req.history.len().saturating_sub(10);
    for m in &req.history[start..] {
        let tag = if m.role == "user" { "User" } else { "Assistant" };
        prompt.push_str(tag);
        prompt.push_str(": ");
        prompt.push_str(&m.content);
        prompt.push('\n');
    }
    prompt.push_str("User: ");
    prompt.push_str(&req.message);
    prompt.push_str("\nAssistant:");

    let result = tokio::task::spawn_blocking(move || {
        use isls_forge_llm::Oracle as _;
        let oracle = isls_forge_llm::oracle::OllamaOracle::new(&model, &url);
        oracle
            .call(&prompt, 1024)
            .map_err(|e| format!("Ollama call failed: {}", e))
    })
    .await;

    match result {
        Ok(Ok(text)) => Json(serde_json::json!({ "ok": true, "response": text })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}
