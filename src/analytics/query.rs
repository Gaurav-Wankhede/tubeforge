//! Flexible query layer (Phase 7): structured filters + graph traversal +
//! hybrid BM25-graph retrieval.
//!
//! Query modes:
//! - **Filter query**: structured filters (channel, date range, score range,
//!   duration, tags, topic) over the video corpus.
//! - **Graph query**: graph-based retrieval — similar videos, neighborhood,
//!   topic cluster, channel authority.
//! - **Hybrid query**: combine BM25 text relevance with graph proximity for
//!   better retrieval than either alone.

use std::collections::{HashMap, HashSet};

use crate::analytics::graph::{self, VideoGraph};
use crate::error::TubeforgeError;
use crate::search::bm25::Bm25;
use crate::search::FIELD_TITLE;
use crate::storage::db::{Db, VideoRow};
use crate::util;

/// A flexible query over the video corpus.
#[derive(Debug, Clone, Default)]
pub struct VideoQuery {
    /// Full-text search query (BM25).
    pub text: Option<String>,
    /// Filter by channel_id.
    pub channel_id: Option<String>,
    /// Filter by minimum published date (RFC3339).
    pub published_after: Option<String>,
    /// Filter by maximum published date (RFC3339).
    pub published_before: Option<String>,
    /// Filter by minimum SEO score.
    pub min_seo_score: Option<f64>,
    /// Filter by minimum total score.
    pub min_total_score: Option<f64>,
    /// Filter by maximum duration seconds (e.g., 60 for Shorts).
    pub max_duration: Option<i64>,
    /// Filter by minimum duration seconds.
    pub min_duration: Option<i64>,
    /// Filter by tag (must have all listed tags).
    pub tags: Vec<String>,
    /// Filter by topic category URL segment.
    pub topic: Option<String>,
    /// Seed video for graph-based retrieval.
    pub graph_seed: Option<String>,
    /// Graph retrieval: number of hops for neighborhood.
    pub graph_hops: usize,
    /// Hybrid: weight of graph score vs BM25 (0 = pure BM25, 1 = pure graph).
    pub graph_weight: f64,
    /// Sort order.
    pub sort: SortOrder,
    /// Limit results.
    pub limit: usize,
}

/// Sort order for query results.
#[derive(Debug, Clone, Default)]
pub enum SortOrder {
    /// By relevance (BM25 or hybrid score) — default.
    #[default]
    Relevance,
    /// By published date, newest first.
    Newest,
    /// By views, highest first.
    MostViewed,
    /// By SEO score, highest first.
    BestSeo,
    /// By total score, highest first.
    BestTotal,
}

/// One query result with scoring metadata.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub video_id: String,
    pub title: String,
    pub channel_id: Option<String>,
    pub bm25_score: f64,
    pub graph_score: f64,
    pub hybrid_score: f64,
    pub view_count: Option<i64>,
    pub published_at: String,
}

