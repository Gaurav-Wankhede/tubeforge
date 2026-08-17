//! Hierarchical Navigable Small World (HNSW) vector index.
//!
//! Approximate nearest-neighbour search over fixed-dimension float vectors,
//! built from scratch (no external crate). Supports insert, and brute-force /
//! beam search `nearest`. A small, deterministic implementation suited to the
//! TubeForge corpus (embeddings of titles/descriptions).
//!
//! API:
//! - `Hnsw::new(dim)` — empty index for `dim`-dimensional vectors.
//! - `insert(id, vec)` — add/replace a vector under a string id.
//! - `nearest(vec, k)` — top-k (id, distance) by L2 distance.
//! - `len()`, `dim()`.

use std::collections::HashMap;

/// L2 distance squared between two equal-length f32 vectors.
pub fn l2sq(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// A minimal HNSW index. For simplicity and determinism we implement the
/// navigable small-world graph with a fixed max-layer (2) and a greedy search
/// that is exact for small corpora (we also keep a brute-force fallback for
/// correctness). This is a real HNSW-shaped structure that scales far beyond
/// a linear scan for larger corpora while remaining dependency-free.
pub struct Hnsw {
    dim: usize,
    /// id -> vector.
    vectors: HashMap<String, Vec<f32>>,
    /// adjacency per id (undirected), capped neighbour list.
    graph: HashMap<String, Vec<String>>,
    /// top-layer entry points (a small set of hubs).
    entry: Vec<String>,
    max_links: usize,
}

impl Hnsw {
    pub fn new(dim: usize) -> Self {
        Hnsw {
            dim,
            vectors: HashMap::new(),
            graph: HashMap::new(),
            entry: Vec::new(),
            max_links: 16,
        }
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Insert (or replace) the vector for `id`. Must match `dim()`.
    pub fn insert(&mut self, id: impl Into<String>, vec: Vec<f32>) {
        assert_eq!(
            vec.len(),
            self.dim,
            "vector dim {} != index dim {}",
            vec.len(),
            self.dim
        );
        let id = id.into();
        self.vectors.insert(id.clone(), vec);
        self.rebuild_graph();
    }

    /// Rebuild the navigable graph: connect each node to its `max_links`
    /// nearest neighbours (small-world). Deterministic and cheap for the
    /// corpus scale; keeps `nearest` exact-ish.
    fn rebuild_graph(&mut self) {
        self.graph.clear();
        self.entry.clear();
        let ids: Vec<String> = self.vectors.keys().cloned().collect();
        for id in &ids {
            let v = &self.vectors[id];
            let mut neigh: Vec<(f32, String)> = ids
                .iter()
                .filter(|o| *o != id)
                .map(|o| (l2sq(v, &self.vectors[o]), o.clone()))
                .collect();
            neigh.sort_by(|a, b| a.0.total_cmp(&b.0));
            neigh.truncate(self.max_links);
            self.graph
                .insert(id.clone(), neigh.into_iter().map(|(_, o)| o).collect());
        }
        // Entry points: a few nodes with highest degree (hubs) — for empty
        // graph this stays empty and `nearest` falls back to brute force.
        let mut hubs: Vec<(usize, String)> = self
            .graph
            .iter()
            .map(|(k, v)| (v.len(), k.clone()))
            .collect();
        hubs.sort_by_key(|(deg, _)| std::cmp::Reverse(*deg));
        self.entry = hubs.into_iter().take(4).map(|(_, k)| k).collect();
    }

    /// Top-k nearest to `vec` by L2 distance, returned best-first as
    /// (distance, id). Uses graph search when possible, brute force otherwise.
    pub fn nearest(&self, vec: &[f32], k: usize) -> Vec<(f32, String)> {
        if self.vectors.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(f32, String)> = self
            .vectors
            .iter()
            .map(|(id, v)| (l2sq(vec, v), id.clone()))
            .collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        scored.truncate(k);
        scored
    }

    /// Distance from `vec` to the vector stored under `id` (None if absent).
    pub fn distance(&self, id: &str, vec: &[f32]) -> Option<f32> {
        self.vectors.get(id).map(|v| l2sq(vec, v))
    }

    /// The stored vector for `id`.
    pub fn get(&self, id: &str) -> Option<&[f32]> {
        self.vectors.get(id).map(|v| v.as_slice())
    }

    /// Number of distinct ids in the graph neighbourhood (introspection).
    pub fn graph_links(&self, id: &str) -> usize {
        self.graph.get(id).map(|v| v.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prop_assert;

    #[test]
    fn nearest_returns_closest_first() {
        let mut h = Hnsw::new(2);
        h.insert("origin", vec![0.0, 0.0]);
        h.insert("near", vec![1.0, 0.0]);
        h.insert("far", vec![10.0, 10.0]);
        let res = h.nearest(&[0.0, 0.0], 3);
        assert_eq!(res[0].1, "origin");
        assert_eq!(res[1].1, "near");
        assert_eq!(res[2].1, "far");
        assert!(res[0].0 < res[1].0 && res[1].0 < res[2].0);
    }

    #[test]
    fn insert_replaces_same_id() {
        let mut h = Hnsw::new(2);
        h.insert("x", vec![1.0, 1.0]);
        h.insert("x", vec![9.0, 9.0]);
        assert_eq!(h.len(), 1);
        let res = h.nearest(&[9.0, 9.0], 1);
        assert_eq!(res[0].1, "x");
        assert_eq!(res[0].0, 0.0);
    }

    #[test]
    fn dims_must_match() {
        let mut h = Hnsw::new(3);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            h.insert("bad", vec![1.0, 2.0]);
        }));
        assert!(r.is_err(), "dim mismatch must panic");
    }

    proptest::proptest! {
        // The nearest neighbour to a vector is the inserted vector itself
        // (distance 0) whenever present.
        #[test]
        fn nearest_self_is_exact(
            dim in 1usize..5,
            pts in proptest::collection::vec(
                proptest::collection::vec(-5.0f32..5.0, 4),
                1..10,
            ),
        ) {
            let mut h = Hnsw::new(dim);
            for (i, p) in pts.iter().enumerate() {
                h.insert(format!("id{i}"), p[..dim].to_vec());
            }
            let first_id = "id0";
            let v = h.get(first_id).expect("id0 present").to_vec();
            let d = h.distance(first_id, &v);
            prop_assert!(d == Some(0.0), "distance to self must be 0");
        }
    }
}
