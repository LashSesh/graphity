//! Universal artifact intermediate representation for ISLS (C22).
//!
//! A flat, serializable IR that bridges `PmhdMonolith` to domain-specific
//! synthesis. Deterministic: the same `(monolith, spec)` always yields the same IR.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use sha2::{Digest, Sha256};
use isls_types::{content_address, FiveDState, Hash256};
use isls_pmhd::{DecisionSpec, PmhdMonolith};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum IrError {
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("empty monolith: no components could be derived")]
    EmptyMonolith,
}

pub type Result<T> = std::result::Result<T, IrError>;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ─── ArtifactHeader ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactHeader {
    pub artifact_id: Hash256,
    pub version: String,
    pub timestamp_tick: u64,
    pub layer_index: u32,
    pub por_decision: Option<String>,
    pub source_monolith_id: String,
    pub domain: String,
}

// ─── Component ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub content: String,
    pub dependencies: Vec<String>,
    pub signature: FiveDState,
}

impl Component {
    fn new(kind: &str, name: &str, content: &str) -> Self {
        let mut h1 = Sha256::new();
        h1.update(kind.as_bytes());
        h1.update(name.as_bytes());
        h1.update(content.as_bytes());
        let id = hex_encode(&h1.finalize());

        // Derive a non-zero FiveDState from a second hash of kind+content
        let mut h2 = Sha256::new();
        h2.update(kind.as_bytes());
        h2.update(content.as_bytes());
        let sig_bytes = h2.finalize();
        let sig = FiveDState {
            p:     (sig_bytes[0] as f64) / 255.0,
            rho:   (sig_bytes[1] as f64) / 255.0,
            omega: (sig_bytes[2] as f64) / 255.0,
            chi:   (sig_bytes[3] as f64) / 255.0,
            eta:   (sig_bytes[4] as f64) / 255.0,
        };
        Self {
            id,
            kind: kind.to_string(),
            name: name.to_string(),
            content: content.to_string(),
            dependencies: Vec::new(),
            signature: sig,
        }
    }
}

// ─── Interface ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Direction {
    Unidirectional,
    Bidirectional,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Interface {
    pub id: String,
    pub provider: String,
    pub consumer: String,
    pub contract: String,
    pub direction: Direction,
}

impl Interface {
    fn new(provider: &str, consumer: &str, contract: &str, dir: Direction) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(provider.as_bytes());
        hasher.update(consumer.as_bytes());
        hasher.update(contract.as_bytes());
        let id = hex_encode(&hasher.finalize());
        Self {
            id,
            provider: provider.to_string(),
            consumer: consumer.to_string(),
            contract: contract.to_string(),
            direction: dir,
        }
    }
}

// ─── ArtifactConstraint ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactConstraint {
    pub id: String,
    pub predicate: String,
    pub satisfied: bool,
}

// ─── ArtifactMetrics ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ArtifactMetrics {
    // Propagated from QualityMetrics
    pub coherence: f64,
    pub diversity: f64,
    pub novelty: f64,
    pub stability: f64,
    pub robustness: f64,
    pub coverage: f64,
    // Synthesis-specific
    pub component_count: usize,
    pub interface_count: usize,
    pub constraint_satisfaction: f64,
    pub quality_score: f64,
}

// ─── ArtifactProvenance ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactProvenance {
    pub decision_spec_id: Hash256,
    pub monolith_id: String,
    pub seed: u64,
    pub config_hash: String,
    pub tick_range: [u64; 2],
    pub drill_strategy: String,
    pub por_evidence: Option<String>,
    pub pattern_memory_size: usize,
}

// ─── ArtifactDelta ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactDelta {
    pub base_artifact_id: Hash256,
    pub changed_component_ids: Vec<String>,
    pub delta_description: String,
}

// ─── ArtifactIR ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactIR {
    pub header: ArtifactHeader,
    pub components: Vec<Component>,
    pub interfaces: Vec<Interface>,
    pub constraints: Vec<ArtifactConstraint>,
    pub metrics: ArtifactMetrics,
    pub provenance: ArtifactProvenance,
    pub deltas: Vec<ArtifactDelta>,
    pub extra: BTreeMap<String, String>,
}

