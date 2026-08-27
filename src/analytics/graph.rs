//! Competitor + video graph (LLD §8.1, enhanced).
//!
//! Two graph layers:
//! 1. **Channel graph** (existing): adjacency from `edges` — manual edges +
//!    auto-suggested competitor-overlap edges (co-occurring tokens in
//!    titles/tags, weight = Jaccard overlap strength) — weighted PageRank
//!    (damped 0.85, 50 iterations), centrality per channel cached in `meta`.
//! 2. **Video graph** (new): video-to-video edges from shared tags, shared
//!    keywords, and topic overlap. Used for graph-based similarity retrieval
//!    and graph-aware SEO scoring components.
//! 3. **Keyword-channel graph** (new): which channels dominate which keywords,
//!    weighted by the channel's dominance in that keyword's SERP neighborhood.

use std::collections::{HashMap, HashSet};

use crate::error::TubeforgeError;
use crate::storage::db::{Db, VideoRow};
use crate::util;

/// PageRank damping factor (LLD §8.1).
pub const DAMPING: f64 = 0.85;
/// PageRank iterations (LLD §8.1 — converges trivially at this scale).
pub const ITERATIONS: usize = 50;

/// meta cache keys for the computed graph (LLD §8.1 "persisted in meta").
const META_PR: &str = "graph_pagerank_json";
const META_HASH: &str = "graph_videos_hash";
const META_AT: &str = "graph_cache_at";
/// Video graph cache key (new).
const META_VIDEO_GRAPH: &str = "video_graph_json";
/// Keyword-channel graph cache key (new).
const META_KW_CHANNEL_GRAPH: &str = "kw_channel_graph_json";

// ---------------------------------------------------------------------------
// Channel graph (existing, preserved)
// ---------------------------------------------------------------------------

