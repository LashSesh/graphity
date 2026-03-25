// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Coupling graph with spectral decomposition methods.
//!
//! Provides Fiedler bisection, Kuramoto phase grouping, and singularity
//! detection for the hypercube decomposer.

use serde::{Deserialize, Serialize};

// ─── Fiedler Result ──────────────────────────────────────────────────────────

/// Result of Fiedler (algebraic connectivity) computation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FiedlerResult {
    /// Second-smallest eigenvalue of the Laplacian (λ₂).
    pub value: f64,
    /// Fiedler vector (eigenvector corresponding to λ₂).
    pub vector: Vec<f64>,
}

// ─── Dimension Group ─────────────────────────────────────────────────────────

/// A group of tightly-coupled dimensions identified by Kuramoto synchronisation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DimGroup {
    /// Dimension names in this group.
    pub dimensions: Vec<String>,
    /// Average coupling strength within the group.
    pub avg_coupling: f64,
    /// Estimated total lines of code.
    pub estimated_loc: usize,
}

// ─── Singularity ─────────────────────────────────────────────────────────────

/// A singularity: a dimension whose removal causes a large spectral gap change.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Singularity {
    /// Dimension name.
    pub dimension: String,
    /// Fraction of the graph affected by this singularity.
    pub impact: f64,
}

// ─── Coupling Graph ──────────────────────────────────────────────────────────

/// Graph of couplings between dimensions, with spectral analysis methods.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouplingGraph {
    /// Dimension names (node labels).
    pub nodes: Vec<String>,
    /// Edges as (from_idx, to_idx, weight).
    pub edges: Vec<(usize, usize, f64)>,
    /// Adjacency list: adjacency[i] = [(j, weight), ...].
    pub adjacency: Vec<Vec<(usize, f64)>>,
}

