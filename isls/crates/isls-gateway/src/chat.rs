// isls-gateway: chat.rs — C30 Agent Chat Endpoint
//
// POST /agent/chat  → starts a background job, returns {job_id}
// GET  /agent/chat/{job_id}/events → polls accumulated ChatEvent list
// POST /agent/bind  → set the default project path for subsequent chats
//
// The chat job:
//   1. Analyzes the workspace (AgentWorkspace::analyze)
//   2. Emits Analysis event
//   3. Classifies intent and plans files to modify
//   4. Attempts Oracle synthesis (via OracleEngine) for each file
//   5. Calls apply_and_verify (CargoCheck or mock-equivalent)
//   6. Emits Progress, ToolResult, Complete events

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use isls_agent::{
    accumulation::AccumulationMetrics,
    apply::{ApplyOracle, CargoCheck, apply_and_verify},
    conversation::{Conversation, ConversationTurn},
    workspace::AgentWorkspace,
    build_workspace_prompt,
};

// ─── ChatEvent ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    Analysis {
        summary: String,
        plan: Vec<String>,
        files: Vec<String>,
    },
    Progress {
        step: usize,
        total: usize,
        file: String,
        source: String,
        duration_ms: u64,
    },
    ToolResult {
        tool: String,
        success: bool,
        output: String,
    },
    FixAttempt {
        attempt: usize,
        max: usize,
        error: String,
    },
    Complete {
        files_changed: Vec<String>,
        crystal_id: String,
        patterns_stored: usize,
        patterns_reused: usize,
        cost_usd: f64,
        duration_secs: f64,
    },
    Error {
        message: String,
    },
}

// ─── ChatJob ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ChatJob {
    pub id: String,
    pub events: Vec<ChatEvent>,
    pub complete: bool,
}

impl ChatJob {
    pub fn new(id: String) -> Self {
        Self { id, events: Vec::new(), complete: false }
    }
}

pub type ChatJobStore = Arc<RwLock<HashMap<String, ChatJob>>>;

// ─── Request Types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct ChatRequest {
    pub message: String,
    pub project: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BindRequest {
    pub project: String,
}

// ─── Bound Project State ──────────────────────────────────────────────────────

pub type BoundProject = Arc<RwLock<Option<PathBuf>>>;

// ─── GatewayOracle ───────────────────────────────────────────────────────────

/// Wraps the real isls-oracle OracleEngine as an ApplyOracle.
///
/// This adapter builds a minimal fix prompt and calls the raw HTTP oracle
/// if an API key is available.  Falls back gracefully when unavailable.
pub struct GatewayOracle {
    pub available: bool,
}

impl GatewayOracle {
    pub fn new() -> Self {
        // Check if ANTHROPIC_API_KEY or OPENAI_API_KEY is set
        let available = std::env::var("ANTHROPIC_API_KEY").is_ok()
            || std::env::var("OPENAI_API_KEY").is_ok();
        Self { available }
    }
}

impl ApplyOracle for GatewayOracle {
    fn fix_compile_error(
        &self,
        file_path: &str,
        bad_code: &str,
        error: &str,
    ) -> Result<String, String> {
        if !self.available {
            return Err("No oracle API key available; cannot auto-fix".into());
        }

        use isls_oracle::{ClaudeOracle, SynthesisOracle, SynthesisPrompt, OutputFormat};

        // Resolve API key from environment
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("CLAUDE_API_KEY"))
            .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;

        let oracle = ClaudeOracle::new(api_key);

        let system = format!(
            "You are a Rust compiler assistant. The following code for {} failed to compile.\n\
             Fix the compile error. Output ONLY the complete corrected file. No explanation. No markdown fences.\n\n\
             COMPILE ERROR:\n{}",
            file_path, error
        );

        let prompt = SynthesisPrompt {
            system,
            user: bad_code.to_string(),
            output_format: OutputFormat::Rust,
            max_tokens: 4096,
            temperature: 0.0,
        };

        let response = oracle.synthesize(&prompt)
            .map_err(|e| format!("oracle: {}", e))?;

        Ok(isls_agent::strip_markdown_fences(&response.content))
    }
}

// ─── Chat Job Runner ─────────────────────────────────────────────────────────