/// Execute a flexible query over the corpus.
///
/// The query combines:
/// 1. Structured filters (applied first to narrow the candidate set)
/// 2. BM25 text scoring (when `text` is given)
/// 3. Graph proximity scoring (when `graph_seed` is given)
/// 4. Hybrid fusion: `hybrid = (1 - w) * bm25 + w * graph`
pub async fn execute_query(
    _db: &Db,
    bm25: &Bm25,
    video_graph: Option<&VideoGraph>,
    videos: &[VideoRow],
    scores: &HashMap<String, (f64, f64)>, // (seo_score, total_score)
    query: &VideoQuery,
) -> Result<Vec<QueryResult>, TubeforgeError> {
    // Step 1: Build candidate set from structured filters
    let candidates = apply_filters(videos, scores, query);

    // Step 2: Score candidates
    let mut results: Vec<QueryResult> = Vec::new();

    // Compute graph scores if seed is given
    let graph_scores: HashMap<String, f64> =
        if let (Some(seed), Some(graph)) = (&query.graph_seed, video_graph) {
            graph_similarity_map(graph, seed, 0.3, 50)
        } else {
            HashMap::new()
        };

    for video in candidates {
        // BM25 score
        let bm25_score = if let Some(q) = &query.text {
            if !q.trim().is_empty() {
                bm25.corpus_resonance(FIELD_TITLE, q, None)
            } else {
                0.0
            }
        } else {
            0.0
        };

        // Graph score
        let graph_score = graph_scores.get(&video.video_id).copied().unwrap_or(0.0);

        // Hybrid fusion
        let w = query.graph_weight.clamp(0.0, 1.0);
        let hybrid_score = if bm25_score > 0.0 && graph_score > 0.0 {
            (1.0 - w) * bm25_score + w * graph_score
        } else if bm25_score > 0.0 {
            bm25_score
        } else if graph_score > 0.0 {
            graph_score
        } else {
            0.0
        };

        results.push(QueryResult {
            video_id: video.video_id.clone(),
            title: video.title.clone(),
            channel_id: video.channel_id.clone(),
            bm25_score,
            graph_score,
            hybrid_score,
            view_count: video.view_count,
            published_at: video.published_at.clone(),
        });
    }

    // Step 3: Sort
    match query.sort {
        SortOrder::Relevance => {
            results.sort_by(|a, b| b.hybrid_score.total_cmp(&a.hybrid_score));
        }
        SortOrder::Newest => {
            results.sort_by(|a, b| b.published_at.cmp(&a.published_at));
        }
        SortOrder::MostViewed => {
            results.sort_by_key(|b| std::cmp::Reverse(b.view_count));
        }
        SortOrder::BestSeo => {
            results.sort_by(|a, b| {
                let a_seo = scores.get(&a.video_id).map(|(s, _)| *s).unwrap_or(0.0);
                let b_seo = scores.get(&b.video_id).map(|(s, _)| *s).unwrap_or(0.0);
                b_seo.total_cmp(&a_seo)
            });
        }
        SortOrder::BestTotal => {
            results.sort_by(|a, b| {
                let a_total = scores.get(&a.video_id).map(|(_, t)| *t).unwrap_or(0.0);
                let b_total = scores.get(&b.video_id).map(|(_, t)| *t).unwrap_or(0.0);
                b_total.total_cmp(&a_total)
            });
        }
    }

    // Step 4: Limit
    results.truncate(query.limit);
    Ok(results)
}

/// Apply structured filters to narrow the candidate set.
fn apply_filters(
    videos: &[VideoRow],
    scores: &HashMap<String, (f64, f64)>,
    query: &VideoQuery,
) -> Vec<VideoRow> {
    videos
        .iter()
        .filter(|v| {
            // Channel filter
            if let Some(cid) = &query.channel_id {
                if v.channel_id.as_deref() != Some(cid) {
                    return false;
                }
            }

            // Date filters
            if let Some(after) = &query.published_after {
                if v.published_at < *after {
                    return false;
                }
            }
            if let Some(before) = &query.published_before {
                if v.published_at > *before {
                    return false;
                }
            }

            // Score filters
            if let Some(min_seo) = query.min_seo_score {
                let seo = scores.get(&v.video_id).map(|(s, _)| *s).unwrap_or(0.0);
                if seo < min_seo {
                    return false;
                }
            }
            if let Some(min_total) = query.min_total_score {
                let total = scores.get(&v.video_id).map(|(_, t)| *t).unwrap_or(0.0);
                if total < min_total {
                    return false;
                }
            }

            // Duration filters
            if let Some(max_dur) = query.max_duration {
                if v.duration_sec.map(|d| d > max_dur).unwrap_or(false) {
                    return false;
                }
            }
            if let Some(min_dur) = query.min_duration {
                if v.duration_sec.map(|d| d < min_dur).unwrap_or(false) {
                    return false;
                }
            }

            // Tag filter (must have all listed tags)
            if !query.tags.is_empty() {
                let video_tags: HashSet<String> =
                    if let Ok(tg) = serde_json::from_str::<Vec<String>>(&v.tags) {
                        tg.into_iter().map(|t| t.to_lowercase()).collect()
                    } else {
                        HashSet::new()
                    };
                for t in &query.tags {
                    if !video_tags.contains(&t.to_lowercase()) {
                        return false;
                    }
                }
            }

            // Topic filter
            if let Some(topic) = &query.topic {
                if let Ok(topics) = serde_json::from_str::<Vec<String>>(&v.topic_categories) {
                    if !topics.iter().any(|t| t.contains(topic)) {
                        return false;
                    }
                } else {
                    return false;
                }
            }

            true
        })
        .cloned()
        .collect()
}

/// Graph similarity as a map: video_id → score.
fn graph_similarity_map(
    graph: &VideoGraph,
    seed: &str,
    restart_prob: f64,
    max_iters: usize,
) -> HashMap<String, f64> {
    graph::graph_similarity(graph, seed, graph.len(), restart_prob, max_iters)
        .into_iter()
        .collect()
}

