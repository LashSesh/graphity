// isls-harness: ISLS Validation Harness — C10
// Benchmark runners, validators, metric collectors, reporters
// Spec: ISLS_ValidationHarness_v1_0_0

pub mod metrics;
pub mod bench;
pub mod validate;
pub mod synthetic;
pub mod report;
pub mod iterate;
#[cfg(test)]
pub mod extension_tests;
#[cfg(test)]
pub mod topology_tests;
#[cfg(test)]
pub mod store_tests;

// Re-export key types for convenience
pub use metrics::{Alert, AlertLevel, MetricCollector, MetricSnapshot};
pub use bench::{BenchResult, BenchSuite, RegressionVerdict, check_regression};
pub use validate::{FormalReport, FormalValidator, LiveValidator, RetroReport, RetroValidator};
pub use synthetic::{PlantedConstraint, RecoveryScore, ScenarioKind, SyntheticGenerator, SyntheticScenario};
pub use report::{FullReport, ReportGenerator, SystemOverview};
pub use iterate::{generate_iteration_guidance, IterationItem, Priority};