impl CouplingGraph {
    /// Compute the graph Laplacian L = D - A as a dense matrix.
    pub fn laplacian(&self) -> Vec<Vec<f64>> {
        let n = self.nodes.len();
        let mut l = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            for &(j, w) in &self.adjacency[i] {
                l[i][j] -= w;
                l[i][i] += w;
            }
        }
        l
    }

    /// Compute the Fiedler value (λ₂) and Fiedler vector using nalgebra.
    pub fn fiedler(&self) -> FiedlerResult {
        let n = self.nodes.len();
        if n <= 1 {
            return FiedlerResult {
                value: 0.0,
                vector: vec![0.0; n],
            };
        }

        let lap = self.laplacian();
        let mat = nalgebra::DMatrix::from_fn(n, n, |i, j| lap[i][j]);
        let eigen = nalgebra::linalg::SymmetricEigen::new(mat);

        // Sort eigenvalues and find second-smallest
        let mut indexed: Vec<(usize, f64)> = eigen
            .eigenvalues
            .iter()
            .enumerate()
            .map(|(i, &v)| (i, v))
            .collect();
        indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let fiedler_idx = if indexed.len() >= 2 { indexed[1].0 } else { 0 };
        let fiedler_val = if indexed.len() >= 2 { indexed[1].1 } else { 0.0 };

        let fiedler_vec: Vec<f64> = (0..n)
            .map(|i| eigen.eigenvectors[(i, fiedler_idx)])
            .collect();

        FiedlerResult {
            value: fiedler_val.max(0.0),
            vector: fiedler_vec,
        }
    }

    /// Bisect the graph using the sign of the Fiedler vector.
    /// Returns (left_partition, right_partition) of dimension names.
    pub fn fiedler_bisect(&self) -> (Vec<String>, Vec<String>) {
        let fiedler = self.fiedler();
        let mut left = Vec::new();
        let mut right = Vec::new();
        for (i, &v) in fiedler.vector.iter().enumerate() {
            if v < 0.0 {
                left.push(self.nodes[i].clone());
            } else {
                right.push(self.nodes[i].clone());
            }
        }
        // Ensure neither partition is empty
        if left.is_empty() && !right.is_empty() {
            left.push(right.pop().expect("right is non-empty"));
        } else if right.is_empty() && !left.is_empty() {
            right.push(left.pop().expect("left is non-empty"));
        }
        (left, right)
    }

    /// Identify groups of synchronized dimensions using Kuramoto oscillator model.
    ///
    /// * `kappa` — coupling strength for the Kuramoto model.
    /// * `threshold` — phase proximity threshold for grouping (radians).
    pub fn kuramoto_groups(&self, kappa: f64, threshold: f64) -> Vec<DimGroup> {
        let n = self.nodes.len();
        if n == 0 {
            return vec![];
        }

        // Initialize phases spread over [0, 2π)
        let mut phases: Vec<f64> = (0..n)
            .map(|i| 2.0 * std::f64::consts::PI * (i as f64) / (n as f64))
            .collect();

        // Natural frequencies from node degree
        let mut omega: Vec<f64> = vec![0.0; n];
        for i in 0..n {
            omega[i] = 0.1 * self.adjacency[i].len() as f64;
        }

        // RK4 integration (100 steps, dt=0.05)
        let dt = 0.05;
        let steps = 100;
        for _ in 0..steps {
            let k1 = self.kuramoto_derivative(&phases, &omega, kappa);
            let p1: Vec<f64> = phases.iter().zip(&k1).map(|(p, k)| p + 0.5 * dt * k).collect();
            let k2 = self.kuramoto_derivative(&p1, &omega, kappa);
            let p2: Vec<f64> = phases.iter().zip(&k2).map(|(p, k)| p + 0.5 * dt * k).collect();
            let k3 = self.kuramoto_derivative(&p2, &omega, kappa);
            let p3: Vec<f64> = phases.iter().zip(&k3).map(|(p, k)| p + dt * k).collect();
            let k4 = self.kuramoto_derivative(&p3, &omega, kappa);

            for i in 0..n {
                phases[i] += dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
            }
        }

        // Wrap phases to [0, 2π)
        for p in &mut phases {
            *p = p.rem_euclid(2.0 * std::f64::consts::PI);
        }

        // Cluster by phase proximity
        let mut assigned = vec![false; n];
        let mut groups = Vec::new();

        for i in 0..n {
            if assigned[i] {
                continue;
            }
            let mut group_dims = vec![self.nodes[i].clone()];
            assigned[i] = true;

            for j in (i + 1)..n {
                if assigned[j] {
                    continue;
                }
                let diff = (phases[i] - phases[j]).abs();
                let phase_dist = diff.min(2.0 * std::f64::consts::PI - diff);
                if phase_dist < threshold {
                    group_dims.push(self.nodes[j].clone());
                    assigned[j] = true;
                }
            }

            if group_dims.len() > 1 {
                let avg_coupling = self.avg_coupling_within(&group_dims);
                groups.push(DimGroup {
                    dimensions: group_dims,
                    avg_coupling,
                    estimated_loc: 0,
                });
            }
        }

        groups
    }

    /// Detect singularities: dimensions whose removal maximally changes the spectral gap.
    ///
    /// Returns dimensions where the relative change exceeds `gap_threshold`.
    pub fn singularities(&self, gap_threshold: f64) -> Vec<Singularity> {
        let n = self.nodes.len();
        if n <= 2 {
            return vec![];
        }

        let base_fiedler = self.fiedler().value;
        if base_fiedler < 1e-12 {
            return vec![];
        }

        let mut results = Vec::new();

        for remove_idx in 0..n {
            // Build sub-graph without node remove_idx
            let mut sub_nodes = Vec::new();
            let mut old_to_new = vec![None; n];
            let mut new_idx = 0usize;
            for i in 0..n {
                if i != remove_idx {
                    old_to_new[i] = Some(new_idx);
                    sub_nodes.push(self.nodes[i].clone());
                    new_idx += 1;
                }
            }

            let m = sub_nodes.len();
            let mut sub_adj: Vec<Vec<(usize, f64)>> = vec![vec![]; m];
            let mut sub_edges = Vec::new();

            for i in 0..n {
                if i == remove_idx {
                    continue;
                }
                let ni = old_to_new[i].expect("mapped");
                for &(j, w) in &self.adjacency[i] {
                    if j == remove_idx {
                        continue;
                    }
                    let nj = old_to_new[j].expect("mapped");
                    sub_adj[ni].push((nj, w));
                    if ni < nj {
                        sub_edges.push((ni, nj, w));
                    }
                }
            }

            let sub_graph = CouplingGraph {
                nodes: sub_nodes,
                edges: sub_edges,
                adjacency: sub_adj,
            };

            let new_fiedler = sub_graph.fiedler().value;
            let change = (new_fiedler - base_fiedler).abs() / base_fiedler;

            if change > gap_threshold {
                results.push(Singularity {
                    dimension: self.nodes[remove_idx].clone(),
                    impact: change,
                });
            }
        }

        // Sort by impact descending
        results.sort_by(|a, b| b.impact.partial_cmp(&a.impact).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    // ─── Private helpers ─────────────────────────────────────────────────────

    fn kuramoto_derivative(&self, phases: &[f64], omega: &[f64], kappa: f64) -> Vec<f64> {
        let n = phases.len();
        let mut dphi = vec![0.0; n];
        for i in 0..n {
            let mut coupling_sum = 0.0;
            for &(j, w) in &self.adjacency[i] {
                coupling_sum += w * (phases[j] - phases[i]).sin();
            }
            dphi[i] = omega[i] + kappa * coupling_sum / n.max(1) as f64;
        }
        dphi
    }

    fn avg_coupling_within(&self, dim_names: &[String]) -> f64 {
        let name_set: std::collections::HashSet<&str> =
            dim_names.iter().map(|s| s.as_str()).collect();
        let mut total = 0.0;
        let mut count = 0usize;
        for &(i, j, w) in &self.edges {
            if name_set.contains(self.nodes[i].as_str())
                && name_set.contains(self.nodes[j].as_str())
            {
                total += w;
                count += 1;
            }
        }
        if count > 0 {
            total / count as f64
        } else {
            0.0
        }
    }
}
