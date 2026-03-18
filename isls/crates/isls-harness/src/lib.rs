// isls-harness: ISLS Validation Harness — C10
// Benchmark runners, validators, metric collectors, reporters
// Spec: ISLS_ValidationHarness_v1_0_0

pub mod metrics;
pub mod bench;
pub mod bench_generative;
pub mod validate;
pub mod synthetic;
pub mod report;
pub mod iterate;
pub mod genesis;
#[cfg(test)]
pub mod extension_tests;
#[cfg(test)]
pub mod topology_tests;
#[cfg(test)]
pub mod store_tests;
#[cfg(test)]
pub mod scale_tests;

// Re-export key types for convenience
pub use metrics::{Alert, AlertLevel, MetricCollector, MetricSnapshot};
pub use bench::{BenchResult, BenchSuite, RegressionVerdict, check_regression};
pub use bench_generative::run_generative_suite;
pub use validate::{FormalReport, FormalValidator, LiveValidator, RetroReport, RetroValidator};
pub use synthetic::{PlantedConstraint, RecoveryScore, ScenarioKind, SyntheticGenerator, SyntheticScenario};
pub use report::{FullReport, ReportGenerator, SystemOverview};
pub use iterate::{generate_iteration_guidance, IterationItem, Priority};
pub use genesis::{
    AmendmentSpec, DriftEntry, GenesisError, GenesisValidationResult,
    build_genesis_crystal, detect_constitutional_drift, evaluate_constitutional_constraints,
    validate_genesis,
};
