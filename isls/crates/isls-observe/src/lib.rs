//! Observation ingestion layer for ISLS (Layer L0).
//!
//! Provides canonicalization adapters that convert raw input bytes into
//! content-addressed, typed observations with provenance tracking.

// isls-observe: Observation adapters (Layer L0)
// C2 — depends on isls-types only

use isls_types::{
    content_address_raw, Hash256, MeasurementContext, Observation, ProvenanceEnvelope,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ObserveError {
    #[error("canonicalization failed: {0}")]
    Canonicalize(String),
    #[error("digest mismatch")]
    DigestMismatch,
}

pub type Result<T> = std::result::Result<T, ObserveError>;

/// Observation canonicalization map: Gamma_obs (ISLS Def 12.1)
pub trait ObservationAdapter: Send + Sync {
    fn source_id(&self) -> &str;
    fn canonicalize(
        &self,
        raw: &[u8],
        context: &MeasurementContext,
    ) -> Result<Observation>;
}

/// Idempotent ingestion: same raw input -> same digest (Req 12.2)
pub fn ingest(
    adapter: &dyn ObservationAdapter,
    raw: &[u8],
    ctx: &MeasurementContext,
) -> Result<Observation> {
    let obs = adapter.canonicalize(raw, ctx)?;
    // Verify: re-canonicalize produces identical digest
    let recomputed = content_address_raw(&obs.payload);
    if recomputed != obs.digest {
        return Err(ObserveError::DigestMismatch);
    }
    Ok(obs)
}

// ─── Observation Classes ──────────────────────────────────────────────────────

/// Raw state observation
#[derive(Debug, Clone)]
pub struct StateObs {
    pub vertex_id: u64,
    pub value: Vec<f64>,
    pub timestamp: f64,
}

/// Relation observation (edge between two vertices)
#[derive(Debug, Clone)]
pub struct RelationObs {
    pub from: u64,
    pub to: u64,
    pub weight: f64,
    pub timestamp: f64,
}

/// Event observation (discrete event)
#[derive(Debug, Clone)]
pub struct EventObs {
    pub event_type: String,
    pub payload: Vec<u8>,
    pub timestamp: f64,
}

/// Phase observation (carrier phase data)
#[derive(Debug, Clone)]
pub struct PhaseObs {
    pub phi: f64,
    pub tau: f64,
    pub r: f64,
    pub timestamp: f64,
}

/// Exogenous observation (external system boundary)
#[derive(Debug, Clone)]
pub struct ExogenousObs {
    pub source: String,
    pub payload: Vec<u8>,
    pub timestamp: f64,
}

// ─── Default Adapter ─────────────────────────────────────────────────────────

/// A passthrough adapter that treats raw bytes as payload
pub struct PassthroughAdapter {
    id: String,
    schema_version: String,
}

impl PassthroughAdapter {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            schema_version: "1.0.0".to_string(),
        }
    }
}

impl ObservationAdapter for PassthroughAdapter {
    fn source_id(&self) -> &str {
        &self.id
    }

    fn canonicalize(
        &self,
        raw: &[u8],
        context: &MeasurementContext,
    ) -> Result<Observation> {
        let payload = raw.to_vec();
        let digest: Hash256 = content_address_raw(&payload);
        Ok(Observation {
            timestamp: 0.0,
            source_id: self.id.clone(),
            provenance: ProvenanceEnvelope {
                origin: self.id.clone(),
                chain: Vec::new(),
                sig: None,
            },
            payload,
            context: context.clone(),
            digest,
            schema_version: self.schema_version.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_adapter_idempotent() {
        let adapter = PassthroughAdapter::new("test");
        let raw = b"hello world";
        let ctx = MeasurementContext::default();
        let obs1 = ingest(&adapter, raw, &ctx).unwrap();
        let obs2 = ingest(&adapter, raw, &ctx).unwrap();
        assert_eq!(obs1.digest, obs2.digest);
        assert_eq!(obs1.payload, obs2.payload);
    }

    #[test]
    fn different_inputs_different_digests() {
        let adapter = PassthroughAdapter::new("test");
        let ctx = MeasurementContext::default();
        let obs1 = ingest(&adapter, b"input1", &ctx).unwrap();
        let obs2 = ingest(&adapter, b"input2", &ctx).unwrap();
        assert_ne!(obs1.digest, obs2.digest);
    }
}
