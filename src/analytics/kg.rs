//! Knowledge Graph Core (PRD §7): in-memory data structures, entity/relation
//! types, and the central KnowledgeGraph struct.
//!
//! The KG is built from existing tables (videos, channels, tags, keywords,
//! edges, etc.) and cached in memory for O(1) lookups and O(E) traversal.
//!
//! Entity ID convention: `{type}:{canonical}` (e.g., `video:abc123`,
//! `tag:rust`, `channel:UC...`). This makes IDs self-describing and
//! collision-free across types.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Entity types
// ---------------------------------------------------------------------------

/// The six entity types in the knowledge graph (PRD §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Video,
    Channel,
    Tag,
    Keyword,
    Topic,
    Entity, // NLP-extracted (Phase 2)
}

impl EntityType {
    /// Prefix used in entity_id (e.g., `video:abc123`).
    pub fn prefix(&self) -> &'static str {
        match self {
            EntityType::Video => "video",
            EntityType::Channel => "channel",
            EntityType::Tag => "tag",
            EntityType::Keyword => "keyword",
            EntityType::Topic => "topic",
            EntityType::Entity => "entity",
        }
    }

    /// Parse an entity_type from its prefix string.
    pub fn from_prefix(s: &str) -> Option<Self> {
        match s {
            "video" => Some(EntityType::Video),
            "channel" => Some(EntityType::Channel),
            "tag" => Some(EntityType::Tag),
            "keyword" => Some(EntityType::Keyword),
            "topic" => Some(EntityType::Topic),
            "entity" => Some(EntityType::Entity),
            _ => None,
        }
    }
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.prefix())
    }
}

// ---------------------------------------------------------------------------
// Relation types
// ---------------------------------------------------------------------------

/// The nine relation types in the knowledge graph (PRD §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    Tags,
    CreatedBy,
    AboutTopic,
    CompetesIn,
    Dominates,
    RelatedTo,
    SimilarTo,
    MentionedIn,
    Contains,
}

impl std::str::FromStr for RelationType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tags" => Ok(RelationType::Tags),
            "created_by" => Ok(RelationType::CreatedBy),
            "about_topic" => Ok(RelationType::AboutTopic),
            "competes_in" => Ok(RelationType::CompetesIn),
            "dominates" => Ok(RelationType::Dominates),
            "related_to" => Ok(RelationType::RelatedTo),
            "similar_to" => Ok(RelationType::SimilarTo),
            "mentioned_in" => Ok(RelationType::MentionedIn),
            "contains" => Ok(RelationType::Contains),
            _ => Err(()),
        }
    }
}

impl RelationType {
    /// Parse a relation_type from its string form.
    pub fn parse_type(s: &str) -> Option<Self> {
        <Self as std::str::FromStr>::from_str(s).ok()
    }
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RelationType::Tags => "tags",
            RelationType::CreatedBy => "created_by",
            RelationType::AboutTopic => "about_topic",
            RelationType::CompetesIn => "competes_in",
            RelationType::Dominates => "dominates",
            RelationType::RelatedTo => "related_to",
            RelationType::SimilarTo => "similar_to",
            RelationType::MentionedIn => "mentioned_in",
            RelationType::Contains => "contains",
        };
        write!(f, "{s}")
    }
}

// ---------------------------------------------------------------------------
// KG entity
// ---------------------------------------------------------------------------

/// One node in the knowledge graph (PRD §7.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgEntity {
    pub entity_id: String,
    pub entity_type: EntityType,
    pub canonical_name: String,
    pub display_name: String,
    #[serde(default)]
    pub properties: serde_json::Value,
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
    #[serde(default)]
    pub centrality: Option<f64>,
    #[serde(default)]
    pub community_id: Option<i64>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_ref: String,
}

impl KgEntity {
    /// Construct a new entity with the standard ID format `{type}:{canonical}`.
    pub fn new(
        entity_type: EntityType,
        canonical_name: &str,
        display_name: &str,
        source_ref: &str,
    ) -> Self {
        let prefix = entity_type.prefix();
        // canonical_name is normalized: lowercase, trimmed, spaces→hyphens
        let canonical = canonical_name.trim().to_lowercase().replace(' ', "-");
        let entity_id = format!("{prefix}:{canonical}");
        KgEntity {
            entity_id,
            entity_type,
            canonical_name: canonical,
            display_name: display_name.to_string(),
            properties: serde_json::Value::Object(serde_json::Map::new()),
            embedding: None,
            centrality: None,
            community_id: None,
            source: "system".to_string(),
            source_ref: source_ref.to_string(),
        }
    }

