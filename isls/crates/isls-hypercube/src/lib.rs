// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Hypercube requirement-space representation for ISLS v2.
//!
//! Models an application specification as a high-dimensional space where each
//! dimension represents a design decision. Spectral graph methods (Fiedler
//! bisection, Kuramoto grouping, singularity detection) guide recursive
//! decomposition into concrete code artifacts.

pub mod domain;
pub mod graph;
pub mod parser;

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use domain::{
    ApiFeatures, BusinessRule, DomainRegistry, DomainTemplate, EntityTemplate, FieldDef,
    IndexDef, Relationship, RelationshipKind, OnDelete, ValidationRule,
};
pub use graph::CouplingGraph;

// ─── Error ───────────────────────────────────────────────────────────────────

/// Errors produced by the hypercube subsystem.
#[derive(Debug, Error)]
pub enum HypercubeError {
    /// Failed to read the requirements TOML file.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parsing failed.
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
    /// Semantic error during cube construction.
    #[error("cube construction error: {0}")]
    Construction(String),
    /// Spectral computation failed.
    #[error("spectral error: {0}")]
    Spectral(String),
}

pub type Result<T> = std::result::Result<T, HypercubeError>;

// ─── Dimension Category ──────────────────────────────────────────────────────

/// Category of a hypercube dimension.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DimCategory {
    /// Architectural decision (language, framework, database).
    Architecture,
    /// Data model (entity struct, fields, validations).
    DataModel,
    /// Storage layer (queries, migrations).
    Storage,
    /// Business logic (services, rules).
    BusinessLogic,
    /// Interface layer (API endpoints).
    Interface,
    /// Security (auth, RBAC).
    Security,
    /// Presentation (frontend pages, components).
    Presentation,
    /// Testing (integration tests, test helpers).
    Testing,
    /// Deployment (Docker, compose, CI).
    Deployment,
    /// Documentation (README, API docs).
    Documentation,
}

// ─── Dimension Value ─────────────────────────────────────────────────────────

/// A concrete value assigned to a dimension.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DimValue {
    /// A named choice (e.g. "actix-web", "postgresql").
    Choice(String),
    /// A code fragment.
    Code(String),
    /// A full entity definition from the domain registry.
    EntityDef(EntityTemplate),
    /// Composite of multiple values.
    Composite(Vec<DimValue>),
}

// ─── Dimension State ─────────────────────────────────────────────────────────

/// State of a single dimension — fixed, free, or derived.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DimState {
    /// Dimension has been collapsed to a concrete value.
    Fixed(DimValue),
    /// Dimension is free with a set of options and an optional default.
    Free {
        /// Available choices.
        options: Vec<DimValue>,
        /// Default if none selected.
        default: Option<DimValue>,
    },
    /// Dimension value is derived from other dimensions.
    Derived {
        /// Names of dimensions this one depends on.
        depends_on: Vec<String>,
    },
}

// ─── Dimension ───────────────────────────────────────────────────────────────

/// A single dimension in the hypercube.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dimension {
    /// Unique name, e.g. "model.inventory.Product".
    pub name: String,
    /// Category for grouping during decomposition.
    pub category: DimCategory,
    /// Current state (fixed, free, or derived).
    pub state: DimState,
    /// Estimated lines of code this dimension contributes.
    pub complexity: u32,
    /// Human-readable description.
    pub description: String,
}

// ─── Coupling ────────────────────────────────────────────────────────────────

/// Direction of a coupling between two dimensions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CouplingDir {
    /// First dimension drives the second.
    Forward,
    /// Second dimension drives the first.
    Backward,
    /// Mutual dependency.
    Mutual,
}

/// A coupling (edge) between two dimensions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coupling {
    /// Source dimension name.
    pub from: String,
    /// Target dimension name.
    pub to: String,
    /// Coupling strength in [0.0, 1.0].
    pub strength: f64,
    /// Coupling direction.
    pub direction: CouplingDir,
}

// ─── HyperCube ───────────────────────────────────────────────────────────────

/// The hypercube: a multi-dimensional requirement space.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HyperCube {
    /// All dimensions in this cube.
    pub dimensions: Vec<Dimension>,
    /// All couplings between dimensions.
    pub couplings: Vec<Coupling>,
    /// Recursion depth (0 = root).
    pub depth: u32,
    /// Content-addressed signature of parent cube (if any).
    pub parent_signature: Option<String>,
    /// Whether entities were parsed from explicit `[[entities]]` in the TOML
    /// (D2 generic path) rather than detected from the domain registry.
    #[serde(default)]
    pub entities_from_toml: bool,
}

impl HyperCube {
    /// Degrees of freedom: number of dimensions that are not Fixed.
    pub fn dof(&self) -> usize {
        self.dimensions
            .iter()
            .filter(|d| !matches!(d.state, DimState::Fixed(_)))
            .count()
    }

