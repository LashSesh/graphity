// Stage 10: LEARN — Results → crystallise patterns, update blueprint registry
use crate::{GenContext, Result};
use super::{Stage, StageResult};
use isls_blueprint::{Blueprint, BlueprintRegistry};
use isls_learner::PatternLibrary;
use isls_reader::parse_directory;

pub fn run(
    ctx: &mut GenContext,
    blueprints: &mut BlueprintRegistry,
    learner: &mut PatternLibrary,
) -> Result<StageResult> {
    let mut notes = Vec::new();
    let initial_count = blueprints.len();

    // Parse all generated backend code for pattern learning
    let backend_src = ctx.output_dir.join("backend/src");
    if backend_src.exists() {
        if let Ok(analysis) = parse_directory(&backend_src) {
            // Add patterns to learner
            learner.add_pattern(&ctx.spec.name, &analysis.files);

            // Crystallise new blueprints from generated services
            let lang = ctx.spec.backend.language.as_str().to_string();
            let fw = ctx.spec.backend.framework.as_str().to_string();

            for module in &ctx.spec.modules {
                // Service blueprint
                let svc_bp = Blueprint::new(
                    format!("{}-{}-service", ctx.spec.name, module.name),
                    "crud_service",
                    lang.clone(),
                    fw.clone(),
                    format!("// CRUD service for {}", module.name),
                    0.9, // first-attempt = high confidence
                );
                blueprints.add(svc_bp);

                // API blueprint
                let api_bp = Blueprint::new(
                    format!("{}-{}-api", ctx.spec.name, module.name),
                    "rest_endpoint",
                    lang.clone(),
                    fw.clone(),
                    format!("// REST endpoints for {}", module.name),
                    0.9,
                );
                blueprints.add(api_bp);
            }
        }
    }

    // Update confidence scores based on usage
    blueprints.update_confidences();

    // Save crystal registry to evidence/
    let evidence_dir = ctx.output_dir.join("evidence");
    let _ = std::fs::create_dir_all(&evidence_dir);

    let registry_path = evidence_dir.join("crystal_registry.json");
    if let Ok(json) = serde_json::to_string_pretty(&serde_json::json!({
        "total_blueprints": blueprints.len(),
        "new_blueprints": blueprints.len().saturating_sub(initial_count),
        "learner_stats": learner.stats(),
    })) {
        let _ = std::fs::write(&registry_path, json);
        ctx.files_written.push(registry_path);
    }

    let new_blueprints = blueprints.len().saturating_sub(initial_count);
    notes.push(format!("blueprints: {} total, {} new", blueprints.len(), new_blueprints));
    notes.push(format!("learner patterns: {}", learner.len()));

    let mut result = StageResult::ok(Stage::Learn, new_blueprints, 0, 0);
    result.notes = notes;
    Ok(result)
}
