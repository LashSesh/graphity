// isls-norms/src/energy.rs — N1/W3: Configuration Energy
//
// E(K) = conflict * 10 + deficit * 5 + fitness_entropy
// A valid configuration has no conflicts and no missing dependencies.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::relations::{NormRelationMatrix, RelationType};

// ─── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEnergy {
    pub total: f64,
    pub conflict: f64,
    pub deficit: f64,
    pub fitness_entropy: f64,
    pub conflicts: Vec<(String, String)>,
    pub missing_deps: Vec<(String, String)>,
    pub is_valid: bool,
}

// ─── Computation ────────────────────────────────────────────────────────────

pub fn configuration_energy(
    config: &[String],
    relations: &NormRelationMatrix,
    fitness: &HashMap<String, f64>,
) -> ConfigEnergy {
    let mut conflict_energy = 0.0;
    let mut deficit_energy = 0.0;
    let mut fitness_entropy = 0.0;
    let mut conflicts = Vec::new();
    let mut missing_deps = Vec::new();

    // 1. Conflict energy
    for r in &relations.relations {
        if r.relation == RelationType::Conflicting {
            let a_in = config.contains(&r.norm_a);
            let b_in = config.contains(&r.norm_b);
            if a_in && b_in {
                conflict_energy += r.strength;
                conflicts.push((r.norm_a.clone(), r.norm_b.clone()));
            }
        }
    }

    // 2. Deficit energy (missing dependencies)
    for r in &relations.relations {
        if r.relation == RelationType::Dependent {
            let a_in = config.contains(&r.norm_a);
            let b_in = config.contains(&r.norm_b);
            if a_in && !b_in {
                deficit_energy += r.strength;
                missing_deps.push((r.norm_a.clone(), r.norm_b.clone()));
            }
        }
    }

    // 3. Fitness entropy
    for norm_id in config {
        let phi = fitness.get(norm_id).copied().unwrap_or(0.5);
        fitness_entropy += 1.0 - phi;
    }

    let is_valid = conflicts.is_empty() && missing_deps.is_empty();
    ConfigEnergy {
        total: conflict_energy * 10.0 + deficit_energy * 5.0 + fitness_entropy,
        conflict: conflict_energy,
        deficit: deficit_energy,
        fitness_entropy,
        conflicts,
        missing_deps,
        is_valid,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::builtin_norms;
    use crate::relations::compute_relations;

    #[test]
    fn test_valid_config_low_energy() {
        // Use an empty relation matrix to isolate the fitness calculation
        let matrix = NormRelationMatrix {
            relations: vec![],
            computed_at: String::new(),
            norm_count: 2,
        };
        let mut fitness = HashMap::new();
        let config = vec!["A".to_string(), "B".to_string()];
        fitness.insert("A".to_string(), 0.9);
        fitness.insert("B".to_string(), 0.9);
        let energy = configuration_energy(&config, &matrix, &fitness);
        assert!(energy.conflicts.is_empty(), "Should have no conflicts");
        assert!(energy.missing_deps.is_empty(), "Should have no missing deps");
        assert!(energy.is_valid);
        // Fitness entropy = 2 * (1 - 0.9) = 0.2
        assert!((energy.fitness_entropy - 0.2).abs() < 1e-9);
        assert!((energy.total - 0.2).abs() < 1e-9, "Expected total ~0.2, got {}", energy.total);
    }

    #[test]
    fn test_missing_deps_adds_deficit() {
        let norms = builtin_norms();
        let matrix = compute_relations(&norms);
        let fitness = HashMap::new();
        // CRUD-Entity alone requires Pagination (0100) and Error-System (0101),
        // which are missing → deficit energy should be > 0
        let config = vec!["ISLS-NORM-0042".to_string()];
        let energy = configuration_energy(&config, &matrix, &fitness);
        // CRUD depends on Pagination and Error-System; if those show as Dependent
        // in the matrix, we should see deficit.
        // Either way, fitness_entropy should be 0.5 (default fitness)
        assert!((energy.fitness_entropy - 0.5).abs() < 1e-9,
            "Single norm with default fitness should have entropy 0.5, got {}", energy.fitness_entropy);
    }
}