    /// Construct a video entity from a video_id.
    pub fn video(video_id: &str, title: &str) -> Self {
        let mut e = KgEntity::new(
            EntityType::Video,
            video_id,
            title,
            &format!("videos:{video_id}"),
        );
        e.entity_id = format!("video:{video_id}"); // video_id is already canonical
        e.canonical_name = video_id.to_string();
        e
    }

    /// Construct a channel entity from a channel_id.
    pub fn channel(channel_id: &str, title: &str) -> Self {
        let mut e = KgEntity::new(
            EntityType::Channel,
            channel_id,
            title,
            &format!("channels:{channel_id}"),
        );
        e.entity_id = format!("channel:{channel_id}");
        e.canonical_name = channel_id.to_string();
        e
    }

    /// Construct a tag entity.
    pub fn tag(name: &str) -> Self {
        let normalized = name.trim().to_lowercase();
        KgEntity::new(
            EntityType::Tag,
            &normalized,
            name,
            &format!("tags:{normalized}"),
        )
    }

    /// Construct a keyword entity.
    pub fn keyword(name: &str) -> Self {
        let normalized = name.trim().to_lowercase().replace(' ', "-");
        KgEntity::new(
            EntityType::Keyword,
            &normalized,
            name,
            &format!("keywords:{normalized}"),
        )
    }

    /// Construct a topic entity from a Wikipedia URL segment.
    pub fn topic(url_segment: &str, display_name: &str) -> Self {
        KgEntity::new(
            EntityType::Topic,
            url_segment,
            display_name,
            &format!("topics:{url_segment}"),
        )
    }
}

// ---------------------------------------------------------------------------
// KG relation
// ---------------------------------------------------------------------------

/// One edge in the knowledge graph (PRD §7.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgRelation {
    pub from_entity: String,
    pub to_entity: String,
    pub relation_type: RelationType,
    pub weight: f64,
    #[serde(default)]
    pub source: String,
}