impl ArtifactIR {
    /// Build an ArtifactIR deterministically from a PmhdMonolith and DecisionSpec.
    /// Same inputs always produce identical artifact_id (content-addressed).
    pub fn build_from_monolith(
        monolith: &PmhdMonolith,
        spec: &DecisionSpec,
        layer: u32,
    ) -> Result<Self> {
        let h = &monolith.core_hypothesis;

        // Component: one for the core claim
        let mut components = Vec::new();
        let claim_comp = Component::new("claim", "core-claim", &h.claim);
        components.push(claim_comp);

        // Components: one per assumption
        for (i, assumption) in h.assumptions.iter().enumerate() {
            let mut comp = Component::new("assumption", &format!("assumption-{i}"), assumption);
            comp.dependencies.push(components[0].id.clone()); // depends on claim
            components.push(comp);
        }

        // Interface: claim ↔ each counterexample (opposition links)
        let mut interfaces = Vec::new();
        for ce in &monolith.counterexamples {
            let iface = Interface::new(
                &components[0].id,
                &ce.id,
                &format!("{{\"severity\":{}}}", ce.severity),
                Direction::Unidirectional,
            );
            interfaces.push(iface);
        }

        // Constraints from spec
        let constraints: Vec<ArtifactConstraint> = spec.constraints.iter()
            .map(|pred| {
                let mut hasher = Sha256::new();
                hasher.update(pred.as_bytes());
                ArtifactConstraint {
                    id: hex_encode(&hasher.finalize()),
                    predicate: pred.clone(),
                    satisfied: true, // optimistically — evaluator validates later
                }
            })
            .collect();
        let constraint_satisfaction = if constraints.is_empty() {
            1.0
        } else {
            constraints.iter().filter(|c| c.satisfied).count() as f64 / constraints.len() as f64
        };

        // Metrics from monolith quality
        let q = &monolith.quality;
        let quality_score = q.mean();
        let metrics = ArtifactMetrics {
            coherence: q.coherence,
            diversity: q.diversity,
            novelty: q.novelty,
            stability: q.stability,
            robustness: q.robustness,
            coverage: q.coverage,
            component_count: components.len(),
            interface_count: interfaces.len(),
            constraint_satisfaction,
            quality_score,
        };

        // Provenance
        let provenance = ArtifactProvenance {
            decision_spec_id: spec.id,
            monolith_id: monolith.id.clone(),
            seed: monolith.provenance.seed,
            config_hash: monolith.provenance.config_hash.clone(),
            tick_range: monolith.provenance.tick_range,
            drill_strategy: "hybrid".to_string(),
            por_evidence: Some(monolith.provenance.por_evidence.clone()),
            pattern_memory_size: 0,
        };

        // Content-address the header core (determines artifact_id)
        #[derive(Serialize)]
        struct HeaderCore<'a> {
            monolith_id: &'a str,
            spec_id: &'a Hash256,
            layer: u32,
            component_count: usize,
            quality_score: f64,
        }
        let core = HeaderCore {
            monolith_id: &monolith.id,
            spec_id: &spec.id,
            layer,
            component_count: components.len(),
            quality_score,
        };
        let artifact_id = content_address(&core);

        let header = ArtifactHeader {
            artifact_id,
            version: "1.0.0".to_string(),
            timestamp_tick: monolith.provenance.tick_range[1],
            layer_index: layer,
            por_decision: Some(monolith.provenance.por_evidence.clone()),
            source_monolith_id: monolith.id.clone(),
            domain: spec.domain.clone(),
        };

        Ok(Self {
            header,
            components,
            interfaces,
            constraints,
            metrics,
            provenance,
            deltas: Vec::new(),
            extra: BTreeMap::new(),
        })
    }
}

// ─── Acceptance Tests (AT-IR1 through AT-IR4) ─────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use isls_pmhd::{DrillEngine, PmhdConfig, QualityThresholds};

    fn run_drill() -> (PmhdMonolith, DecisionSpec) {
        let mut goals = BTreeMap::new();
        goals.insert("coherence".to_string(), 0.7);
        goals.insert("robustness".to_string(), 0.8);
        let spec = DecisionSpec::new(
            "Build a REST API health-check",
            goals,
            vec!["must return JSON".to_string()],
            "rust",
            PmhdConfig {
                ticks: 10,
                pool_size: 4,
                commit_budget: 2,
                thresholds: QualityThresholds::default(), // all 0.0
                ..Default::default()
            },
        );
        let mut eng = DrillEngine::new(spec.config.clone());
        let res = eng.drill(&spec);
        let monolith = res.monoliths.into_iter().next().expect("at least one monolith from drill");
        (monolith, spec)
    }

    // AT-IR1: IR determinism — build IR from same monolith twice → identical artifact_id.
    #[test]
    fn at_ir1_ir_determinism() {
        let (monolith, spec) = run_drill();
        let ir1 = ArtifactIR::build_from_monolith(&monolith, &spec, 0).unwrap();
        let ir2 = ArtifactIR::build_from_monolith(&monolith, &spec, 0).unwrap();
        assert_eq!(ir1.header.artifact_id, ir2.header.artifact_id,
            "AT-IR1: artifact_id must be identical for same monolith");
    }

    // AT-IR2: Serde round-trip — serialize → deserialize → equal.
    #[test]
    fn at_ir2_serde_roundtrip() {
        let (monolith, spec) = run_drill();
        let ir = ArtifactIR::build_from_monolith(&monolith, &spec, 0).unwrap();
        let json = serde_json::to_string(&ir).expect("serialize");
        let ir2: ArtifactIR = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ir.header.artifact_id, ir2.header.artifact_id,
            "AT-IR2: artifact_id must survive serde round-trip");
        assert_eq!(ir.components.len(), ir2.components.len(),
            "AT-IR2: component count must survive round-trip");
    }

    // AT-IR3: Provenance link — verify provenance.monolith_id matches source monolith.
    #[test]
    fn at_ir3_provenance_link() {
        let (monolith, spec) = run_drill();
        let ir = ArtifactIR::build_from_monolith(&monolith, &spec, 0).unwrap();
        assert_eq!(ir.provenance.monolith_id, monolith.id,
            "AT-IR3: provenance.monolith_id must match source monolith ID");
        assert_eq!(ir.header.source_monolith_id, monolith.id,
            "AT-IR3: header.source_monolith_id must match");
        assert_eq!(ir.provenance.decision_spec_id, spec.id,
            "AT-IR3: provenance.decision_spec_id must match spec");
    }

    // AT-IR4: Component signature — every component has a non-zero FiveDState.
    #[test]
    fn at_ir4_component_signature() {
        let (monolith, spec) = run_drill();
        let ir = ArtifactIR::build_from_monolith(&monolith, &spec, 0).unwrap();
        assert!(!ir.components.is_empty(), "AT-IR4: IR must have at least one component");
        for comp in &ir.components {
            let sig = &comp.signature;
            let norm_sq = sig.norm_sq();
            assert!(
                norm_sq > 0.0,
                "AT-IR4: component '{}' has zero FiveDState signature",
                comp.name
            );
        }
    }
}
