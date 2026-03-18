// isls-multilang/src/glyph_ir.rs
//
// Self-contained glyph-ir implementation for C28.
//
// Mirrors the interface described in the ISLS Phase 10 spec.
// The real Babylon crates (glyph-ir, glyph-embed, glyph-canon, glyph-q16)
// are Babylon-compiler workspace dependencies; we implement the structural
// contract here so the ISLS workspace remains self-contained.
//
// Capabilities:
//   - IrDocument with 10 NodeKinds, 11 EdgeKinds
//   - Canonical sort (deterministic node/edge ordering)
//   - SHA-256 digest of canonical JSON

use std::collections::BTreeMap;
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};

// ─── NodeKind ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NodeKind {
    Module,
    Function,
    Type,
    Constant,
    Interface,
    Service,
    Table,
    Column,
    Container,
    File,
}

impl NodeKind {
    pub fn as_str(&self) -> &str {
        match self {
            NodeKind::Module    => "module",
            NodeKind::Function  => "function",
            NodeKind::Type      => "type",
            NodeKind::Constant  => "constant",
            NodeKind::Interface => "interface",
            NodeKind::Service   => "service",
            NodeKind::Table     => "table",
            NodeKind::Column    => "column",
            NodeKind::Container => "container",
            NodeKind::File      => "file",
        }
    }
}

// ─── EdgeKind ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EdgeKind {
    Contains,
    CalleeRef,
    TypeRef,
    Implements,
    DependsOn,
    Inherits,
    HasField,
    HasIndex,
    Deploys,
    Documents,
    Configures,
}

impl EdgeKind {
    pub fn as_str(&self) -> &str {
        match self {
            EdgeKind::Contains   => "contains",
            EdgeKind::CalleeRef  => "callee_ref",
            EdgeKind::TypeRef    => "type_ref",
            EdgeKind::Implements => "implements",
            EdgeKind::DependsOn  => "depends_on",
            EdgeKind::Inherits   => "inherits",
            EdgeKind::HasField   => "has_field",
            EdgeKind::HasIndex   => "has_index",
            EdgeKind::Deploys    => "deploys",
            EdgeKind::Documents  => "documents",
            EdgeKind::Configures => "configures",
        }
    }
}

// ─── IrNode ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IrNode {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    /// Parameter list (e.g. function params or type fields)
    pub params: Option<Vec<String>>,
    /// Arbitrary metadata from ArtifactIR component
    pub properties: Option<BTreeMap<String, serde_json::Value>>,
}

impl IrNode {
    pub fn new(id: &str, kind: NodeKind, name: &str) -> Self {
        Self {
            id: id.to_string(),
            kind,
            name: name.to_string(),
            params: None,
            properties: None,
        }
    }

    /// Returns the "kind" property value, if present.
    pub fn kind_prop(&self) -> Option<&str> {
        self.properties.as_ref()
            .and_then(|p| p.get("kind"))
            .and_then(|v| v.as_str())
    }

    /// Returns the "description" property value, if present.
    pub fn description(&self) -> Option<&str> {
        self.properties.as_ref()
            .and_then(|p| p.get("description"))
            .and_then(|v| v.as_str())
    }
}

// ─── IrEdge ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IrEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

impl IrEdge {
    pub fn new(from: &str, to: &str, kind: EdgeKind) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
            kind,
        }
    }
}

// ─── IrDocument ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IrDocument {
    pub domain: String,
    pub artifact_id: String,
    pub nodes: Vec<IrNode>,
    pub edges: Vec<IrEdge>,
    /// SHA-256 of canonical JSON (set after canonicalize())
    pub digest: String,
}

impl IrDocument {
    pub fn new(domain: &str, artifact_id: &str) -> Self {
        Self {
            domain: domain.to_string(),
            artifact_id: artifact_id.to_string(),
            nodes: Vec::new(),
            edges: Vec::new(),
            digest: String::new(),
        }
    }

    /// Sort nodes and edges deterministically, then compute SHA-256 digest.
    pub fn canonicalize(&mut self) {
        self.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        self.edges.sort_by(|a, b| {
            a.from.cmp(&b.from)
                .then(a.to.cmp(&b.to))
                .then(a.kind.cmp(&b.kind))
        });
        self.digest = self.compute_digest();
    }

    fn compute_digest(&self) -> String {
        // Canonical JSON: sorted keys, no whitespace.
        #[derive(Serialize)]
        struct Canonical<'a> {
            artifact_id: &'a str,
            domain: &'a str,
            edges: Vec<EdgeCanon<'a>>,
            nodes: Vec<NodeCanon<'a>>,
        }
        #[derive(Serialize)]
        struct NodeCanon<'a> {
            id: &'a str,
            kind: &'a str,
            name: &'a str,
        }
        #[derive(Serialize)]
        struct EdgeCanon<'a> {
            from: &'a str,
            kind: &'a str,
            to: &'a str,
        }
        let canon = Canonical {
            artifact_id: &self.artifact_id,
            domain: &self.domain,
            nodes: self.nodes.iter().map(|n| NodeCanon { id: &n.id, kind: n.kind.as_str(), name: &n.name }).collect(),
            edges: self.edges.iter().map(|e| EdgeCanon { from: &e.from, kind: e.kind.as_str(), to: &e.to }).collect(),
        };
        let json = serde_json::to_vec(&canon).unwrap_or_default();
        let hash = Sha256::digest(&json);
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Find a node's ID by its name.
    pub fn find_node_id_by_name(&self, name: &str) -> Option<String> {
        self.nodes.iter()
            .find(|n| n.name == name)
            .map(|n| n.id.clone())
    }

    /// Return all Function nodes (non-root, non-type).
    pub fn function_nodes(&self) -> Vec<&IrNode> {
        self.nodes.iter()
            .filter(|n| n.kind == NodeKind::Function)
            .collect()
    }

    /// Return CaleeRef edges.
    pub fn callee_edges(&self) -> Vec<&IrEdge> {
        self.edges.iter()
            .filter(|e| e.kind == EdgeKind::CalleeRef)
            .collect()
    }

    /// Return Contains edges.
    pub fn contains_edges(&self) -> Vec<&IrEdge> {
        self.edges.iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .collect()
    }
}
