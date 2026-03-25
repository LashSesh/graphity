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

/// Strip markdown code fences from an LLM response.
///
/// Handles both ` ``` ` and `~~~` fence delimiters, with any language tag on
/// the opening line (e.g. ` ```rust `, ` ```toml `, `~~~javascript `).
/// Searches for the **first** closing fence from the end, so any trailing
/// explanation the LLM appends after the block is also discarded.
/// If no complete fence pair is found the input is returned unchanged.
fn strip_markdown_fences(response: &str) -> String {
    let trimmed = response.trim();

    let delimiter = if trimmed.starts_with("```") {
        "```"
    } else if trimmed.starts_with("~~~") {
        "~~~"
    } else {
        return response.to_string();
    };

    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() < 2 {
        return response.to_string();
    }

    // Find the first closing fence scanning from the end
    let mut end = lines.len(); // sentinel: "not found"
    for i in (1..lines.len()).rev() {
        if lines[i].trim() == delimiter {
            end = i;
            break;
        }
    }

    if end == lines.len() {
        // No closing fence — leave unchanged
        return response.to_string();
    }

    lines[1..end].join("\n")
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

        eprintln!("[Oracle] Calling {} ({} chars prompt, max_tokens={})...",
            self.model, user.len(), max_tokens);

        let resp = self.client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|e| RenderloopError::OracleCall(e.to_string()))?;

        let status = resp.status();
        let text = resp.text()
            .map_err(|e| RenderloopError::OracleCall(
                format!("Failed to read response: {}", e)
            ))?;

        eprintln!("[Oracle] Response status: {}, {} bytes", status, text.len());

        if !status.is_success() {
            return Err(RenderloopError::OracleCall(
                format!("OpenAI API error {}: {}", status, text)
            ));
        }

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| RenderloopError::OracleCall(
                format!("Failed to parse response: {e}: {text}")
            ))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| RenderloopError::OracleCall(
                format!("No content in response: {}", text)
            ))?
            .to_string();

        Ok(strip_markdown_fences(&content))
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
        // chat_completions already strips fences; strip again defensively and trim
        let text = self.chat_completions(Self::SYSTEM_PROMPT_JSON, prompt, max_tokens)?;
        serde_json::from_str(text.trim())
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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::strip_markdown_fences;

    #[test]
    fn strips_rust_fence() {
        let input = "```rust\nfn main() {}\n```";
        assert_eq!(strip_markdown_fences(input), "fn main() {}");
    }

    #[test]
    fn strips_plain_fence() {
        let input = "```\nsome content\n```";
        assert_eq!(strip_markdown_fences(input), "some content");
    }

    #[test]
    fn strips_toml_fence() {
        let input = "```toml\n[package]\nname = \"x\"\n```";
        assert_eq!(strip_markdown_fences(input), "[package]\nname = \"x\"");
    }

    #[test]
    fn strips_json_fence() {
        let input = "```json\n{\"k\":1}\n```";
        assert_eq!(strip_markdown_fences(input), "{\"k\":1}");
    }

    #[test]
    fn no_fence_unchanged() {
        let input = "fn foo() -> u32 { 42 }";
        assert_eq!(strip_markdown_fences(input), input);
    }

    #[test]
    fn leading_trailing_whitespace_ignored() {
        let input = "\n  ```rust\nlet x = 1;\n```  \n";
        assert_eq!(strip_markdown_fences(input), "let x = 1;");
    }

    #[test]
    fn multiline_body_preserved() {
        let input = "```rust\nuse std::env;\n\nfn main() {\n    println!(\"hi\");\n}\n```";
        let expected = "use std::env;\n\nfn main() {\n    println!(\"hi\");\n}";
        assert_eq!(strip_markdown_fences(input), expected);
    }

    #[test]
    fn unclosed_fence_unchanged() {
        // No closing ``` — must not strip anything
        let input = "```rust\nfn main() {}";
        assert_eq!(strip_markdown_fences(input), input);
    }

    #[test]
    fn strips_tilde_fence() {
        let input = "~~~rust\nfn foo() {}\n~~~";
        assert_eq!(strip_markdown_fences(input), "fn foo() {}");
    }

    #[test]
    fn unclosed_tilde_fence_unchanged() {
        let input = "~~~rust\nfn foo() {}";
        assert_eq!(strip_markdown_fences(input), input);
    }

    #[test]
    fn trailing_explanation_discarded() {
        // LLM sometimes appends prose after the closing fence
        let input = "```rust\nfn foo() {}\n```\nThis implements foo.";
        // The closing fence is found on the first scan from the end, so
        // everything from the fence onward is dropped.
        assert_eq!(strip_markdown_fences(input), "fn foo() {}");
    }
}
