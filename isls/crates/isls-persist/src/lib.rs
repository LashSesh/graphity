// isls-persist: Persistent HDAG engine (Layer L1 / MCCE assimilated)
// C3 — depends on isls-types, isls-observe

use std::collections::BTreeMap;
use isls_types::{
    CommitIndex, EdgeAnnotation, FiveDState, Observation, PersistenceConfig, VertexId,
};
use petgraph::graph::{DiGraph, NodeIndex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PersistError {
    #[error("vertex not found: {0}")]
    VertexNotFound(VertexId),
    #[error("graph capacity exceeded")]
    CapacityExceeded,
    #[error("integrity check failed: {0}")]
    IntegrityFail(String),
}

pub type Result<T> = std::result::Result<T, PersistError>;

// ─── Tensor Archive ───────────────────────────────────────────────────────────

/// Tensor archive: stores historical 5D state snapshots for a vertex
/// Lambda: V -> R^{5xTxK}
#[derive(Clone, Debug, Default)]
pub struct TensorArchive {
    pub snapshots: Vec<FiveDState>,
    pub timestamps: Vec<f64>,
}

impl TensorArchive {
    pub fn push(&mut self, state: FiveDState, timestamp: f64) {
        self.snapshots.push(state);
        self.timestamps.push(timestamp);
    }

    pub fn latest(&self) -> Option<&FiveDState> {
        self.snapshots.last()
    }
}

// ─── Storage Tiers ────────────────────────────────────────────────────────────

/// Hot tier: in-memory, last 7 days of activity
#[derive(Default, Debug)]
pub struct HotTier {
    pub data: BTreeMap<VertexId, Vec<(f64, Vec<u8>)>>, // ts -> raw bytes
}

/// Warm tier: compressed on-disk (simulated in-memory for now)
#[derive(Default, Debug)]
pub struct WarmTier {
    pub data: BTreeMap<VertexId, Vec<(f64, Vec<u8>)>>,
    pub corrupted: bool, // for AT-09 testing
}

/// Cold tier: indefinite, append-only files (simulated in-memory)
#[derive(Default, Debug)]
pub struct ColdTier {
    pub data: BTreeMap<VertexId, Vec<(f64, Vec<u8>)>>,
}

// ─── Vertex Data ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct VertexData {
    pub id: VertexId,
    pub active: bool,
    pub first_seen: f64,
    pub last_seen: f64,
    pub activation_count: u64,
}

impl VertexData {
    pub fn new(id: VertexId, timestamp: f64) -> Self {
        Self {
            id,
            active: true,
            first_seen: timestamp,
            last_seen: timestamp,
            activation_count: 1,
        }
    }
}

// ─── Persistent Graph ─────────────────────────────────────────────────────────

/// Persistent graph (ISLS Def 4.5, MCCE HDAG)
pub struct PersistentGraph {
    pub graph: DiGraph<VertexData, EdgeAnnotation>,
    pub id_map: BTreeMap<VertexId, NodeIndex>,
    pub tensor: BTreeMap<VertexId, TensorArchive>,  // Lambda: V -> R^{5xTxK}
    pub embedding: BTreeMap<VertexId, FiveDState>,  // Phi: V -> H5
    pub hot: HotTier,
    pub warm: WarmTier,
    pub cold: ColdTier,
    pub commit_index: CommitIndex,
    pub history: Vec<ObservationRecord>, // append-only history for Inv I1
}

#[derive(Clone, Debug)]
pub struct ObservationRecord {
    pub commit_index: CommitIndex,
    pub digest: isls_types::Hash256,
    pub timestamp: f64,
}

impl Default for PersistentGraph {
    fn default() -> Self {
        Self {
            graph: DiGraph::new(),
            id_map: BTreeMap::new(),
            tensor: BTreeMap::new(),
            embedding: BTreeMap::new(),
            hot: HotTier::default(),
            warm: WarmTier::default(),
            cold: ColdTier::default(),
            commit_index: 0,
            history: Vec::new(),
        }
    }
}

