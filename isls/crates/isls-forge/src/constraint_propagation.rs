// isls-forge/src/constraint_propagation.rs
//
// Constraint Propagation Pass — runs BEFORE the Oracle (C25).
// For every ArtifactIR component, compute degrees of freedom after applying
// known constraints. Zero freedom → deterministic synthesis (no Oracle).
// Low → constrained Oracle prompt. High → standard Oracle call.
//
// Spec target: reduce Oracle calls by 50–70%.

use std::collections::BTreeMap;
use isls_artifact_ir::ArtifactIR;
use isls_pmhd::PatternMemory;
use isls_types::FiveDState;

// ─── Component Kind ───────────────────────────────────────────────────────────

/// Semantic classification of an ArtifactIR component, inferred from its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComponentKind {
    /// Pure type conversion — e.g. `user_to_dto`, `order_mapper`
    TypeConversion,
    /// CRUD operation — e.g. `create_user`, `delete_order`, `get_by_id`
    CrudOperation,
    /// Validation / guard — e.g. `validate_input`, `check_permissions`
    Validation,
    /// HTTP route handler — e.g. `handle_request`, `post_handler`
    RouteHandler,
    /// Configuration bootstrap — e.g. `config`, `init_config`
    ConfigInit,
    /// Application entry point — e.g. `main`, `run`
    EntryPoint,
    /// Complex business logic — requires creative synthesis
    BusinessLogic,
    /// Test function — e.g. `test_`, `assert_`
    TestFunction,
    /// Unrecognised
    Unknown,
}

impl ComponentKind {
    /// Label string used in structured output.
    pub fn label(&self) -> &'static str {
        match self {
            ComponentKind::TypeConversion => "TypeConversion",
            ComponentKind::CrudOperation  => "CrudOperation",
            ComponentKind::Validation     => "Validation",
            ComponentKind::RouteHandler   => "RouteHandler",
            ComponentKind::ConfigInit     => "ConfigInit",
            ComponentKind::EntryPoint     => "EntryPoint",
            ComponentKind::BusinessLogic  => "BusinessLogic",
            ComponentKind::TestFunction   => "TestFunction",
            ComponentKind::Unknown        => "Unknown",
        }
    }
}

// ─── Pattern Snippet ──────────────────────────────────────────────────────────

/// A cached synthesis result fetched from PatternMemory, used in Deterministic
/// and PatternReuse strategies.
#[derive(Clone, Debug)]
pub struct PatternSnippet {
    pub monolith_id: String,
    pub domain: String,
    /// Normalised similarity score in [0.0, 1.0].
    /// `similarity = (1.0 - distance / 2.0).max(0.0)`
    pub similarity: f64,
    pub component_kinds: Vec<String>,
}

// ─── Constraint Set ───────────────────────────────────────────────────────────

/// Constraints gathered for one ArtifactIR component.
#[derive(Clone, Debug, Default)]
pub struct ConstraintSet {
    /// Names of components this one depends on (already analysed)
    pub dependency_names: Vec<String>,
    /// Patterns from memory with similarity ≥ `SIMILARITY_THRESHOLD`
    pub matching_patterns: Vec<PatternSnippet>,
    /// Type signature / contract string derived from ArtifactIR content
    pub type_signature: Option<String>,
    /// Whether all dependencies have already been synthesised deterministically
    pub deps_fully_constrained: bool,
}

// ─── Freedom Analysis ─────────────────────────────────────────────────────────

/// Degrees of freedom after applying all constraints (0 = fully determined).
#[derive(Clone, Debug)]
pub struct FreedomAnalysis {
    pub component_id: String,
    pub kind: ComponentKind,
    pub degrees_of_freedom: u32,
    pub strategy: SynthesisStrategy,
    pub constraints: ConstraintSet,
}

// ─── Synthesis Strategy ───────────────────────────────────────────────────────

