//! Property-graph layer: typed nodes + typed, weighted edges with adjacency
//! and PageRank. Operates on in-memory `Vec`s of nodes/edges — a reusable,
//! dependency-free graph engine that can back the DB's `edges`/`kg_relations`
//! tables and GNN-friendly workloads.
//!
//! Nodes and edges are generic over a `String` node id and hold optional
//! `properties` (a JSON value). `Edges` are directed `(from, to, ty, weight)`.

use std::collections::HashMap;

use serde_json::Value as J;

/// A typed, weighted directed edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub ty: String,
    pub weight: f64,
    pub properties: Option<J>,
}

impl Edge {
    pub fn new(from: impl Into<String>, to: impl Into<String>, ty: impl Into<String>) -> Self {
        Edge {
            from: from.into(),
            to: to.into(),
            ty: ty.into(),
            weight: 1.0,
            properties: None,
        }
    }
}

/// A mutable in-memory graph.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    nodes: Vec<String>,
    edges: Vec<Edge>,
    index: HashMap<String, usize>,
}

impl Graph {
    pub fn new() -> Self {
        Graph::default()
    }

    /// Add a node id (no-op if present). Returns its stable index.
    pub fn add_node(&mut self, id: impl Into<String>) -> usize {
        let id = id.into();
        if let Some(&i) = self.index.get(&id) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(id.clone());
        self.index.insert(id, i);
        i
    }

    /// Add or replace an edge (keyed by from/to/ty). Returns the prior weight
    /// if it replaced one.
    pub fn upsert_edge(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        ty: impl Into<String>,
        weight: f64,
    ) -> Option<f64> {
        let from = from.into();
        let to = to.into();
        let ty = ty.into();
        self.add_node(from.clone());
        self.add_node(to.clone());
        self.upsert_edge_inner(from, to, ty, weight)
    }