impl PersistentGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Estimate heap size in bytes (structural estimate, not exact RSS)
    pub fn estimate_heap_size(&self) -> usize {
        use isls_types::{FiveDState, EdgeAnnotation};
        let vertices = self.graph.node_count();
        let edges = self.graph.edge_count();
        // NodeIndex is usize; VertexId is u64
        let id_map_bytes = self.id_map.len() * (std::mem::size_of::<u64>() + std::mem::size_of::<usize>());
        let vertex_bytes = vertices * std::mem::size_of::<VertexData>();
        let edge_bytes = edges * std::mem::size_of::<EdgeAnnotation>();
        let embedding_bytes = self.embedding.len() * std::mem::size_of::<FiveDState>();
        let history_bytes = self.history.len() * std::mem::size_of::<ObservationRecord>();
        let tensor_bytes = self.tensor.len() * 64; // TensorArchive estimate per vertex
        id_map_bytes + vertex_bytes + edge_bytes + embedding_bytes + history_bytes + tensor_bytes
    }

    /// Upsert a vertex; returns its NodeIndex
    pub fn upsert_vertex(&mut self, id: VertexId, timestamp: f64) -> NodeIndex {
        if let Some(&nidx) = self.id_map.get(&id) {
            if let Some(data) = self.graph.node_weight_mut(nidx) {
                data.last_seen = timestamp;
                data.activation_count += 1;
            }
            nidx
        } else {
            let nidx = self.graph.add_node(VertexData::new(id, timestamp));
            self.id_map.insert(id, nidx);
            self.embedding.insert(id, FiveDState::default());
            self.tensor.insert(id, TensorArchive::default());
            nidx
        }
    }

    /// Upsert edge between two vertices
    pub fn upsert_edge(&mut self, from: VertexId, to: VertexId, timestamp: f64) {
        let from_idx = self.upsert_vertex(from, timestamp);
        let to_idx = self.upsert_vertex(to, timestamp);

        // Check if edge exists; if not, add it
        if !self.graph.contains_edge(from_idx, to_idx) {
            self.graph.add_edge(
                from_idx,
                to_idx,
                EdgeAnnotation {
                    birth_time: timestamp,
                    last_update: timestamp,
                    weight: 1.0,
                    active_windows: 1,
                    ..Default::default()
                },
            );
        } else {
            // Update existing edge
            if let Some(edge_idx) = self.graph.find_edge(from_idx, to_idx) {
                if let Some(ann) = self.graph.edge_weight_mut(edge_idx) {
                    ann.last_update = timestamp;
                    ann.active_windows += 1;
                    ann.weight = (ann.weight + 1.0) * 0.5; // rolling average
                }
            }
        }
    }

    /// Persistence transition: G_{k+1} = T_persist(G_k, D_obs, theta) (ISLS Sec 13.2)
    pub fn apply_observations(
        &mut self,
        obs_batch: &[Observation],
        config: &PersistenceConfig,
    ) -> Result<()> {
        if self.id_map.len() + obs_batch.len() > config.max_vertices {
            return Err(PersistError::CapacityExceeded);
        }

        // Collect vertex IDs for the whole batch upfront so we can wire
        // co-observation edges in a second pass without borrowing conflicts.
        let mut batch_vids: Vec<VertexId> = Vec::with_capacity(obs_batch.len());

        for obs in obs_batch {
            let timestamp = obs.timestamp;

            // 1. Upsert vertex
            let vid = derive_vertex_id(&obs.source_id);
            self.upsert_vertex(vid, timestamp);
            batch_vids.push(vid);

            // 2. Update edge annotations via binary payload (non-UTF-8 only).
            //    Guard: JSON/text payloads are always valid UTF-8; interpreting
            //    their bytes as raw u64 pairs creates phantom vertices.
            if obs.payload.len() >= 16 && std::str::from_utf8(&obs.payload).is_err() {
                for chunk in obs.payload.chunks_exact(16) {
                    let from_bytes: [u8; 8] = chunk[0..8].try_into().unwrap_or([0u8; 8]);
                    let to_bytes: [u8; 8] = chunk[8..16].try_into().unwrap_or([0u8; 8]);
                    let from_vid = u64::from_le_bytes(from_bytes);
                    let to_vid = u64::from_le_bytes(to_bytes);
                    if from_vid != to_vid {
                        self.upsert_edge(from_vid, to_vid, timestamp);
                    }
                }
            }

            // 3. Derive a non-trivial FiveDState embedding from the observation payload:
            //    p (potential)  — FNV-1a hash of payload bytes, mapped to [0, 1].
            //                     Changes every window because the JSON value field changes.
            //    rho (density)  — how often this entity has been observed so far,
            //                     clamped to [0, 1] with a 0.1-per-observation step.
            //    omega (freq)   — timestamp folded into one full cycle per 24 h → [0, 2π].
            //    chi            — will be set from graph degree after the edge pass.
            //    eta (causality)— neutral default 0.5; updated by Granger analysis later.
            let p = fnv_to_unit(&obs.payload);

            let activation = self
                .graph
                .node_weight(*self.id_map.get(&vid).expect("just upserted"))
                .map(|d| d.activation_count)
                .unwrap_or(1);
            let rho = (activation as f64 * 0.1_f64).min(1.0);

            let omega = (timestamp * (std::f64::consts::TAU / 86_400.0))
                .rem_euclid(std::f64::consts::TAU); // one full cycle per 24 h

            if let Some(embed) = self.embedding.get_mut(&vid) {
                embed.p     = p;
                embed.rho   = rho;
                embed.omega = omega;
                embed.eta   = 0.5;
                // chi updated below, after co-observation edges are wired
            }

            // 4. Append to tensor archive with the updated embedding snapshot.
            let snap = self.embedding.get(&vid).cloned().unwrap_or_default();
            if let Some(archive) = self.tensor.get_mut(&vid) {
                archive.push(snap, timestamp);
            }

            // 5. Decay dormant edges (one decay event per observation).
            let lambda = config.lambda_decay;
            let edge_indices: Vec<_> = self.graph.edge_indices().collect();
            for eidx in edge_indices {
                if let Some(ann) = self.graph.edge_weight_mut(eidx) {
                    ann.weight *= (-lambda).exp();
                }
            }

            // 6. Append to history (append-only, Inv I1).
            self.history.push(ObservationRecord {
                commit_index: self.commit_index,
                digest: obs.digest,
                timestamp,
            });

            // 7. Hot tier storage.
            self.hot
                .data
                .entry(vid)
                .or_default()
                .push((timestamp, obs.payload.clone()));
        }

        // ── Co-observation edges (MCCE hypha layer) ──────────────────────────
        // Entities observed in the same batch are structurally coupled: create
        // or reinforce an undirected edge between every ordered pair (u < v).
        // This ensures j = edge_count / vertex_count > 0, which is required for
        // the Kairos j-gate and for inverse_weave to find stable patterns.
        let batch_ts = obs_batch.first().map(|o| o.timestamp).unwrap_or(0.0);
        let n = batch_vids.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let (u, v) = if batch_vids[i] < batch_vids[j] {
                    (batch_vids[i], batch_vids[j])
                } else {
                    (batch_vids[j], batch_vids[i])
                };
                self.upsert_edge(u, v, batch_ts);
            }
        }

        // ── Update chi (connectivity) from graph degree ───────────────────────
        // Now that all edges exist, compute chi = degree / max_degree for every
        // vertex so the 5D embeddings reflect the graph topology.
        let max_degree = self
            .graph
            .node_indices()
            .map(|ni| self.graph.neighbors_undirected(ni).count())
            .max()
            .unwrap_or(1)
            .max(1) as f64;

        let all_vids: Vec<VertexId> = self.embedding.keys().cloned().collect();
        for vid in all_vids {
            if let Some(&nidx) = self.id_map.get(&vid) {
                let degree = self.graph.neighbors_undirected(nidx).count() as f64;
                if let Some(embed) = self.embedding.get_mut(&vid) {
                    embed.chi = degree / max_degree;
                }
            }
        }

        self.commit_index += 1;
        Ok(())
    }

    /// Append-only invariant: never delete edges or vertex history (Inv I1)
    pub fn deactivate_vertex(&mut self, id: VertexId) {
        // Mark inactive, don't remove
        if let Some(&nidx) = self.id_map.get(&id) {
            if let Some(data) = self.graph.node_weight_mut(nidx) {
                data.active = false;
            }
        }
    }

    /// Get all active vertices
    pub fn active_vertices(&self) -> Vec<VertexId> {
        self.graph
            .node_weights()
            .filter(|d| d.active)
            .map(|d| d.id)
            .collect()
    }

    /// Get embedding for a vertex
    pub fn get_embedding(&self, id: VertexId) -> Option<&FiveDState> {
        self.embedding.get(&id)
    }

    /// Get all embeddings as a point cloud (for extraction)
    pub fn point_cloud(&self) -> Vec<(VertexId, FiveDState)> {
        self.embedding
            .iter()
            .map(|(vid, state)| (*vid, state.clone()))
            .collect()
    }

    /// Compute topology signature for the current graph
    pub fn topology_signature(&self) -> isls_types::TopologySignature {
        let n = self.graph.node_count() as u64;
        let e = self.graph.edge_count() as u64;

        // Betti-0: connected components (simplified: count weakly connected components)
        let betti_0 = if n == 0 { 0 } else { count_weakly_connected(&self.graph) };
        // Betti-1: cycles estimate = E - V + components
        let betti_1 = if e + betti_0 > n { e + betti_0 - n } else { 0 };
        // Betti-2: 0 for a graph (no 2-voids in 1-skeleton)
        let betti_2 = 0u64;
        // Spectral gap: simplified estimate (for small graphs)
        let spectral_gap = compute_spectral_gap(&self.graph);
        // Euler characteristic: V - E + F (F=0 for graph)
        let euler_char = n as i64 - e as i64;

        isls_types::TopologySignature {
            betti_0,
            betti_1,
            betti_2,
            spectral_gap,
            euler_char,
        }
    }
}

