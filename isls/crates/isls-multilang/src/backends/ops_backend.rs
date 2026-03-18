// isls-multilang/src/backends/ops_backend.rs
//
// OpsBackend: Dockerfile skeleton, docker-compose services, CI pipeline steps.
// Fully deterministic — no Oracle needed.

use std::collections::BTreeMap;
use crate::codegen::{CodegenBackend, EmittedFile, ScaffoldFile, count_lines};
use crate::glyph_ir::IrDocument;

pub struct OpsBackend;

impl CodegenBackend for OpsBackend {
    fn language(&self) -> &str { "yaml" }
    fn extension(&self) -> &str { "yml" }
    fn needs_oracle(&self) -> bool { false }

    fn scaffold(&self, doc: &IrDocument) -> Vec<ScaffoldFile> {
        let services: Vec<String> = doc.function_nodes()
            .iter()
            .map(|n| format!(
                "  {}:\n    build: .\n    environment:\n      - SERVICE={}\n    restart: unless-stopped",
                to_kebab_case(&n.name),
                n.name
            ))
            .collect();

        vec![
            // Dockerfile
            ScaffoldFile {
                path: "Dockerfile".to_string(),
                language: "dockerfile".to_string(),
                imports: vec![],
                type_definitions: vec![
                    format!(
                        "FROM rust:1.75-slim AS builder\nWORKDIR /app\nCOPY . .\nRUN cargo build --release\n\nFROM debian:bookworm-slim\nWORKDIR /app\nCOPY --from=builder /app/target/release/{} .\nEXPOSE 8080\nCMD [\"./{}\"]",
                        doc.domain, doc.domain
                    )
                ],
                function_stubs: vec![],
            },
            // docker-compose.yml
            ScaffoldFile {
                path: "docker-compose.yml".to_string(),
                language: "yaml".to_string(),
                imports: vec![],
                type_definitions: vec![
                    format!("version: '3.8'\nservices:\n{}", services.join("\n"))
                ],
                function_stubs: vec![],
            },
            // CI pipeline
            ScaffoldFile {
                path: ".github/workflows/ci.yml".to_string(),
                language: "yaml".to_string(),
                imports: vec![],
                type_definitions: vec![
                    "name: CI\non: [push, pull_request]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: dtolnay/rust-toolchain@stable\n      - run: cargo test\n      - run: cargo clippy -- -D warnings".to_string()
                ],
                function_stubs: vec![],
            },
        ]
    }

    fn assemble(
        &self,
        scaffolds: &[ScaffoldFile],
        _oracle_bodies: &BTreeMap<String, String>,
    ) -> Vec<EmittedFile> {
        scaffolds.iter().map(|s| {
            let content = s.type_definitions.join("\n\n");
            EmittedFile {
                path: s.path.clone(),
                content: content.clone(),
                language: s.language.clone(),
                scaffold_lines: count_lines(&content),
                oracle_lines: 0,
            }
        }).collect()
    }
}

fn to_kebab_case(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 { out.push('-'); }
        out.push(ch.to_lowercase().next().unwrap_or(ch));
    }
    out.replace('_', "-").replace(' ', "-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph_ir::{IrDocument, IrNode, IrEdge, NodeKind, EdgeKind};

    fn make_doc() -> IrDocument {
        let mut doc = IrDocument::new("myapp", "test-id");
        let root = IrNode::new("n_root", NodeKind::Module, "root");
        let svc = IrNode::new("n_fn_0", NodeKind::Function, "ApiServer");
        doc.nodes.push(root);
        doc.nodes.push(svc);
        doc.edges.push(IrEdge::new("n_root", "n_fn_0", EdgeKind::Contains));
        doc.canonicalize();
        doc
    }

    // AT-BB10: Generate Dockerfile + compose from IR; verify valid YAML (no Oracle needed).
    #[test]
    fn at_bb10_ops_backend() {
        let backend = OpsBackend;
        let doc = make_doc();
        let scaffolds = backend.scaffold(&doc);
        let files = backend.assemble(&scaffolds, &BTreeMap::new());
        assert!(!files.is_empty(), "AT-BB10: must produce ops files");
        let dockerfile = files.iter().find(|f| f.path == "Dockerfile").expect("Dockerfile");
        assert!(dockerfile.content.contains("FROM"), "AT-BB10: Dockerfile must have FROM");
        let compose = files.iter().find(|f| f.path == "docker-compose.yml").expect("compose");
        assert!(compose.content.contains("services:"), "AT-BB10: compose must have services:");
        assert_eq!(compose.oracle_lines, 0, "AT-BB10: ops backend should have zero oracle lines");
    }
}
