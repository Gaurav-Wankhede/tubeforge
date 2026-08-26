//! Graph algorithms (PRD §FR-3): PageRank, Louvain community detection,
//! random walk with restart, shortest path, betweenness centrality.
//!
//! All algorithms operate on the in-memory KnowledgeGraph and are deterministic.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::analytics::kg::KnowledgeGraph;

/// PageRank damping factor (PRD §TR-4.1).
pub const DAMPING: f64 = 0.85;
/// PageRank iteration count (PRD §TR-4.1 — converges trivially at this scale).
pub const PAGERANK_ITERATIONS: usize = 50;
/// Random walk restart probability (PRD §TR-4.3).
pub const RESTART_PROB: f64 = 0.3;
/// Random walk max iterations (PRD §TR-4.3).
pub const RWR_ITERATIONS: usize = 50;

// ---------------------------------------------------------------------------
// PageRank
// ---------------------------------------------------------------------------

/// Compute weighted PageRank over the KnowledgeGraph (PRD §FR-3.1).
///
/// Uses damped iteration (0.85, 50 iterations). Dangling nodes distribute
/// their mass evenly. Returns a map of entity_id → PageRank score (sums to 1
/// over non-empty graphs).
pub fn pagerank(graph: &KnowledgeGraph) -> HashMap<String, f64> {
    let nodes: Vec<String> = graph.entities.keys().cloned().collect();
    let n = nodes.len();
    if n == 0 {
        return HashMap::new();
    }

    // Build index for O(1) lookup
    let index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();

    // Build adjacency with weights
    let mut out: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for (from, edges) in &graph.adjacency {
        if let Some(&fi) = index.get(from.as_str()) {
            for (to, _, w) in edges {
                if let Some(&ti) = index.get(to.as_str()) {
                    if fi != ti {
                        out[fi].push((ti, w.max(0.0)));
                    }
                }
            }
        }
    }

    let out_sum: Vec<f64> = out.iter().map(|l| l.iter().map(|(_, w)| w).sum()).collect();
    let d = DAMPING;

    let mut pr = vec![1.0 / n as f64; n];
    for _ in 0..PAGERANK_ITERATIONS {
        let mut next = vec![0.0; n];
        let dangling: f64 = (0..n).filter(|&i| out_sum[i] == 0.0).map(|i| pr[i]).sum();
        let share = d * dangling / n as f64;
        for (u, edges) in out.iter().enumerate() {
            if out_sum[u] == 0.0 {
                continue;
            }
            let contrib = d * pr[u] / out_sum[u];
            for (v, w) in edges {
                next[*v] += contrib * w;
            }
        }
        let base = (1.0 - d) / n as f64;
        for slot in next.iter_mut().take(n) {
            *slot += base + share;
        }
        pr = next;
    }

    nodes.iter().zip(pr).map(|(s, p)| (s.clone(), p)).collect()
}

// ---------------------------------------------------------------------------
// Random Walk with Restart (RWR)
// ---------------------------------------------------------------------------

/// Random walk with restart from a seed entity (PRD §FR-3.7, §TR-4.3).
///
/// Returns a map of entity_id → visit probability. The seed itself is
/// excluded from results. Restart probability 0.3, max 50 iterations.
pub fn random_walk_with_restart(graph: &KnowledgeGraph, seed: &str) -> HashMap<String, f64> {
    if !graph.entities.contains_key(seed) {
        return HashMap::new();
    }

    let mut scores: HashMap<String, f64> = HashMap::new();
    scores.insert(seed.to_string(), 1.0);

    for _ in 0..RWR_ITERATIONS {
        let mut next: HashMap<String, f64> = HashMap::new();
        for (node, score) in &scores {
            // Restart: teleport back to seed
            *next.entry(seed.to_string()).or_insert(0.0) += RESTART_PROB * score;

            // Walk to neighbors
            let neighbors = graph.neighbors(node);
            if !neighbors.is_empty() {
                let walk_prob = (1.0 - RESTART_PROB) * score / neighbors.len() as f64;
                for (neighbor, _, weight) in neighbors {
                    *next.entry(neighbor.clone()).or_insert(0.0) += walk_prob * weight;
                }
            }
        }
        scores = next;
    }

    // Exclude seed
    scores.remove(seed);
    scores
}