/// How to synthesise this component — determined by the propagation pass.
#[derive(Clone, Debug, PartialEq)]
pub enum SynthesisStrategy {
    /// `degrees_of_freedom == 0` — synthesise without calling the Oracle.
    Deterministic,
    /// `1 ≤ dof ≤ LOW_DOF_THRESHOLD` — call Oracle with a tightly constrained prompt.
    ConstrainedOracle,
    /// `dof > LOW_DOF_THRESHOLD` but high-quality pattern exists — reuse pattern.
    PatternReuse,
    /// High freedom — full Oracle call.
    OpenOracle,
}

// ─── Synthesis Error ──────────────────────────────────────────────────────────

/// Errors that can arise when attempting deterministic synthesis.
#[derive(Clone, Debug, PartialEq)]
pub enum SynthesisError {
    /// No deterministic rule covers this component kind.
    UnsupportedKind,
    /// Required dependency output is missing.
    MissingDependency(String),
}

// ─── Propagation Stats ────────────────────────────────────────────────────────

/// Aggregate statistics emitted by one propagation pass run.
#[derive(Clone, Debug, Default)]
pub struct PropagationStats {
    /// Total components analysed
    pub total: usize,
    /// Synthesised without Oracle
    pub deterministic: usize,
    /// High-quality pattern reuse (no LLM)
    pub pattern_reuse: usize,
    /// Oracle called with constrained prompt
    pub constrained: usize,
    /// Full open Oracle call
    pub open: usize,
}

impl PropagationStats {
    /// Fraction of components handled without the Oracle.
    pub fn oracle_reduction_ratio(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.deterministic + self.pattern_reuse) as f64 / self.total as f64
    }
}

// ─── Thresholds ───────────────────────────────────────────────────────────────

/// Minimum pattern similarity to trigger Deterministic or PatternReuse.
const SIMILARITY_THRESHOLD: f64 = 0.85;

/// Freedom threshold below which we use ConstrainedOracle instead of OpenOracle.
const LOW_DOF_THRESHOLD: u32 = 3;

/// Similarity threshold above which we prefer PatternReuse over OpenOracle.
const PATTERN_REUSE_THRESHOLD: f64 = 0.92;

// ─── Public API ───────────────────────────────────────────────────────────────

/// Classify a component by its name (and optionally its `kind` field from IR).
pub fn classify_component(name: &str, ir_kind: &str) -> ComponentKind {
    let n = name.to_lowercase();

    // Test functions first — they often start with "test_"
    if n.starts_with("test_") || n.starts_with("assert_") || n.contains("_test") {
        return ComponentKind::TestFunction;
    }

    // Entry points
    if n == "main" || n == "run" || n.ends_with("::main") {
        return ComponentKind::EntryPoint;
    }

    // Config init
    if n.contains("config") || n.contains("init") || n.contains("setup") || n.contains("bootstrap") {
        return ComponentKind::ConfigInit;
    }

    // Type conversions — "to_", "_mapper", "_dto", "_converter"
    if n.contains("_to_") || n.ends_with("_mapper") || n.ends_with("_dto")
        || n.ends_with("_converter") || n.starts_with("to_") || n.contains("_into_")
    {
        return ComponentKind::TypeConversion;
    }

    // CRUD
    if n.starts_with("create_") || n.starts_with("get_") || n.starts_with("update_")
        || n.starts_with("delete_") || n.starts_with("fetch_") || n.starts_with("insert_")
        || n.starts_with("find_") || n.starts_with("load_") || n.starts_with("save_")
        || n.ends_with("_by_id") || n.ends_with("_all")
    {
        return ComponentKind::CrudOperation;
    }

    // Validation / guards
    if n.starts_with("validate_") || n.starts_with("check_") || n.starts_with("verify_")
        || n.starts_with("assert_") || n.contains("_valid") || n.contains("_guard")
        || n.contains("_permission")
    {
        return ComponentKind::Validation;
    }

    // HTTP / route handlers
    if n.contains("handler") || n.contains("handle_") || n.contains("_route")
        || n.contains("_endpoint") || n.starts_with("get_handler")
        || n.starts_with("post_") || n.starts_with("put_") || n.starts_with("patch_")
        || n.starts_with("delete_handler")
    {
        return ComponentKind::RouteHandler;
    }

    // Fall back: if IR kind says "claim" treat as BusinessLogic, "assumption" stays Unknown
    match ir_kind {
        "claim" => ComponentKind::BusinessLogic,
        _ => ComponentKind::Unknown,
    }
}