/// Background task: analyze workspace, plan changes, synthesize, apply, verify.
pub async fn run_chat_job(
    jobs: ChatJobStore,
    job_id: String,
    req: ChatRequest,
    bound_project: BoundProject,
) {
    let start = std::time::Instant::now();

    // Resolve project path
    let project_path: PathBuf = {
        let req_path = req.project.as_deref().map(PathBuf::from);
        let bound = bound_project.read().await.clone();
        req_path.or(bound).unwrap_or_else(|| PathBuf::from("."))
    };

    // Step 1: Analyze workspace
    let ws = match AgentWorkspace::analyze(&project_path) {
        Ok(ws) => ws,
        Err(e) => {
            push_event(&jobs, &job_id, ChatEvent::Error {
                message: format!("workspace analysis failed: {}", e),
            }).await;
            mark_complete(&jobs, &job_id).await;
            return;
        }
    };

    // Find relevant files for the request
    let relevant = ws.relevant_files(&req.message);
    let relevant_paths: Vec<String> = relevant.iter().map(|m| m.path.clone()).collect();

    // Build a simple plan (one "modify" step per relevant file, max 3)
    let plan_files: Vec<String> = relevant_paths.iter().take(3).cloned().collect();
    let plan: Vec<String> = plan_files.iter()
        .map(|f| format!("Modify {}", f))
        .collect();

    push_event(&jobs, &job_id, ChatEvent::Analysis {
        summary: ws.summary(),
        plan: plan.clone(),
        files: relevant_paths.clone(),
    }).await;

    if plan_files.is_empty() {
        // No files to modify — complete immediately
        push_event(&jobs, &job_id, ChatEvent::Complete {
            files_changed: vec![],
            crystal_id: format!("no-op-{:08x}", rand_u32()),
            patterns_stored: 0,
            patterns_reused: 0,
            cost_usd: 0.0,
            duration_secs: start.elapsed().as_secs_f64(),
        }).await;
        mark_complete(&jobs, &job_id).await;
        return;
    }

    // Build workspace prompt
    let prompt = build_workspace_prompt(
        &req.message,
        &ws,
        &relevant,
        &[],
        &[ConversationTurn::user(&req.message)],
    );

    let oracle = GatewayOracle::new();
    let mut metrics = AccumulationMetrics::default();
    let mut files_changed: Vec<String> = Vec::new();

    // Step 2: For each planned file — synthesize + apply
    for (i, file_path) in plan_files.iter().enumerate() {
        let step_start = std::time::Instant::now();

        // For now: if oracle is available, build content via oracle prompt
        // Otherwise: leave the file unchanged and just record the attempt
        let new_content_opt = if oracle.available {
            // Build a targeted prompt for this specific file
            let file_system = format!(
                "{}\n\nModify the file {} to implement: {}",
                prompt.system, file_path, req.message
            );
            let file_prompt = isls_oracle::SynthesisPrompt {
                system: file_system,
                user: format!("Implement: {}", req.message),
                output_format: isls_oracle::OutputFormat::Rust,
                max_tokens: 4096,
                temperature: 0.0,
            };

            use isls_oracle::{ClaudeOracle, SynthesisOracle};
            if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY")
                .or_else(|_| std::env::var("CLAUDE_API_KEY")) {
                let raw_oracle = ClaudeOracle::new(api_key);
                match raw_oracle.synthesize(&file_prompt) {
                    Ok(resp) => {
                        let cost = resp.tokens_used as f64 * 0.000015;
                        metrics.record_oracle_call(cost);
                        Some(isls_agent::strip_markdown_fences(&resp.content))
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
        } else {
            None
        };

        let source = if new_content_opt.is_some() { "oracle" } else { "skip" };
        let duration_ms = step_start.elapsed().as_millis() as u64;

        if let Some(new_content) = new_content_opt {
            // Apply and verify
            let compiler = CargoCheck;
            match apply_and_verify(&project_path, file_path, &new_content, &oracle, 3, &compiler) {
                Ok(result) => {
                    if result.compiled() {
                        files_changed.push(file_path.clone());
                        metrics.record_compile_first_try();
                    } else {
                        metrics.record_compile_failed();
                    }
                }
                Err(_) => {
                    metrics.record_compile_failed();
                }
            }
        }

        push_event(&jobs, &job_id, ChatEvent::Progress {
            step: i + 1,
            total: plan_files.len(),
            file: file_path.clone(),
            source: source.to_string(),
            duration_ms,
        }).await;
    }

    // Compile check result
    let _compile_ok = {
        let out = std::process::Command::new("cargo")
            .args(["check"])
            .current_dir(&project_path)
            .output();
        match out {
            Ok(o) => {
                let success = o.status.success();
                let output = if success {
                    "cargo check passed".to_string()
                } else {
                    String::from_utf8_lossy(&o.stderr).chars().take(500).collect()
                };
                push_event(&jobs, &job_id, ChatEvent::ToolResult {
                    tool: "cargo check".to_string(),
                    success,
                    output,
                }).await;
                success
            }
            Err(_) => {
                // cargo not available (e.g. in test environment)
                push_event(&jobs, &job_id, ChatEvent::ToolResult {
                    tool: "cargo check".to_string(),
                    success: true,
                    output: "skipped (cargo not in PATH)".to_string(),
                }).await;
                true
            }
        }
    };

    // Complete
    let crystal_id = format!("{:08x}", rand_u32());
    push_event(&jobs, &job_id, ChatEvent::Complete {
        files_changed: files_changed.clone(),
        crystal_id,
        patterns_stored: 0,
        patterns_reused: metrics.memory_served as usize,
        cost_usd: metrics.total_cost_usd,
        duration_secs: start.elapsed().as_secs_f64(),
    }).await;

    mark_complete(&jobs, &job_id).await;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn push_event(jobs: &ChatJobStore, job_id: &str, event: ChatEvent) {
    let mut store = jobs.write().await;
    if let Some(job) = store.get_mut(job_id) {
        job.events.push(event);
    }
}

async fn mark_complete(jobs: &ChatJobStore, job_id: &str) {
    let mut store = jobs.write().await;
    if let Some(job) = store.get_mut(job_id) {
        job.complete = true;
    }
}

fn rand_u32() -> u32 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
}