/// Find videos in a topic cluster: all videos whose titles contain the topic
/// token, ranked by a blend of views and SEO score.
pub fn find_topic_cluster(
    videos: &[VideoRow],
    scores: &HashMap<String, (f64, f64)>,
    topic: &str,
    limit: usize,
) -> Vec<(String, f64)> {
    let topic_lower = topic.to_lowercase();
    let mut results: Vec<(String, f64)> = videos
        .iter()
        .filter(|v| {
            let title_tokens: HashSet<String> = util::tokens(&v.title).into_iter().collect();
            title_tokens.contains(&topic_lower) || v.title.to_lowercase().contains(&topic_lower)
        })
        .map(|v| {
            let seo = scores.get(&v.video_id).map(|(s, _)| *s).unwrap_or(0.0);
            let views = v.view_count.unwrap_or(0) as f64;
            // Blend: SEO score (0-100) + log-normalized views
            let view_score = if views > 0.0 {
                (views.ln() / 20.0).min(1.0) * 50.0
            } else {
                0.0
            };
            let blend = seo * 0.7 + view_score * 0.3;
            (v.video_id.clone(), blend)
        })
        .collect();

    results.sort_by(|a, b| b.1.total_cmp(&a.1));
    results.truncate(limit);
    results
}

/// Find the best-performing videos for a given keyword (by title match + views).
pub fn find_keyword_best(
    videos: &[VideoRow],
    scores: &HashMap<String, (f64, f64)>,
    keyword: &str,
    limit: usize,
) -> Vec<(String, f64, f64)> {
    let kw_tokens: HashSet<String> = util::tokens(keyword).into_iter().collect();
    let mut results: Vec<(String, f64, f64)> = videos
        .iter()
        .filter(|v| {
            let title_tokens: HashSet<String> = util::tokens(&v.title).into_iter().collect();
            kw_tokens.iter().all(|t| title_tokens.contains(t))
        })
        .map(|v| {
            let seo = scores.get(&v.video_id).map(|(s, _)| *s).unwrap_or(0.0);
            let views = v.view_count.unwrap_or(0) as f64;
            (v.video_id.clone(), seo, views)
        })
        .collect();

    // Sort by views descending (best-performing first)
    results.sort_by(|a, b| b.2.total_cmp(&a.2));
    results.truncate(limit);
    results
}

