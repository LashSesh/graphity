// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Oracle abstraction for ISLS v2.1 multi-pass rendering.
//!
//! The `Oracle` trait decouples the render loop from any specific LLM backend.
//! `MockOracle` is used for offline / CI runs. `OpenAiOracle` calls the OpenAI
//! chat completions API with temperature 0.2 for deterministic code generation.

use serde_json::json;

use crate::RenderloopError;

pub type Result<T> = std::result::Result<T, RenderloopError>;

/// Approximate token count: 1 token ≈ 4 bytes of UTF-8 text.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64).saturating_add(3) / 4
}

// ─── Oracle trait ─────────────────────────────────────────────────────────────

/// Abstraction over an LLM backend used during multi-pass rendering.
pub trait Oracle: Send + Sync {
    /// Send a prompt and return the model's text response.
    fn call(&self, prompt: &str, max_tokens: u32) -> Result<String>;

    /// Send a prompt that must return a JSON value.
    fn call_json(&self, prompt: &str, max_tokens: u32) -> Result<serde_json::Value>;

    /// Identifier of the underlying model, e.g. `"gpt-4o-mini"`.
    fn model_name(&self) -> &str;

    /// Estimated cost per 1 000 tokens in USD.
    fn cost_per_1k_tokens(&self) -> f64;

    /// Estimate the number of tokens in the given text.
    fn count_tokens(&self, text: &str) -> u64 {
        estimate_tokens(text)
    }
}

// ─── MockOracle ───────────────────────────────────────────────────────────────

/// No-op oracle for offline and CI use.
///
/// Returns deterministic placeholder strings without making any network calls.
/// Useful for verifying the render loop machinery without an API key.
pub struct MockOracle;

impl Oracle for MockOracle {
    fn call(&self, _prompt: &str, _max_tokens: u32) -> Result<String> {
        // Return a minimal valid Rust comment so parse checks succeed
        Ok("// [mock: no implementation generated]".to_string())
    }

    fn call_json(&self, _prompt: &str, _max_tokens: u32) -> Result<serde_json::Value> {
        Ok(json!({"fixes": [], "notes": "mock oracle — no analysis performed"}))
    }

    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_1k_tokens(&self) -> f64 {
        0.0
    }
}

// ─── OpenAiOracle ─────────────────────────────────────────────────────────────

/// Oracle implementation backed by the OpenAI chat completions API.
///
/// Uses temperature 0.2 for code generation to balance creativity with
/// determinism. Reads the API key from the `api_key` field or falls back to
/// the `OPENAI_API_KEY` environment variable.
pub struct OpenAiOracle {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::blocking::Client,
}

impl OpenAiOracle {
    /// Create a new OpenAI oracle.
    ///
    /// If `api_key` is `None`, the key is read from `OPENAI_API_KEY` env var
    /// (or from a `.env` file loaded by the caller).
    pub fn new(api_key: Option<String>, model: Option<String>) -> Result<Self> {
        let key = match api_key {
            Some(k) if !k.is_empty() => k,
            _ => std::env::var("OPENAI_API_KEY")
                .map_err(|_| RenderloopError::OracleConfig(
                    "OPENAI_API_KEY not set and no --api-key provided".into()
                ))?,
        };
        let model = model.unwrap_or_else(|| "gpt-4o-mini".to_string());
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| RenderloopError::OracleConfig(e.to_string()))?;
        Ok(OpenAiOracle {
            api_key: key,
            model,
            base_url: "https://api.openai.com/v1".to_string(),
            client,
        })
    }

    fn chat_completions(&self, system: &str, user: &str, max_tokens: u32) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = json!({
            "model": self.model,
            "temperature": 0.2,
            "max_tokens": max_tokens,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ]
        });

        let resp = self.client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|e| RenderloopError::OracleCall(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(RenderloopError::OracleCall(
                format!("OpenAI API error {}: {}", status, text)
            ));
        }

        let json: serde_json::Value = resp.json()
            .map_err(|e| RenderloopError::OracleCall(e.to_string()))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(content)
    }

    const SYSTEM_PROMPT: &'static str =
        "You are a precise code generator. \
         Return only the requested code, no markdown fences, no explanation.";

    const SYSTEM_PROMPT_JSON: &'static str =
        "You are a precise code analyzer. \
         Return only valid JSON, no markdown fences, no explanation.";
}

impl Oracle for OpenAiOracle {
    fn call(&self, prompt: &str, max_tokens: u32) -> Result<String> {
        self.chat_completions(Self::SYSTEM_PROMPT, prompt, max_tokens)
    }

    fn call_json(&self, prompt: &str, max_tokens: u32) -> Result<serde_json::Value> {
        let text = self.chat_completions(Self::SYSTEM_PROMPT_JSON, prompt, max_tokens)?;
        serde_json::from_str(&text)
            .map_err(|e| RenderloopError::OracleCall(
                format!("failed to parse oracle JSON response: {e}: {text}")
            ))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn cost_per_1k_tokens(&self) -> f64 {
        // gpt-4o-mini pricing as of early 2026 (approximate)
        0.15
    }
}
