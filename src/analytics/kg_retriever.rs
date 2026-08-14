//! Hybrid Retriever (PRD §FR-2, §7.3): combines BM25 text search, vector
//! similarity, and graph traversal into a single retrieval pipeline.
//!
//! The retriever prevents context ROT by:
//! 1. Returning results with full provenance chains (why each result)
//! 2. Including neighborhood context (1-2 hops) with each result
//! 3. Supporting three modes: local, global, mix
//!
//! All retrieval is deterministic over the current KG state.

use std::collections::HashMap;

use crate::analytics::kg::{
    EntityType, KnowledgeGraph, ProvenanceStep, RelationType, RetrievalContext, RetrievalResult,
    SignalType,
};
use crate::error::TubeforgeError;
use crate::search::bm25::Bm25;
use crate::search::FIELD_TITLE;

/// Retrieval mode (PRD §FR-2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetrievalMode {
    /// Local: specific entities + direct neighbors (1-2 hops).
    Local,
    /// Global: community-level summaries.
    Global,
    /// Mix: both local + global combined (default).
    #[default]
    Mix,
}

/// A hybrid query combining all retrieval signals.
#[derive(Debug, Clone, Default)]
pub struct HybridQuery {
    /// Text query for BM25 search.
    pub text: Option<String>,
    /// Vector embedding for semantic search.
    pub embedding: Option<Vec<f32>>,
    /// Seed entity for graph-based retrieval.
    pub seed_entity: Option<String>,
    /// Retrieval mode.
    pub mode: RetrievalMode,
    /// Maximum graph hops (1-5).
    pub max_depth: usize,
    /// Number of results to return.
    pub limit: usize,
    /// Weight for graph score vs BM25 (0 = pure BM25, 1 = pure graph).
    pub graph_weight: f64,
    /// Filter by entity type.
    pub entity_type_filter: Option<EntityType>,
}

impl HybridQuery {
    pub fn new() -> Self {
        Self {
            max_depth: 2,
            limit: 10,
            graph_weight: 0.3,
            ..Default::default()
        }
    }

    pub fn with_text(mut self, text: &str) -> Self {
        self.text = Some(text.to_string());
        self
    }

    pub fn with_seed(mut self, entity_id: &str) -> Self {
        self.seed_entity = Some(entity_id.to_string());
        self
    }

    pub fn with_mode(mut self, mode: RetrievalMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// The hybrid retriever (PRD §7.2).
pub struct HybridRetriever<'a> {
    pub kg: &'a KnowledgeGraph,
    pub bm25: Option<&'a Bm25>,
}

impl<'a> HybridRetriever<'a> {
    pub fn new(kg: &'a KnowledgeGraph) -> Self {
        HybridRetriever { kg, bm25: None }
    }

    pub fn with_bm25(kg: &'a KnowledgeGraph, bm25: &'a Bm25) -> Self {
        HybridRetriever {
            kg,
            bm25: Some(bm25),
        }
    }

    /// Execute a hybrid query (PRD §7.3).
    pub fn retrieve(&self, query: &HybridQuery) -> Result<Vec<RetrievalResult>, TubeforgeError> {
        let mut results: HashMap<String, RetrievalResult> = HashMap::new();

        // Step 1: BM25 text recall
        if let Some(ref text) = query.text {
            if let Some(bm25) = self.bm25 {
                let bm25_results = bm25.matches(FIELD_TITLE, text);
                for (video_id, score) in bm25_results {
                    let entity_id = format!("video:{video_id}");
                    if self.kg.get_entity(&entity_id).is_some() {
                        let entry = results
                            .entry(entity_id.clone())
                            .or_insert_with(|| self.make_result(&entity_id, 0.0, 0));
                        let normalized = (score as f64 / 4.0).min(1.0) * 100.0;
                        entry.score = entry.score.max(normalized);
                        entry.provenance.push(ProvenanceStep {
                            from: format!("query:{text}"),
                            to: entity_id,
                            relation: None,
                            weight: normalized,
                            signal: SignalType::Bm25,
                        });
                    }
                }
            }
        }

        // Step 2: Vector recall (semantic similarity)
        if let Some(ref embedding) = query.embedding {
            self.vector_recall(embedding, &mut results, query);
        }

        // Step 3: Graph expansion
        if let Some(ref seed) = query.seed_entity {
            self.graph_expansion(seed, &mut results, query);
        } else if results.is_empty() && query.text.is_some() {
            // No seed and no BM25 results — try graph search from text-matched entities
            self.graph_expansion_from_text(&mut results, query);
        }

        // Step 4: Apply mode-specific processing
        match query.mode {
            RetrievalMode::Local => {
                // Keep only direct neighbors (depth 1-2)
                results.retain(|_, r| r.depth <= 2);
            }
            RetrievalMode::Global => {
                // Add community-level results
                self.add_community_results(&mut results, query);
            }
            RetrievalMode::Mix => {
                // Both local + global
                self.add_community_results(&mut results, query);
            }
        }

        // Step 5: Filter by entity type
        if let Some(ref ty) = query.entity_type_filter {
            results.retain(|_, r| r.entity_type == *ty);
        }

        // Step 6: Sort by score and limit
        let mut sorted: Vec<RetrievalResult> = results.into_values().collect();
        sorted.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted.truncate(query.limit);

        Ok(sorted)
    }