/// Map a byte slice to a float in [0, 1) using FNV-1a (deterministic, no rand).
/// Used to derive a non-trivial `p` coordinate from an observation payload.
fn fnv_to_unit(data: &[u8]) -> f64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    // Use the top 53 bits as the IEEE 754 mantissa for a uniform float in [0, 1).
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Derive a vertex ID from a string (deterministic, no rand)
pub fn derive_vertex_id(s: &str) -> VertexId {
    
    
    // Note: DefaultHasher is not guaranteed deterministic across runs in general,
    // but we use a FNV-like manual hash for determinism
    let bytes = s.as_bytes();
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Count weakly connected components using union-find
fn count_weakly_connected(graph: &DiGraph<VertexData, EdgeAnnotation>) -> u64 {
    let n = graph.node_count();
    if n == 0 {
        return 0;
    }
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut Vec<usize>, x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }

    fn union(parent: &mut Vec<usize>, x: usize, y: usize) {
        let rx = find(parent, x);
        let ry = find(parent, y);
        if rx != ry {
            parent[rx] = ry;
        }
    }

    for edge in graph.raw_edges() {
        union(&mut parent, edge.source().index(), edge.target().index());
    }

    let mut roots: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for i in 0..n {
        roots.insert(find(&mut parent, i));
    }
    roots.len() as u64
}

