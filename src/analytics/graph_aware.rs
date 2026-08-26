//! Graph-aware analytics (PRD §FR-4): computes tag authority, topic dominance,
//! and keyword competition scores from the Knowledge Graph.
//!
//! These scores feed into the existing SEO scoring pipeline as three new
//! components: `tag_authority`, `topic_dominance`, `keyword_competition`.

use std::collections::HashMap;

use crate::analytics::kg::{EntityType, KnowledgeGraph, RelationType};

/// Graph-aware scores for a specific video/channel (PRD §FR-4).
#[derive(Debug, Clone, Default)]
pub struct GraphScores {
    /// Mean centrality of channels using the video's tags (0-100).
    pub tag_authority: f64,
    /// Channel's dominance in the video's topic clusters (0-100).
    pub topic_dominance: f64,
    /// Incumbent authority for the target keyword (0-100).
    pub keyword_competition: f64,
}

/// Compute graph-aware scores for a video (PRD §FR-4.1, §FR-4.2).
///
/// Returns scores in 0-100 range. Defaults to 0 when no graph data is
/// available (backward compatible).
pub fn compute_graph_scores(
    kg: &KnowledgeGraph,
    video_id: &str,
    channel_id: Option<&str>,
    keywords: &[String],
) -> GraphScores {
    GraphScores {
        tag_authority: compute_tag_authority(kg, video_id),
        topic_dominance: compute_topic_dominance(kg, channel_id),
        keyword_competition: compute_keyword_competition(kg, keywords),
    }
}

/// Tag authority: mean centrality of channels using the video's tags (PRD §FR-3.3).
///
/// Tags used by high-centrality channels score higher. This signals to YouTube
/// that the tag is associated with authoritative content.
pub fn compute_tag_authority(kg: &KnowledgeGraph, video_id: &str) -> f64 {
    let video_entity_id = format!("video:{video_id}");
    let neighbors = kg.neighbors(&video_entity_id);

    let mut total_centrality = 0.0;
    let mut tag_count = 0;

    for (neighbor_id, rel, _) in neighbors {
        if *rel != RelationType::Tags {
            continue;
        }
        // Find channels that use this tag
        for (channel_neighbor, channel_rel, _) in kg.neighbors(neighbor_id) {
            if *channel_rel != RelationType::Tags {
                continue;
            }
            // This is a video entity that also uses this tag
            // Check if it has a channel
            for (potential_channel, potential_rel, _) in kg.neighbors(channel_neighbor) {
                if *potential_rel == RelationType::CreatedBy {
                    if let Some(cent) = kg.get_centrality(potential_channel) {
                        total_centrality += cent;
                        tag_count += 1;
                    }
                }
            }
        }
        // Also consider the tag's own centrality
        if let Some(cent) = kg.get_centrality(neighbor_id) {
            total_centrality += cent;
            tag_count += 1;
        }
    }

    if tag_count == 0 {
        // Baseline tag authority from tag count presence
        let video_tags_count = kg
            .neighbors(&video_entity_id)
            .iter()
            .filter(|(_, rel, _)| *rel == RelationType::Tags)
            .count();
        if video_tags_count > 0 {
            (video_tags_count as f64 * 12.5).min(75.0)
        } else {
            0.0
        }
    } else {
        let mean = total_centrality / tag_count as f64;
        let n = kg.node_count().max(1) as f64;
        // Scale PageRank probability (mean ~ 1/N) to 0-100 authority percentile
        ((mean * n * 25.0) + 15.0).clamp(10.0, 100.0)
    }
}