    /// Vector similarity recall using brute-force cosine.
    fn vector_recall(
        &self,
        embedding: &[f32],
        results: &mut HashMap<String, RetrievalResult>,
        _query: &HybridQuery,
    ) {
        for (entity_id, entity) in &self.kg.entities {
            if let Some(ref entity_embedding) = entity.embedding {
                let sim = cosine_similarity(embedding, entity_embedding);
                if sim > 0.5 {
                    let entry = results
                        .entry(entity_id.clone())
                        .or_insert_with(|| self.make_result(entity_id, 0.0, 0));
                    let score = sim * 100.0;
                    entry.score = entry.score.max(score);
                    entry.provenance.push(ProvenanceStep {
                        from: "query:embedding".to_string(),
                        to: entity_id.clone(),
                        relation: None,
                        weight: score,
                        signal: SignalType::Vector,
                    });
                }
            }
        }
    }

    /// Graph expansion from a seed entity using random walk with restart.
    fn graph_expansion(
        &self,
        seed: &str,
        results: &mut HashMap<String, RetrievalResult>,
        query: &HybridQuery,
    ) {
        let rwr_scores = crate::analytics::kg_algorithms::random_walk_with_restart(self.kg, seed);

        for (entity_id, visit_prob) in rwr_scores {
            if entity_id == seed {
                continue;
            }
            let depth = self
                .kg
                .neighborhood(seed, query.max_depth)
                .get(&entity_id)
                .copied()
                .unwrap_or(0);
            if depth > query.max_depth {
                continue;
            }

            let entry = results
                .entry(entity_id.clone())
                .or_insert_with(|| self.make_result(&entity_id, 0.0, depth));

            let score = visit_prob * 100.0;
            entry.score = entry.score.max(score);
            entry.depth = entry.depth.max(depth);
            entry.provenance.push(ProvenanceStep {
                from: seed.to_string(),
                to: entity_id,
                relation: None,
                weight: score,
                signal: SignalType::Graph,
            });
        }
    }

    /// Graph expansion when no seed is given — use text-matched entities as seeds.
    fn graph_expansion_from_text(
        &self,
        results: &mut HashMap<String, RetrievalResult>,
        query: &HybridQuery,
    ) {
        // Use existing results as seeds for graph expansion
        let seeds: Vec<String> = results.keys().cloned().collect();
        for seed in &seeds {
            self.graph_expansion(seed, results, query);
        }
    }

    /// Add community-level results (global mode).
    fn add_community_results(
        &self,
        results: &mut HashMap<String, RetrievalResult>,
        _query: &HybridQuery,
    ) {
        // For each result, add its community members
        let existing: Vec<(String, i64)> = results
            .keys()
            .filter_map(|id| {
                self.kg
                    .get_entity(id)
                    .and_then(|e| e.community_id)
                    .map(|cid| (id.clone(), cid))
            })
            .collect();

        for (entity_id, comm_id) in existing {
            let members = self.kg.community_members(comm_id);
            for member_id in members {
                if member_id == &entity_id || results.contains_key(member_id) {
                    continue;
                }
                let entry = results
                    .entry(member_id.clone())
                    .or_insert_with(|| self.make_result(member_id, 0.0, 1));
                entry.score = entry.score.max(10.0); // Low baseline for community members
                entry.provenance.push(ProvenanceStep {
                    from: entity_id.clone(),
                    to: member_id.clone(),
                    relation: Some(RelationType::Contains),
                    weight: 0.5,
                    signal: SignalType::Graph,
                });
            }
        }
    }

    /// Build a RetrievalResult for an entity.
    fn make_result(&self, entity_id: &str, score: f64, depth: usize) -> RetrievalResult {
        let (entity_type, display_name) = self
            .kg
            .get_entity(entity_id)
            .map(|e| (e.entity_type, e.display_name.clone()))
            .unwrap_or((EntityType::Entity, entity_id.to_string()));

        RetrievalResult {
            entity_id: entity_id.to_string(),
            entity_type,
            display_name,
            score,
            provenance: Vec::new(),
            depth,
        }
    }