impl KgRelation {
    pub fn new(from: &str, to: &str, relation_type: RelationType, weight: f64) -> Self {
        KgRelation {
            from_entity: from.to_string(),
            to_entity: to.to_string(),
            relation_type,
            weight: weight.max(0.0),
            source: "system".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// KG community
// ---------------------------------------------------------------------------

/// One community detected by Louvain algorithm (PRD §5.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgCommunity {
    pub community_id: i64,
    pub community_type: String,
    pub summary: Option<String>,
    pub member_count: usize,
    pub mean_views: Option<f64>,
    pub mean_seo_score: Option<f64>,
    pub top_entities: Vec<String>,
}

// ---------------------------------------------------------------------------
// Central in-memory Knowledge Graph
// ---------------------------------------------------------------------------

/// The central in-memory data structure for the Knowledge Graph (PRD §7.1).
///
/// Optimized for:
/// - O(1) entity lookup via `entities` HashMap
/// - O(1) neighbor access via `adjacency` HashMap
/// - O(E) graph traversal (BFS/DFS/PageRank)
/// - Filtered queries via `by_type` index
/// - Community queries via `communities` index
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeGraph {
    /// entity_id → entity data (O(1) lookup)
    pub entities: HashMap<String, KgEntity>,
    /// entity_id → [(neighbor_id, relation_type, weight)] (O(1) neighbor access)
    pub adjacency: HashMap<String, Vec<(String, RelationType, f64)>>,
    /// Reverse adjacency for bidirectional traversal
    pub reverse_adj: HashMap<String, Vec<(String, RelationType, f64)>>,
    /// entity_type → [entity_id] (filtered traversal)
    pub by_type: HashMap<EntityType, Vec<String>>,
    /// community_id → [entity_id] (community queries)
    pub communities: HashMap<i64, Vec<String>>,
    /// Centrality cache: entity_id → PageRank score
    pub centrality: HashMap<String, f64>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of entities (nodes) in the graph.
    pub fn node_count(&self) -> usize {
        self.entities.len()
    }

    /// Number of relations (edges) in the graph.
    pub fn edge_count(&self) -> usize {
        self.adjacency.values().map(|v| v.len()).sum()
    }

    /// True if the graph is empty.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Insert an entity. Replaces if already exists.
    ///
    /// Keeps the `centrality` index consistent with the entity's own
    /// `centrality` field — an entity loaded from the DB via
    /// `load_from_db` carries its precomputed PageRank score in
    /// `KgEntity::centrality`, and that must be queryable through
    /// `get_centrality()` for the graph-aware scoring pipeline to work.
    pub fn insert_entity(&mut self, entity: KgEntity) {
        let id = entity.entity_id.clone();
        let ty = entity.entity_type;
        if let Some(cent) = entity.centrality {
            self.centrality.insert(id.clone(), cent);
        }
        self.entities.insert(id.clone(), entity);
        self.by_type.entry(ty).or_default().push(id);
    }

    /// Insert an undirected edge. Creates entries in both directions in the
    /// adjacency list (so `neighbors()` returns connected nodes regardless of
    /// insertion order), plus reverse adjacency for bidirectional traversal.
    pub fn insert_edge(&mut self, from: &str, to: &str, rel: RelationType, weight: f64) {
        let w = weight.max(0.0);
        self.adjacency
            .entry(from.to_string())
            .or_default()
            .push((to.to_string(), rel, w));
        self.adjacency
            .entry(to.to_string())
            .or_default()
            .push((from.to_string(), rel, w));
        self.reverse_adj
            .entry(to.to_string())
            .or_default()
            .push((from.to_string(), rel, w));
        self.reverse_adj
            .entry(from.to_string())
            .or_default()
            .push((to.to_string(), rel, w));
    }

    /// Get neighbors of an entity (outgoing edges).
    pub fn neighbors(&self, entity_id: &str) -> &[(String, RelationType, f64)] {
        self.adjacency
            .get(entity_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get reverse neighbors of an entity (incoming edges).
    pub fn reverse_neighbors(&self, entity_id: &str) -> &[(String, RelationType, f64)] {
        self.reverse_adj
            .get(entity_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all entities of a given type.
    pub fn entities_of_type(&self, ty: EntityType) -> &[String] {
        self.by_type.get(&ty).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get an entity by ID.
    pub fn get_entity(&self, entity_id: &str) -> Option<&KgEntity> {
        self.entities.get(entity_id)
    }

    /// Get the centrality score of an entity.
    pub fn get_centrality(&self, entity_id: &str) -> Option<f64> {
        self.centrality.get(entity_id).copied()
    }

    /// Set centrality for an entity.
    pub fn set_centrality(&mut self, entity_id: &str, score: f64) {
        self.centrality.insert(entity_id.to_string(), score);
        if let Some(e) = self.entities.get_mut(entity_id) {
            e.centrality = Some(score);
        }
    }

    /// Set community for an entity.
    pub fn set_community(&mut self, entity_id: &str, community_id: i64) {
        if let Some(e) = self.entities.get_mut(entity_id) {
            e.community_id = Some(community_id);
        }
    }

    /// Assign a community's members.
    pub fn set_community_members(&mut self, community_id: i64, members: Vec<String>) {
        self.communities.insert(community_id, members);
    }

    /// Get community members.
    pub fn community_members(&self, community_id: i64) -> &[String] {
        self.communities
            .get(&community_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// BFS traversal from a seed entity, returning (entity_id, depth) pairs.
    /// `max_depth` bounds the traversal (0 = seed only).
    pub fn bfs(&self, seed: &str, max_depth: usize) -> Vec<(String, usize)> {
        let mut visited = Vec::new();
        let mut seen = HashSet::new();
        let mut frontier = vec![seed.to_string()];
        seen.insert(seed.to_string());
        let mut depth = 0;

        while !frontier.is_empty() && depth <= max_depth {
            let mut next = Vec::new();
            for node in &frontier {
                visited.push((node.clone(), depth));
                if depth < max_depth {
                    for (neighbor, _, _) in self.neighbors(node) {
                        if seen.insert(neighbor.clone()) {
                            next.push(neighbor.clone());
                        }
                    }
                }
            }
            frontier = next;
            depth += 1;
        }
        visited
    }

    /// Get the N-hop neighborhood of an entity (all entities within N hops).
    /// Traverses edges bidirectionally (both outgoing and incoming).
    pub fn neighborhood(&self, seed: &str, hops: usize) -> HashMap<String, usize> {
        let mut result = HashMap::new();
        let mut seen = HashSet::new();
        let mut frontier = vec![seed.to_string()];
        seen.insert(seed.to_string());
        let mut depth = 0;

        while !frontier.is_empty() && depth <= hops {
            let mut next = Vec::new();
            for node in &frontier {
                if depth > 0 {
                    result.insert(node.clone(), depth);
                }
                if depth < hops {
                    // Follow outgoing edges
                    for (neighbor, _, _) in self.neighbors(node) {
                        if seen.insert(neighbor.clone()) {
                            next.push(neighbor.clone());
                        }
                    }
                    // Follow incoming edges (bidirectional)
                    for (neighbor, _, _) in self.reverse_neighbors(node) {
                        if seen.insert(neighbor.clone()) {
                            next.push(neighbor.clone());
                        }
                    }
                }
            }
            frontier = next;
            depth += 1;
        }
        result
    }

    /// Remove duplicate edges (dedup by (from, to) keeping highest weight).
    pub fn dedup_edges(&mut self) {
        for edges in self.adjacency.values_mut() {
            let mut best: HashMap<(String, RelationType), f64> = HashMap::new();
            for (to, rel, w) in edges.drain(..) {
                let key = (to, rel);
                let entry = best.entry(key).or_insert(0.0);
                if w > *entry {
                    *entry = w;
                }
            }
            for ((to, rel), w) in best {
                edges.push((to, rel, w));
            }
        }
        // Rebuild reverse adjacency
        self.reverse_adj.clear();
        for (from, edges) in &self.adjacency {
            for (to, rel, w) in edges {
                self.reverse_adj
                    .entry(to.clone())
                    .or_default()
                    .push((from.clone(), *rel, *w));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Signal types for provenance tracking
// ---------------------------------------------------------------------------

/// The signal that contributed to a retrieval result (PRD §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    Bm25,
    Vector,
    Graph,
    Score,
}

/// One step in a provenance chain (PRD §7.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceStep {
    pub from: String,
    pub to: String,
    pub relation: Option<RelationType>,
    pub weight: f64,
    pub signal: SignalType,
}

/// A retrieval result with full provenance (anti-ROT).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub entity_id: String,
    pub entity_type: EntityType,
    pub display_name: String,
    pub score: f64,
    pub provenance: Vec<ProvenanceStep>,
    pub depth: usize,
}

/// Context packet — prevents context ROT by carrying the full neighborhood.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetrievalContext {
    pub neighbors: Vec<(String, RelationType, f64)>,
    pub community_id: Option<i64>,
    pub centrality: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::proptest;

    // --- Helper: generate a random KnowledgeGraph ---

    fn arbitrary_entity_id() -> impl Strategy<Value = String> {
        "[a-z]{1,6}:[a-z0-9]{1,8}"
    }

    fn arbitrary_entity_type() -> impl Strategy<Value = EntityType> {
        prop_oneof![
            Just(EntityType::Video),
            Just(EntityType::Channel),
            Just(EntityType::Tag),
            Just(EntityType::Keyword),
            Just(EntityType::Topic),
            Just(EntityType::Entity),
        ]
    }

    fn arbitrary_relation_type() -> impl Strategy<Value = RelationType> {
        prop_oneof![
            Just(RelationType::Tags),
            Just(RelationType::CreatedBy),
            Just(RelationType::AboutTopic),
            Just(RelationType::CompetesIn),
            Just(RelationType::Dominates),
            Just(RelationType::RelatedTo),
            Just(RelationType::SimilarTo),
            Just(RelationType::MentionedIn),
            Just(RelationType::Contains),
        ]
    }

    /// Build a random KG with the given number of entities and edges.
    fn build_random_graph(entity_count: usize, edge_count: usize) -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();
        // Create entities
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
            kg.insert_entity(KgEntity::new(ty, &id, &id, &id));
        }
        // Create random edges
        let entity_ids: Vec<String> = kg.entities.keys().cloned().collect();
        for i in 0..edge_count.min(entity_ids.len() * entity_ids.len()) {
            let from_idx = i % entity_ids.len();
            let to_idx = (i + 1) % entity_ids.len();
            if from_idx != to_idx {
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

    // --- Property tests ---

    proptest! {
        #[test]
        fn entity_id_format_is_correct(
            id in "[a-z0-9]{1,11}",
        ) {
            let e = KgEntity::video(&id, "Test Title");
            prop_assert_eq!(e.entity_id, format!("video:{id}"));
            prop_assert_eq!(e.entity_type, EntityType::Video);
        }

        #[test]
        fn entity_type_prefix_roundtrip(
            ty in arbitrary_entity_type(),
        ) {
            let prefix = ty.prefix();
            let parsed = EntityType::from_prefix(prefix).unwrap();
            prop_assert_eq!(ty, parsed);
        }

        #[test]
        fn relation_type_str_roundtrip(
            rel in arbitrary_relation_type(),
        ) {
            let s = rel.to_string();
            let parsed = RelationType::parse_type(&s).unwrap();
            prop_assert_eq!(rel, parsed);
        }

        #[test]
        fn kg_insert_entity_increments_count(
            id in arbitrary_entity_id(),
            ty in arbitrary_entity_type(),
        ) {
            let mut kg = KnowledgeGraph::new();
            let entity = KgEntity::new(ty, &id, &id, &id);
            kg.insert_entity(entity);
            prop_assert_eq!(kg.node_count(), 1);
            let _ = ty;
        }

        #[test]
        fn kg_insert_edge_is_bidirectional(
            a in arbitrary_entity_id(),
            b in arbitrary_entity_id(),
        ) {
            prop_assume!(a != b);
            let mut kg = KnowledgeGraph::new();
            kg.insert_entity(KgEntity::new(EntityType::Video, &a, &a, &a));
            kg.insert_entity(KgEntity::new(EntityType::Tag, &b, &b, &b));
            let a_id = format!("video:{a}");
            let b_id = format!("tag:{b}");
            kg.insert_edge(&a_id, &b_id, RelationType::Tags, 1.0);

            let a_neighbors = kg.neighbors(&a_id);
            prop_assert!(a_neighbors.iter().any(|(n, _, _)| n == &b_id));

            let b_neighbors = kg.neighbors(&b_id);
            prop_assert!(b_neighbors.iter().any(|(n, _, _)| n == &a_id));
        }

        #[test]
        fn kg_empty_graph_has_zero_counts(
            _dummy in 0..10u8,
        ) {
            let kg = KnowledgeGraph::new();
            prop_assert!(kg.is_empty());
            prop_assert_eq!(kg.node_count(), 0);
            prop_assert_eq!(kg.edge_count(), 0);
        }

        #[test]
        fn kg_bfs_respects_max_depth(
            entity_count in 1..20usize,
            edge_count in 0..50usize,
            max_depth in 0..5usize,
        ) {
            let kg = build_random_graph(entity_count, edge_count);
            if let Some(seed) = kg.entities.keys().next() {
                let result = kg.bfs(seed, max_depth);
                for (_, depth) in &result {
                    prop_assert!(*depth <= max_depth);
                }
            }
        }

        #[test]
        fn kg_neighborhood_excludes_seed(
            entity_count in 1..20usize,
            edge_count in 0..50usize,
            hops in 1..3usize,
        ) {
            let kg = build_random_graph(entity_count, edge_count);
            if let Some(seed) = kg.entities.keys().next() {
                let n = kg.neighborhood(seed, hops);
                prop_assert!(!n.contains_key(seed));
            }
        }

        #[test]
        fn kg_centrality_roundtrip(
            id in arbitrary_entity_id(),
            score in 0.0..1.0f64,
        ) {
            let mut kg = KnowledgeGraph::new();
            kg.insert_entity(KgEntity::new(EntityType::Video, &id, &id, &id));
            let entity_id = format!("video:{id}");
            kg.set_centrality(&entity_id, score);
            prop_assert_eq!(kg.get_centrality(&entity_id), Some(score));
            prop_assert_eq!(kg.get_entity(&entity_id).unwrap().centrality, Some(score));
        }

        #[test]
        fn kg_dedup_edges_keeps_highest_weight(
            a in arbitrary_entity_id(),
            b in arbitrary_entity_id(),
            w1 in 0.0..1.0f64,
            w2 in 0.0..1.0f64,
        ) {
            prop_assume!(a != b);
            let mut kg = KnowledgeGraph::new();
            kg.insert_entity(KgEntity::new(EntityType::Video, &a, &a, &a));
            kg.insert_entity(KgEntity::new(EntityType::Tag, &b, &b, &b));
            let a_id = format!("video:{a}");
            let b_id = format!("tag:{b}");
            kg.insert_edge(&a_id, &b_id, RelationType::Tags, w1);
            kg.insert_edge(&a_id, &b_id, RelationType::Tags, w2);
            kg.dedup_edges();
            let neighbors = kg.neighbors(&a_id);
            prop_assert_eq!(neighbors.len(), 1);
            let expected = w1.max(w2);
            prop_assert!((neighbors[0].2 - expected).abs() < 1e-9);
        }

        #[test]
        fn kg_node_count_matches_entities(
            entity_count in 1..50usize,
            edge_count in 0..100usize,
        ) {
            let kg = build_random_graph(entity_count, edge_count);
            prop_assert!(kg.node_count() <= entity_count);
        }
    }
}