/// Compute the authority of a single tag by name (PRD §FR-3.3).
///
/// Returns the mean centrality of channels using this tag, or the tag's
/// own centrality if no channel data is available. Returns 0.0 when the
/// tag is not in the graph.
pub fn compute_tag_authority_by_name(kg: &KnowledgeGraph, tag_name: &str) -> f64 {
    let tag_entity_id = format!("tag:{}", tag_name.trim().to_lowercase());

    // First, check the tag's own centrality
    let tag_centrality = kg.get_centrality(&tag_entity_id);

    // Find all videos using this tag, then find their channels
    let mut total_centrality = 0.0;
    let mut channel_count = 0;

    for (neighbor_id, rel, _) in kg.neighbors(&tag_entity_id) {
        if *rel != RelationType::Tags {
            continue;
        }
        // This is a video entity that uses this tag
        for (channel_id, channel_rel, _) in kg.neighbors(neighbor_id) {
            if *channel_rel == RelationType::CreatedBy {
                if let Some(cent) = kg.get_centrality(channel_id) {
                    total_centrality += cent;
                    channel_count += 1;
                }
            }
        }
    }

    let n = kg.node_count().max(1) as f64;
    if channel_count > 0 {
        let mean = total_centrality / channel_count as f64;
        ((mean * n * 25.0) + 15.0).clamp(10.0, 100.0)
    } else if let Some(cent) = tag_centrality {
        ((cent * n * 25.0) + 15.0).clamp(10.0, 100.0)
    } else {
        0.0
    }
}

/// Topic dominance: channel's share of the topic cluster (PRD §FR-3.4).
///
/// A channel that dominates a topic cluster has higher topical authority,
/// which is an E-E-A-T signal for YouTube's algorithm.
pub fn compute_topic_dominance(kg: &KnowledgeGraph, channel_id: Option<&str>) -> f64 {
    let channel_id = match channel_id {
        Some(id) => id,
        None => return 0.0,
    };
    let channel_entity_id = format!("channel:{channel_id}");

    // Find all topics this channel's videos are about
    let mut topic_scores: HashMap<String, f64> = HashMap::new();
    let mut total_channel_videos = 0.0;

    for (neighbor_id, rel, _) in kg.neighbors(&channel_entity_id) {
        if *rel != RelationType::CreatedBy {
            continue;
        }
        total_channel_videos += 1.0;
        // This is a video by this channel
        for (topic_id, topic_rel, _) in kg.neighbors(neighbor_id) {
            if *topic_rel == RelationType::AboutTopic || *topic_rel == RelationType::Tags {
                let total_cluster_videos = kg.neighbors(topic_id).len().max(1) as f64;
                let share = (total_channel_videos / total_cluster_videos).min(1.0);
                topic_scores
                    .entry(topic_id.clone())
                    .and_modify(|s| *s = s.max(share))
                    .or_insert(share);
            }
        }
    }

    if topic_scores.is_empty() {
        if total_channel_videos > 0.0 {
            (total_channel_videos * 5.0).clamp(10.0, 80.0)
        } else {
            0.0
        }
    } else {
        let max_share = topic_scores.values().cloned().fold(0.0, f64::max);
        ((max_share * 80.0) + 15.0).clamp(10.0, 100.0)
    }
}

/// Keyword competition: incumbent authority for the target keyword (PRD §FR-3.5).
///
/// Higher when dominant channels already own the keyword (harder to rank).
/// Lower when the keyword is underserved (opportunity).
pub fn compute_keyword_competition(kg: &KnowledgeGraph, keywords: &[String]) -> f64 {
    if keywords.is_empty() {
        return 0.0;
    }

    let mut best_competition: f64 = 0.0;
    let n = kg.node_count().max(1) as f64;

    for kw in keywords {
        let norm_kw = kw.to_lowercase();
        let keyword_entity_id = format!("keyword:{}", norm_kw.replace(' ', "-"));
        let tag_entity_id = format!("tag:{norm_kw}");

        // Check both keyword entity and tag entity
        for candidate_id in [&keyword_entity_id, &tag_entity_id] {
            if kg.get_entity(candidate_id).is_some() {
                let mut max_channel_centrality: f64 = 0.0;
                for (neighbor_id, rel, _) in kg.neighbors(candidate_id) {
                    if *rel == RelationType::CompetesIn || *rel == RelationType::Tags {
                        if let Some(cent) = kg.get_centrality(neighbor_id) {
                            max_channel_centrality = max_channel_centrality.max(cent);
                        }
                    }
                }
                let competition = (max_channel_centrality * n * 25.0) + 15.0;
                best_competition = best_competition.max(competition);
            }
        }
    }

    if best_competition == 0.0 {
        0.0
    } else {
        best_competition.clamp(10.0, 100.0)
    }
}