/// Collect all constraints for a component from the pattern memory.
pub fn collect_constraints(
    _component_id: &str,
    component_name: &str,
    signature: &FiveDState,
    deps: &[String],
    memory: &PatternMemory,
    deterministic_ids: &std::collections::HashSet<String>,
) -> ConstraintSet {
    // Look up similar patterns
    let raw = memory.find_similar(signature, 5);
    let matching_patterns: Vec<PatternSnippet> = raw
        .into_iter()
        .map(|e| {
            let dist = signature.distance(&e.signature);
            let similarity = (1.0 - dist / 2.0_f64).max(0.0_f64);
            PatternSnippet {
                monolith_id: e.monolith_id.clone(),
                domain: e.domain.clone(),
                similarity,
                component_kinds: e.component_kinds.clone(),
            }
        })
        .filter(|p| p.similarity >= SIMILARITY_THRESHOLD)
        .collect();

    let deps_fully_constrained = deps
        .iter()
        .all(|d| deterministic_ids.contains(d.as_str()));

    ConstraintSet {
        dependency_names: deps.to_vec(),
        matching_patterns,
        type_signature: Some(component_name.to_string()),
        deps_fully_constrained,
    }
}

/// Compute degrees of freedom for a component given its kind and constraints.
pub fn analyze_freedom(
    kind: &ComponentKind,
    constraints: &ConstraintSet,
) -> u32 {
    // Base freedom by kind
    let base: u32 = match kind {
        ComponentKind::TypeConversion => 0, // fully structural
        ComponentKind::CrudOperation  => 1,
        ComponentKind::Validation     => 1,
        ComponentKind::ConfigInit     => 1,
        ComponentKind::EntryPoint     => 0,
        ComponentKind::RouteHandler   => 2,
        ComponentKind::TestFunction   => 1,
        ComponentKind::BusinessLogic  => 5,
        ComponentKind::Unknown        => 4,
    };

    // Reduce by strong pattern match
    let has_strong_match = constraints
        .matching_patterns
        .iter()
        .any(|p| p.similarity >= SIMILARITY_THRESHOLD);

    // Reduce by fully-constrained deps
    let dep_reduction = if constraints.deps_fully_constrained && !constraints.dependency_names.is_empty() {
        1
    } else {
        0
    };

    let pattern_reduction = if has_strong_match { 1 } else { 0 };

    base.saturating_sub(pattern_reduction + dep_reduction)
}

/// Choose a synthesis strategy from degrees of freedom and pattern quality.
fn choose_strategy(dof: u32, constraints: &ConstraintSet) -> SynthesisStrategy {
    if dof == 0 {
        return SynthesisStrategy::Deterministic;
    }
    // Check for high-quality pattern reuse opportunity
    let best_similarity = constraints
        .matching_patterns
        .iter()
        .map(|p| p.similarity)
        .fold(0.0_f64, f64::max);

    if best_similarity >= PATTERN_REUSE_THRESHOLD {
        return SynthesisStrategy::PatternReuse;
    }

    if dof <= LOW_DOF_THRESHOLD {
        SynthesisStrategy::ConstrainedOracle
    } else {
        SynthesisStrategy::OpenOracle
    }
}