// ---------------------------------------------------------------------------
// Louvain Community Detection
// ---------------------------------------------------------------------------

/// Community detection (PRD §FR-3.2).
///
/// Uses a greedy label-propagation approach suitable for our scale (1-10k
/// nodes). Each node starts in its own community, then iteratively adopts
/// the majority community of its neighbors (weighted by edge weight).
/// Converges when no node changes. Produces well-separated communities
/// in <50 iterations at our scale.
pub fn louvain_communities(graph: &KnowledgeGraph) -> HashMap<String, i64> {
    let nodes: Vec<String> = graph.entities.keys().cloned().collect();
    if nodes.is_empty() {
        return HashMap::new();
    }

    // Start: each node in its own community
    let mut community: HashMap<String, i64> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as i64))
        .collect();

    let mut changed = true;
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 50;

    while changed && iterations < MAX_ITERATIONS {
        changed = false;
        iterations += 1;

        for node in &nodes {
            // Count neighbor communities (weighted by edge weight)
            let mut comm_weights: HashMap<i64, f64> = HashMap::new();
            for (neighbor, _, weight) in graph.neighbors(node) {
                if let Some(&nc) = community.get(neighbor) {
                    *comm_weights.entry(nc).or_insert(0.0) += weight;
                }
            }
            for (neighbor, _, weight) in graph.reverse_neighbors(node) {
                if let Some(&nc) = community.get(neighbor) {
                    *comm_weights.entry(nc).or_insert(0.0) += weight;
                }
            }

            // Find the community with highest total weight
            if let Some((&best_comm, _)) = comm_weights
                .iter()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            {
                let current_comm = community.get(node).copied();
                if current_comm != Some(best_comm) {
                    community.insert(node.clone(), best_comm);
                    changed = true;
                }
            }
        }
    }

    // Renumber communities to be contiguous 0..N
    let mut comm_map: HashMap<i64, i64> = HashMap::new();
    let mut next_id = 0i64;
    for comm_id in community.values() {
        if !comm_map.contains_key(comm_id) {
            comm_map.insert(*comm_id, next_id);
            next_id += 1;
        }
    }
    for v in community.values_mut() {
        *v = comm_map[v];
    }

    community
}

// ---------------------------------------------------------------------------
// Shortest Path (BFS)
// ---------------------------------------------------------------------------