    /// Build a context packet for a result (anti-ROT).
    pub fn build_context(&self, result: &RetrievalResult) -> RetrievalContext {
        let neighbors = self
            .kg
            .neighbors(&result.entity_id)
            .iter()
            .take(10)
            .map(|(n, r, w)| (n.clone(), *r, *w))
            .collect();

        RetrievalContext {
            neighbors,
            community_id: self
                .kg
                .get_entity(&result.entity_id)
                .and_then(|e| e.community_id),
            centrality: self.kg.get_centrality(&result.entity_id),
        }
    }
}

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| *x as f64 * *y as f64).sum();
    let norm_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::kg::KgEntity;

    fn build_test_kg() -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();
        kg.insert_entity(KgEntity::video("v1", "Rust Async Guide"));
        kg.insert_entity(KgEntity::video("v2", "Rust Tokio Tutorial"));
        kg.insert_entity(KgEntity::video("v3", "Python Basics"));
        kg.insert_entity(KgEntity::tag("rust"));
        kg.insert_entity(KgEntity::tag("python"));

        kg.insert_edge("video:v1", "tag:rust", RelationType::Tags, 1.0);
        kg.insert_edge("video:v2", "tag:rust", RelationType::Tags, 1.0);
        kg.insert_edge("video:v3", "tag:python", RelationType::Tags, 1.0);
        kg.insert_edge("video:v1", "video:v2", RelationType::SimilarTo, 0.8);

        kg
    }

    #[test]
    fn hybrid_retrieve_by_text_falls_back_to_graph() {
        // Without BM25 index, text query falls back to graph expansion
        // from text-matched entities (empty in this case, so no results).
        // This tests the graceful fallback path.
        let kg = build_test_kg();
        let retriever = HybridRetriever::new(&kg);
        let query = HybridQuery::new().with_text("rust").with_limit(10);
        let results = retriever.retrieve(&query).unwrap();
        // Without BM25 index, text-only query returns empty (no index to search)
        assert!(results.is_empty());
    }

    #[test]
    fn hybrid_retrieve_by_seed_finds_connected() {
        let kg = build_test_kg();
        let retriever = HybridRetriever::new(&kg);
        let query = HybridQuery::new().with_seed("tag:rust").with_limit(10);
        let results = retriever.retrieve(&query).unwrap();
        // Should find v1 and v2 (both connected to tag:rust)
        assert!(!results.is_empty());
        let ids: Vec<&str> = results.iter().map(|r| r.entity_id.as_str()).collect();
        assert!(ids.contains(&"video:v1"));
        assert!(ids.contains(&"video:v2"));
    }

    #[test]
    fn hybrid_retrieve_by_seed() {
        let kg = build_test_kg();
        let retriever = HybridRetriever::new(&kg);
        let query = HybridQuery::new()
            .with_seed("video:v1")
            .with_mode(RetrievalMode::Local)
            .with_limit(10);
        let results = retriever.retrieve(&query).unwrap();
        // Should find v2 (similar to v1) and tag:rust
        assert!(!results.is_empty());
        let ids: Vec<&str> = results.iter().map(|r| r.entity_id.as_str()).collect();
        assert!(ids.contains(&"video:v2") || ids.contains(&"tag:rust"));
    }

    #[test]
    fn hybrid_retrieve_with_provenance() {
        let kg = build_test_kg();
        let retriever = HybridRetriever::new(&kg);
        let query = HybridQuery::new().with_seed("video:v1").with_limit(10);
        let results = retriever.retrieve(&query).unwrap();
        // Every result should have at least one provenance step
        for r in &results {
            assert!(
                !r.provenance.is_empty(),
                "result {} has no provenance",
                r.entity_id
            );
        }
    }

    #[test]
    fn hybrid_retrieve_local_mode() {
        let kg = build_test_kg();
        let retriever = HybridRetriever::new(&kg);
        let query = HybridQuery::new()
            .with_seed("video:v1")
            .with_mode(RetrievalMode::Local)
            .with_limit(10);
        let results = retriever.retrieve(&query).unwrap();
        // All results should be within 2 hops
        for r in &results {
            assert!(r.depth <= 2, "result {} has depth {}", r.entity_id, r.depth);
        }
    }

    #[test]
    fn hybrid_retrieve_filters_by_type() {
        let kg = build_test_kg();
        let retriever = HybridRetriever::new(&kg);
        let query = HybridQuery {
            seed_entity: Some("video:v1".to_string()),
            entity_type_filter: Some(EntityType::Tag),
            limit: 10,
            ..Default::default()
        };
        let results = retriever.retrieve(&query).unwrap();
        // All results should be tags
        for r in &results {
            assert_eq!(r.entity_type, EntityType::Tag);
        }
    }

    #[test]
    fn hybrid_retrieve_empty_kg() {
        let kg = KnowledgeGraph::new();
        let retriever = HybridRetriever::new(&kg);
        let query = HybridQuery::new().with_text("rust").with_limit(10);
        let results = retriever.retrieve(&query).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn build_context_packet() {
        let kg = build_test_kg();
        let retriever = HybridRetriever::new(&kg);
        let query = HybridQuery::new().with_seed("video:v1").with_limit(5);
        let results = retriever.retrieve(&query).unwrap();
        if let Some(result) = results.first() {
            let context = retriever.build_context(result);
            // Context should include neighbors
            assert!(
                !context.neighbors.is_empty(),
                "context should have neighbors"
            );
        }
    }

    #[test]
    fn cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-9);
    }

    #[test]
    fn cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - (-1.0)).abs() < 1e-9);
    }
}
