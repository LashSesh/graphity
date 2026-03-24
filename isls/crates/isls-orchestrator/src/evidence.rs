// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//! SHA-256 evidence chain construction for ISLS full-stack generation.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A single entry in the evidence chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceEntry {
    pub stage: String,
    pub file_path: PathBuf,
    pub sha256: String,
    pub prev_hash: String,
    pub chain_hash: String,
}

/// Append-only evidence chain linking all generated artifacts.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct EvidenceChain {
    entries: Vec<EvidenceEntry>,
    chain_tip: String,
}

impl EvidenceChain {
    /// Create a new empty evidence chain.
    pub fn new() -> Self {
        Self { entries: Vec::new(), chain_tip: "genesis".to_string() }
    }

    /// Record a generated file in the chain.
    pub fn record(&mut self, stage: &str, file_path: PathBuf, content: &[u8]) {
        let sha256 = sha256_hex(content);
        let chain_input = format!("{}|{}|{}|{}", stage, file_path.display(), sha256, self.chain_tip);
        let chain_hash = sha256_hex(chain_input.as_bytes());
        self.entries.push(EvidenceEntry {
            stage: stage.to_string(),
            file_path,
            sha256,
            prev_hash: self.chain_tip.clone(),
            chain_hash: chain_hash.clone(),
        });
        self.chain_tip = chain_hash;
    }

    /// Verify the chain is internally consistent (each entry links correctly).
    pub fn is_valid(&self) -> bool {
        let mut tip = "genesis".to_string();
        for entry in &self.entries {
            if entry.prev_hash != tip { return false; }
            tip = entry.chain_hash.clone();
        }
        true
    }

    /// Number of entries in the chain.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// All entries in the chain.
    pub fn entries(&self) -> &[EvidenceEntry] {
        &self.entries
    }

    /// Current chain tip hash.
    pub fn tip(&self) -> &str {
        &self.chain_tip
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}
