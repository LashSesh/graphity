// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Oracle abstraction for ISLS code generation.
//!
//! The `Oracle` trait decouples the forge pipeline from any specific LLM backend.
//! `MockOracle` is used for offline / CI runs. `OpenAiOracle` calls the OpenAI
//! chat completions API with temperature 0.2 for deterministic code generation.

use serde_json::json;

use crate::forge::ForgeLlmError;

/// Oracle result type using [`ForgeLlmError`].
pub type Result<T> = std::result::Result<T, ForgeLlmError>;

/// Approximate token count: 1 token ~ 4 bytes of UTF-8 text.
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
pub fn strip_markdown_fences(response: &str) -> String {
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

/// Abstraction over an LLM backend used during code generation.
pub trait Oracle: Send + Sync {
    /// Send a prompt and return the model's text response.
    fn call(&self, prompt: &str, max_tokens: u32) -> Result<String>;

    /// Send a prompt that must return a JSON value.
    fn call_json(&self, prompt: &str, max_tokens: u32) -> Result<serde_json::Value>;

    /// Identifier of the underlying model, e.g. `"gpt-4o"`.
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
/// Useful for verifying the forge pipeline machinery without an API key.
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
                .map_err(|_| ForgeLlmError::Oracle(
                    "OPENAI_API_KEY not set and no --api-key provided".into()
                ))?,
        };
        let model = model.unwrap_or_else(|| "gpt-4o".to_string());
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ForgeLlmError::Oracle(e.to_string()))?;
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
            .map_err(|e| ForgeLlmError::Oracle(e.to_string()))?;

        let status = resp.status();
        let text = resp.text()
            .map_err(|e| ForgeLlmError::Oracle(
                format!("Failed to read response: {}", e)
            ))?;

        eprintln!("[Oracle] Response status: {}, {} bytes", status, text.len());

        if !status.is_success() {
            return Err(ForgeLlmError::Oracle(
                format!("OpenAI API error {}: {}", status, text)
            ));
        }

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| ForgeLlmError::Oracle(
                format!("Failed to parse response: {e}: {text}")
            ))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| ForgeLlmError::Oracle(
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
        let text = self.chat_completions(Self::SYSTEM_PROMPT_JSON, prompt, max_tokens)?;
        serde_json::from_str(text.trim())
            .map_err(|e| ForgeLlmError::Oracle(
                format!("failed to parse oracle JSON response: {e}: {text}")
            ))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn cost_per_1k_tokens(&self) -> f64 {
        0.15
    }
}

// ─── OllamaOracle ────────────────────────────────────────────────────────────

/// Oracle implementation backed by a locally running Ollama instance.
///
/// Calls the Ollama HTTP API (`/api/generate`) with `stream: false`.
/// Zero API cost — runs entirely on the local machine. Default model is
/// `codellama:7b` with temperature 0.1 for deterministic code generation.
///
/// Requires Ollama to be running (`ollama serve`) and the model to be pulled
/// (`ollama pull codellama:7b`).
pub struct OllamaOracle {
    model: String,
    base_url: String,
    client: reqwest::blocking::Client,
}

impl OllamaOracle {
    /// Create a new Ollama oracle.
    ///
    /// - `model`: Ollama model name, e.g. `"codellama:7b"`.
    /// - `base_url`: Ollama API base URL, e.g. `"http://localhost:11434"`.
    pub fn new(model: &str, base_url: &str) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("failed to build HTTP client for Ollama");
        Self {
            model: model.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        }
    }

    /// Check whether Ollama is running and reachable.
    ///
    /// Calls `GET {base_url}/api/tags` — if the request fails, Ollama is not
    /// running. Returns `Ok(())` on success or an error message on failure.
    pub fn check_availability(base_url: &str) -> std::result::Result<(), String> {
        let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("HTTP client error: {e}"))?;
        client.get(&url).send()
            .map_err(|_| format!(
                "Ollama is not running. Start it with `ollama serve` \
                 or install from https://ollama.com"
            ))?;
        Ok(())
    }

    /// Check whether a specific model is available in the local Ollama instance.
    ///
    /// Calls `GET {base_url}/api/tags` and checks if the model name appears
    /// in the response. Returns `Ok(())` if found, or an error message.
    pub fn check_model(base_url: &str, model: &str) -> std::result::Result<(), String> {
        let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("HTTP client error: {e}"))?;
        let resp = client.get(&url).send()
            .map_err(|_| "Ollama is not running".to_string())?;
        let json: serde_json::Value = resp.json()
            .map_err(|e| format!("Failed to parse Ollama response: {e}"))?;
        let models = json["models"].as_array()
            .ok_or_else(|| "Unexpected Ollama response format".to_string())?;
        let found = models.iter().any(|m| {
            m["name"].as_str().map_or(false, |n| n == model || n.starts_with(&format!("{model}:")))
                || m["model"].as_str().map_or(false, |n| n == model || n.starts_with(&format!("{model}:")))
        });
        if found {
            Ok(())
        } else {
            Err(format!(
                "Model '{model}' not found. Pull it with `ollama pull {model}`"
            ))
        }
    }
}

impl Oracle for OllamaOracle {
    fn call(&self, prompt: &str, max_tokens: u32) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url);
        let body = json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": 0.1,
                "num_predict": max_tokens
            }
        });

        eprintln!("[Ollama] Calling {} ({} chars prompt, max_tokens={})...",
            self.model, prompt.len(), max_tokens);

        let resp = self.client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| ForgeLlmError::Oracle(format!("Ollama request failed: {e}")))?;

        let status = resp.status();
        let text = resp.text()
            .map_err(|e| ForgeLlmError::Oracle(format!("Failed to read Ollama response: {e}")))?;

        eprintln!("[Ollama] Response status: {}, {} bytes", status, text.len());

        if !status.is_success() {
            return Err(ForgeLlmError::Oracle(
                format!("Ollama API error {}: {}", status, text)
            ));
        }

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| ForgeLlmError::Oracle(
                format!("Failed to parse Ollama response: {e}: {text}")
            ))?;

        let response = json["response"].as_str()
            .ok_or_else(|| ForgeLlmError::Oracle(
                format!("No 'response' field in Ollama response: {text}")
            ))?
            .to_string();

        Ok(strip_markdown_fences(&response))
    }

    fn call_json(&self, prompt: &str, max_tokens: u32) -> Result<serde_json::Value> {
        let text = self.call(prompt, max_tokens)?;
        serde_json::from_str(text.trim())
            .map_err(|e| ForgeLlmError::Oracle(
                format!("failed to parse Ollama JSON response: {e}: {text}")
            ))
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn cost_per_1k_tokens(&self) -> f64 {
        0.0
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
        let input = "```rust\nfn foo() {}\n```\nThis implements foo.";
        assert_eq!(strip_markdown_fences(input), "fn foo() {}");
    }

    #[test]
    fn ollama_oracle_creation() {
        use super::Oracle;
        let oracle = super::OllamaOracle::new("codellama:7b", "http://localhost:11434");
        assert_eq!(oracle.model, "codellama:7b");
        assert_eq!(oracle.base_url, "http://localhost:11434");
        assert_eq!(oracle.model_name(), "codellama:7b");
        assert_eq!(oracle.cost_per_1k_tokens(), 0.0);
    }

    #[test]
    fn ollama_oracle_trims_trailing_slash() {
        let oracle = super::OllamaOracle::new("mistral:7b", "http://localhost:11434/");
        assert_eq!(oracle.base_url, "http://localhost:11434");
    }
}
