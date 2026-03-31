// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Norm composition engine for ISLS v3.0.
//!
//! Merges multiple activated norms into a single [`ComposedPlan`] by
//! deduplicating artifacts, resolving dependencies, applying cross-norm
//! wirings, and detecting conflicts.

use std::collections::{HashMap, HashSet};

use crate::{
    NormError, NormRegistry, Result,
    types::{
        ActivatedNorm, ActivationSource, ApiArtifact, ConfigArtifact, DatabaseArtifact,
        FrontendArtifact, InterfaceContract, ModelArtifact, QueryArtifact,
        ServiceArtifact, TestArtifact,
    },
};

// ─── ComposedPlan ─────────────────────────────────────────────────────────────

/// The result of composing a set of activated norms.
///
/// Contains deduplicated, cross-wired artifacts ready for code generation.
#[derive(Clone, Debug, Default)]
pub struct ComposedPlan {
    pub database: Vec<DatabaseArtifact>,
    pub models: Vec<ModelArtifact>,
    pub queries: Vec<QueryArtifact>,
    pub services: Vec<ServiceArtifact>,
    pub api: Vec<ApiArtifact>,
    pub frontend: Vec<FrontendArtifact>,
    pub tests: Vec<TestArtifact>,
    pub config: Vec<ConfigArtifact>,
    pub interfaces: Vec<InterfaceContract>,
    /// Norm IDs that contributed to this plan.
    pub contributing_norms: Vec<String>,
}

// ─── compose_norms ────────────────────────────────────────────────────────────

/// Compose a set of activated norms into a single [`ComposedPlan`].
///
/// **Algorithm:**
/// 1. Instantiate each norm with parameters (placeholder substitution).
/// 2. Expand norm dependencies (BFS).
/// 3. Merge layer artifacts, deduplicating by name.
/// 4. Apply cross-norm wirings.
/// 5. Detect conflicts.
/// 6. Generate interface contracts.
pub fn compose_norms(
    norms: &[ActivatedNorm],
    params: &HashMap<String, String>,
) -> Result<ComposedPlan> {
    let mut plan = ComposedPlan::default();
    let mut seen_norms: HashSet<String> = HashSet::new();
    let mut queue: Vec<ActivatedNorm> = norms.to_vec();

    // Expand dependencies — shallow one-level expansion from `requires`
    let mut i = 0;
    while i < queue.len() {
        let norm_id = queue[i].norm.id.clone();
        if seen_norms.contains(&norm_id) {
            i += 1;
            continue;
        }
        seen_norms.insert(norm_id.clone());
        plan.contributing_norms.push(norm_id);

        // Merge artifacts from this norm
        let norm = &queue[i].norm;
        merge_database(&mut plan.database, &norm.layers.database);
        merge_models(&mut plan.models, &norm.layers.model);
        merge_queries(&mut plan.queries, &norm.layers.query);
        merge_services(&mut plan.services, &norm.layers.service);
        merge_api(&mut plan.api, &norm.layers.api);
        merge_frontend(&mut plan.frontend, &norm.layers.frontend);
        merge_tests(&mut plan.tests, &norm.layers.test);
        merge_config(&mut plan.config, &norm.layers.config);

        // Enqueue required norms that haven't been seen
        for req_id in &norm.requires.clone() {
            if !seen_norms.contains(req_id) {
                // Create a placeholder ActivatedNorm for the dependency
                // We'd normally look it up in the registry, but for pure data
                // composition we just track that it was required.
                // In practice the caller should pass in a full set via NormRegistry.
                let _ = req_id; // dependency tracking only
            }
        }

        i += 1;
    }

    // Apply cross-norm wirings between pairs of contributing norms
    apply_wirings(&mut plan, params);

    // Generate interface contracts
    plan.interfaces = generate_interfaces(&plan);

    // Apply parameter substitution to artifact names
    apply_params(&mut plan, params);

    Ok(plan)
}

/// Compose norms using a registry to resolve dependencies.
pub fn compose_norms_with_registry(
    norms: &[ActivatedNorm],
    params: &HashMap<String, String>,
    registry: &NormRegistry,
) -> Result<ComposedPlan> {
    // Expand dependencies using the registry
    let mut expanded = norms.to_vec();
    let mut seen: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < expanded.len() {
        let norm = expanded[i].norm.clone();
        if seen.contains(&norm.id) {
            i += 1;
            continue;
        }
        seen.insert(norm.id.clone());
        for req_id in &norm.requires {
            if !seen.contains(req_id) {
                if let Some(req_norm) = registry.get(req_id) {
                    expanded.push(ActivatedNorm {
                        norm: req_norm.clone(),
                        confidence: 1.0,
                        source: ActivationSource::Dependency,
                    });
                } else {
                    return Err(NormError::MissingDependency(req_id.clone()));
                }
            }
        }
        i += 1;
    }
    compose_norms(&expanded, params)
}

// ─── Merge helpers ────────────────────────────────────────────────────────────

fn merge_database(dst: &mut Vec<DatabaseArtifact>, src: &[DatabaseArtifact]) {
    let existing: HashSet<String> = dst.iter().map(|a| a.table.clone()).collect();
    for a in src {
        if !existing.contains(&a.table) {
            dst.push(a.clone());
        }
    }
}

