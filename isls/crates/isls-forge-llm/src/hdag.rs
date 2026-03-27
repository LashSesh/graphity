// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Codegen-HDAG: ISLS v3.4 PCR-Conformant HDAG structures and algorithms.
//!
//! Each source file is a node. Each dependency is a typed edge carrying
//! [`ProvidedSymbol`] structs. Topological traversal ensures every LLM node
//! receives exact import paths and type signatures — no guessing.

use std::collections::BTreeSet;

use crate::{AppSpec, EntityDef};
use crate::provided;

// ─── Symbol types ─────────────────────────────────────────────────────────────

/// Classification of a provided symbol.
#[derive(Clone, Debug)]
pub enum SymbolKind {
    Enum,
    Struct,
    Trait,
    Function,
    Module,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymbolKind::Enum => write!(f, "Enum"),
            SymbolKind::Struct => write!(f, "Struct"),
            SymbolKind::Trait => write!(f, "Trait"),
            SymbolKind::Function => write!(f, "Function"),
            SymbolKind::Module => write!(f, "Module"),
        }
    }
}

/// A symbol that one HDAG node provides to its successors.
///
/// The import path and signature are injected verbatim into LLM prompts,
/// eliminating guessing of import paths and type definitions.
#[derive(Clone, Debug)]
pub struct ProvidedSymbol {
    /// Exact crate-relative import path, e.g. `"crate::errors::AppError"`.
    pub import_path: String,
    /// Kind of the symbol (Enum, Struct, Function, …).
    pub kind: SymbolKind,
    /// Full type definition / signature injected into the LLM prompt.
    pub signature: String,
}

// ─── Node types ───────────────────────────────────────────────────────────────

/// Classification of a HDAG node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeType {
    /// Deterministic — written directly from entity list, no LLM.
    Structural,
    /// LLM-generated — prompt is built from predecessor-provided symbols.
    Llm,
}

/// A single file node in the Codegen-HDAG.
#[derive(Clone, Debug)]
pub struct HdagNode {
    /// Index in `CodegenHdag::nodes`.
    pub index: usize,
    /// Relative path (e.g. `"backend/src/errors.rs"`).
    pub path: String,
    /// Whether this node is deterministic or LLM-generated.
    pub node_type: NodeType,
    /// Generation layer (0 = structural/static, 1–9 = LLM layers).
    pub layer: u8,
    /// Entity name this file primarily serves (if applicable).
    pub entity: Option<String>,
    /// Whether this is a Rust source file (triggers TypeContext update).
    pub is_rust: bool,
    /// Brief purpose description used in LLM prompts.
    pub purpose: String,
}

/// A typed dependency edge in the Codegen-HDAG.
#[derive(Clone, Debug)]
pub struct HdagEdge {
    /// Index of the source node.
    pub from: usize,
    /// Index of the target node.
    pub to: usize,
    /// Symbols provided by `from` to `to`.
    pub provides: Vec<ProvidedSymbol>,
}

// ─── CodegenHdag ──────────────────────────────────────────────────────────────

/// The Codegen-HDAG: a deterministic, typed dependency graph over source files.
///
/// Built from an [`AppSpec`] via [`CodegenHdag::build`]. Every LLM node receives
/// exactly the symbols propagated by its predecessor edges — no import guessing.
pub struct CodegenHdag {
    pub nodes: Vec<HdagNode>,
    pub edges: Vec<HdagEdge>,
}

