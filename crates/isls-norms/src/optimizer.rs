// isls-norms/src/optimizer.rs — N1/W4: Greedy Norm Optimizer
//
// Given requirements (ResoniteClasses), find the optimal conflict-free
// configuration with all dependencies satisfied. Greedy by fitness.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::energy::{configuration_energy, ConfigEnergy};
use crate::groups::NormGroup;
use crate::relations::{NormRelationMatrix, RelationType};
use crate::spectroscopy::ResoniteClass;
use crate::types::Norm;

// ─── Types ───────────────────────────────────────────���──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimalConfig {
    pub norms: Vec<String>,
    pub covered: Vec<String>,
    pub uncovered: Vec<String>,
    pub energy: ConfigEnergy,
}

// ─── Requirement extraction ──��──────────────────────────────────────────────

/// Simple entity definition for requirement extraction (avoids depending on
/// isls-forge-llm's EntityDef).
pub struct SimpleEntity {
    pub name: String,
    pub field_names: Vec<String>,
}

pub fn extract_requirements_from_entities(entities: &[SimpleEntity]) -> Vec<ResoniteClass> {
    let mut reqs = vec![ResoniteClass::CrudEntity];
    reqs.push(ResoniteClass::Authentication);

    if entities.len() > 1 {
        reqs.push(ResoniteClass::Pagination);
    }

    for entity in entities {
        for field_name in &entity.field_names {
            let name = field_name.to_lowercase();
            if name.contains("status") || name.contains("state") {
                reqs.push(ResoniteClass::StateMachine);
            }
            if name.contains("file") || name.contains("image") || name.contains("attachment") {
                reqs.push(ResoniteClass::FileUpload);
            }
            if name.contains("email") {
                reqs.push(ResoniteClass::Notification);
            }
        }
    }

    reqs.sort();
    reqs.dedup();
    reqs
}

// ─── Norm classification ────────────────────────────────────────────────────

/// Classify which ResoniteClasses a norm covers, based on keywords/concepts.
fn classify_norm(norm: &Norm) -> BTreeSet<String> {
    let mut classes = BTreeSet::new();
    let all_keywords: Vec<String> = norm.triggers.iter()
        .flat_map(|t| t.keywords.iter().chain(t.concepts.iter()))
        .cloned()
        .collect();

    let joined = all_keywords.join(" ").to_lowercase();

    let mappings: &[(&str, &str)] = &[
        ("crud", "CrudEntity"),
        ("entity", "CrudEntity"),
        ("auth", "Authentication"),
        ("jwt", "Authentication"),
        ("login", "Authentication"),
        ("session", "Authentication"),
        ("pagination", "Pagination"),
        ("page", "Pagination"),
        ("search", "Search"),
        ("file", "FileUpload"),
        ("upload", "FileUpload"),
        ("state machine", "StateMachine"),
        ("status", "StateMachine"),
        ("workflow", "Workflow"),
        ("notification", "Notification"),
        ("email", "Notification"),
        ("websocket", "RealtimeWebSocket"),
        ("realtime", "RealtimeWebSocket"),
        ("cache", "Caching"),
        ("rate limit", "RateLimiting"),
        ("graphql", "GraphQLApi"),
        ("export", "ExportImport"),
        ("import", "ExportImport"),
        ("schedule", "Scheduling"),
        ("cron", "Scheduling"),
        ("health", "HealthCheck"),
        ("log", "Logging"),
        ("metric", "Metrics"),
        ("docker", "Docker"),
        ("migration", "Migration"),
        ("error", "Configuration"),
        ("config", "Configuration"),
        ("database", "Configuration"),
    ];

    for (keyword, class) in mappings {
        if joined.contains(keyword) {
            classes.insert(class.to_string());
        }
    }

    classes
}

// ─── Transitive closure ���────────────────────────────────────────────────────

/// Compute all transitive dependencies for a norm.
pub fn transitive_closure(norm_id: &str, relations: &NormRelationMatrix) -> Vec<String> {
    let mut deps = Vec::new();
    let mut visited = BTreeSet::new();
    let mut stack = vec![norm_id.to_string()];

    while let Some(current) = stack.pop() {
        for r in &relations.relations {
            if r.relation == RelationType::Dependent && r.norm_a == current {
                if visited.insert(r.norm_b.clone()) {
                    deps.push(r.norm_b.clone());
                    stack.push(r.norm_b.clone());
                }
            }
        }
    }

    deps
}

// ─── Resolve group conflicts ────────────────────────────────────────────────