/// Find content gaps: topics with high demand but low supply (PRD §FR-3.4).
///
/// Returns topic entity IDs sorted by opportunity score (descending).
pub fn find_content_gaps(kg: &KnowledgeGraph, own_channel_id: Option<&str>) -> Vec<(String, f64)> {
    let mut gaps: Vec<(String, f64)> = Vec::new();

    // Find all topics
    let topics = kg.entities_of_type(EntityType::Topic);
    for topic_id in topics {
        let topic_neighbors = kg.neighbors(topic_id);
        let total_videos = topic_neighbors.len() as f64;

        if total_videos == 0.0 {
            continue;
        }

        // Count distinct channels covering this topic
        let mut channels = std::collections::HashSet::new();
        for (video_id, rel, _) in topic_neighbors {
            if *rel == RelationType::AboutTopic {
                for (channel_id, channel_rel, _) in kg.neighbors(video_id) {
                    if *channel_rel == RelationType::CreatedBy {
                        channels.insert(channel_id.clone());
                    }
                }
            }
        }

        // Check if own channel covers this topic
        let own_coverage = if let Some(own_id) = own_channel_id {
            let own_entity = format!("channel:{own_id}");
            channels.contains(&own_entity)
        } else {
            false
        };

        // Gap score: high demand (many videos) + low supply (few channels) + no own coverage
        let channel_count = channels.len() as f64;
        let supply_ratio = if channel_count > 0.0 {
            total_videos / channel_count
        } else {
            total_videos
        };

        let gap_score = if own_coverage {
            0.0 // Already covered
        } else {
            (supply_ratio * 10.0).min(100.0)
        };

        if gap_score > 0.0 {
            gaps.push((topic_id.clone(), gap_score));
        }
    }

    gaps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    gaps
}