    /// Names of all free (non-fixed) dimensions.
    pub fn free_dimensions(&self) -> Vec<String> {
        self.dimensions
            .iter()
            .filter(|d| !matches!(d.state, DimState::Fixed(_)))
            .map(|d| d.name.clone())
            .collect()
    }

    /// Extract all `EntityTemplate`s stored in model dimensions.
    ///
    /// Returns the entity templates that were injected during TOML parsing,
    /// enabling the CLI to build a `ForgePlan` directly from parsed entities.
    pub fn extract_entities(&self) -> Vec<EntityTemplate> {
        self.dimensions
            .iter()
            .filter(|d| d.name.starts_with("model."))
            .filter_map(|d| match &d.state {
                DimState::Free { default: Some(DimValue::EntityDef(et)), .. } => Some(et.clone()),
                _ => None,
            })
            .collect()
    }

    /// Build a `CouplingGraph` from the current dimensions and couplings.
    pub fn coupling_graph(&self) -> CouplingGraph {
        let names: Vec<String> = self.dimensions.iter().map(|d| d.name.clone()).collect();
        let name_idx: BTreeMap<&str, usize> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();

        let n = names.len();
        let mut edges = Vec::new();
        let mut adjacency: Vec<Vec<(usize, f64)>> = vec![vec![]; n];

        for c in &self.couplings {
            if let (Some(&i), Some(&j)) = (name_idx.get(c.from.as_str()), name_idx.get(c.to.as_str())) {
                edges.push((i, j, c.strength));
                adjacency[i].push((j, c.strength));
                if c.direction == CouplingDir::Mutual || c.direction == CouplingDir::Backward {
                    adjacency[j].push((i, c.strength));
                }
                // Ensure symmetry for spectral methods
                if c.direction == CouplingDir::Forward {
                    adjacency[j].push((i, c.strength));
                }
            }
        }

        CouplingGraph {
            nodes: names,
            edges,
            adjacency,
        }
    }

    /// Fix a dimension to a specific value.
    pub fn fix(&mut self, name: &str, value: DimValue) {
        if let Some(dim) = self.dimensions.iter_mut().find(|d| d.name == name) {
            dim.state = DimState::Fixed(value);
        }
    }

    /// Extract a sub-cube containing only the named dimensions and their mutual couplings.
    pub fn extract_subcube(&self, dim_names: &[String]) -> HyperCube {
        let name_set: std::collections::HashSet<&str> =
            dim_names.iter().map(|s| s.as_str()).collect();

        let dimensions: Vec<Dimension> = self
            .dimensions
            .iter()
            .filter(|d| name_set.contains(d.name.as_str()))
            .cloned()
            .collect();

        let couplings: Vec<Coupling> = self
            .couplings
            .iter()
            .filter(|c| name_set.contains(c.from.as_str()) && name_set.contains(c.to.as_str()))
            .cloned()
            .collect();

        HyperCube {
            dimensions,
            couplings,
            depth: self.depth + 1,
            parent_signature: Some(self.signature()),
            entities_from_toml: self.entities_from_toml,
        }
    }