    fn upsert_edge_inner(&mut self, from: String, to: String, ty: String, weight: f64) -> Option<f64> {
        for e in &mut self.edges {
            if e.from == from && e.to == to && e.ty == ty {
                let old = e.weight;
                e.weight = weight;
                return Some(old);
            }
        }
        self.edges.push(Edge {
            from,
            to,
            ty,
            weight,
            properties: None,
        });
        None
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Outgoing edges of `id` (all types).
    pub fn out_edges(&self, id: &str) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.from == id).collect()
    }

    /// Incoming edges of `id` (all types).
    pub fn in_edges(&self, id: &str) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.to == id).collect()
    }

    /// The adjacency list (from node -> target node ids) over edges of any
    /// type. Used for reachability and PageRank.
    pub fn adjacency(&self) -> HashMap<String, Vec<String>> {
        let mut m: HashMap<String, Vec<String>> = HashMap::new();
        for e in &self.edges {
            m.entry(e.from.clone()).or_default().push(e.to.clone());
        }
        m
    }

    /// Reverse adjacency (in-degree map).
    pub fn in_adjacency(&self) -> HashMap<String, Vec<String>> {
        let mut m: HashMap<String, Vec<String>> = HashMap::new();
        for e in &self.edges {
            m.entry(e.to.clone()).or_default().push(e.from.clone());
        }
        m
    }

    /// PageRank over all nodes, `damping` ∈ (0,1), `iterations` passes.
    /// Mass is conserved (sum ≈ 1.0) including for dangling nodes: the mass
    /// of nodes with no out-edges is redistributed uniformly across all nodes
    /// each iteration (standard Google-style dangling handling), so no rank
    /// leaks out of the graph.
    pub fn pagerank(&self, damping: f64, iterations: usize) -> HashMap<String, f64> {
        let n = self.nodes.len() as f64;
        if n == 0.0 {
            return HashMap::new();
        }
        let mut pr: HashMap<String, f64> =
            self.nodes.iter().map(|id| (id.clone(), 1.0 / n)).collect();
        let out = self.adjacency();
        let out_degree: HashMap<&String, usize> =
            out.iter().map(|(k, v)| (k, v.len())).collect();

        for _ in 0..iterations {
            let base = (1.0 - damping) / n;
            // Mass contributed by dangling nodes (no out-links) is spread
            // over all nodes, preserving total mass.
            let dangling_mass: f64 = self
                .nodes
                .iter()
                .filter(|id| out_degree.get(*id).copied().unwrap_or(0) == 0)
                .map(|id| pr.get(id).copied().unwrap_or(0.0))
                .sum();

            let mut next: HashMap<String, f64> = self
                .nodes
                .iter()
                .map(|id| (id.clone(), base + damping * dangling_mass / n))
                .collect();
            for e in &self.edges {
                let deg = out_degree.get(&e.from).copied().unwrap_or(0);
                if deg == 0 {
                    continue;
                }
                let share = pr.get(&e.from).copied().unwrap_or(0.0) / deg as f64;
                *next.entry(e.to.clone()).or_insert(0.0) += damping * share;
            }
            pr = next;
        }
        pr
    }

    /// Connected components (undirected, union-find over all edges).
    pub fn components(&self) -> Vec<Vec<String>> {
        let mut parent: HashMap<String, String> = self.nodes.iter().map(|n| (n.clone(), n.clone())).collect();
        fn find(parent: &mut HashMap<String, String>, x: &str) -> String {
            let p = parent.get(x).cloned().unwrap_or_else(|| x.to_string());
            if p != x {
                let root = find(parent, &p);
                parent.insert(x.to_string(), root.clone());
                root
            } else {
                p
            }
        }
        fn union(parent: &mut HashMap<String, String>, a: &str, b: &str) {
            let ra = find(parent, a);
            let rb = find(parent, b);
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
        for e in &self.edges {
            union(&mut parent, &e.from, &e.to);
        }
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for n in &self.nodes {
            map.entry(find(&mut parent, n)).or_default().push(n.clone());
        }
        map.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prop_assert;

    #[test]
    fn pagerank_conserves_mass_on_star() {
        let mut g = Graph::new();
        g.add_node("center");
        for leaf in ["a", "b", "c", "d"] {
            g.add_node(leaf);
            // Leaves link INTO the center → center accumulates rank.
            g.upsert_edge(leaf, "center", "links", 1.0);
        }
        let pr = g.pagerank(0.85, 50);
        let sum: f64 = pr.values().sum();
        assert!((sum - 1.0).abs() < 1e-6, "mass conserved, sum={sum}");
        assert!(pr["center"] > pr["a"], "center dominates leaves");
    }

    #[test]
    fn components_find_disconnected_groups() {
        let mut g = Graph::new();
        for n in ["a", "b", "c", "d"] {
            g.add_node(n);
        }
        g.upsert_edge("a", "b", "x", 1.0);
        g.upsert_edge("c", "d", "x", 1.0);
        let comps = g.components();
        assert_eq!(comps.len(), 2);
    }

    #[test]
    fn upsert_edge_replaces_weight() {
        let mut g = Graph::new();
        g.upsert_edge("a", "b", "overlap", 1.0);
        let replaced = g.upsert_edge("a", "b", "overlap", 2.5);
        assert_eq!(replaced, Some(1.0));
        assert_eq!(g.edge_count(), 1);
        assert_eq!(g.edges()[0].weight, 2.5);
    }

    proptest::proptest! {
        // PageRank mass is conserved for ANY edge set over a fixed node set.
        #[test]
        fn pagerank_mass_conserved(
            nodes in proptest::collection::vec("(a-z){2}", 1..8),
            pairs in proptest::collection::vec(("(a-z){2}", "(a-z){2}"), 0..30),
        ) {
            let mut g = Graph::new();
            for n in &nodes {
                g.add_node(n.clone());
            }
            for (a, b) in &pairs {
                // Only create edges between known nodes to keep semantics clear.
                if nodes.contains(a) && nodes.contains(b) {
                    g.upsert_edge(a.clone(), b.clone(), "links", 1.0);
                }
            }
            let pr = g.pagerank(0.85, 30);
            let sum: f64 = pr.values().sum();
            prop_assert!((sum - 1.0).abs() < 1e-6, "mass conserved, sum={sum}");
        }
    }
}
