//! Multi-language code generation bridge for ISLS (Babylon Bridge).
//!
//! Transforms ISLS artifacts into a glyph intermediate representation, validates
//! via H5 embedding, and scaffolds target-language code through pluggable backends.

// isls-multilang: C28 — Babylon Bridge
// IR-Validated Multi-Language Forge.
//
// Part A of ISLS Extension Phase 10 v1.0.0 (Revised v2).
//
// Architecture:
//   ArtifactIR → glyph_ir::IrDocument → H5 embedding (validation)
//                                      → CodegenBackend::scaffold()
//                                      + Oracle body strings
//                                      → CodegenBackend::assemble()
//                                      → EmittedFile list

pub mod glyph_ir;
pub mod embed;
pub mod bridge;
pub mod codegen;
pub mod backends;
pub mod templates;

pub use glyph_ir::{IrDocument, IrNode, IrEdge, NodeKind, EdgeKind};
pub use embed::{H5Embedding, EmbedError, EmbedConfig, compute_embedding, validate_embedding};
pub use bridge::{artifact_to_glyph_ir, BridgeError};
pub use codegen::{
    CodegenBackend, ScaffoldFile, FunctionStub, EmittedFile, EmittedDir,
    MultiLangAtom, FillStrategy,
};
pub use templates::{MultiLangTemplate, TemplateCatalog as MultiLangCatalog};

use std::collections::BTreeMap;
use thiserror::Error;

// ─── Top-level Error ─────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum MultilangError {
    #[error("bridge: {0}")]
    Bridge(#[from] BridgeError),
    #[error("embed: {0}")]
    Embed(#[from] EmbedError),
    #[error("codegen backend '{0}' not registered")]
    UnknownBackend(String),
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MultilangError>;

// ─── BabylonForge ─────────────────────────────────────────────────────────────

/// Orchestrates ArtifactIR → multi-language project generation.
pub struct BabylonForge {
    backends: BTreeMap<String, Box<dyn CodegenBackend>>,
    embed_config: EmbedConfig,
}

impl Default for BabylonForge {
    fn default() -> Self {
        Self::new()
    }
}

impl BabylonForge {
    pub fn new() -> Self {
        let mut forge = Self {
            backends: BTreeMap::new(),
            embed_config: EmbedConfig::default(),
        };
        forge.register_defaults();
        forge
    }

    fn register_defaults(&mut self) {
        self.register(Box::new(backends::RustBackend));
        self.register(Box::new(backends::TypeScriptBackend));
        self.register(Box::new(backends::PythonBackend));
        self.register(Box::new(backends::SqlBackend));
        self.register(Box::new(backends::OpsBackend));
        self.register(Box::new(backends::DocsBackend));
    }

    pub fn register(&mut self, backend: Box<dyn CodegenBackend>) {
        self.backends.insert(backend.language().to_string(), backend);
    }

    /// Full pipeline: ArtifactIR → H5 validation → scaffold → assemble.
    ///
    /// `oracle_bodies` maps node_id → implementation code string.
    /// Pass an empty map for structure-only backends (Ops, Docs).
    pub fn generate(
        &self,
        ir: &isls_artifact_ir::ArtifactIR,
        language: &str,
        oracle_bodies: &BTreeMap<String, String>,
    ) -> Result<Vec<EmittedFile>> {
        let doc = bridge::artifact_to_glyph_ir(ir)?;
        let embedding = embed::compute_embedding(&doc);
        embed::validate_embedding(&embedding, &self.embed_config)?;

        let backend = self.backends.get(language)
            .ok_or_else(|| MultilangError::UnknownBackend(language.to_string()))?;

        let scaffolds = backend.scaffold(&doc);
        let files = backend.assemble(&scaffolds, oracle_bodies);
        Ok(files)
    }

    /// Dump the glyph-ir document for inspection.
    pub fn dump_ir(
        &self,
        ir: &isls_artifact_ir::ArtifactIR,
    ) -> Result<String> {
        let doc = bridge::artifact_to_glyph_ir(ir)?;
        Ok(serde_json::to_string_pretty(&doc)?)
    }

    /// Validate IR structure via H5 embedding.
    pub fn check_ir(
        &self,
        ir: &isls_artifact_ir::ArtifactIR,
    ) -> Result<H5Embedding> {
        let doc = bridge::artifact_to_glyph_ir(ir)?;
        let embedding = embed::compute_embedding(&doc);
        embed::validate_embedding(&embedding, &self.embed_config)?;
        Ok(embedding)
    }

    pub fn available_languages(&self) -> Vec<String> {
        self.backends.keys().cloned().collect()
    }
}