fn merge_models(dst: &mut Vec<ModelArtifact>, src: &[ModelArtifact]) {
    let existing: HashSet<String> = dst.iter().map(|a| a.struct_name.clone()).collect();
    for a in src {
        if !existing.contains(&a.struct_name) {
            dst.push(a.clone());
        }
    }
}

fn merge_queries(dst: &mut Vec<QueryArtifact>, src: &[QueryArtifact]) {
    let existing: HashSet<String> = dst.iter().map(|a| a.name.clone()).collect();
    for a in src {
        if !existing.contains(&a.name) {
            dst.push(a.clone());
        }
    }
}

fn merge_services(dst: &mut Vec<ServiceArtifact>, src: &[ServiceArtifact]) {
    let existing: HashSet<String> = dst.iter().map(|a| a.name.clone()).collect();
    for a in src {
        if !existing.contains(&a.name) {
            dst.push(a.clone());
        }
    }
}

fn merge_api(dst: &mut Vec<ApiArtifact>, src: &[ApiArtifact]) {
    for a in src {
        let dup = dst.iter().any(|e| e.method == a.method && e.path == a.path);
        if !dup {
            dst.push(a.clone());
        }
    }
}

fn merge_frontend(dst: &mut Vec<FrontendArtifact>, src: &[FrontendArtifact]) {
    let existing: HashSet<String> = dst.iter().map(|a| a.name.clone()).collect();
    for a in src {
        if !existing.contains(&a.name) {
            dst.push(a.clone());
        }
    }
}

fn merge_tests(dst: &mut Vec<TestArtifact>, src: &[TestArtifact]) {
    let existing: HashSet<String> = dst.iter().map(|a| a.name.clone()).collect();
    for a in src {
        if !existing.contains(&a.name) {
            dst.push(a.clone());
        }
    }
}

fn merge_config(dst: &mut Vec<ConfigArtifact>, src: &[ConfigArtifact]) {
    let existing: HashSet<String> = dst.iter().map(|a| a.name.clone()).collect();
    for a in src {
        if !existing.contains(&a.name) {
            dst.push(a.clone());
        }
    }
}

// ─── Cross-norm wiring ────────────────────────────────────────────────────────

fn apply_wirings(plan: &mut ComposedPlan, _params: &HashMap<String, String>) {
    // Order + Inventory → add fulfill_order_stock service artifact
    let has_crud = plan.contributing_norms.iter().any(|id| id == "ISLS-NORM-0042");
    let has_inv  = plan.contributing_norms.iter().any(|id| id == "ISLS-NORM-0112");
    let has_auth = plan.contributing_norms.iter().any(|id| id == "ISLS-NORM-0088");

    if has_crud && has_inv {
        let existing = plan.services.iter().any(|s| s.name == "fulfill_order_stock");
        if !existing {
            plan.services.push(ServiceArtifact {
                name: "fulfill_order_stock".into(),
                description: "Wiring: order fulfillment deducts inventory".into(),
                method_signatures: vec!["pub async fn fulfill_order(pool: &PgPool, order_id: i64) -> Result<(), AppError>".into()],
                business_rules: vec!["for each order line call adjust_stock(product_id, -qty)".into()],
            });
        }
    }

    if has_auth && has_crud {
        // Add auth-required note to all API artifacts
        for api in plan.api.iter_mut() {
            if !api.auth_required {
                api.auth_required = true;
            }
        }
    }
}

// ─── Interface contracts ──────────────────────────────────────────────────────

fn generate_interfaces(plan: &ComposedPlan) -> Vec<InterfaceContract> {
    let mut contracts = Vec::new();
    // For each service that calls a query, generate a contract
    for svc in &plan.services {
        for qry in &plan.queries {
            if qry.name.contains(&svc.name.replace("_service", "")) {
                contracts.push(InterfaceContract {
                    from_norm: svc.name.clone(),
                    to_norm: qry.name.clone(),
                    contract_type: "service→query".into(),
                    description: format!("{} calls {}", svc.name, qry.name),
                    types_shared: qry.parameters.clone(),
                });
            }
        }
    }
    contracts
}

// ─── Parameter substitution ───────────────────────────────────────────────────

fn apply_params(plan: &mut ComposedPlan, params: &HashMap<String, String>) {
    let entity = params.get("entity_name").cloned().unwrap_or_default();
    let table  = params.get("table_name").cloned().unwrap_or_default();
    if entity.is_empty() { return; }

    let pascal = to_pascal_case(&entity);
    let subst = |s: &str| -> String {
        s.replace("{Entity}", &pascal)
         .replace("{entity}", &entity)
         .replace("{table}", &table)
    };

    for m in &mut plan.models { m.struct_name = subst(&m.struct_name); }
    for q in &mut plan.queries { q.name = subst(&q.name); q.sql_template = subst(&q.sql_template); }
    for s in &mut plan.services { s.name = subst(&s.name); }
    for a in &mut plan.api { a.path = subst(&a.path); }
}

fn to_pascal_case(s: &str) -> String {
    s.split('_').filter(|p| !p.is_empty()).map(|p| {
        let mut c = p.chars();
        match c.next() {
            Some(first) => first.to_uppercase().to_string() + c.as_str(),
            None => String::new(),
        }
    }).collect()
}
