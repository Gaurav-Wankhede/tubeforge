//! Competitor graph (LLD §8.1): adjacency from the `edges` table — manual
//! edges plus auto-suggested competitor-overlap edges (co-occurring tokens in
//! titles/tags, weight = Jaccard overlap strength) — then weighted PageRank
//! (damped 0.85, 50 iterations), centrality per channel cached in `meta`.

use std::collections::{HashMap, HashSet};

use crate::error::TubeforgeError;
use crate::storage::db::{VideoRow, Db};
use crate::util;

/// PageRank damping factor (LLD §8.1).
pub const DAMPING: f64 = 0.85;
/// PageRank iterations (LLD §8.1 — converges trivially at this scale).
pub const ITERATIONS: usize = 50;

/// meta cache keys for the computed graph (LLD §8.1 "persisted in meta").
const META_PR: &str = "graph_pagerank_json";
const META_HASH: &str = "graph_videos_hash";
const META_AT: &str = "graph_cache_at";

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
        let dangling: f64 = (0..n)
            .filter(|&i| out_sum[i] == 0.0)
            .map(|i| pr[i])
            .sum();
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

    nodes
        .iter()
        .zip(pr)
        .map(|(s, p)| (s.clone(), p))
        .collect()
}

/// Recompute the auto-suggested overlap edges (LLD §8.1): every channel pair
/// sharing ≥1 title/tag token gets a symmetric `overlap` edge whose weight is
/// the token Jaccard. Stale overlap edges are dropped first; `manual` edges
/// are untouched. Returns the number of overlap edges written.
pub async fn sync_overlap_edges(
    db: &Db,
    videos: &[VideoRow],
) -> Result<usize, TubeforgeError> {
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
            by_channel
                .entry(cid.clone())
                .or_default()
                .extend(tokens);
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
            let weight = if union == 0 { 1.0 } else { inter as f64 / union as f64 };
            fresh.push((channels[i].clone(), channels[j].clone(), weight));
        }
    }

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
}