impl CodegenHdag {
    /// Build the Codegen-HDAG deterministically from the given [`AppSpec`].
    ///
    /// Creates nodes for every file in the generated project (both Structural
    /// and LLM) and wires typed edges with [`ProvidedSymbol`] sets.
    pub fn build(spec: &AppSpec) -> Self {
        let mut nodes: Vec<HdagNode> = Vec::new();
        let mut edges: Vec<HdagEdge> = Vec::new();

        // ── Helper: add a node ────────────────────────────────────────────────
        let mut add_node = |path: &str, node_type: NodeType, layer: u8,
                             entity: Option<String>, is_rust: bool,
                             purpose: &str| -> usize {
            let idx = nodes.len();
            nodes.push(HdagNode {
                index: idx,
                path: path.to_string(),
                node_type,
                layer,
                entity,
                is_rust,
                purpose: purpose.to_string(),
            });
            idx
        };

        // ── Layer 0: Structural / static files ───────────────────────────────
        // These are all written deterministically — no LLM token cost.

        let _cargo_toml    = add_node("backend/Cargo.toml",          NodeType::Structural, 0, None, false, "Workspace Cargo.toml");
        let _dockerfile    = add_node("backend/Dockerfile",          NodeType::Structural, 0, None, false, "Multi-stage Dockerfile");
        let _compose       = add_node("docker-compose.yml",          NodeType::Structural, 0, None, false, "Docker Compose orchestration");
        let _env_example   = add_node(".env.example",                NodeType::Structural, 0, None, false, "Environment template");
        let _gitignore     = add_node(".gitignore",                  NodeType::Structural, 0, None, false, "Git ignore rules");
        let _nginx         = add_node("frontend/nginx.conf",         NodeType::Structural, 0, None, false, "Nginx reverse proxy config");
        let _migration     = add_node("backend/migrations/001_initial.sql", NodeType::Structural, 0, None, false, "Initial database migration");
        let _main_rs       = add_node("backend/src/main.rs",         NodeType::Structural, 0, None, true,  "Application entry point");
        let _models_mod    = add_node("backend/src/models/mod.rs",   NodeType::Structural, 0, None, true,  "Models module declarations");
        let _db_mod        = add_node("backend/src/database/mod.rs", NodeType::Structural, 0, None, true,  "Database module declarations");
        let _svc_mod       = add_node("backend/src/services/mod.rs", NodeType::Structural, 0, None, true,  "Services module declarations");
        let _api_mod       = add_node("backend/src/api/mod.rs",      NodeType::Structural, 0, None, true,  "API module declarations + configure_routes");

        // ── Layer 0: Frontend (structural) ───────────────────────────────────
        let _fe_index  = add_node("frontend/index.html",             NodeType::Structural, 8, None, false, "SPA shell");
        let _fe_style  = add_node("frontend/style.css",              NodeType::Structural, 8, None, false, "Application styles");
        let _fe_client = add_node("frontend/src/api/client.js",      NodeType::Structural, 8, None, false, "Fetch-based API client");
        for entity in spec.entities.iter().filter(|e| e.name != "User") {
            add_node(
                &format!("frontend/src/pages/{}.js", entity.snake_name),
                NodeType::Structural, 8, Some(entity.name.clone()), false,
                &format!("{} CRUD page", entity.name),
            );
        }

        // ── Layer 9: Tests (structural / mock) ───────────────────────────────
        let _tests = add_node("backend/tests/api_tests.rs", NodeType::Structural, 9, None, true, "Integration test placeholders");

        // ── Layer 1: Foundation structural nodes (deterministic — no LLM, no token cost) ──
        // These files have known, fixed shapes that MUST match the ProvidedSymbol signatures
        // in provided.rs exactly.  Making them structural eliminates the #1 source of
        // type-mismatch compile errors downstream.
        let errors_idx = add_node(
            "backend/src/errors.rs", NodeType::Structural, 1, None, true,
            "AppError enum — generated by structural::generate_errors_rs()",
        );
        let config_idx = add_node(
            "backend/src/config.rs", NodeType::Structural, 1, None, true,
            "AppConfig from env — generated by structural::generate_config_rs()",
        );
        let pagination_idx = add_node(
            "backend/src/pagination.rs", NodeType::Structural, 1, None, true,
            "PaginationParams + PaginatedResponse<T> — generated by structural::generate_pagination_rs()",
        );

        // ── Layer 2: Auth structural nodes ────────────────────────────────────
        // user.rs is structural — hardcoded auth fields match provides_user_model_types() exactly
        let user_model_idx = add_node(
            "backend/src/models/user.rs", NodeType::Structural, 2,
            Some("User".into()), true,
            "User struct + CreateUserPayload + UpdateUserPayload — generated by structural::generate_user_model_rs()",
        );
        let auth_idx = add_node(
            "backend/src/auth.rs", NodeType::Structural, 2, None, true,
            "AuthUser extractor + Claims + encode_jwt + require_role — generated by structural::generate_auth_rs()",
        );

        // ── Layer 3: Entity model structural nodes ────────────────────────────
        // Models are deterministic from EntityDef — no LLM needed.
        // Structural generator guarantees output matches provides_model_types() signatures.
        let mut entity_model_indices: Vec<(String, usize)> = Vec::new();
        for entity in spec.entities.iter().filter(|e| e.name != "User") {
            let idx = add_node(
                &format!("backend/src/models/{}.rs", entity.snake_name),
                NodeType::Structural, 3, Some(entity.name.clone()), true,
                &format!(
                    "{} struct + Create{}Payload + Update{}Payload — generated by structural::generate_model_rs()",
                    entity.name, entity.name, entity.name
                ),
            );
            entity_model_indices.push((entity.name.clone(), idx));
        }

        // ── Layer 4: Database nodes ───────────────────────────────────────────
        // pool.rs is structural — fixed shape, no LLM needed
        let pool_idx = add_node(
            "backend/src/database/pool.rs", NodeType::Structural, 4, None, true,
            "create_pool() — generated by structural::generate_pool_rs()",
        );

        // query nodes for every entity (including User)
        let mut entity_query_indices: Vec<(String, usize)> = Vec::new();
        let all_entities: Vec<&EntityDef> = spec.entities.iter().collect();
        for entity in &all_entities {
            let purpose = if entity.name == "User" {
                format!(
                    "CRUD query fns for {}: get_{s}, list_{s}s, create_{s}, update_{s}, delete_{s}, \
                     AND ALSO get_user_by_email (needed by auth_routes). \
                     Use sqlx::query_as::<_, Type>(). Never query_as!() macro.",
                    entity.name, s = entity.snake_name
                )
            } else {
                format!(
                    "CRUD query fns for {}: get_{s}, list_{s}s, create_{s}, update_{s}, delete_{s}. \
                     Use sqlx::query_as::<_, Type>(). Never query_as!() macro.",
                    entity.name, s = entity.snake_name
                )
            };
            let idx = add_node(
                &format!("backend/src/database/{}_queries.rs", entity.snake_name),
                NodeType::Llm, 4, Some(entity.name.clone()), true,
                &purpose,
            );
            // errors → queries
            edges.push(HdagEdge { from: errors_idx, to: idx, provides: provided::provides_apperror() });
            // pool → queries
            edges.push(HdagEdge { from: pool_idx, to: idx, provides: provided::provides_pool() });
            // pagination → queries
            edges.push(HdagEdge { from: pagination_idx, to: idx, provides: provided::provides_pagination() });

            // model → queries (find the matching model index)
            let model_sym = if entity.name == "User" {
                // user model symbols
                provided::provides_user_model_types()
            } else {
                // look up entity model index and provide its types
                provided::provides_model_types(entity)
            };
            if entity.name == "User" {
                edges.push(HdagEdge { from: user_model_idx, to: idx, provides: model_sym });
            } else if let Some(&(_, model_idx)) = entity_model_indices.iter().find(|(n, _)| n == &entity.name) {
                edges.push(HdagEdge { from: model_idx, to: idx, provides: model_sym });
            }

            entity_query_indices.push((entity.name.clone(), idx));
        }

        // ── Layer 5: Service LLM nodes ────────────────────────────────────────
        let mut entity_service_indices: Vec<(String, usize)> = Vec::new();
        for entity in &all_entities {
            let idx = add_node(
                &format!("backend/src/services/{}.rs", entity.snake_name),
                NodeType::Llm, 5, Some(entity.name.clone()), true,
                &format!(
                    "Business logic for {}: thin wrappers around DB queries with validation and business rules. All fns take &PgPool, return Result<_, AppError>.",
                    entity.name
                ),
            );
            // errors → service
            edges.push(HdagEdge { from: errors_idx, to: idx, provides: provided::provides_apperror() });
            // pagination → service
            edges.push(HdagEdge { from: pagination_idx, to: idx, provides: provided::provides_pagination() });
            // pool → service
            edges.push(HdagEdge { from: pool_idx, to: idx, provides: provided::provides_pool() });

            // model → service
            let model_sym = if entity.name == "User" {
                provided::provides_user_model_types()
            } else {
                provided::provides_model_types(entity)
            };
            if entity.name == "User" {
                edges.push(HdagEdge { from: user_model_idx, to: idx, provides: model_sym });
            } else if let Some(&(_, model_idx)) = entity_model_indices.iter().find(|(n, _)| n == &entity.name) {
                edges.push(HdagEdge { from: model_idx, to: idx, provides: model_sym });
            }

            // queries → service
            if let Some(&(_, q_idx)) = entity_query_indices.iter().find(|(n, _)| n == &entity.name) {
                edges.push(HdagEdge { from: q_idx, to: idx, provides: provided::provides_query_fns(entity) });
            }

            entity_service_indices.push((entity.name.clone(), idx));
        }

        // ── Layer 6: API LLM nodes ────────────────────────────────────────────
        // auth_routes
        let auth_routes_idx = add_node(
            "backend/src/api/auth_routes.rs", NodeType::Llm, 6,
            Some("User".into()), true,
            "Auth endpoints: POST /api/auth/register, POST /api/auth/login, GET /api/auth/me. Hash passwords with bcrypt. Return JWT on login.",
        );
        edges.push(HdagEdge { from: errors_idx, to: auth_routes_idx, provides: provided::provides_apperror() });
        edges.push(HdagEdge { from: auth_idx, to: auth_routes_idx, provides: provided::provides_authuser() });
        edges.push(HdagEdge { from: user_model_idx, to: auth_routes_idx, provides: provided::provides_user_model_types() });
        if let Some(&(_, q_idx)) = entity_query_indices.iter().find(|(n, _)| n == "User") {
            edges.push(HdagEdge { from: q_idx, to: auth_routes_idx, provides: provided::provides_query_fns_user() });
        }

        // entity api handlers
        for entity in spec.entities.iter().filter(|e| e.name != "User") {
            let idx = add_node(
                &format!("backend/src/api/{}.rs", entity.snake_name),
                NodeType::Llm, 6, Some(entity.name.clone()), true,
                &format!(
                    "Actix-web handlers for {}: list (paginated), get by id, create, update, delete. Require AuthUser. Admin-only delete.",
                    entity.name
                ),
            );
            edges.push(HdagEdge { from: errors_idx, to: idx, provides: provided::provides_apperror() });
            edges.push(HdagEdge { from: pagination_idx, to: idx, provides: provided::provides_pagination() });
            edges.push(HdagEdge { from: auth_idx, to: idx, provides: provided::provides_authuser() });
            edges.push(HdagEdge { from: config_idx, to: idx, provides: provided::provides_config() });

            // model → api
            let model_sym = provided::provides_model_types(entity);
            if let Some(&(_, model_idx)) = entity_model_indices.iter().find(|(n, _)| n == &entity.name) {
                edges.push(HdagEdge { from: model_idx, to: idx, provides: model_sym });
            }

            // service → api
            if let Some(&(_, svc_idx)) = entity_service_indices.iter().find(|(n, _)| n == &entity.name) {
                edges.push(HdagEdge { from: svc_idx, to: idx, provides: provided::provides_service_fns(entity) });
            }
        }

        CodegenHdag { nodes, edges }
    }

