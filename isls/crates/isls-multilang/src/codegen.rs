// isls-multilang/src/codegen.rs
//
// CodegenBackend trait + scaffold/assemble data types.
// Definition 4.1 of spec.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

use crate::glyph_ir::IrDocument;

// ─── Scaffold Types ──────────────────────────────────────────────────────────

/// One source file with stubs where Oracle content will be inserted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScaffoldFile {
    pub path: String,
    pub language: String,
    /// Import lines derived from IR edges
    pub imports: Vec<String>,
    /// Type definitions derived from IR nodes
    pub type_definitions: Vec<String>,
    /// Function stubs with placeholders for Oracle bodies
    pub function_stubs: Vec<FunctionStub>,
}

/// A single function stub in a scaffold file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionStub {
    /// glyph-ir node ID
    pub node_id: String,
    pub name: String,
    /// Full language-appropriate signature
    pub signature: String,
    /// Placeholder text indicating Oracle should fill this
    pub body_placeholder: String,
}

// ─── Emitted Types ───────────────────────────────────────────────────────────

/// A fully assembled source file (scaffolding + Oracle bodies merged).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmittedFile {
    pub path: String,
    pub content: String,
    pub language: String,
    /// Lines contributed by the backend (structure/scaffolding)
    pub scaffold_lines: usize,
    /// Lines contributed by the Oracle (logic)
    pub oracle_lines: usize,
}

/// A directory of emitted files with optional config files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmittedDir {
    pub files: Vec<EmittedFile>,
    /// Config files (Cargo.toml, package.json, requirements.txt, etc.)
    pub config_files: Vec<EmittedFile>,
}

// ─── Fill Strategy ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillStrategy {
    /// Oracle fills function bodies
    Oracle,
    /// Content is entirely static (no Oracle needed)
    Static,
    /// Content is derived deterministically from IR structure
    Derive,
}

// ─── MultiLangAtom ───────────────────────────────────────────────────────────

/// One atom in a multi-language template (spec §7).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MultiLangAtom {
    pub name: String,
    pub backend: String,
    pub fill: FillStrategy,
    pub ir_node_ids: Vec<String>,
}

// ─── CodegenBackend Trait ────────────────────────────────────────────────────

/// A language-specific code generation backend (Definition 4.1).
///
/// The backend writes SCAFFOLDING.
/// The Oracle writes LOGIC.
/// The backend assembles the final files from scaffolding + Oracle content.
pub trait CodegenBackend: Send + Sync {
    fn language(&self) -> &str;
    fn extension(&self) -> &str;

    /// Generate scaffolding from IR structure.
    /// Returns files with stubs where Oracle content will be inserted.
    fn scaffold(&self, doc: &IrDocument) -> Vec<ScaffoldFile>;

    /// Assemble final files by merging scaffolding with Oracle bodies.
    /// `oracle_bodies` maps node_id → implementation code string.
    fn assemble(
        &self,
        scaffolds: &[ScaffoldFile],
        oracle_bodies: &BTreeMap<String, String>,
    ) -> Vec<EmittedFile>;

    /// True if this backend needs Oracle bodies (false for Ops, Docs).
    fn needs_oracle(&self) -> bool { true }
}

// ─── Shared Helpers ──────────────────────────────────────────────────────────

/// Count non-empty lines in a string.
pub fn count_lines(s: &str) -> usize {
    s.lines().count()
}

/// Build an EmittedFile from scaffold text and optional oracle body insertions.
pub fn merge_scaffold_and_oracle(
    path: &str,
    language: &str,
    scaffold_text: &str,
    oracle_sections: &[(String, String)], // (placeholder, body)
) -> EmittedFile {
    let mut content = scaffold_text.to_string();
    let mut oracle_lines = 0;
    for (placeholder, body) in oracle_sections {
        if content.contains(placeholder.as_str()) {
            let body_lines = count_lines(body);
            content = content.replace(placeholder.as_str(), body);
            oracle_lines += body_lines;
        }
    }
    EmittedFile {
        path: path.to_string(),
        content: content.clone(),
        language: language.to_string(),
        scaffold_lines: count_lines(&content).saturating_sub(oracle_lines),
        oracle_lines,
    }
}