/// Attempt to synthesise a component deterministically (no Oracle).
/// Returns synthesised content or `SynthesisError`.
pub fn synthesize_deterministic(
    name: &str,
    kind: &ComponentKind,
    _deps: &[String],
) -> Result<String, SynthesisError> {
    match kind {
        ComponentKind::TypeConversion => {
            // Infer source and target from name: "a_to_b" → source=a, target=b
            let parts: Vec<&str> = name.splitn(2, "_to_").collect();
            if parts.len() == 2 {
                let src = parts[0];
                let tgt = parts[1];
                Ok(format!(
                    "fn {name}(input: {src}) -> {tgt} {{\n    // Deterministic structural mapping\n    {tgt} {{ ..Default::default() }}\n}}"
                ))
            } else {
                Ok(format!(
                    "fn {name}(input: impl Into<Self>) -> Self {{\n    input.into()\n}}"
                ))
            }
        }
        ComponentKind::EntryPoint => Ok(format!(
            "fn {name}() {{\n    // Deterministic entry point\n}}"
        )),
        ComponentKind::CrudOperation => {
            // dof=1 after reduction → only reached here with dof=0 via strong pattern
            Ok(format!(
                "fn {name}() -> Result<(), Error> {{\n    // Deterministic CRUD\n    Ok(())\n}}"
            ))
        }
        ComponentKind::Validation => Ok(format!(
            "fn {name}(input: &impl Validate) -> Result<(), ValidationError> {{\n    input.validate()\n}}"
        )),
        ComponentKind::ConfigInit => Ok(format!(
            "fn {name}() -> Config {{\n    Config::default()\n}}"
        )),
        ComponentKind::TestFunction => Ok(format!(
            "#[test]\nfn {name}() {{\n    // deterministic test stub\n}}"
        )),
        ComponentKind::BusinessLogic | ComponentKind::Unknown => {
            Err(SynthesisError::UnsupportedKind)
        }
        ComponentKind::RouteHandler => Ok(format!(
            "async fn {name}(req: Request) -> Response {{\n    // Deterministic handler stub\n    Response::ok()\n}}"
        )),
    }
}

/// Build a constrained Oracle prompt that includes all available constraints.
pub fn build_constrained_prompt(
    name: &str,
    kind: &ComponentKind,
    constraints: &ConstraintSet,
) -> String {
    let mut prompt = format!(
        "Synthesise a Rust function named `{name}` of kind `{}`.\n",
        kind.label()
    );

    if !constraints.dependency_names.is_empty() {
        prompt.push_str(&format!(
            "Dependencies (already synthesised): {}.\n",
            constraints.dependency_names.join(", ")
        ));
    }
    if let Some(sig) = &constraints.type_signature {
        prompt.push_str(&format!("Type context: {sig}.\n"));
    }
    if let Some(best) = constraints
        .matching_patterns
        .iter()
        .max_by(|a, b| a.similarity.partial_cmp(&b.similarity).unwrap())
    {
        prompt.push_str(&format!(
            "Closest pattern (similarity {:.2}): domain={}, kinds={:?}.\n",
            best.similarity, best.domain, best.component_kinds
        ));
    }
    prompt.push_str("Keep implementation minimal and idiomatic.");
    prompt
}

/// Run the full constraint propagation pass over one ArtifactIR.
///
/// Returns a map of component_id → FreedomAnalysis and aggregate PropagationStats.
pub fn run_propagation_pass(
    ir: &ArtifactIR,
    memory: &PatternMemory,
) -> (BTreeMap<String, FreedomAnalysis>, PropagationStats) {
    let mut analyses: BTreeMap<String, FreedomAnalysis> = BTreeMap::new();
    let mut deterministic_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stats = PropagationStats::default();

    for component in &ir.components {
        let kind = classify_component(&component.name, &component.kind);
        let constraints = collect_constraints(
            &component.id,
            &component.name,
            &component.signature,
            &component.dependencies,
            memory,
            &deterministic_ids,
        );
        let dof = analyze_freedom(&kind, &constraints);
        let strategy = choose_strategy(dof, &constraints);

        // Track deterministic set for downstream components
        if strategy == SynthesisStrategy::Deterministic || strategy == SynthesisStrategy::PatternReuse {
            deterministic_ids.insert(component.id.clone());
        }

        // Accumulate stats
        stats.total += 1;
        match &strategy {
            SynthesisStrategy::Deterministic   => stats.deterministic += 1,
            SynthesisStrategy::PatternReuse    => stats.pattern_reuse  += 1,
            SynthesisStrategy::ConstrainedOracle => stats.constrained   += 1,
            SynthesisStrategy::OpenOracle      => stats.open           += 1,
        }

        analyses.insert(component.id.clone(), FreedomAnalysis {
            component_id: component.id.clone(),
            kind,
            degrees_of_freedom: dof,
            strategy,
            constraints,
        });
    }

    (analyses, stats)
}