    /// Topological sort using Kahn's algorithm with deterministic lexicographic
    /// tie-breaking on node path. Guarantees each node is processed after all
    /// its predecessors.
    pub fn topological_sort(&self) -> Vec<usize> {
        let n = self.nodes.len();
        let mut in_degree = vec![0usize; n];
        for edge in &self.edges {
            in_degree[edge.to] += 1;
        }

        // BTreeSet gives lexicographic ordering on (path, index) — deterministic
        let mut ready: BTreeSet<(String, usize)> = BTreeSet::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if in_degree[i] == 0 {
                ready.insert((node.path.clone(), i));
            }
        }

        let mut result = Vec::with_capacity(n);
        while !ready.is_empty() {
            // pop lexicographically smallest
            let (_, idx) = ready.iter().next().cloned().expect("ready not empty");
            ready.remove(&(self.nodes[idx].path.clone(), idx));

            result.push(idx);

            for edge in self.edges.iter().filter(|e| e.from == idx) {
                in_degree[edge.to] -= 1;
                if in_degree[edge.to] == 0 {
                    ready.insert((self.nodes[edge.to].path.clone(), edge.to));
                }
            }
        }

        result
    }

    /// Return the indices of all predecessor nodes for `node_idx`.
    pub fn predecessors(&self, node_idx: usize) -> Vec<usize> {
        self.edges
            .iter()
            .filter(|e| e.to == node_idx)
            .map(|e| e.from)
            .collect()
    }

    /// Return the union of all [`ProvidedSymbol`]s from incoming edges of `node_idx`.
    ///
    /// This is the exact set of symbols injected into the LLM prompt for this node —
    /// no more, no less.
    pub fn provided_symbols(&self, node_idx: usize) -> Vec<ProvidedSymbol> {
        let mut symbols: Vec<ProvidedSymbol> = Vec::new();
        for edge in self.edges.iter().filter(|e| e.to == node_idx) {
            symbols.extend(edge.provides.iter().cloned());
        }
        // Deduplicate by import_path
        let mut seen = std::collections::HashSet::new();
        symbols.retain(|s| seen.insert(s.import_path.clone()));
        symbols
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppSpec, EntityDef, ValidationRule};
    use isls_hypercube::domain::FieldDef;

    fn test_spec() -> AppSpec {
        AppSpec {
            app_name: "test-app".into(),
            description: "Test application".into(),
            domain_name: "test".into(),
            entities: vec![
                EntityDef {
                    name: "User".into(),
                    snake_name: "user".into(),
                    fields: vec![
                        FieldDef { name: "id".into(), rust_type: "i64".into(), sql_type: "BIGSERIAL PRIMARY KEY".into(), nullable: false, default_value: None, description: "PK".into() },
                        FieldDef { name: "email".into(), rust_type: "String".into(), sql_type: "VARCHAR(255)".into(), nullable: false, default_value: None, description: "Email".into() },
                    ],
                    validations: vec![],
                    business_rules: vec![],
                    relationships: vec![],
                },
                EntityDef {
                    name: "Product".into(),
                    snake_name: "product".into(),
                    fields: vec![
                        FieldDef { name: "id".into(), rust_type: "i64".into(), sql_type: "BIGSERIAL PRIMARY KEY".into(), nullable: false, default_value: None, description: "PK".into() },
                        FieldDef { name: "name".into(), rust_type: "String".into(), sql_type: "VARCHAR(255)".into(), nullable: false, default_value: None, description: "Name".into() },
                    ],
                    validations: vec![],
                    business_rules: vec![],
                    relationships: vec![],
                },
            ],
            business_rules: vec![],
        }
    }

    #[test]
    fn test_build_has_nodes_and_edges() {
        let spec = test_spec();
        let hdag = CodegenHdag::build(&spec);
        assert!(!hdag.nodes.is_empty(), "HDAG must have nodes");
        assert!(!hdag.edges.is_empty(), "HDAG must have edges");
    }

    #[test]
    fn test_topological_sort_covers_all_nodes() {
        let spec = test_spec();
        let hdag = CodegenHdag::build(&spec);
        let order = hdag.topological_sort();
        assert_eq!(order.len(), hdag.nodes.len(), "topological sort must cover all nodes");
    }

    #[test]
    fn test_topological_sort_respects_dependencies() {
        let spec = test_spec();
        let hdag = CodegenHdag::build(&spec);
        let order = hdag.topological_sort();

        // Build position map
        let mut pos = vec![0usize; hdag.nodes.len()];
        for (i, &idx) in order.iter().enumerate() {
            pos[idx] = i;
        }

        // Every edge from→to must have pos[from] < pos[to]
        for edge in &hdag.edges {
            assert!(
                pos[edge.from] < pos[edge.to],
                "edge {} → {} violates topological order",
                hdag.nodes[edge.from].path,
                hdag.nodes[edge.to].path
            );
        }
    }

    #[test]
    fn test_errors_node_provides_apperror() {
        let spec = test_spec();
        let hdag = CodegenHdag::build(&spec);
        let errors_node = hdag.nodes.iter().find(|n| n.path.ends_with("errors.rs")).unwrap();
        // errors.rs is now structural — no incoming edges
        assert_eq!(hdag.predecessors(errors_node.index).len(), 0, "errors.rs has no predecessors");
        // errors.rs is structural (no LLM cost)
        assert_eq!(errors_node.node_type, NodeType::Structural, "errors.rs must be Structural");

        // auth.rs is also structural now — it hard-codes its AppError dependency
        let auth_node = hdag.nodes.iter().find(|n| n.path.ends_with("auth.rs")).unwrap();
        assert_eq!(auth_node.node_type, NodeType::Structural, "auth.rs must be Structural");

        // user.rs is also structural now — hardcoded auth fields, no HDAG edges needed
        let user_model_node = hdag.nodes.iter().find(|n| n.path.ends_with("models/user.rs")).unwrap();
        assert_eq!(user_model_node.node_type, NodeType::Structural, "user.rs must be Structural");

        // errors.rs still provides AppError to LLM nodes downstream (e.g. query nodes)
        let product_queries_idx = hdag.nodes.iter()
            .find(|n| n.path.ends_with("product_queries.rs"))
            .unwrap().index;
        let provided = hdag.provided_symbols(product_queries_idx);
        assert!(
            provided.iter().any(|s| s.import_path.contains("AppError")),
            "product_queries.rs must receive AppError symbol from errors.rs"
        );
    }
}
