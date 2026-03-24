// Stage 7: TEST — All layers → Integration tests (template-driven)
use tera::Context;
use crate::{GenContext, OrchestratorError, Result};
use super::{Stage, StageResult, write_file};
use isls_blueprint::BlueprintRegistry;

pub fn run(ctx: &mut GenContext, _blueprints: &BlueprintRegistry) -> Result<StageResult> {
    let tera = crate::templates::build_engine()
        .map_err(|e| OrchestratorError::Template(e.to_string()))?;

    let backend_dir = ctx.output_dir.join("backend");
    let mut components_generated = 0;

    for module in &ctx.spec.modules {
        let primary_entity = module.entities.first()
            .cloned()
            .unwrap_or_else(|| capitalize(&module.name));

        let mut tctx = Context::new();
        tctx.insert("module", &module.name);
        tctx.insert("entities", &module.entities);
        tctx.insert("primary_entity", &primary_entity);
        tctx.insert("app_name", &ctx.spec.name);

        let content = tera.render("integration_test_rs", &tctx)
            .map_err(|e| OrchestratorError::Template(e.to_string()))?;

        let path = backend_dir.join(format!("tests/{}_tests.rs", module.name));
        let bytes = write_file(&path, &content)?;
        ctx.evidence.record("test", path.clone(), &bytes);
        ctx.files_written.push(path);
        components_generated += 1;
    }

    let mut result = StageResult::ok(Stage::Test, components_generated, 0, components_generated);
    result.notes = vec![
        format!("test files: {}", components_generated),
        "coverage: integration tests for all API endpoints".to_string(),
    ];
    Ok(result)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}
