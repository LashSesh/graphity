// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Recursive hypercube decomposition engine for ISLS v2.
//!
//! Collapses a high-dimensional requirement hypercube into concrete code
//! artifacts using spectral graph methods: Fiedler bisection, Kuramoto
//! grouping, and singularity detection.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use isls_hypercube::{
    DimState, DimValue, HyperCube,
    DomainRegistry, EntityTemplate,
    graph::CouplingGraph,
};
use isls_blueprint::BlueprintRegistry;
use isls_orchestrator::v2_emission::{self, EmittedFile};
use isls_renderloop::{Artifact, CrystalRegistry, MockOracle, OpenAiOracle, Oracle, RenderLoop, RenderPass};

// ─── Error ───────────────────────────────────────────────────────────────────

/// Errors from the decomposition engine.
#[derive(Debug, Error)]
pub enum DecomposerError {
    /// Hypercube construction or parsing failed.
    #[error("hypercube error: {0}")]
    Hypercube(#[from] isls_hypercube::HypercubeError),
    /// Orchestrator emission failed.
    #[error("emission error: {0}")]
    Emission(#[from] isls_orchestrator::OrchestratorError),
    /// Blueprint error.
    #[error("blueprint error: {0}")]
    Blueprint(#[from] isls_blueprint::BlueprintError),
    /// IO error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Generic decomposition failure.
    #[error("decomposition failed: {0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, DecomposerError>;

// ─── Collapse Step ───────────────────────────────────────────────────────────

/// A single step in the collapse plan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CollapseStep {
    /// Fix a singularity (architectural decision).
    Singularity {
        dimension: String,
        impact: f64,
    },
    /// Emit a group of tightly-coupled dimensions together.
    Group {
        dimensions: Vec<String>,
        avg_coupling: f64,
        method: String,
    },
    /// Bisect the graph using Fiedler vector.
    Bisect {
        left: Vec<String>,
        right: Vec<String>,
        fiedler_value: f64,
    },
    /// Emit a single leaf dimension.
    Leaf {
        dimension: String,
        method: String,
    },
}

/// A collapse plan: ordered sequence of steps to reduce the hypercube DOF to zero.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollapsePlan {
    pub steps: Vec<CollapseStep>,
}

// ─── Trace ───────────────────────────────────────────────────────────────────

/// A single trace entry recording a decomposition action.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceEntry {
    /// Recursion depth.
    pub depth: u32,
    /// Description of the action.
    pub action: String,
    /// Number of dimensions involved.
    pub dim_count: usize,
    /// Method used (singularity, fiedler, kuramoto, leaf, blueprint).
    pub method: String,
    /// Additional details.
    pub details: String,
}

// ─── Decomposition Context ───────────────────────────────────────────────────

/// Mutable context threaded through the recursive decomposition.
pub struct DecompositionContext {
    /// Blueprint registry for pattern matching and crystallisation.
    pub blueprints: BlueprintRegistry,
    /// Trace log of decomposition steps.
    pub trace: Vec<TraceEntry>,
    /// All artifacts emitted so far.
    pub files: Vec<EmittedFile>,
    /// Output directory.
    pub output_dir: PathBuf,
    /// Application name.
    pub app_name: String,
    /// Domain template being used.
    pub domain: isls_hypercube::domain::DomainTemplate,
    /// Entities extracted from the domain.
    pub entities: Vec<EntityTemplate>,
    /// Blueprint hits counter.
    pub blueprint_hits: usize,
    /// Template hits counter.
    pub template_hits: usize,
}

// ─── Collapse Planner ────────────────────────────────────────────────────────

/// Build a collapse plan from the coupling graph.
///
/// Strategy (in priority order):
/// 1. Singularities first (gap_threshold=0.01)
/// 2. Fiedler bisection if λ₂ < 0.5 and nodes > 3
/// 3. Kuramoto groups (κ=2.0, threshold=0.8)
/// 4. Remaining dimensions as leaves
pub fn plan_collapse(graph: &CouplingGraph, _blueprints: &BlueprintRegistry) -> CollapsePlan {
    let mut steps = Vec::new();

    if graph.nodes.is_empty() {
        return CollapsePlan { steps };
    }

    // 1. Singularities
    let singularities = graph.singularities(0.01);
    if !singularities.is_empty() {
        // Take the highest-impact singularity
        let s = &singularities[0];
        steps.push(CollapseStep::Singularity {
            dimension: s.dimension.clone(),
            impact: s.impact,
        });
        // After fixing a singularity, the graph changes fundamentally.
        // Return early — decompose() will rebuild and re-plan.
        return CollapsePlan { steps };
    }

    // 2. Fiedler bisection
    let fiedler = graph.fiedler();
    if graph.nodes.len() > 3 && fiedler.value < 0.5 {
        let (left, right) = graph.fiedler_bisect();
        if left.len() > 1 && right.len() > 1 {
            steps.push(CollapseStep::Bisect {
                left,
                right,
                fiedler_value: fiedler.value,
            });
            return CollapsePlan { steps };
        }
    }

    // 3. Kuramoto groups
    let groups = graph.kuramoto_groups(2.0, 0.8);
    let mut grouped_dims: std::collections::HashSet<String> = std::collections::HashSet::new();

    for group in &groups {
        if group.dimensions.len() > 1 {
            for d in &group.dimensions {
                grouped_dims.insert(d.clone());
            }
            steps.push(CollapseStep::Group {
                dimensions: group.dimensions.clone(),
                avg_coupling: group.avg_coupling,
                method: "kuramoto".into(),
            });
        }
    }

    // 4. Remaining as leaves
    for node in &graph.nodes {
        if !grouped_dims.contains(node) {
            steps.push(CollapseStep::Leaf {
                dimension: node.clone(),
                method: "individual".into(),
            });
        }
    }

    CollapsePlan { steps }
}

// ─── Main Decomposition ─────────────────────────────────────────────────────

/// Recursively decompose a hypercube into code artifacts.
pub fn decompose(cube: &mut HyperCube, ctx: &mut DecompositionContext) -> Result<()> {
    // Base case: fully constrained
    if cube.dof() == 0 {
        ctx.trace.push(TraceEntry {
            depth: cube.depth,
            action: "fully_constrained".into(),
            dim_count: 0,
            method: "base_case".into(),
            details: "All dimensions fixed".into(),
        });
        return Ok(());
    }

    // Base case: single free dimension → emit as leaf
    if cube.dof() == 1 {
        let free = cube.free_dimensions();
        ctx.trace.push(TraceEntry {
            depth: cube.depth,
            action: format!("leaf: {}", free[0]),
            dim_count: 1,
            method: "leaf".into(),
            details: "Single free dimension".into(),
        });
        cube.fix(&free[0], DimValue::Choice("generated".into()));
        return Ok(());
    }

    // Build coupling graph from free dimensions only
    let free_dims = cube.free_dimensions();
    let sub = cube.extract_subcube(&free_dims);
    let graph = sub.coupling_graph();

    if graph.nodes.is_empty() {
        return Ok(());
    }

    // Plan the collapse
    let plan = plan_collapse(&graph, &ctx.blueprints);

    ctx.trace.push(TraceEntry {
        depth: cube.depth,
        action: format!("plan: {} steps for {} free dims", plan.steps.len(), cube.dof()),
        dim_count: cube.dof(),
        method: "plan_collapse".into(),
        details: plan.steps.iter().map(|s| match s {
            CollapseStep::Singularity { dimension, impact } =>
                format!("singularity({}, {:.0}%)", dimension, impact * 100.0),
            CollapseStep::Bisect { fiedler_value, .. } =>
                format!("fiedler(λ₂={:.3})", fiedler_value),
            CollapseStep::Group { dimensions, method, .. } =>
                format!("{}({}dims)", method, dimensions.len()),
            CollapseStep::Leaf { dimension, .. } =>
                format!("leaf({})", dimension),
        }).collect::<Vec<_>>().join(", "),
    });

    for step in &plan.steps {
        match step {
            CollapseStep::Singularity { dimension, impact } => {
                ctx.trace.push(TraceEntry {
                    depth: cube.depth,
                    action: format!("fix_singularity: {}", dimension),
                    dim_count: 1,
                    method: "singularity".into(),
                    details: format!("impact={:.0}%, graph will be rebuilt", impact * 100.0),
                });

                // Fix the singularity dimension
                let value = resolve_singularity(dimension, cube);
                cube.fix(dimension, value);

                // Restart decomposition (graph changed fundamentally)
                return decompose(cube, ctx);
            }

            CollapseStep::Bisect { left, right, fiedler_value } => {
                ctx.trace.push(TraceEntry {
                    depth: cube.depth,
                    action: format!("fiedler_bisect: left={}, right={}", left.len(), right.len()),
                    dim_count: left.len() + right.len(),
                    method: "fiedler".into(),
                    details: format!("λ₂={:.4}", fiedler_value),
                });

                // Decompose left subtree
                let mut left_cube = cube.extract_subcube(left);
                decompose(&mut left_cube, ctx)?;

                // Decompose right subtree
                let mut right_cube = cube.extract_subcube(right);
                decompose(&mut right_cube, ctx)?;

                // Mark all dims as fixed in parent
                for dim_name in left.iter().chain(right.iter()) {
                    cube.fix(dim_name, DimValue::Choice("generated".into()));
                }
            }

            CollapseStep::Group { dimensions, avg_coupling, method } => {
                ctx.trace.push(TraceEntry {
                    depth: cube.depth,
                    action: format!("emit_group: {} dims via {}", dimensions.len(), method),
                    dim_count: dimensions.len(),
                    method: method.clone(),
                    details: format!("avg_coupling={:.2}", avg_coupling),
                });

                // Mark group dims as fixed
                for dim_name in dimensions {
                    cube.fix(dim_name, DimValue::Choice("generated".into()));
                }
                ctx.template_hits += dimensions.len();
            }

            CollapseStep::Leaf { dimension, method } => {
                ctx.trace.push(TraceEntry {
                    depth: cube.depth,
                    action: format!("emit_leaf: {}", dimension),
                    dim_count: 1,
                    method: method.clone(),
                    details: String::new(),
                });

                cube.fix(dimension, DimValue::Choice("generated".into()));
                ctx.template_hits += 1;
            }
        }
    }

    Ok(())
}

/// Resolve a singularity dimension to its concrete value.
fn resolve_singularity(dimension: &str, cube: &HyperCube) -> DimValue {
    // Look up the dimension's options/default
    if let Some(dim) = cube.dimensions.iter().find(|d| d.name == dimension) {
        match &dim.state {
            DimState::Fixed(v) => v.clone(),
            DimState::Free { default: Some(v), .. } => v.clone(),
            DimState::Free { options, .. } if !options.is_empty() => options[0].clone(),
            _ => DimValue::Choice("resolved".into()),
        }
    } else {
        DimValue::Choice("resolved".into())
    }
}

// ─── Full Pipeline ───────────────────────────────────────────────────────────

/// Configuration for the v2 decomposition pipeline.
#[derive(Clone, Debug)]
pub struct DecomposerConfig {
    /// Whether to emit trace output.
    pub trace: bool,
    /// Whether to use mock oracle (template-only generation, no LLM calls).
    pub mock_oracle: bool,
    /// Blueprint registry path for persistence.
    pub blueprint_path: Option<PathBuf>,
    // ── v2.1 render-loop fields ──────────────────────────────────────────────
    /// OpenAI API key. When `None` (or mock_oracle is true), `MockOracle` is used.
    pub api_key: Option<String>,
    /// Model name for LLM oracle (default: `"gpt-4o-mini"`).
    pub model: String,
    /// Number of render passes to execute (0 = Structure only, 5 = all passes).
    pub passes: u32,
    /// Pass labels to skip (e.g. `["polish"]`).
    pub skip_passes: Vec<String>,
    /// Optional global token-budget override applied to every pass.
    pub token_budget_override: Option<u64>,
    /// Path where the crystal registry is persisted between runs.
    pub crystal_path: Option<PathBuf>,
}

impl Default for DecomposerConfig {
    fn default() -> Self {
        DecomposerConfig {
            trace: false,
            mock_oracle: true,
            blueprint_path: None,
            api_key: None,
            model: "gpt-4o-mini".to_string(),
            passes: 0,
            skip_passes: vec![],
            token_budget_override: None,
            crystal_path: None,
        }
    }
}

/// Full result of a v2 forge run.
#[derive(Clone, Debug)]
pub struct ForgeV2Result {
    /// Application name.
    pub app_name: String,
    /// Total files generated.
    pub total_files: usize,
    /// Total lines of code.
    pub total_loc: usize,
    /// Blueprint hits during decomposition.
    pub blueprint_hits: usize,
    /// Template hits during decomposition.
    pub template_hits: usize,
    /// Decomposition trace entries.
    pub trace: Vec<TraceEntry>,
    /// Time taken in seconds.
    pub time_secs: f64,
    /// Output directory.
    pub output_dir: PathBuf,
    /// Detected domain name.
    pub domain_name: String,
    /// Number of dimensions in the hypercube.
    pub total_dims: usize,
    /// Degrees of freedom before decomposition.
    pub initial_dof: usize,
    /// Number of couplings.
    pub total_couplings: usize,
    // ── v2.1 render-loop stats ───────────────────────────────────────────────
    /// Number of render passes executed (0 when mock_oracle or passes=0).
    pub render_passes_executed: u32,
    /// Total tokens consumed across all render passes.
    pub total_tokens_used: u64,
    /// Number of crystals updated in the registry after this run.
    pub crystals_updated: usize,
}

/// Run the full v2 forge pipeline: TOML → HyperCube → Decompose → Emit.
pub fn forge_v2(
    requirements_path: &Path,
    output_dir: &Path,
    config: &DecomposerConfig,
) -> Result<ForgeV2Result> {
    let start = std::time::Instant::now();

    // 1. Parse TOML into HyperCube
    let mut cube = isls_hypercube::parser::parse_toml_to_cube(requirements_path)?;
    let total_dims = cube.dimensions.len();
    let initial_dof = cube.dof();
    let total_couplings = cube.couplings.len();

    // 2. Detect domain and extract entities
    let registry = DomainRegistry::new();
    let toml_content = std::fs::read_to_string(requirements_path)?;
    let domain = registry.detect(&toml_content)
        .cloned()
        .unwrap_or_else(|| {
            // Fallback minimal domain
            isls_hypercube::domain::DomainTemplate {
                name: "generic".into(),
                keywords: vec![],
                entities: vec![],
                relationships: vec![],
                business_rules: vec![],
                api_features: isls_hypercube::domain::ApiFeatures {
                    pagination: true,
                    filtering: vec![],
                    sorting: vec!["created_at".into()],
                    search_fields: vec!["name".into()],
                    export_formats: vec!["json".into()],
                },
            }
        });

    let entities = domain.entities.clone();
    let domain_name = domain.name.clone();

    // Extract app name from cube
    let app_name = cube.dimensions.iter()
        .find(|d| d.name == "arch.app_name")
        .and_then(|d| match &d.state {
            DimState::Fixed(DimValue::Choice(s)) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "generated-app".into());

    // 3. Load or create blueprint registry
    let blueprints = config.blueprint_path.as_ref()
        .and_then(|p| BlueprintRegistry::load(p).ok())
        .unwrap_or_else(BlueprintRegistry::with_builtins);

    // 4. Create decomposition context
    let mut ctx = DecompositionContext {
        blueprints,
        trace: Vec::new(),
        files: Vec::new(),
        output_dir: output_dir.to_path_buf(),
        app_name: app_name.clone(),
        domain: domain.clone(),
        entities: entities.clone(),
        blueprint_hits: 0,
        template_hits: 0,
    };

    // 5. Recursive decomposition
    decompose(&mut cube, &mut ctx)?;

    // 6. Emit all files via v2 emission engine
    std::fs::create_dir_all(output_dir)?;
    let emission_result = v2_emission::emit_v2(
        &app_name,
        &domain,
        &entities,
        output_dir,
    )?;

    ctx.files = emission_result.files.clone();

    // 7. Save blueprints
    if let Some(bp_path) = &config.blueprint_path {
        if let Some(parent) = bp_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = ctx.blueprints.save(bp_path);
    }

    // 8. v2.1 Multi-pass render loop (runs when passes > 0)
    let (render_passes_executed, total_tokens_used, crystals_updated) =
        run_render_loop(config, &emission_result.files, output_dir, &domain_name)?;

    let time_secs = start.elapsed().as_secs_f64();

    Ok(ForgeV2Result {
        app_name,
        total_files: ctx.files.len(),
        total_loc: emission_result.total_loc,
        blueprint_hits: ctx.blueprint_hits,
        template_hits: ctx.template_hits,
        trace: ctx.trace,
        time_secs,
        output_dir: output_dir.to_path_buf(),
        domain_name,
        total_dims,
        initial_dof,
        total_couplings,
        render_passes_executed,
        total_tokens_used,
        crystals_updated,
    })
}

// ─── Render Loop Integration ─────────────────────────────────────────────────

/// Infer a layer category from a relative file path for pass scoping.
fn infer_category(rel_path: &str) -> String {
    if rel_path.contains("service") { return "services".into(); }
    if rel_path.contains("test") || rel_path.contains("spec") { return "tests".into(); }
    if rel_path.contains("api") || rel_path.contains("handler") || rel_path.contains("route") {
        return "api".into();
    }
    if rel_path.contains("frontend") || rel_path.ends_with(".html") || rel_path.ends_with(".js") {
        return "frontend".into();
    }
    "other".into()
}

/// Run the v2.1 multi-pass render loop after emission.
///
/// Returns `(passes_executed, total_tokens_used, crystals_updated)`.
fn run_render_loop(
    config: &DecomposerConfig,
    emitted_files: &[EmittedFile],
    output_dir: &Path,
    domain_name: &str,
) -> Result<(u32, u64, usize)> {
    // Skip render loop when no passes are requested or mock_oracle is set without passes
    if config.passes == 0 {
        return Ok((0, 0, 0));
    }

    // Build oracle
    let oracle: Box<dyn Oracle> = if config.mock_oracle || config.api_key.is_none() {
        Box::new(MockOracle)
    } else {
        match OpenAiOracle::new(config.api_key.clone(), Some(config.model.clone())) {
            Ok(o) => Box::new(o),
            Err(e) => {
                eprintln!("[render-loop] oracle init failed: {e}; falling back to mock");
                Box::new(MockOracle)
            }
        }
    };

    // Load or create crystal registry
    let mut crystals = match &config.crystal_path {
        Some(p) => CrystalRegistry::load(p).unwrap_or_else(|_| CrystalRegistry::with_builtins()),
        None => CrystalRegistry::with_builtins(),
    };

    // Build pass list filtered by config.passes and skip_passes
    let all_passes = RenderPass::default_passes();
    let active_passes: Vec<RenderPass> = all_passes
        .into_iter()
        .filter(|p| p.depth <= config.passes)
        .filter(|p| !config.skip_passes.contains(&p.pass_type.label().to_string()))
        .map(|mut p| {
            if let Some(budget) = config.token_budget_override {
                p.token_budget = budget;
            }
            p
        })
        .collect();

    // Convert EmittedFile → Artifact (read content from disk)
    let mut artifacts: Vec<Artifact> = Vec::new();
    for ef in emitted_files {
        let full_path = output_dir.join(&ef.rel_path);
        match std::fs::read_to_string(&full_path) {
            Ok(content) => {
                let rel = ef.rel_path.to_string_lossy().to_string();
                let category = infer_category(&rel);
                artifacts.push(Artifact { rel_path: rel, content, category });
            }
            Err(_) => {
                // File may not have been written yet; skip silently
            }
        }
    }

    if artifacts.is_empty() {
        return Ok((0, 0, 0));
    }

    let mut render_loop = RenderLoop::new(oracle)
        .with_passes(active_passes)
        .with_crystals(crystals.clone());

    let enriched = render_loop.render(artifacts, domain_name)
        .map_err(|e| DecomposerError::Failed(format!("render loop failed: {e}")))?;

    // Write enriched artifacts back to disk
    for artifact in &enriched {
        let full_path = output_dir.join(&artifact.rel_path);
        if let Some(parent) = full_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&full_path, &artifact.content);
    }

    // Persist crystal registry
    crystals = render_loop.crystals.clone();
    if let Some(crystal_path) = &config.crystal_path {
        let _ = crystals.save(crystal_path);
    }

    let stats = &render_loop.stats;
    Ok((stats.passes_executed, stats.total_tokens_used, stats.crystals_updated))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_collapse_empty() {
        let graph = CouplingGraph {
            nodes: vec![],
            edges: vec![],
            adjacency: vec![],
        };
        let plan = plan_collapse(&graph, &BlueprintRegistry::with_builtins());
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn test_plan_collapse_simple() {
        let graph = CouplingGraph {
            nodes: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            edges: vec![(0,1,1.0),(1,2,0.1),(2,3,1.0)],
            adjacency: vec![
                vec![(1, 1.0)],
                vec![(0, 1.0), (2, 0.1)],
                vec![(1, 0.1), (3, 1.0)],
                vec![(2, 1.0)],
            ],
        };
        let plan = plan_collapse(&graph, &BlueprintRegistry::with_builtins());
        assert!(!plan.steps.is_empty());
    }

    #[test]
    fn test_decompose_fully_constrained() {
        let mut cube = HyperCube {
            dimensions: vec![isls_hypercube::Dimension {
                name: "x".into(),
                category: isls_hypercube::DimCategory::Architecture,
                state: DimState::Fixed(DimValue::Choice("rust".into())),
                complexity: 0,
                description: "".into(),
            }],
            couplings: vec![],
            depth: 0,
            parent_signature: None,
        };
        let domain = isls_hypercube::domain::DomainTemplate {
            name: "test".into(),
            keywords: vec![],
            entities: vec![],
            relationships: vec![],
            business_rules: vec![],
            api_features: isls_hypercube::domain::ApiFeatures {
                pagination: false,
                filtering: vec![],
                sorting: vec![],
                search_fields: vec![],
                export_formats: vec![],
            },
        };
        let mut ctx = DecompositionContext {
            blueprints: BlueprintRegistry::with_builtins(),
            trace: Vec::new(),
            files: Vec::new(),
            output_dir: "/tmp/test".into(),
            app_name: "test".into(),
            domain,
            entities: vec![],
            blueprint_hits: 0,
            template_hits: 0,
        };
        let result = decompose(&mut cube, &mut ctx);
        assert!(result.is_ok());
        assert_eq!(cube.dof(), 0);
    }
}