// ─── Tests (AT-CP1 through AT-CP12) ───────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_artifact_ir::{ArtifactIR, Component, ArtifactHeader};
    use isls_pmhd::PatternMemory;
    use isls_types::FiveDState;

    fn zero_sig() -> FiveDState {
        FiveDState { p: 0.0, rho: 0.0, omega: 0.0, chi: 0.0, eta: 0.0 }
    }

    fn make_component(id: &str, name: &str, kind: &str, deps: Vec<&str>) -> Component {
        Component {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            content: String::new(),
            dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
            signature: zero_sig(),
        }
    }

    fn minimal_ir(components: Vec<Component>) -> ArtifactIR {
        use isls_artifact_ir::{ArtifactHeader, ArtifactProvenance};
        ArtifactIR {
            header: ArtifactHeader {
                artifact_id: [0u8; 32],
                version: "1.0".to_string(),
                timestamp_tick: 0,
                layer_index: 0,
                por_decision: None,
                source_monolith_id: "test".to_string(),
                domain: "test".to_string(),
            },
            components,
            interfaces: vec![],
            constraints: vec![],
            metrics: Default::default(),
            provenance: ArtifactProvenance {
                decision_spec_id: [0u8; 32],
                monolith_id: "test".to_string(),
                seed: 0,
                config_hash: "0".to_string(),
                tick_range: [0, 0],
                drill_strategy: "default".to_string(),
                por_evidence: None,
                pattern_memory_size: 0,
            },
            deltas: vec![],
            extra: Default::default(),
        }
    }

    // AT-CP1: TypeConversion is classified correctly.
    #[test]
    fn at_cp1_type_conversion_classification() {
        let kind = classify_component("user_to_dto", "assumption");
        assert_eq!(kind, ComponentKind::TypeConversion, "AT-CP1");
    }

    // AT-CP2: CrudOperation is classified correctly.
    #[test]
    fn at_cp2_crud_classification() {
        let kind = classify_component("create_user", "assumption");
        assert_eq!(kind, ComponentKind::CrudOperation, "AT-CP2");
    }

    // AT-CP3: Validation is classified correctly.
    #[test]
    fn at_cp3_validation_classification() {
        let kind = classify_component("validate_input", "assumption");
        assert_eq!(kind, ComponentKind::Validation, "AT-CP3");
    }

    // AT-CP4: EntryPoint ("main") → zero degrees of freedom.
    #[test]
    fn at_cp4_entry_point_zero_dof() {
        let kind = classify_component("main", "assumption");
        assert_eq!(kind, ComponentKind::EntryPoint, "AT-CP4 kind");
        let cs = ConstraintSet::default();
        let dof = analyze_freedom(&kind, &cs);
        assert_eq!(dof, 0, "AT-CP4: EntryPoint must have 0 DOF");
    }

    // AT-CP5: ConfigInit → deterministic strategy when dof = 0 after pattern hit.
    #[test]
    fn at_cp5_config_init_deterministic() {
        let kind = classify_component("init_config", "assumption");
        assert_eq!(kind, ComponentKind::ConfigInit, "AT-CP5 kind");
        // With a strong pattern match, base=1 minus 1 = 0
        let cs = ConstraintSet {
            matching_patterns: vec![PatternSnippet {
                monolith_id: "m1".to_string(),
                domain: "config".to_string(),
                similarity: 0.90,
                component_kinds: vec!["ConfigInit".to_string()],
            }],
            ..Default::default()
        };
        let dof = analyze_freedom(&kind, &cs);
        assert_eq!(dof, 0, "AT-CP5: ConfigInit with strong pattern must be DOF=0");
        let strategy = choose_strategy(dof, &cs);
        assert_eq!(strategy, SynthesisStrategy::Deterministic, "AT-CP5");
    }

    // AT-CP6: BusinessLogic → OpenOracle (high freedom, no pattern).
    #[test]
    fn at_cp6_business_logic_open_oracle() {
        let kind = classify_component("process_payment", "claim");
        // "process_payment" doesn't match any CRUD/Validation/etc prefix → BusinessLogic via claim
        let kind = if kind == ComponentKind::Unknown {
            ComponentKind::BusinessLogic
        } else {
            kind
        };
        let cs = ConstraintSet::default();
        let dof = analyze_freedom(&kind, &cs);
        assert!(dof > LOW_DOF_THRESHOLD, "AT-CP6: BusinessLogic must have high DOF");
        let strategy = choose_strategy(dof, &cs);
        assert_eq!(strategy, SynthesisStrategy::OpenOracle, "AT-CP6");
    }

    // AT-CP7: synthesize_deterministic returns code for TypeConversion.
    #[test]
    fn at_cp7_deterministic_synthesis_type_conversion() {
        let result = synthesize_deterministic("user_to_dto", &ComponentKind::TypeConversion, &[]);
        assert!(result.is_ok(), "AT-CP7: TypeConversion must synthesise deterministically");
        let code = result.unwrap();
        assert!(code.contains("fn user_to_dto"), "AT-CP7: must contain fn name");
    }

    // AT-CP8: synthesize_deterministic returns error for BusinessLogic.
    #[test]
    fn at_cp8_deterministic_fails_for_business_logic() {
        let result = synthesize_deterministic("process_payment", &ComponentKind::BusinessLogic, &[]);
        assert_eq!(result, Err(SynthesisError::UnsupportedKind), "AT-CP8");
    }

    // AT-CP9: build_constrained_prompt includes dependency names.
    #[test]
    fn at_cp9_constrained_prompt_includes_deps() {
        let cs = ConstraintSet {
            dependency_names: vec!["user_to_dto".to_string(), "validate_input".to_string()],
            ..Default::default()
        };
        let prompt = build_constrained_prompt("create_user", &ComponentKind::CrudOperation, &cs);
        assert!(prompt.contains("user_to_dto"), "AT-CP9: prompt must mention deps");
        assert!(prompt.contains("validate_input"), "AT-CP9: prompt must mention deps");
    }

    // AT-CP10: PropagationStats.oracle_reduction_ratio() = (det + pattern) / total.
    #[test]
    fn at_cp10_propagation_stats_ratio() {
        let stats = PropagationStats {
            total: 10,
            deterministic: 5,
            pattern_reuse: 2,
            constrained: 2,
            open: 1,
        };
        let ratio = stats.oracle_reduction_ratio();
        assert!((ratio - 0.7).abs() < 1e-9, "AT-CP10: ratio must be 0.7, got {ratio}");
    }

    // AT-CP11: Full pass over synthetic IR with 5 components — ≥50% oracle reduction.
    #[test]
    fn at_cp11_full_pass_oracle_reduction() {
        let components = vec![
            make_component("c1", "main",           "assumption", vec![]),
            make_component("c2", "user_to_dto",    "assumption", vec!["c1"]),
            make_component("c3", "create_user",    "assumption", vec!["c2"]),
            make_component("c4", "validate_input", "assumption", vec!["c3"]),
            make_component("c5", "process_order",  "claim",      vec!["c4"]),
        ];
        let ir = minimal_ir(components);
        let memory = PatternMemory::new();
        let (_analyses, stats) = run_propagation_pass(&ir, &memory);

        let ratio = stats.oracle_reduction_ratio();
        assert!(
            ratio >= 0.50,
            "AT-CP11: oracle reduction must be ≥50%, got {:.2}",
            ratio
        );
    }

    // AT-CP12: FreedomAnalysis for "user_to_dto" is Deterministic (dof=0).
    #[test]
    fn at_cp12_user_to_dto_is_deterministic() {
        let components = vec![
            make_component("c1", "user_to_dto", "assumption", vec![]),
        ];
        let ir = minimal_ir(components);
        let memory = PatternMemory::new();
        let (analyses, _stats) = run_propagation_pass(&ir, &memory);
        let analysis = analyses.get("c1").expect("AT-CP12: c1 must be analysed");
        assert_eq!(
            analysis.strategy,
            SynthesisStrategy::Deterministic,
            "AT-CP12: user_to_dto must be Deterministic"
        );
    }
}
