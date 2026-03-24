// Stage 0: DESCRIBE — TOML → AppSpec (deterministic, no Oracle)
use crate::{GenContext, OrchestratorError, Result};
use super::{Stage, StageResult};

pub fn run(ctx: &mut GenContext) -> Result<StageResult> {
    // AppSpec is already parsed before run(); just validate
    if ctx.spec.name.is_empty() {
        return Err(OrchestratorError::StageFailed {
            stage: Stage::Describe,
            reason: "AppSpec name is empty".to_string(),
        });
    }
    if ctx.spec.modules.is_empty() {
        return Err(OrchestratorError::StageFailed {
            stage: Stage::Describe,
            reason: "AppSpec has no modules".to_string(),
        });
    }

    let notes = vec![
        format!("app: {}", ctx.spec.name),
        format!("modules: {}", ctx.spec.modules.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")),
        format!("backend: {} / {}", ctx.spec.backend.language, ctx.spec.backend.framework),
        format!("frontend: {} / {}", ctx.spec.frontend.framework, ctx.spec.frontend.app_type),
    ];

    let mut result = StageResult::ok(Stage::Describe, 1, 0, 0);
    result.notes = notes;
    Ok(result)
}
