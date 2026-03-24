// Stage 1: PLAN — AppSpec → Architecture (deterministic, no Oracle)
use crate::{GenContext, OrchestratorError, Result};
use super::{Stage, StageResult};
use isls_blueprint::BlueprintRegistry;
use isls_planner::plan;

pub fn run(ctx: &mut GenContext, blueprints: &BlueprintRegistry) -> Result<StageResult> {
    let arch = plan(&ctx.spec, blueprints)
        .map_err(|e| OrchestratorError::StageFailed {
            stage: Stage::Plan,
            reason: e.to_string(),
        })?;

    let component_count: usize = arch.layers.iter().map(|l| l.components.len()).sum();
    let blueprint_hits: usize = arch.layers.iter()
        .flat_map(|l| l.components.iter())
        .filter(|c| c.blueprint_id.is_some())
        .count();

    let notes = vec![
        format!("layers: {}", arch.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", ")),
        format!("components: {}", component_count),
        format!("blueprint hits: {}", blueprint_hits),
        format!("estimated files: {}", arch.estimated_files),
        format!("estimated LOC: {}", arch.estimated_loc),
        format!("generation steps: {}", arch.generation_order.len()),
    ];

    ctx.blueprint_hits += blueprint_hits;
    ctx.architecture = Some(arch);

    let mut result = StageResult::ok(Stage::Plan, component_count, 0, blueprint_hits);
    result.notes = notes;
    Ok(result)
}