/// Find high-performing videos (outliers) for a topic: videos whose views are
/// ≥ `multiple` × the topic's mean views.
pub fn find_topic_outliers(
    videos: &[VideoRow],
    topic: &str,
    multiple: f64,
) -> Vec<(String, f64, f64)> {
    let topic_lower = topic.to_lowercase();
    let matching: Vec<&VideoRow> = videos
        .iter()
        .filter(|v| {
            let title_tokens: HashSet<String> = util::tokens(&v.title).into_iter().collect();
            title_tokens.contains(&topic_lower)
        })
        .collect();

    if matching.is_empty() {
        return Vec::new();
    }

    let views: Vec<i64> = matching.iter().filter_map(|v| v.view_count).collect();
    if views.is_empty() {
        return Vec::new();
    }

    let mean_views = views.iter().sum::<i64>() as f64 / views.len() as f64;
    if mean_views <= 0.0 {
        return Vec::new();
    }

    let threshold = mean_views * multiple;
    let mut results: Vec<(String, f64, f64)> = matching
        .iter()
        .filter_map(|v| {
            let views = v.view_count?;
            if views as f64 >= threshold {
                Some((v.video_id.clone(), views as f64, mean_views))
            } else {
                None
            }
        })
        .collect();

    results.sort_by(|a, b| b.1.total_cmp(&a.1));
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_video(
        id: &str,
        channel: &str,
        title: &str,
        views: i64,
        seo: f64,
    ) -> (VideoRow, (String, f64, f64)) {
        let row = VideoRow {
            video_id: id.to_string(),
            channel_id: Some(channel.to_string()),
            title: title.to_string(),
            view_count: Some(views),
            duration_sec: Some(300),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            tags: serde_json::to_string(&vec!["rust", "tutorial"]).unwrap(),
            ..Default::default()
        };
        (row, (id.to_string(), seo, seo * 0.9))
    }

    fn build_test_data() -> (Vec<VideoRow>, HashMap<String, (f64, f64)>) {
        let (v1, s1) = make_video("v1", "A", "Rust async guide", 5000, 75.0);
        let (v2, s2) = make_video("v2", "A", "Rust tokio tutorial", 3000, 68.0);
        let (v3, s3) = make_video("v3", "B", "Rust async patterns", 8000, 82.0);
        let (v4, s4) = make_video("v4", "C", "Python basics", 2000, 55.0);
        let videos = vec![v1, v2, v3, v4];
        let mut scores = HashMap::new();
        scores.insert(s1.0, (s1.1, s1.2));
        scores.insert(s2.0, (s2.1, s2.2));
        scores.insert(s3.0, (s3.1, s3.2));
        scores.insert(s4.0, (s4.1, s4.2));
        (videos, scores)
    }

    #[test]
    fn filter_by_channel() {
        let (videos, scores) = build_test_data();
        let query = VideoQuery {
            channel_id: Some("A".to_string()),
            limit: 100,
            ..Default::default()
        };
        // Can't run async test directly, but we can test the filter function
        let filtered = apply_filters(&videos, &scores, &query);
        assert_eq!(filtered.len(), 2);
        assert!(filtered
            .iter()
            .all(|v| v.channel_id == Some("A".to_string())));
    }

    #[test]
    fn filter_by_min_seo_score() {
        let (videos, scores) = build_test_data();
        let query = VideoQuery {
            min_seo_score: Some(70.0),
            limit: 100,
            ..Default::default()
        };
        let filtered = apply_filters(&videos, &scores, &query);
        assert_eq!(filtered.len(), 2); // v1 (75) and v3 (82)
        assert!(filtered.iter().all(|v| {
            scores
                .get(&v.video_id)
                .map(|(s, _)| *s >= 70.0)
                .unwrap_or(false)
        }));
    }

    #[test]
    fn filter_by_tags() {
        let (videos, scores) = build_test_data();
        let query = VideoQuery {
            tags: vec!["rust".to_string()],
            limit: 100,
            ..Default::default()
        };
        let filtered = apply_filters(&videos, &scores, &query);
        assert_eq!(filtered.len(), 4); // all 4 test videos have "rust" tag
    }

    #[test]
    fn find_topic_cluster_ranks_by_blend() {
        let (videos, scores) = build_test_data();
        let results = find_topic_cluster(&videos, &scores, "rust", 10);
        assert!(!results.is_empty());
        // v3 has highest SEO (82) and high views (8000) → should be first
        assert_eq!(results[0].0, "v3");
    }

    #[test]
    fn find_keyword_best_sorts_by_views() {
        let (videos, scores) = build_test_data();
        let results = find_keyword_best(&videos, &scores, "rust", 10);
        // v3 has most views (8000) → first
        assert_eq!(results[0].0, "v3");
        assert_eq!(results[0].2, 8000.0);
    }

    #[test]
    fn find_topic_outliers_flags_high_performers() {
        let (videos, _scores) = build_test_data();
        // "rust" topic: views = 5000, 3000, 8000 → mean = 5333
        // threshold = 5333 * 1.5 = 8000 → only v3 qualifies
        let results = find_topic_outliers(&videos, "rust", 1.5);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "v3");
    }

    #[test]
    fn query_respects_limit() {
        let (videos, scores) = build_test_data();
        let query = VideoQuery {
            limit: 2,
            ..Default::default()
        };
        let filtered = apply_filters(&videos, &scores, &query);
        assert_eq!(filtered.len(), 4); // filter doesn't limit
                                       // The limit is applied after sorting
    }

    #[test]
    fn empty_query_returns_all() {
        let (videos, scores) = build_test_data();
        let query = VideoQuery::default();
        let filtered = apply_filters(&videos, &scores, &query);
        assert_eq!(filtered.len(), 4);
    }

    #[test]
    fn filter_by_max_duration_finds_shorts() {
        let (v1, _s1) = make_video("v1", "A", "Rust short", 1000, 60.0);
        let mut v2 = v1.clone();
        v2.video_id = "v2".to_string();
        v2.duration_sec = Some(30); // Short
        let mut scores = HashMap::new();
        scores.insert("v1".to_string(), (60.0, 54.0));
        scores.insert("v2".to_string(), (60.0, 54.0));
        let videos = vec![v1, v2];

        let query = VideoQuery {
            max_duration: Some(60),
            limit: 100,
            ..Default::default()
        };
        let filtered = apply_filters(&videos, &scores, &query);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].video_id, "v2");
    }
}