fn resolve_group_conflicts(
    selected: &mut Vec<String>,
    groups: &[NormGroup],
    fitness: &HashMap<String, f64>,
) {
    for group in groups {
        let in_selected: Vec<String> = group.members.iter()
            .filter(|m| selected.contains(m))
            .cloned()
            .collect();

        if in_selected.len() > 1 {
            // Keep the one with highest fitness
            let best = in_selected.iter()
                .max_by(|a, b| {
                    let fa = fitness.get(a.as_str()).unwrap_or(&0.5);
                    let fb = fitness.get(b.as_str()).unwrap_or(&0.5);
                    fa.partial_cmp(fb).unwrap_or(std::cmp::Ordering::Equal)
                })
                .cloned()
                .unwrap();

            for m in &in_selected {
                if *m != best {
                    selected.retain(|s| s != m);
                }
            }
        }
    }
}

// ─── Greedy optimizer ─────────��─────────────────────────────────────────────

pub fn optimize_configuration(
    requirements: &[ResoniteClass],
    norms: &[Norm],
    relations: &NormRelationMatrix,
    fitness: &HashMap<String, f64>,
    groups: &[NormGroup],
) -> OptimalConfig {
    if norms.is_empty() || requirements.is_empty() {
        return OptimalConfig {
            norms: vec![],
            covered: vec![],
            uncovered: requirements.iter().map(|r| r.as_str()).collect(),
            energy: ConfigEnergy {
                total: 0.0,
                conflict: 0.0,
                deficit: 0.0,
                fitness_entropy: 0.0,
                conflicts: vec![],
                missing_deps: vec![],
                is_valid: true,
            },
        };
    }

    let req_strings: BTreeSet<String> = requirements.iter().map(|r| r.as_str()).collect();
    let mut selected: Vec<String> = Vec::new();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut remaining: BTreeSet<String> = req_strings.clone();

    // Sort norms by fitness (descending)
    let mut candidates: Vec<(&Norm, f64)> = norms.iter()
        .map(|n| (n, *fitness.get(&n.id).unwrap_or(&0.5)))
        .collect();
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    while !remaining.is_empty() {
        let mut best: Option<(&Norm, usize)> = None;

        for (norm, _phi) in &candidates {
            if selected.contains(&norm.id) { continue; }

            // Check conflicts with selected norms
            let has_conflict = selected.iter().any(|s| {
                relations.relations.iter().any(|r| {
                    r.relation == RelationType::Conflicting &&
                    ((r.norm_a == *s && r.norm_b == norm.id) ||
                     (r.norm_b == *s && r.norm_a == norm.id))
                })
            });
            if has_conflict { continue; }

            let norm_classes = classify_norm(norm);
            let covers: usize = remaining.intersection(&norm_classes).count();

            if covers > 0 {
                if best.is_none() || covers > best.unwrap().1 {
                    best = Some((norm, covers));
                }
            }
        }

        match best {
            Some((norm, _)) => {
                selected.push(norm.id.clone());
                let norm_classes = classify_norm(norm);
                for c in &norm_classes {
                    remaining.remove(c);
                    covered.insert(c.clone());
                }

                // Add transitive dependencies
                let deps = transitive_closure(&norm.id, relations);
                for dep in deps {
                    if !selected.contains(&dep) {
                        selected.push(dep);
                    }
                }
            }
            None => break,
        }
    }

    resolve_group_conflicts(&mut selected, groups, fitness);

    let energy = configuration_energy(&selected, relations, fitness);

    OptimalConfig {
        norms: selected,
        covered: covered.into_iter().collect(),
        uncovered: remaining.into_iter().collect(),
        energy,
    }
}

// ─── Tests ───────────────────────────────��──────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::builtin_norms;
    use crate::groups::detect_groups;
    use crate::relations::compute_relations;

    #[test]
    fn test_optimize_basic() {
        let norms = builtin_norms();
        let matrix = compute_relations(&norms);
        let groups = detect_groups(&norms, &matrix);
        let fitness = HashMap::new(); // all default 0.5

        let requirements = vec![ResoniteClass::CrudEntity, ResoniteClass::Authentication];
        let result = optimize_configuration(&requirements, &norms, &matrix, &fitness, &groups);

        assert!(!result.norms.is_empty(), "Should select at least one norm");
        assert!(!result.covered.is_empty(), "Should cover at least one requirement");
    }

    #[test]
    fn test_optimize_empty_norms() {
        let norms: Vec<Norm> = vec![];
        let matrix = compute_relations(&norms);
        let groups = vec![];
        let fitness = HashMap::new();

        let requirements = vec![ResoniteClass::CrudEntity];
        let result = optimize_configuration(&requirements, &norms, &matrix, &fitness, &groups);

        assert!(result.norms.is_empty());
        assert_eq!(result.uncovered.len(), 1);
    }
}
