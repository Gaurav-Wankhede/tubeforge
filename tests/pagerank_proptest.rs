//! Property-based tests (proptest) for the PageRank algorithm.
//!
//! Verifies mathematical invariants that must hold for ANY graph:
//! - Mass conservation: ranks sum to 1.0
//! - Non-negativity: all ranks >= 0
//! - Determinism: same input → same output
//! - Empty/singleton edge cases

use proptest::prelude::*;
use std::collections::HashMap;

/// Re-export the pagerank function under test.
use tubeforge::analytics::graph::pagerank;

/// Strategy: generate a graph with UNIQUE node names + valid edges.
fn graph_strategy() -> impl Strategy<Value = (Vec<String>, Vec<(String, String, f64)>)> {
    // 2-7 unique node names (use index prefixes to guarantee uniqueness)
    (2..7usize)
        .prop_flat_map(|n| {
            let nodes: Vec<String> = (0..n).map(|i| format!("n{}", i)).collect();
            // Generate edges referencing valid node indices
            let edge_count = n * 2;
            let edges = prop::collection::vec(
                (
                    0..n,
                    0..n,
                    prop::num::f64::POSITIVE.prop_filter("gt0", |&w| w > 0.0),
                )
                    .prop_filter("no self-loops", |&(f, t, _)| f != t),
                0..edge_count,
            );
            (Just(nodes), edges)
        })
        .prop_map(
            |(nodes, raw_edges): (Vec<String>, Vec<(usize, usize, f64)>)| {
                let edges: Vec<(String, String, f64)> = raw_edges
                    .into_iter()
                    .map(|(f, t, w)| (nodes[f].clone(), nodes[t].clone(), w))
                    .collect();
                (nodes, edges)
            },
        )
}

/// Strategy: damping factor in (0, 1).
fn damping() -> impl Strategy<Value = f64> {
    prop::num::f64::POSITIVE.prop_filter("lt1", |&d| d < 1.0)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// PROPERTY: Mass conservation — ranks always sum to 1.0 for any non-empty graph.
    #[test]
    fn pagerank_mass_conserved(
        (nodes, edges) in graph_strategy(),
        damp in damping(),
    ) {
        let result = pagerank(&nodes, &edges, damp, 50);
        let sum: f64 = result.values().sum();
        prop_assert!(
            (sum - 1.0).abs() < 1e-9,
            "mass not conserved: sum={} nodes={:?} edges={}",
            sum, nodes, edges.len()
        );
    }

    /// PROPERTY: Non-negativity — all ranks are >= 0.
    #[test]
    fn pagerank_non_negative(
        (nodes, edges) in graph_strategy(),
        damp in damping(),
    ) {
        let result = pagerank(&nodes, &edges, damp, 50);
        for (node, &rank) in &result {
            prop_assert!(
                rank >= 0.0,
                "negative rank for {}: {} (nodes={:?})",
                node, rank, nodes
            );
        }
    }

    /// PROPERTY: Determinism — same input always produces same output.
    #[test]
    fn pagerank_deterministic(
        (nodes, edges) in graph_strategy(),
        damp in damping(),
    ) {
        let r1 = pagerank(&nodes, &edges, damp, 50);
        let r2 = pagerank(&nodes, &edges, damp, 50);
        prop_assert_eq!(r1, r2, "pagerank is not deterministic");
    }

    /// PROPERTY: Every node gets a rank entry (no node is dropped).
    #[test]
    fn pagerank_all_nodes_ranked(
        (nodes, edges) in graph_strategy(),
        damp in damping(),
    ) {
        let result = pagerank(&nodes, &edges, damp, 50);
        for node in &nodes {
            prop_assert!(
                result.contains_key(node),
                "node {} missing from ranks (nodes={:?})",
                node, nodes
            );
        }
    }

    /// PROPERTY: Ranks are bounded — no rank exceeds 1.0.
    #[test]
    fn pagerank_bounded_by_one(
        (nodes, edges) in graph_strategy(),
        damp in damping(),
    ) {
        let result = pagerank(&nodes, &edges, damp, 50);
        for (node, &rank) in &result {
            prop_assert!(
                rank <= 1.0 + 1e-9,
                "rank exceeds 1.0 for {}: {}", node, rank
            );
        }
    }
}

/// Unit tests for edge cases (kept separate from proptest for clarity).
#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn pagerank_empty_graph() {
        let result: HashMap<String, f64> = pagerank(&[], &[], 0.85, 50);
        assert!(result.is_empty());
    }

    #[test]
    fn pagerank_singleton() {
        let nodes = vec!["solo".to_string()];
        let result = pagerank(&nodes, &[], 0.85, 50);
        assert_eq!(result["solo"], 1.0);
    }
}