    /// Content-addressed signature of this cube.
    pub fn signature(&self) -> String {
        let mut hasher = Sha256::new();
        // Hash dimension names and states deterministically
        for d in &self.dimensions {
            hasher.update(d.name.as_bytes());
            hasher.update(format!("{:?}", d.category).as_bytes());
            let state_tag = match &d.state {
                DimState::Fixed(_) => "F",
                DimState::Free { .. } => "R",
                DimState::Derived { .. } => "D",
            };
            hasher.update(state_tag.as_bytes());
        }
        for c in &self.couplings {
            hasher.update(c.from.as_bytes());
            hasher.update(c.to.as_bytes());
            hasher.update(c.strength.to_le_bytes());
        }
        hex_encode(&hasher.finalize())
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_cube_dof() {
        let cube = HyperCube {
            dimensions: vec![],
            couplings: vec![],
            depth: 0,
            parent_signature: None,
            entities_from_toml: false,
        };
        assert_eq!(cube.dof(), 0);
    }

    #[test]
    fn test_fix_reduces_dof() {
        let mut cube = HyperCube {
            dimensions: vec![
                Dimension {
                    name: "a".into(),
                    category: DimCategory::Architecture,
                    state: DimState::Free { options: vec![], default: None },
                    complexity: 10,
                    description: "test".into(),
                },
                Dimension {
                    name: "b".into(),
                    category: DimCategory::DataModel,
                    state: DimState::Free { options: vec![], default: None },
                    complexity: 20,
                    description: "test".into(),
                },
            ],
            couplings: vec![],
            depth: 0,
            parent_signature: None,
            entities_from_toml: false,
        };
        assert_eq!(cube.dof(), 2);
        cube.fix("a", DimValue::Choice("fixed".into()));
        assert_eq!(cube.dof(), 1);
    }

    #[test]
    fn test_signature_deterministic() {
        let cube = HyperCube {
            dimensions: vec![Dimension {
                name: "x".into(),
                category: DimCategory::Architecture,
                state: DimState::Fixed(DimValue::Choice("rust".into())),
                complexity: 0,
                description: "".into(),
            }],
            couplings: vec![],
            depth: 0,
            parent_signature: None,
            entities_from_toml: false,
        };
        assert_eq!(cube.signature(), cube.signature());
    }

    #[test]
    fn test_extract_subcube() {
        let cube = HyperCube {
            dimensions: vec![
                Dimension { name: "a".into(), category: DimCategory::Architecture, state: DimState::Free { options: vec![], default: None }, complexity: 0, description: "".into() },
                Dimension { name: "b".into(), category: DimCategory::DataModel, state: DimState::Free { options: vec![], default: None }, complexity: 0, description: "".into() },
                Dimension { name: "c".into(), category: DimCategory::Storage, state: DimState::Free { options: vec![], default: None }, complexity: 0, description: "".into() },
            ],
            couplings: vec![
                Coupling { from: "a".into(), to: "b".into(), strength: 0.9, direction: CouplingDir::Forward },
                Coupling { from: "b".into(), to: "c".into(), strength: 0.8, direction: CouplingDir::Forward },
                Coupling { from: "a".into(), to: "c".into(), strength: 0.5, direction: CouplingDir::Forward },
            ],
            depth: 0,
            parent_signature: None,
            entities_from_toml: false,
        };
        let sub = cube.extract_subcube(&["a".into(), "b".into()]);
        assert_eq!(sub.dimensions.len(), 2);
        assert_eq!(sub.couplings.len(), 1); // only a→b
        assert_eq!(sub.depth, 1);
    }

    #[test]
    fn test_domain_registry_warehouse() {
        let registry = DomainRegistry::new();
        let wh = registry.get("warehouse").expect("warehouse domain should exist");
        assert_eq!(wh.entities.len(), 7);
        let product = wh.entities.iter().find(|e| e.name == "Product").unwrap();
        assert!(product.fields.len() >= 15, "Product should have ≥15 fields, got {}", product.fields.len());
        assert_eq!(wh.business_rules.len(), 5);
        assert_eq!(wh.relationships.len(), 7);
    }

    #[test]
    fn test_parse_warehouse_toml() {
        let toml_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap()
            .parent().unwrap()
            .join("examples/warehouse.toml");
        if !toml_path.exists() {
            // Skip if warehouse.toml not found in expected location
            return;
        }
        let cube = parser::parse_toml_to_cube(&toml_path).unwrap();
        assert!(cube.dimensions.len() >= 40, "Expected ≥40 dims, got {}", cube.dimensions.len());
        assert!(cube.couplings.len() >= 50, "Expected ≥50 couplings, got {}", cube.couplings.len());
        assert!(cube.dof() >= 30, "Expected ≥30 DOF, got {}", cube.dof());
    }

    #[test]
    fn test_coupling_graph_fiedler() {
        // Simple 4-node path graph: 0-1-2-3
        let graph = CouplingGraph {
            nodes: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            edges: vec![(0,1,1.0),(1,2,1.0),(2,3,1.0)],
            adjacency: vec![
                vec![(1, 1.0)],
                vec![(0, 1.0), (2, 1.0)],
                vec![(1, 1.0), (3, 1.0)],
                vec![(2, 1.0)],
            ],
        };
        let fiedler = graph.fiedler();
        assert!(fiedler.value > 0.0, "Fiedler value should be positive for connected graph");
        assert_eq!(fiedler.vector.len(), 4);
    }

    #[test]
    fn test_fiedler_bisect() {
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
        let (left, right) = graph.fiedler_bisect();
        assert!(!left.is_empty());
        assert!(!right.is_empty());
        assert_eq!(left.len() + right.len(), 4);
    }

    #[test]
    fn test_kuramoto_groups() {
        let graph = CouplingGraph {
            nodes: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            edges: vec![(0,1,1.0),(2,3,1.0)],
            adjacency: vec![
                vec![(1, 1.0)],
                vec![(0, 1.0)],
                vec![(3, 1.0)],
                vec![(2, 1.0)],
            ],
        };
        let groups = graph.kuramoto_groups(2.0, 0.8);
        // Should find at least 2 groups: {a,b} and {c,d}
        assert!(groups.len() >= 2, "Expected ≥2 Kuramoto groups, got {}", groups.len());
    }
}