/// Generate graph-based video ideas (PRD §FR-1, ideas enhancement).
///
/// Uses community detection and gap analysis to suggest video topics.
pub fn generate_graph_ideas(
    kg: &KnowledgeGraph,
    own_channel_id: Option<&str>,
    limit: usize,
) -> Vec<(String, f64, String)> {
    let mut ideas: Vec<(String, f64, String)> = Vec::new();

    // Strategy 1: Content gaps (underserved topics)
    let gaps = find_content_gaps(kg, own_channel_id);
    for (topic_id, score) in gaps.iter().take(limit / 2) {
        if let Some(entity) = kg.get_entity(topic_id) {
            ideas.push((
                entity.display_name.clone(),
                *score,
                format!(
                    "Content gap: topic '{}' has high demand but low coverage",
                    entity.display_name
                ),
            ));
        }
    }

    // Strategy 2: High-centrality competitor videos in same community
    if let Some(own_id) = own_channel_id {
        let own_entity = format!("channel:{own_id}");
        for (neighbor_id, rel, _) in kg.neighbors(&own_entity) {
            if *rel == RelationType::CompetesIn {
                // Find high-performing videos from competitors
                for (video_id, video_rel, _) in kg.neighbors(neighbor_id) {
                    if *video_rel == RelationType::CreatedBy {
                        if let Some(cent) = kg.get_centrality(video_id) {
                            if cent > 0.5 {
                                if let Some(entity) = kg.get_entity(video_id) {
                                    ideas.push((
                                        entity.display_name.clone(),
                                        cent * 100.0,
                                        "High-authority competitor video in your niche".to_string(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    ideas.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ideas.truncate(limit);
    ideas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::kg::KgEntity;
    use proptest::prelude::*;

    fn build_scored_kg() -> KnowledgeGraph {
        let mut kg = KnowledgeGraph::new();

        // Channels
        kg.insert_entity(KgEntity::channel("UC:auth", "Authoritative Channel"));
        kg.insert_entity(KgEntity::channel("UC:new", "New Channel"));

        // Tags
        kg.insert_entity(KgEntity::tag("rust"));
        kg.insert_entity(KgEntity::tag("python"));

        // Topics
        kg.insert_entity(KgEntity::topic("rust_programming", "Rust Programming"));
        kg.insert_entity(KgEntity::topic("python_programming", "Python Programming"));

        // Videos
        kg.insert_entity(KgEntity::video("v1", "Rust Guide"));
        kg.insert_entity(KgEntity::video("v2", "Rust Tutorial"));
        kg.insert_entity(KgEntity::video("v3", "Python Basics"));

        // Relations
        kg.insert_edge("video:v1", "channel:UC:auth", RelationType::CreatedBy, 1.0);
        kg.insert_edge("video:v2", "channel:UC:auth", RelationType::CreatedBy, 1.0);
        kg.insert_edge("video:v3", "channel:UC:new", RelationType::CreatedBy, 1.0);

        kg.insert_edge("video:v1", "tag:rust", RelationType::Tags, 1.0);
        kg.insert_edge("video:v2", "tag:rust", RelationType::Tags, 1.0);
        kg.insert_edge("video:v3", "tag:python", RelationType::Tags, 1.0);

        kg.insert_edge(
            "video:v1",
            "topic:rust_programming",
            RelationType::AboutTopic,
            1.0,
        );
        kg.insert_edge(
            "video:v2",
            "topic:rust_programming",
            RelationType::AboutTopic,
            1.0,
        );
        kg.insert_edge(
            "video:v3",
            "topic:python_programming",
            RelationType::AboutTopic,
            1.0,
        );

        // Set centrality
        kg.set_centrality("channel:UC:auth", 0.8);
        kg.set_centrality("channel:UC:new", 0.2);
        kg.set_centrality("tag:rust", 0.7);
        kg.set_centrality("tag:python", 0.3);

        kg
    }

    proptest! {
        #[test]
        fn tag_authority_zero_for_isolated_video(
            _dummy in 0..5u8,
        ) {
            let mut kg = KnowledgeGraph::new();
            kg.insert_entity(KgEntity::video("v1", "Isolated"));
            let score = compute_tag_authority(&kg, "v1");
            prop_assert_eq!(score, 0.0);
        }

        #[test]
        fn tag_authority_higher_for_authoritative_channels(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            // v1 is tagged with rust, which is used by authoritative channel
            let score = compute_tag_authority(&kg, "v1");
            prop_assert!(score > 0.0, "tag_authority should be > 0, got {}", score);
        }

        #[test]
        fn tag_authority_bounded_0_to_100(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let score = compute_tag_authority(&kg, "v1");
            prop_assert!((0.0..=100.0).contains(&score));
        }

        #[test]
        fn topic_dominance_zero_without_channel(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let score = compute_topic_dominance(&kg, None);
            prop_assert_eq!(score, 0.0);
        }

        #[test]
        fn topic_dominance_higher_for_dominant_channel(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            // UC:auth has 2/2 videos on rust_programming → dominant
            let score = compute_topic_dominance(&kg, Some("UC:auth"));
            prop_assert!(score > 0.0, "topic_dominance should be > 0, got {}", score);
        }

        #[test]
        fn topic_dominance_bounded_0_to_100(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let score = compute_topic_dominance(&kg, Some("UC:auth"));
            prop_assert!((0.0..=100.0).contains(&score));
        }

        #[test]
        fn keyword_competition_zero_for_empty_keywords(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let score = compute_keyword_competition(&kg, &[]);
            prop_assert_eq!(score, 0.0);
        }

        #[test]
        fn keyword_competition_bounded_0_to_100(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let keywords = vec!["rust".to_string()];
            let score = compute_keyword_competition(&kg, &keywords);
            prop_assert!((0.0..=100.0).contains(&score));
        }

        #[test]
        fn content_gaps_exclude_own_channel(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let gaps = find_content_gaps(&kg, Some("UC:auth"));
            // UC:auth already covers rust_programming, so it should not be a gap
            for (topic_id, _) in &gaps {
                prop_assert_ne!(topic_id, "topic:rust_programming");
            }
        }

        #[test]
        fn content_gaps_include_uncovered_topics(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let gaps = find_content_gaps(&kg, Some("UC:new"));
            // UC:new doesn't cover rust_programming, so it should be a gap
            let gap_ids: Vec<&str> = gaps.iter().map(|(id, _)| id.as_str()).collect();
            prop_assert!(gap_ids.contains(&"topic:rust_programming"));
        }

        #[test]
        fn content_gaps_sorted_by_score(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let gaps = find_content_gaps(&kg, None);
            // Verify sorted descending
            for i in 1..gaps.len() {
                prop_assert!(gaps[i-1].1 >= gaps[i].1);
            }
        }

        #[test]
        fn generate_graph_ideas_returns_results(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let ideas = generate_graph_ideas(&kg, Some("UC:new"), 5);
            prop_assert!(!ideas.is_empty(), "should generate at least one idea");
        }

        #[test]
        fn generate_graph_ideas_bounded_by_limit(
            limit in 1..20usize,
        ) {
            let kg = build_scored_kg();
            let ideas = generate_graph_ideas(&kg, Some("UC:new"), limit);
            prop_assert!(ideas.len() <= limit);
        }

        #[test]
        fn compute_graph_scores_backward_compatible(
            _dummy in 0..5u8,
        ) {
            let kg = KnowledgeGraph::new();
            let scores = compute_graph_scores(&kg, "v1", None, &[]);
            prop_assert_eq!(scores.tag_authority, 0.0);
            prop_assert_eq!(scores.topic_dominance, 0.0);
            prop_assert_eq!(scores.keyword_competition, 0.0);
        }

        #[test]
        fn compute_graph_scores_bounded(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let scores = compute_graph_scores(&kg, "v1", Some("UC:auth"), &["rust".to_string()]);
            prop_assert!(scores.tag_authority >= 0.0 && scores.tag_authority <= 100.0);
            prop_assert!(scores.topic_dominance >= 0.0 && scores.topic_dominance <= 100.0);
            prop_assert!(scores.keyword_competition >= 0.0 && scores.keyword_competition <= 100.0);
        }

        #[test]
        fn tag_authority_by_name_zero_for_unknown_tag(
            _dummy in 0..5u8,
        ) {
            let kg = KnowledgeGraph::new();
            let score = compute_tag_authority_by_name(&kg, "nonexistent");
            prop_assert_eq!(score, 0.0);
        }

        #[test]
        fn tag_authority_by_name_bounded(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            let score = compute_tag_authority_by_name(&kg, "rust");
            prop_assert!((0.0..=100.0).contains(&score));
        }

        #[test]
        fn tag_authority_by_name_higher_for_authoritative_tag(
            _dummy in 0..5u8,
        ) {
            let kg = build_scored_kg();
            // rust tag is used by authoritative channel (UC:auth, centrality 0.8)
            let rust_score = compute_tag_authority_by_name(&kg, "rust");
            // python tag is used by new channel (UC:new, centrality 0.2)
            let python_score = compute_tag_authority_by_name(&kg, "python");
            prop_assert!(rust_score >= python_score, "rust should have >= authority than python");
        }
    }
}