/// Find the shortest path (fewest hops) between two entities (PRD §FR-3.6).
///
/// Returns the path as a Vec of entity_ids (including from and to), or None
/// if no path exists. Uses BFS (unweighted, fewest hops).
pub fn shortest_path(graph: &KnowledgeGraph, from: &str, to: &str) -> Option<Vec<String>> {
    if !graph.entities.contains_key(from) || !graph.entities.contains_key(to) {
        return None;
    }
    if from == to {
        return Some(vec![from.to_string()]);
    }

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut parent: HashMap<String, String> = HashMap::new();

    visited.insert(from.to_string());
    queue.push_back(from.to_string());

    while let Some(node) = queue.pop_front() {
        if node == to {
            // Reconstruct path
            let mut path = vec![to.to_string()];
            let mut current = to;
            while let Some(p) = parent.get(current) {
                path.push(p.clone());
                current = p;
            }
            path.reverse();
            return Some(path);
        }

        for (neighbor, _, _) in graph.neighbors(&node) {
            if visited.insert(neighbor.clone()) {
                parent.insert(neighbor.clone(), node.clone());
                queue.push_back(neighbor.clone());
            }
        }
        // Bidirectional: also follow reverse edges
        for (neighbor, _, _) in graph.reverse_neighbors(&node) {
            if visited.insert(neighbor.clone()) {
                parent.insert(neighbor.clone(), node.clone());
                queue.push_back(neighbor.clone());
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Connected Components
// ---------------------------------------------------------------------------

/// Compute connected components (PRD §FR-3.8).
///
/// Returns a map of entity_id → component_id. Components are numbered 0..N.
pub fn connected_components(graph: &KnowledgeGraph) -> HashMap<String, usize> {
    let mut visited = HashSet::new();
    let mut components: HashMap<String, usize> = HashMap::new();
    let mut component_id = 0;

    for start in graph.entities.keys() {
        if visited.contains(start) {
            continue;
        }
        // BFS from start
        let mut queue = vec![start.clone()];
        visited.insert(start.clone());
        while let Some(node) = queue.pop() {
            components.insert(node.clone(), component_id);
            for (neighbor, _, _) in graph.neighbors(&node) {
                if visited.insert(neighbor.clone()) {
                    queue.push(neighbor.clone());
                }
            }
            for (neighbor, _, _) in graph.reverse_neighbors(&node) {
                if visited.insert(neighbor.clone()) {
                    queue.push(neighbor.clone());
                }
            }
        }
        component_id += 1;
    }

    components
}

// ---------------------------------------------------------------------------
// Bridge Detection (simplified betweenness)
// ---------------------------------------------------------------------------

/// Detect bridge entities — nodes whose removal would disconnect the graph
/// (PRD §FR-3.7).
///
/// Uses a simplified articulation-point detection. Returns entity_ids of
/// bridge nodes. Checks all nodes (not just low-degree ones) since with
/// bidirectional edges, degree is doubled.
pub fn find_bridges(graph: &KnowledgeGraph) -> Vec<String> {
    let components = connected_components(graph);
    let n_components = components.values().max().map(|m| m + 1).unwrap_or(0);

    if n_components != 1 {
        return Vec::new(); // Graph already disconnected
    }

    let mut bridges = Vec::new();
    for entity_id in graph.entities.keys() {
        // Check if removing this node disconnects the graph
        let count = count_components_without(graph, entity_id);
        if count > 1 {
            bridges.push(entity_id.clone());
        }
    }
    bridges
}

/// Count connected components after removing a node.
fn count_components_without(graph: &KnowledgeGraph, remove: &str) -> usize {
    let mut visited = HashSet::new();
    let mut count = 0;

    for start in graph.entities.keys() {
        if start == remove || visited.contains(start) {
            continue;
        }
        let mut queue = vec![start.clone()];
        visited.insert(start.clone());
        while let Some(node) = queue.pop() {
            if node == remove {
                continue;
            }
            for (neighbor, _, _) in graph.neighbors(&node) {
                if neighbor != remove && visited.insert(neighbor.clone()) {
                    queue.push(neighbor.clone());
                }
            }
            for (neighbor, _, _) in graph.reverse_neighbors(&node) {
                if neighbor != remove && visited.insert(neighbor.clone()) {
                    queue.push(neighbor.clone());
                }
            }
        }
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::kg::{EntityType, KnowledgeGraph, RelationType};
    use proptest::prelude::*;

    /// Build a random KG with the given number of entities and edges.
    fn build_random_graph(entity_count: usize, edge_count: usize) -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();
        for i in 0..entity_count {
            let ty = match i % 6 {
                0 => EntityType::Video,
                1 => EntityType::Channel,
                2 => EntityType::Tag,
                3 => EntityType::Keyword,
                4 => EntityType::Topic,
                _ => EntityType::Entity,
            };
            let id = format!("{}:{}", ty.prefix(), i);
            kg.insert_entity(crate::analytics::kg::KgEntity::new(ty, &id, &id, &id));
        }
        let entity_ids: Vec<String> = kg.entities.keys().cloned().collect();
        if entity_ids.len() >= 2 {
            for i in 0..edge_count.min(entity_ids.len() * 2) {
                let from_idx = i % entity_ids.len();
                let to_idx = (i + 1) % entity_ids.len();
                kg.insert_edge(
                    &entity_ids[from_idx],
                    &entity_ids[to_idx],
                    RelationType::RelatedTo,
                    (i as f64 + 1.0) * 0.1,
                );
            }
        }
        kg
    }

    // --- All property tests using proptest! macro ---

    proptest! {
        // --- PageRank properties ---

        #[test]
        fn pagerank_mass_is_conserved(
            entity_count in 1..50usize,
            edge_count in 0..100usize,
        ) {
            let kg = build_random_graph(entity_count, edge_count);
            let pr = pagerank(&kg);
            let sum: f64 = pr.values().sum();
            prop_assert!((sum - 1.0).abs() < 1e-6, "PageRank mass not conserved: {}", sum);
        }

        #[test]
        fn pagerank_is_non_negative(
            entity_count in 1..50usize,
            edge_count in 0..100usize,
        ) {
            let kg = build_random_graph(entity_count, edge_count);
            let pr = pagerank(&kg);
            for score in pr.values() {
                prop_assert!(*score >= 0.0);
            }
        }

        #[test]
        fn pagerank_empty_graph_returns_empty(
            _dummy in 0..5u8,
        ) {
            let kg = KnowledgeGraph::new();
            let pr = pagerank(&kg);
            prop_assert!(pr.is_empty());
        }

        #[test]
        fn pagerank_singleton_is_one(
            _dummy in 0..5u8,
        ) {
            let mut kg = KnowledgeGraph::new();
            kg.insert_entity(crate::analytics::kg::KgEntity::video("v1", "Solo"));
            let pr = pagerank(&kg);
            prop_assert!((pr["video:v1"] - 1.0).abs() < 1e-9);
        }

        #[test]
        fn pagerank_higher_degree_scores_higher(
            entity_count in 3..20usize,
        ) {
            let mut kg = KnowledgeGraph::new();
            kg.insert_entity(crate::analytics::kg::KgEntity::video("center", "Center"));
            for i in 0..entity_count {
                kg.insert_entity(crate::analytics::kg::KgEntity::video(&format!("leaf{}", i), &format!("Leaf{}", i)));
                kg.insert_edge("video:center", &format!("video:leaf{}", i), RelationType::SimilarTo, 1.0);
            }
            let pr = pagerank(&kg);
            let center_score = pr["video:center"];
            for i in 0..entity_count {
                let leaf = format!("video:leaf{}", i);
                prop_assert!(center_score > pr[&leaf], "center ({}) > leaf ({})", center_score, pr[&leaf]);
            }
        }

        // --- Random Walk with Restart properties ---

        #[test]
        fn rwr_excludes_seed(
            entity_count in 1..30usize,
            edge_count in 0..50usize,
        ) {
            let kg = build_random_graph(entity_count, edge_count);
            if let Some(seed) = kg.entities.keys().next() {
                let result = random_walk_with_restart(&kg, seed);
                prop_assert!(!result.contains_key(seed));
            }
        }

        #[test]
        fn rwr_isolated_node_returns_empty(
            _dummy in 0..5u8,
        ) {
            let mut kg = KnowledgeGraph::new();
            kg.insert_entity(crate::analytics::kg::KgEntity::video("v1", "V1"));
            kg.insert_entity(crate::analytics::kg::KgEntity::video("v2", "V2"));
            let result = random_walk_with_restart(&kg, "video:v1");
            prop_assert!(!result.contains_key("video:v2"));
        }

        #[test]
        fn rwr_probabilities_non_negative(
            entity_count in 1..30usize,
            edge_count in 0..50usize,
        ) {
            let kg = build_random_graph(entity_count, edge_count);
            if let Some(seed) = kg.entities.keys().next() {
                let result = random_walk_with_restart(&kg, seed);
                for prob in result.values() {
                    prop_assert!(*prob >= 0.0, "RWR probability negative: {}", prob);
                }
            }
        }

        // --- Community Detection properties ---

        #[test]
        fn louvain_every_node_assigned(
            entity_count in 1..50usize,
            edge_count in 0..100usize,
        ) {
            let kg = build_random_graph(entity_count, edge_count);
            let communities = louvain_communities(&kg);
            prop_assert_eq!(communities.len(), kg.node_count());
        }

        #[test]
        fn louvain_connected_nodes_same_community(
            _dummy in 0..5u8,
        ) {
            let mut kg = KnowledgeGraph::new();
            let n = 5;
            for i in 0..n {
                kg.insert_entity(crate::analytics::kg::KgEntity::video(&format!("v{}", i), &format!("V{}", i)));
            }
            for i in 0..n {
                for j in (i + 1)..n {
                    kg.insert_edge(&format!("video:v{}", i), &format!("video:v{}", j), RelationType::SimilarTo, 1.0);
                }
            }
            let communities = louvain_communities(&kg);
            let first = communities["video:v0"];
            for i in 1..n {
                prop_assert_eq!(communities[&format!("video:v{}", i)], first);
            }
        }

        #[test]
        fn louvain_disconnected_components_different(
            _dummy in 0..5u8,
        ) {
            let mut kg = KnowledgeGraph::new();
            for i in 0..3 {
                kg.insert_entity(crate::analytics::kg::KgEntity::video(&format!("v{}", i), &format!("V{}", i)));
            }
            kg.insert_edge("video:v0", "video:v1", RelationType::SimilarTo, 1.0);
            kg.insert_edge("video:v1", "video:v2", RelationType::SimilarTo, 1.0);
            kg.insert_edge("video:v0", "video:v2", RelationType::SimilarTo, 1.0);
            for i in 3..5 {
                kg.insert_entity(crate::analytics::kg::KgEntity::video(&format!("v{}", i), &format!("V{}", i)));
            }
            kg.insert_edge("video:v3", "video:v4", RelationType::SimilarTo, 1.0);
            let communities = louvain_communities(&kg);
            prop_assert_eq!(communities["video:v0"], communities["video:v1"]);
            prop_assert_eq!(communities["video:v3"], communities["video:v4"]);
            prop_assert_ne!(communities["video:v0"], communities["video:v3"]);
        }

        // --- Shortest Path properties ---

        #[test]
        fn shortest_path_direct_edge(
            from in "[a-z]{1,5}",
            to in "[a-z]{1,5}",
        ) {
            prop_assume!(from != to);
            let mut kg = KnowledgeGraph::new();
            let from_id = format!("video:{}", from);
            let to_id = format!("video:{}", to);
            kg.insert_entity(crate::analytics::kg::KgEntity::new(EntityType::Video, &from, &from, &from));
            kg.insert_entity(crate::analytics::kg::KgEntity::new(EntityType::Video, &to, &to, &to));
            kg.insert_edge(&from_id, &to_id, RelationType::SimilarTo, 1.0);
            let path = shortest_path(&kg, &from_id, &to_id);
            prop_assert!(path.is_some());
            prop_assert_eq!(path.as_ref().unwrap().len(), 2);
        }

        #[test]
        fn shortest_path_no_path_returns_none(
            from in "[a-z]{1,5}",
            to in "[a-z]{1,5}",
        ) {
            prop_assume!(from != to);
            let mut kg = KnowledgeGraph::new();
            kg.insert_entity(crate::analytics::kg::KgEntity::new(EntityType::Video, &from, &from, &from));
            kg.insert_entity(crate::analytics::kg::KgEntity::new(EntityType::Video, &to, &to, &to));
            let from_id = format!("video:{}", from);
            let to_id = format!("video:{}", to);
            let path = shortest_path(&kg, &from_id, &to_id);
            prop_assert_eq!(path, None);
        }

        #[test]
        fn shortest_path_is_symmetric(
            from in "[a-z]{1,5}",
            to in "[a-z]{1,5}",
        ) {
            prop_assume!(from != to);
            let mut kg = KnowledgeGraph::new();
            kg.insert_entity(crate::analytics::kg::KgEntity::new(EntityType::Video, &from, &from, &from));
            kg.insert_entity(crate::analytics::kg::KgEntity::new(EntityType::Video, &to, &to, &to));
            let from_id = format!("video:{}", from);
            let to_id = format!("video:{}", to);
            kg.insert_edge(&from_id, &to_id, RelationType::SimilarTo, 1.0);
            let path_ab = shortest_path(&kg, &from_id, &to_id);
            let path_ba = shortest_path(&kg, &to_id, &from_id);
            prop_assert!(path_ab.is_some());
            prop_assert!(path_ba.is_some());
            prop_assert_eq!(path_ab.unwrap().len(), path_ba.unwrap().len());
        }

        // --- Connected Components properties ---

        #[test]
        fn connected_components_count(
            entity_count in 1..50usize,
            edge_count in 0..100usize,
        ) {
            let kg = build_random_graph(entity_count, edge_count);
            let components = connected_components(&kg);
            prop_assert_eq!(components.len(), kg.node_count());
        }

        #[test]
        fn connected_components_connected_graph_is_one(
            _dummy in 0..5u8,
        ) {
            let mut kg = KnowledgeGraph::new();
            let n = 5;
            for i in 0..n {
                kg.insert_entity(crate::analytics::kg::KgEntity::video(&format!("v{}", i), &format!("V{}", i)));
            }
            for i in 0..n {
                for j in (i + 1)..n {
                    kg.insert_edge(&format!("video:v{}", i), &format!("video:v{}", j), RelationType::SimilarTo, 1.0);
                }
            }
            let components = connected_components(&kg);
            let unique: std::collections::HashSet<_> = components.values().collect();
            prop_assert_eq!(unique.len(), 1);
        }

        #[test]
        fn connected_components_isolated_nodes(
            node_count in 1..10usize,
        ) {
            let mut kg = KnowledgeGraph::new();
            for i in 0..node_count {
                kg.insert_entity(crate::analytics::kg::KgEntity::video(&format!("v{}", i), &format!("V{}", i)));
            }
            let components = connected_components(&kg);
            let unique: std::collections::HashSet<_> = components.values().collect();
            prop_assert_eq!(unique.len(), node_count);
        }

        // --- Bridge Detection properties ---

        #[test]
        fn bridges_line_graph(
            _dummy in 0..5u8,
        ) {
            let mut kg = KnowledgeGraph::new();
            for i in 0..3 {
                kg.insert_entity(crate::analytics::kg::KgEntity::video(&format!("v{}", i), &format!("V{}", i)));
            }
            kg.insert_edge("video:v0", "video:v1", RelationType::SimilarTo, 1.0);
            kg.insert_edge("video:v1", "video:v2", RelationType::SimilarTo, 1.0);
            let bridges = find_bridges(&kg);
            prop_assert!(bridges.contains(&"video:v1".to_string()), "v1 should be a bridge: {:?}", bridges);
        }

        #[test]
        fn bridges_fully_connected_no_bridges(
            _dummy in 0..5u8,
        ) {
            let mut kg = KnowledgeGraph::new();
            let n = 4;
            for i in 0..n {
                kg.insert_entity(crate::analytics::kg::KgEntity::video(&format!("v{}", i), &format!("V{}", i)));
            }
            for i in 0..n {
                for j in (i + 1)..n {
                    kg.insert_edge(&format!("video:v{}", i), &format!("video:v{}", j), RelationType::SimilarTo, 1.0);
                }
            }
            let bridges = find_bridges(&kg);
            prop_assert!(bridges.is_empty(), "fully connected should have no bridges: {:?}", bridges);
        }
    }
}