/// Weighted PageRank over `nodes` with `edges` (from, to, weight).
/// Deterministic; returns a dense rank per node (sums to 1 over non-empty
/// graphs). Dangling nodes (no outgoing edges) distribute their mass evenly.
pub fn pagerank(
    nodes: &[String],
    edges: &[(String, String, f64)],
    damping: f64,
    iterations: usize,
) -> HashMap<String, f64> {
    let n = nodes.len();
    if n == 0 {
        return HashMap::new();
    }
    let index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();

    let mut out: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for (from, to, weight) in edges {
        if let (Some(&fi), Some(&ti)) = (index.get(from.as_str()), index.get(to.as_str())) {
            if fi != ti {
                out[fi].push((ti, weight.max(0.0)));
            }
        }
    }
    let out_sum: Vec<f64> = out.iter().map(|l| l.iter().map(|(_, w)| w).sum()).collect();
    let d = damping.clamp(0.0, 1.0);

    let mut pr = vec![1.0 / n as f64; n];
    for _ in 0..iterations {
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

/// Recompute the auto-suggested overlap edges (LLD §8.1): every channel pair
/// sharing ≥1 title/tag token gets a symmetric `overlap` edge whose weight is
/// the token Jaccard. Stale overlap edges are dropped first; `manual` edges
/// are untouched. Returns the number of overlap edges written.
pub async fn sync_overlap_edges(db: &Db, videos: &[VideoRow]) -> Result<usize, TubeforgeError> {
    let mut by_channel: HashMap<String, HashSet<String>> = HashMap::new();
    for v in videos {
        let Some(cid) = &v.channel_id else { continue };
        let mut tokens: HashSet<String> = util::tokens(&v.title).into_iter().collect();
        if let Ok(tags) = serde_json::from_str::<Vec<String>>(&v.tags) {
            for t in &tags {
                tokens.extend(util::tokens(t));
            }
        }
        if !tokens.is_empty() {
            by_channel.entry(cid.clone()).or_default().extend(tokens);
        }
    }

    let mut channels: Vec<String> = by_channel.keys().cloned().collect();
    channels.sort();
    let mut fresh: Vec<(String, String, f64)> = Vec::new();
    for i in 0..channels.len() {
        for j in (i + 1)..channels.len() {
            let a = &by_channel[&channels[i]];
            let b = &by_channel[&channels[j]];
            let inter = a.intersection(b).count();
            if inter == 0 {
                continue;
            }
            let union = a.len() + b.len() - inter;
            let weight = if union == 0 {
                1.0
            } else {
                inter as f64 / union as f64
            };
            if weight >= 0.08 {
                fresh.push((channels[i].clone(), channels[j].clone(), weight));
            }
        }
    }

    fresh.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    fresh.truncate(1000);

    db.delete_overlap_edges().await?;
    let mut written = 0;
    for (a, b, w) in fresh {
        db.upsert_edge(&a, &b, w, "overlap").await?;
        db.upsert_edge(&b, &a, w, "overlap").await?;
        written += 2;
    }
    Ok(written)
}

/// Centrality per channel: sync overlap edges when the corpus changed,
/// run PageRank over all channels, cache the result in `meta` (LLD §8.1).
pub async fn build(db: &Db, videos: &[VideoRow]) -> Result<HashMap<String, f64>, TubeforgeError> {
    let hash = content_hash(videos);
    if db.meta_get(META_HASH).await?.as_deref() == Some(hash.as_str()) {
        if let Some(cached) = db.meta_get(META_PR).await? {
            if let Ok(map) = serde_json::from_str::<HashMap<String, f64>>(&cached) {
                return Ok(map);
            }
        }
    }

    let _overlap = sync_overlap_edges(db, videos).await?;
    let nodes: Vec<String> = db
        .all_channels()
        .await?
        .into_iter()
        .map(|c| c.channel_id)
        .collect();
    let edges: Vec<(String, String, f64)> = db
        .list_edges()
        .await?
        .into_iter()
        .map(|e| (e.from_channel, e.to_channel, e.weight))
        .collect();
    let pr = pagerank(&nodes, &edges, DAMPING, ITERATIONS);

    db.meta_set(META_PR, &serde_json::to_string(&pr)?).await?;
    db.meta_set(META_HASH, &hash).await?;
    db.meta_set(META_AT, &util::now_rfc3339()).await?;
    Ok(pr)
}

/// Cache-invalidation hash over (video_id, title, tags) — the overlap edges
/// and centrality depend only on this content.
fn content_hash(videos: &[VideoRow]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    for v in videos {
        v.video_id.hash(&mut hasher);
        v.title.hash(&mut hasher);
        v.tags.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

// ---------------------------------------------------------------------------
// Video graph (new): video-to-video edges for similarity retrieval
// ---------------------------------------------------------------------------

/// A video-to-video edge with weight.
#[derive(Debug, Clone)]
pub struct VideoEdge {
    pub from_video: String,
    pub to_video: String,
    pub weight: f64,
    /// Why the edge exists: "tags" | "topic" | "keyword"
    pub kind: String,
}

/// The full video graph: adjacency list + node set.
#[derive(Debug, Clone)]
pub struct VideoGraph {
    /// video_id → [(neighbor_id, weight)]
    pub adj: HashMap<String, Vec<(String, f64)>>,
    /// video_id → channel_id (for filtering)
    pub video_channel: HashMap<String, Option<String>>,
    /// Set of all node IDs (including isolated nodes with no edges)
    pub nodes: HashSet<String>,
}

impl Default for VideoGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoGraph {
    pub fn new() -> Self {
        VideoGraph {
            adj: HashMap::new(),
            video_channel: HashMap::new(),
            nodes: HashSet::new(),
        }
    }

    /// Register a node (video) in the graph, even if it has no edges.
    pub fn add_node(&mut self, video_id: &str, channel_id: Option<String>) {
        self.nodes.insert(video_id.to_string());
        self.video_channel.insert(video_id.to_string(), channel_id);
    }

    /// Add an undirected edge (both directions).
    pub fn add_edge(&mut self, a: &str, b: &str, weight: f64) {
        if a == b {
            return;
        }
        self.nodes.insert(a.to_string());
        self.nodes.insert(b.to_string());
        self.adj
            .entry(a.to_string())
            .or_default()
            .push((b.to_string(), weight));
        self.adj
            .entry(b.to_string())
            .or_default()
            .push((a.to_string(), weight));
    }

    /// Get neighbors of a video.
    pub fn neighbors(&self, video_id: &str) -> &[(String, f64)] {
        self.adj.get(video_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Number of videos in the graph (including isolated nodes).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Build the video graph from the corpus. Edges are created from:
/// 1. Shared tags (Jaccard similarity of tag sets)
/// 2. Shared topic categories (Jaccard similarity of topic URL sets)
/// 3. Title token overlap (Jaccard similarity of title tokens, threshold ≥ 0.3)
///
/// Only edges with weight ≥ `min_weight` are kept (sparse graph).
pub fn build_video_graph(videos: &[VideoRow], min_weight: f64) -> VideoGraph {
    let mut graph = VideoGraph::new();

    // Index: tag → video_ids
    let mut tag_index: HashMap<String, Vec<String>> = HashMap::new();
    // Index: topic → video_ids
    let mut topic_index: HashMap<String, Vec<String>> = HashMap::new();
    // Video data for title tokens
    let mut video_titles: HashMap<String, HashSet<String>> = HashMap::new();
    let mut video_tags: HashMap<String, HashSet<String>> = HashMap::new();

    for v in videos {
        graph.add_node(&v.video_id, v.channel_id.clone());

        // Parse tags
        let tags: HashSet<String> = if let Ok(tg) = serde_json::from_str::<Vec<String>>(&v.tags) {
            tg.into_iter().map(|t| t.to_lowercase()).collect()
        } else {
            HashSet::new()
        };
        for t in &tags {
            tag_index
                .entry(t.clone())
                .or_default()
                .push(v.video_id.clone());
        }
        video_tags.insert(v.video_id.clone(), tags);

        // Parse topics
        let topics: HashSet<String> =
            if let Ok(tp) = serde_json::from_str::<Vec<String>>(&v.topic_categories) {
                tp.into_iter().collect()
            } else {
                HashSet::new()
            };
        for t in &topics {
            topic_index
                .entry(t.clone())
                .or_default()
                .push(v.video_id.clone());
        }

        // Title tokens
        let title_tokens: HashSet<String> = util::tokens(&v.title).into_iter().collect();
        video_titles.insert(v.video_id.clone(), title_tokens);
    }

    // Accumulate edge weights from all signals
    let mut edge_weights: HashMap<(String, String), f64> = HashMap::new();

    // Signal 1: Shared tags (weight = count of shared tags)
    for video_ids in tag_index.values() {
        for i in 0..video_ids.len() {
            for j in (i + 1)..video_ids.len() {
                let a = &video_ids[i];
                let b = &video_ids[j];
                let key = if a < b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                };
                *edge_weights.entry(key).or_insert(0.0) += 1.0;
            }
        }
    }

    // Signal 2: Shared topics (weight += 2.0 per shared topic)
    for video_ids in topic_index.values() {
        for i in 0..video_ids.len() {
            for j in (i + 1)..video_ids.len() {
                let a = &video_ids[i];
                let b = &video_ids[j];
                let key = if a < b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                };
                *edge_weights.entry(key).or_insert(0.0) += 2.0;
            }
        }
    }

    // Signal 3: Title token overlap (weight += jaccard * 0.5)
    let video_ids: Vec<String> = videos.iter().map(|v| v.video_id.clone()).collect();
    for i in 0..video_ids.len() {
        for j in (i + 1)..video_ids.len() {
            let a = &video_ids[i];
            let b = &video_ids[j];
            let key = if a < b {
                (a.clone(), b.clone())
            } else {
                (b.clone(), a.clone())
            };
            let titles_a = video_titles.get(a);
            let titles_b = video_titles.get(b);
            if let (Some(ta), Some(tb)) = (titles_a, titles_b) {
                let inter = ta.intersection(tb).count();
                if inter > 0 {
                    let union = ta.len() + tb.len() - inter;
                    let jaccard = inter as f64 / union as f64;
                    *edge_weights.entry(key).or_insert(0.0) += jaccard * 0.5;
                }
            }
        }
    }

    // Normalize tag-based edges by Jaccard and create graph edges
    for ((a, b), raw_weight) in &edge_weights {
        let tags_a = video_tags.get(a);
        let tags_b = video_tags.get(b);
        let tag_jaccard = if let (Some(ta), Some(tb)) = (tags_a, tags_b) {
            let inter = ta.intersection(tb).count();
            let union = ta.len() + tb.len() - inter;
            if union > 0 {
                inter as f64 / union as f64
            } else {
                0.0
            }
        } else {
            0.0
        };
        // Use the max of raw weight and tag jaccard, apply threshold
        let final_weight = raw_weight.max(tag_jaccard);
        if final_weight >= min_weight {
            graph.add_edge(a, b, final_weight);
        }
    }

    graph
}

/// Graph-based similarity: find the top-N most similar videos to a seed video
/// using random walk with restart (personalized PageRank from the seed).
///
/// This is the graph-based retrieval mechanism — it finds videos that are
/// "close" to the seed in the video graph, even if they don't share exact
/// keyword matches (unlike BM25).
pub fn graph_similarity(
    graph: &VideoGraph,
    seed_video: &str,
    top_n: usize,
    restart_prob: f64,
    max_iters: usize,
) -> Vec<(String, f64)> {
    if !graph.adj.contains_key(seed_video) {
        return Vec::new();
    }

    // Random walk with restart
    let mut scores: HashMap<String, f64> = HashMap::new();
    scores.insert(seed_video.to_string(), 1.0);

    for _ in 0..max_iters {
        let mut next: HashMap<String, f64> = HashMap::new();
        for (node, score) in &scores {
            // Restart probability: teleport back to seed
            *next.entry(seed_video.to_string()).or_insert(0.0) += restart_prob * score;

            // Walk to neighbors
            let neighbors = graph.neighbors(node);
            if !neighbors.is_empty() {
                let walk_prob = (1.0 - restart_prob) * score / neighbors.len() as f64;
                for (neighbor, weight) in neighbors {
                    *next.entry(neighbor.clone()).or_insert(0.0) += walk_prob * weight;
                }
            }
        }
        scores = next;
    }

    // Exclude the seed itself, sort by score descending
    let mut results: Vec<(String, f64)> = scores
        .into_iter()
        .filter(|(k, _)| k != seed_video)
        .collect();
    results.sort_by(|a, b| b.1.total_cmp(&a.1));
    results.truncate(top_n);
    results
}

/// Graph neighborhood: find all videos within `hops` hops of the seed.
pub fn graph_neighborhood(
    graph: &VideoGraph,
    seed_video: &str,
    hops: usize,
) -> HashMap<String, usize> {
    let mut visited: HashMap<String, usize> = HashMap::new();
    let mut frontier: Vec<String> = vec![seed_video.to_string()];
    visited.insert(seed_video.to_string(), 0);

    for hop in 0..hops {
        let mut next_frontier: Vec<String> = Vec::new();
        for node in &frontier {
            for (neighbor, _) in graph.neighbors(node) {
                if !visited.contains_key(neighbor) {
                    visited.insert(neighbor.clone(), hop + 1);
                    next_frontier.push(neighbor.clone());
                }
            }
        }
        frontier = next_frontier;
    }

    visited.remove(seed_video);
    visited
}

// ---------------------------------------------------------------------------
// Keyword-channel graph (new): which channels dominate which keywords
// ---------------------------------------------------------------------------

/// A keyword-channel edge: a channel's dominance for a keyword.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KwChannelEdge {
    pub keyword: String,
    pub channel_id: String,
    /// Dominance score: how strongly this channel owns this keyword (0-1).
    pub dominance: f64,
    /// Number of videos the channel has for this keyword.
    pub video_count: usize,
    /// Mean views for the channel's videos on this keyword.
    pub mean_views: f64,
}

/// Build the keyword-channel graph: for each tracked keyword, find which
/// channels have videos matching it (by title tokens) and compute dominance.
pub fn build_kw_channel_graph(videos: &[VideoRow], keywords: &[String]) -> Vec<KwChannelEdge> {
    let mut edges: Vec<KwChannelEdge> = Vec::new();

    for kw in keywords {
        let kw_tokens: HashSet<String> = util::tokens(kw).into_iter().collect();
        if kw_tokens.is_empty() {
            continue;
        }

        // Find all videos matching this keyword
        let mut channel_videos: HashMap<String, Vec<&VideoRow>> = HashMap::new();
        for v in videos {
            let Some(cid) = &v.channel_id else { continue };
            let title_tokens: HashSet<String> = util::tokens(&v.title).into_iter().collect();
            let matches = kw_tokens.iter().all(|t| title_tokens.contains(t));
            if matches {
                channel_videos.entry(cid.clone()).or_default().push(v);
            }
        }

        if channel_videos.is_empty() {
            continue;
        }

        // Compute dominance: share of matching videos + share of views
        let total_videos: usize = channel_videos.values().map(|v| v.len()).sum();
        let total_views: f64 = channel_videos
            .values()
            .flat_map(|v| v.iter().filter_map(|vid| vid.view_count))
            .sum::<i64>() as f64;

        for (cid, vids) in &channel_videos {
            let video_share = vids.len() as f64 / total_videos as f64;
            let views: f64 = vids.iter().filter_map(|v| v.view_count).sum::<i64>() as f64;
            let view_share = if total_views > 0.0 {
                views / total_views
            } else {
                0.0
            };
            let dominance = 0.5 * video_share + 0.5 * view_share;

            edges.push(KwChannelEdge {
                keyword: kw.clone(),
                channel_id: cid.clone(),
                dominance,
                video_count: vids.len(),
                mean_views: if !vids.is_empty() {
                    views / vids.len() as f64
                } else {
                    0.0
                },
            });
        }
    }

    edges
}

// ---------------------------------------------------------------------------
// Tag authority (new): tags weighted by the centrality of channels using them
// ---------------------------------------------------------------------------

/// Compute tag authority scores: a tag is more authoritative when it's used
/// by high-centrality channels (PageRank centrality from the channel graph).
///
/// Returns: tag → authority score (0-100).
pub fn tag_authority_scores(
    videos: &[VideoRow],
    centrality: &HashMap<String, f64>,
) -> HashMap<String, f64> {
    let mut tag_channels: HashMap<String, Vec<(String, f64)>> = HashMap::new();

    for v in videos {
        let Some(cid) = &v.channel_id else { continue };
        let cent = centrality.get(cid).copied().unwrap_or(0.0);
        let tags: Vec<String> = serde_json::from_str::<Vec<String>>(&v.tags).unwrap_or_default();
        for t in tags {
            let t = t.to_lowercase();
            tag_channels.entry(t).or_default().push((cid.clone(), cent));
        }
    }

    let mut scores: HashMap<String, f64> = HashMap::new();
    for (tag, channels) in &tag_channels {
        if channels.is_empty() {
            continue;
        }
        // Authority = mean centrality of channels using this tag × log(channel count)
        let mean_cent: f64 = channels.iter().map(|(_, c)| c).sum::<f64>() / channels.len() as f64;
        let channel_count = channels.len() as f64;
        let authority = mean_cent * channel_count.ln_1p() * 100.0;
        scores.insert(tag.clone(), authority.min(100.0));
    }

    scores
}

// ---------------------------------------------------------------------------
// Topic dominance (new): channel's dominance in topic clusters
// ---------------------------------------------------------------------------

/// Compute topic dominance: for each (channel, topic) pair, how dominant is
/// the channel in that topic cluster?
///
/// Returns: (channel_id, topic) → dominance score (0-100).
pub fn topic_dominance_scores(videos: &[VideoRow]) -> HashMap<(String, String), f64> {
    let mut topic_channels: HashMap<String, Vec<(String, i64)>> = HashMap::new();

    for v in videos {
        let Some(cid) = &v.channel_id else { continue };
        let title_tokens: Vec<String> = util::tokens(&v.title);
        for t in &title_tokens {
            if t.len() < 3 {
                continue;
            }
            topic_channels
                .entry(t.clone())
                .or_default()
                .push((cid.clone(), v.view_count.unwrap_or(0)));
        }
    }

    let mut scores: HashMap<(String, String), f64> = HashMap::new();
    for (topic, channels) in &topic_channels {
        if channels.len() < 2 {
            continue;
        }
        let total_views: i64 = channels.iter().map(|(_, v)| v).sum();
        let total_videos = channels.len() as f64;

        // Count videos per channel for this topic
        let mut channel_video_count: HashMap<String, usize> = HashMap::new();
        let mut channel_views: HashMap<String, i64> = HashMap::new();
        for (cid, views) in channels {
            *channel_video_count.entry(cid.clone()).or_insert(0) += 1;
            *channel_views.entry(cid.clone()).or_insert(0) += views;
        }

        for (cid, &vcount) in &channel_video_count {
            let views = channel_views.get(cid).copied().unwrap_or(0);
            let video_share = vcount as f64 / total_videos;
            let view_share = if total_views > 0 {
                views as f64 / total_views as f64
            } else {
                0.0
            };
            let dominance = (0.5 * video_share + 0.5 * view_share) * 100.0;
            scores.insert((cid.clone(), topic.clone()), dominance.min(100.0));
        }
    }

    scores
}

// ---------------------------------------------------------------------------
// Graph cache: build and cache the video graph + keyword-channel graph
// ---------------------------------------------------------------------------

/// Build all graph layers and cache them. Returns (video_graph, kw_channel_edges).
pub async fn build_all_graphs(
    db: &Db,
    videos: &[VideoRow],
    keywords: &[String],
) -> Result<(VideoGraph, Vec<KwChannelEdge>), TubeforgeError> {
    let video_graph = build_video_graph(videos, 0.1);
    let kw_channel_edges = build_kw_channel_graph(videos, keywords);

    // Cache the graphs
    let vg_json = serde_json::to_string(&video_graph.adj)?;
    db.meta_set(META_VIDEO_GRAPH, &vg_json).await?;

    let kw_json = serde_json::to_string(&kw_channel_edges)?;
    db.meta_set(META_KW_CHANNEL_GRAPH, &kw_json).await?;

    Ok((video_graph, kw_channel_edges))
}

/// Load the cached video graph (None when cache miss).
pub async fn load_video_graph(db: &Db) -> Result<Option<VideoGraph>, TubeforgeError> {
    if let Some(json) = db.meta_get(META_VIDEO_GRAPH).await? {
        if let Ok(adj) = serde_json::from_str::<HashMap<String, Vec<(String, f64)>>>(&json) {
            let mut graph = VideoGraph::new();
            graph.adj = adj;
            return Ok(Some(graph));
        }
    }
    Ok(None)
}

/// Load the cached keyword-channel edges (None when cache miss).
pub async fn load_kw_channel_edges(db: &Db) -> Result<Option<Vec<KwChannelEdge>>, TubeforgeError> {
    if let Some(json) = db.meta_get(META_KW_CHANNEL_GRAPH).await? {
        if let Ok(edges) = serde_json::from_str::<Vec<KwChannelEdge>>(&json) {
            return Ok(Some(edges));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn edge(a: &str, b: &str, w: f64) -> (String, String, f64) {
        (a.to_string(), b.to_string(), w)
    }

    /// Star graph: the center must carry the highest centrality.
    #[test]
    fn pagerank_star_center_dominates() {
        let n = nodes(&["A", "B", "C", "D"]);
        let edges = vec![
            edge("A", "C", 1.0),
            edge("B", "C", 1.0),
            edge("D", "C", 1.0),
        ];
        let pr = pagerank(&n, &edges, DAMPING, ITERATIONS);
        let center = pr["C"];
        for k in ["A", "B", "D"] {
            assert!(center > pr[k], "C ({center}) must dominate {k} ({})", pr[k]);
        }
        let sum: f64 = pr.values().sum();
        assert!((sum - 1.0).abs() < 1e-9, "mass conserved: {sum}");
    }

    /// Directed line A→B→C: influence flows toward C, so C leads.
    #[test]
    fn pagerank_line_flows_to_sink() {
        let n = nodes(&["A", "B", "C"]);
        let edges = vec![edge("A", "B", 1.0), edge("B", "C", 1.0)];
        let pr = pagerank(&n, &edges, DAMPING, ITERATIONS);
        assert!(pr["C"] > pr["B"] && pr["B"] > pr["A"], "got {pr:?}");
    }

    /// Dangling nodes keep the walk total at 1 (mass conservation).
    #[test]
    fn pagerank_dangling_node_conserves_mass() {
        let n = nodes(&["A", "B"]);
        let edges = vec![edge("A", "B", 1.0)]; // B has no out-edges
        let pr = pagerank(&n, &edges, DAMPING, ITERATIONS);
        let sum: f64 = pr.values().sum();
        assert!((sum - 1.0).abs() < 1e-9, "sum {sum}");
    }

    /// Edge weights shift mass toward the stronger link.
    #[test]
    fn pagerank_weights_matter() {
        let n = nodes(&["A", "B", "C"]);
        // A splits 10:1 between B and C → B gets far more.
        let edges = vec![edge("A", "B", 10.0), edge("A", "C", 1.0)];
        let pr = pagerank(&n, &edges, DAMPING, ITERATIONS);
        assert!(pr["B"] > pr["C"], "got {pr:?}");
    }

    #[test]
    fn pagerank_empty_and_singleton() {
        assert!(pagerank(&[], &[], 0.85, 50).is_empty());
        let pr = pagerank(&nodes(&["solo"]), &[], 0.85, 50);
        assert_eq!(pr["solo"], 1.0);
    }

    // --- Video graph tests ---

    fn make_video(
        id: &str,
        channel: &str,
        title: &str,
        tags: &[&str],
        topics: &[&str],
    ) -> VideoRow {
        VideoRow {
            video_id: id.to_string(),
            channel_id: Some(channel.to_string()),
            title: title.to_string(),
            tags: serde_json::to_string(&tags.iter().map(|s| s.to_string()).collect::<Vec<_>>())
                .unwrap(),
            topic_categories: serde_json::to_string(
                &topics.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )
            .unwrap(),
            view_count: Some(1000),
            ..Default::default()
        }
    }

    #[test]
    fn video_graph_connects_shared_tags() {
        let videos = vec![
            make_video("v1", "A", "Rust async guide", &["rust", "async"], &[]),
            make_video("v2", "A", "Rust tokio tutorial", &["rust", "tokio"], &[]),
            make_video("v3", "B", "Python async guide", &["python", "async"], &[]),
        ];
        let graph = build_video_graph(&videos, 0.1);
        // v1 and v2 share "rust" → connected
        assert!(graph.neighbors("v1").iter().any(|(n, _)| n == "v2"));
        // v1 and v3 share "async" → connected
        assert!(graph.neighbors("v1").iter().any(|(n, _)| n == "v3"));
    }

    #[test]
    fn video_graph_topic_edges_stronger() {
        let videos = vec![
            make_video(
                "v1",
                "A",
                "Rust guide",
                &[],
                &["https://en.wikipedia.org/wiki/Rust_(programming_language)"],
            ),
            make_video(
                "v2",
                "B",
                "Rust tutorial",
                &[],
                &["https://en.wikipedia.org/wiki/Rust_(programming_language)"],
            ),
        ];
        let graph = build_video_graph(&videos, 0.1);
        // Both share a topic → connected with higher weight
        let w = graph
            .neighbors("v1")
            .iter()
            .find(|(n, _)| n == "v2")
            .map(|(_, w)| *w)
            .unwrap_or(0.0);
        assert!(w > 0.0, "topic edge should exist with weight > 0");
    }

    #[test]
    fn graph_similarity_finds_connected_videos() {
        let videos = vec![
            make_video("v1", "A", "Rust async guide", &["rust", "async"], &[]),
            make_video("v2", "A", "Rust tokio tutorial", &["rust", "tokio"], &[]),
            make_video("v3", "B", "Rust async patterns", &["rust", "async"], &[]),
            make_video("v4", "C", "Python basics", &["python"], &[]),
        ];
        let graph = build_video_graph(&videos, 0.1);
        let similar = graph_similarity(&graph, "v1", 10, 0.3, 20);
        // v1 should be similar to v2 and v3 (both share tags)
        let ids: Vec<&str> = similar.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"v2"), "v2 should be similar to v1");
        assert!(ids.contains(&"v3"), "v3 should be similar to v1");
        // v4 should NOT be similar (no shared tags)
        assert!(!ids.contains(&"v4"), "v4 should not be similar to v1");
    }

    #[test]
    fn graph_neighborhood_finds_hops() {
        let videos = vec![
            make_video("v1", "A", "Rust async guide", &["rust"], &[]),
            make_video("v2", "A", "Rust tokio", &["rust"], &[]),
            make_video("v3", "B", "Rust futures", &["rust"], &[]),
            make_video("v4", "C", "Python basics", &["python"], &[]),
        ];
        let graph = build_video_graph(&videos, 0.1);
        let neighborhood = graph_neighborhood(&graph, "v1", 2);
        // v2 and v3 are within 1 hop (all share "rust")
        assert!(neighborhood.contains_key("v2"));
        assert!(neighborhood.contains_key("v3"));
        // v4 is disconnected
        assert!(!neighborhood.contains_key("v4"));
    }

    #[test]
    fn tag_authority_scores_higher_for_central_channels() {
        let videos = vec![
            make_video("v1", "central", "Rust guide", &["rust"], &[]),
            make_video("v2", "central", "Rust tutorial", &["rust"], &[]),
            make_video("v3", "peripheral", "Rust basics", &["rust"], &[]),
        ];
        let mut centrality = HashMap::new();
        centrality.insert("central".to_string(), 0.8);
        centrality.insert("peripheral".to_string(), 0.2);

        let scores = tag_authority_scores(&videos, &centrality);
        // "rust" tag should have high authority (used by central channel)
        let rust_auth = scores.get("rust").copied().unwrap_or(0.0);
        assert!(rust_auth > 0.0, "rust tag should have positive authority");
    }

    #[test]
    fn topic_dominance_scores_channel_dominance() {
        let videos = vec![
            make_video("v1", "A", "Rust async guide", &[], &[]),
            make_video("v2", "A", "Rust tokio tutorial", &[], &[]),
            make_video("v3", "B", "Rust basics", &[], &[]),
        ];
        let scores = topic_dominance_scores(&videos);
        // Channel A has 2/3 videos on "rust" → higher dominance
        let a_dom = scores
            .get(&("A".to_string(), "rust".to_string()))
            .copied()
            .unwrap_or(0.0);
        let b_dom = scores
            .get(&("B".to_string(), "rust".to_string()))
            .copied()
            .unwrap_or(0.0);
        assert!(
            a_dom > b_dom,
            "Channel A should dominate topic 'rust' over B"
        );
    }

    #[test]
    fn kw_channel_graph_finds_dominant_channels() {
        let videos = vec![
            make_video("v1", "A", "Rust async guide", &[], &[]),
            make_video("v2", "A", "Rust tokio tutorial", &[], &[]),
            make_video("v3", "B", "Rust basics", &[], &[]),
        ];
        let keywords = vec!["rust".to_string()];
        let edges = build_kw_channel_graph(&videos, &keywords);
        assert!(!edges.is_empty(), "should find keyword-channel edges");
        // Channel A has more videos → higher dominance
        let a_edge = edges.iter().find(|e| e.channel_id == "A").unwrap();
        let b_edge = edges.iter().find(|e| e.channel_id == "B").unwrap();
        assert!(
            a_edge.dominance > b_edge.dominance,
            "Channel A should have higher dominance"
        );
    }

    #[test]
    fn video_graph_empty_corpus() {
        let videos: Vec<VideoRow> = vec![];
        let graph = build_video_graph(&videos, 0.1);
        assert!(graph.is_empty());
        let similar = graph_similarity(&graph, "v1", 10, 0.3, 20);
        assert!(similar.is_empty());
    }

    #[test]
    fn video_graph_single_video() {
        let videos = vec![make_video("v1", "A", "Rust guide", &["rust"], &[])];
        let graph = build_video_graph(&videos, 0.1);
        assert_eq!(graph.len(), 1);
        // No neighbors for a single video
        assert!(graph.neighbors("v1").is_empty());
    }
}