/// Compute spectral gap of graph Laplacian (simplified for small graphs)
fn compute_spectral_gap(graph: &DiGraph<VertexData, EdgeAnnotation>) -> f64 {
    let n = graph.node_count();
    if n < 2 {
        return 0.0;
    }
    // For large graphs, skip detailed computation; return a placeholder
    if n > 100 {
        return 0.1; // placeholder
    }

    // Build degree vector and adjacency
    let node_indices: Vec<_> = graph.node_indices().collect();
    let idx_map: BTreeMap<petgraph::graph::NodeIndex, usize> = node_indices
        .iter()
        .enumerate()
        .map(|(i, &nidx)| (nidx, i))
        .collect();

    let mut laplacian = vec![vec![0.0f64; n]; n];
    for edge in graph.raw_edges() {
        let i = idx_map[&edge.source()];
        let j = idx_map[&edge.target()];
        laplacian[i][i] += 1.0;
        laplacian[j][j] += 1.0;
        laplacian[i][j] -= 1.0;
        laplacian[j][i] -= 1.0;
    }

    // Power iteration to estimate lambda_2 - lambda_1 (spectral gap)
    // Using a simplified Gershgorin estimate
    let max_diag = laplacian
        .iter()
        .enumerate()
        .map(|(i, row)| row[i])
        .fold(0.0f64, f64::max);
    let min_diag = laplacian
        .iter()
        .enumerate()
        .map(|(i, row)| row[i])
        .fold(f64::INFINITY, f64::min);

    (max_diag - min_diag).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use isls_types::Observation;

    fn make_obs(src: &str, payload: Vec<u8>, ts: f64) -> Observation {
        let digest = isls_types::content_address_raw(&payload);
        Observation {
            timestamp: ts,
            source_id: src.to_string(),
            provenance: isls_types::ProvenanceEnvelope::default(),
            payload,
            context: isls_types::MeasurementContext::default(),
            digest,
            schema_version: "1.0.0".to_string(),
        }
    }

    #[test]
    fn upsert_vertex_idempotent() {
        let mut g = PersistentGraph::new();
        let idx1 = g.upsert_vertex(42, 0.0);
        let idx2 = g.upsert_vertex(42, 1.0);
        assert_eq!(idx1, idx2);
        assert_eq!(g.id_map.len(), 1);
    }

    #[test]
    fn apply_observations_increments_commit_index() {
        let mut g = PersistentGraph::new();
        let config = PersistenceConfig::default();
        let obs = vec![make_obs("src1", b"hello".to_vec(), 1.0)];
        g.apply_observations(&obs, &config).unwrap();
        assert_eq!(g.commit_index, 1);
    }

    #[test]
    fn deactivate_vertex_preserves_history() {
        let mut g = PersistentGraph::new();
        let config = PersistenceConfig::default();
        let obs = vec![make_obs("src1", b"data".to_vec(), 1.0)];
        g.apply_observations(&obs, &config).unwrap();
        let vid = derive_vertex_id("src1");
        g.deactivate_vertex(vid);
        // History still exists
        assert!(!g.history.is_empty());
        // Vertex still exists in graph (just deactivated)
        assert!(g.id_map.contains_key(&vid));
    }

    #[test]
    fn topology_signature_empty_graph() {
        let g = PersistentGraph::new();
        let topo = g.topology_signature();
        assert_eq!(topo.betti_0, 0);
    }
}
