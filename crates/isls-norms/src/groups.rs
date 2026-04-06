// isls-norms/src/groups.rs — N1/W2: Valence + Norm Groups
//
// Valence = number of compatible relations a norm has.
// Groups = sets of mutually conflicting norms that share dependencies
// (like JWT vs Session — alternatives for the same role).

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::relations::{NormRelationMatrix, RelationType};
use crate::types::Norm;

// ─── Types ──────────────────────────────────────────────────────────────────

/// A group of exchangeable norms (alternatives for the same role).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormGroup {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
    pub shared_deps: Vec<String>,
    pub role: String,
}

// ─── Valence ────────────────────────────────────────────────────────────────

/// Number of compatible relations for a given norm.
pub fn compute_valence(norm_id: &str, relations: &NormRelationMatrix) -> usize {
    relations.relations.iter()
        .filter(|r|
            (r.norm_a == norm_id || r.norm_b == norm_id) &&
            r.relation == RelationType::Compatible
        )
        .count()
}

// ─── Helper: get dependencies for a norm ────────────────────────────────────

fn get_dependencies(norm_id: &str, relations: &NormRelationMatrix) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();
    for r in &relations.relations {
        if r.relation == RelationType::Dependent {
            if r.norm_a == norm_id {
                deps.insert(r.norm_b.clone());
            }
        }
    }
    deps
}

/// Infer a human-readable group name from member norm names.
fn infer_group_name(member_ids: &[String], norms: &[Norm]) -> String {
    let names: Vec<&str> = member_ids.iter()
        .filter_map(|id| norms.iter().find(|n| n.id == *id).map(|n| n.name.as_str()))
        .collect();
    if names.is_empty() {
        return "Unknown".to_string();
    }
    // Try to find common suffix/prefix in names
    let first = names[0];
    // Simple heuristic: use the last word common across names, or just join them
    let words: Vec<&str> = first.split(|c: char| c == '-' || c == '_' || c == ' ').collect();
    for word in words.iter().rev() {
        if names.iter().all(|n| n.to_lowercase().contains(&word.to_lowercase())) && word.len() > 2 {
            return word.to_string();
        }
    }
    names.join(" / ")
}

// ─── Group detection ────────────────────────────────────────────────────────

pub fn detect_groups(norms: &[Norm], relations: &NormRelationMatrix) -> Vec<NormGroup> {
    let mut groups = Vec::new();
    let mut assigned: BTreeSet<String> = BTreeSet::new();

    for i in 0..norms.len() {
        if assigned.contains(&norms[i].id) { continue; }

        let mut group_members = vec![norms[i].id.clone()];

        for j in (i + 1)..norms.len() {
            if assigned.contains(&norms[j].id) { continue; }

            // Check: does j conflict with all current members?
            let conflicts_with_all = group_members.iter().all(|m| {
                relations.relations.iter().any(|r| {
                    r.relation == RelationType::Conflicting &&
                    ((r.norm_a == *m && r.norm_b == norms[j].id) ||
                     (r.norm_b == *m && r.norm_a == norms[j].id))
                })
            });

            // AND: shares the same dependencies
            let same_deps = get_dependencies(&norms[i].id, relations) ==
                           get_dependencies(&norms[j].id, relations);

            if conflicts_with_all && same_deps {
                group_members.push(norms[j].id.clone());
            }
        }

        if group_members.len() > 1 {
            let shared_deps: Vec<String> = get_dependencies(&group_members[0], relations)
                .into_iter().collect();
            let name = infer_group_name(&group_members, norms);

            for m in &group_members {
                assigned.insert(m.clone());
            }

            groups.push(NormGroup {
                id: format!("GROUP-{}", groups.len() + 1),
                name,
                members: group_members,
                shared_deps,
                role: String::new(),
            });
        }
    }

    groups
}

// ─── Persistence ────────────────────────────────────────────────────────────

fn groups_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(|h| PathBuf::from(h).join(".isls").join("groups.json"))
}

pub fn save_groups(groups: &[NormGroup], path: Option<&std::path::Path>) -> std::io::Result<()> {
    let p = match path {
        Some(p) => p.to_path_buf(),
        None => match groups_path() {
            Some(p) => p,
            None => return Ok(()),
        },
    };
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(groups).unwrap_or_default())?;
    Ok(())
}

pub fn load_groups(path: Option<&std::path::Path>) -> Option<Vec<NormGroup>> {
    let p = match path {
        Some(p) => p.to_path_buf(),
        None => groups_path()?,
    };
    let content = std::fs::read_to_string(&p).ok()?;
    serde_json::from_str(&content).ok()
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::builtin_norms;
    use crate::relations::compute_relations;

    #[test]
    fn test_valence_for_popular_norm() {
        let norms = builtin_norms();
        let matrix = compute_relations(&norms);
        // CRUD-Entity should have some compatible relations
        let v = compute_valence("ISLS-NORM-0042", &matrix);
        // It's okay if valence is 0 (few norms with shared keywords),
        // but it should not panic.
        assert!(v < norms.len(), "Valence should be bounded");
    }

    #[test]
    fn test_detect_groups_no_panic() {
        let norms = builtin_norms();
        let matrix = compute_relations(&norms);
        let groups = detect_groups(&norms, &matrix);
        // Groups may be empty if no conflicts exist — that's fine.
        // Each group should have at least 2 members.
        for g in &groups {
            assert!(g.members.len() >= 2,
                "Group {} should have >= 2 members, got {}", g.id, g.members.len());
        }
    }
}
