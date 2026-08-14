//! Next Ideas (LLD §8.2): candidates from high-scoring competitor titles
//! (top BM25 neighborhoods) plus tracked keywords; rank =
//! `0.5·seo_total + 0.3·idea_fit + 0.2·competitor_gap`; persisted into
//! `ideas` with a rationale JSON; status marking (draft|saved|discarded).

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::analytics::graph;
use crate::error::TubeforgeError;
use crate::scoring::weights::Weights;
use crate::search::bm25::Bm25;
use crate::search::FIELD_TITLE;
use crate::storage::db::{Db, VideoRow};
use crate::util;

/// Candidate rank weights (LLD §8.2).
pub const W_SEO: f64 = 0.5;
pub const W_FIT: f64 = 0.3;
pub const W_GAP: f64 = 0.2;

/// BM25 neighborhood size per keyword (top-N competitor titles).
pub const NEIGHBORHOOD: usize = 5;

/// One generated idea, already persisted (status draft unless re-marked).
#[derive(Debug, Clone)]
pub struct IdeaCandidate {
    pub idea_id: i64,
    pub title_suggestion: String,
    pub source_video: Option<String>,
    pub score: f64,
    pub status: String,
    pub rationale: Value,
}

/// A computed idea from runtime analysis — NOT persisted to DB.
#[derive(Debug, Clone)]
pub struct ComputedIdea {
    pub title_suggestion: String,
    pub source_video: Option<String>,
    pub score: f64,
    pub rationale: Value,
}

/// Generate + persist the idea pool (LLD §8.2). `top_n` bounds the output
/// pool; `niche` feeds the idea-fit similarity. Returns the pool sorted by
/// rank descending.
pub async fn generate(
    db: &Db,
    bm25: &Bm25,
    videos: &[VideoRow],
    weights: &Weights,
    niche: Option<&str>,
    top_n: usize,
) -> Result<Vec<IdeaCandidate>, TubeforgeError> {
    let computed = analyze(db, bm25, videos, weights, niche, top_n).await?;

    let mut out: Vec<IdeaCandidate> = Vec::new();
    for c in computed {
        let id = db
            .upsert_idea(
                &c.title_suggestion,
                &c.rationale.to_string(),
                round2(c.score),
                "draft",
                c.source_video.as_deref(),
            )
            .await?;
        out.push(IdeaCandidate {
            idea_id: id,
            title_suggestion: c.title_suggestion,
            source_video: c.source_video,
            score: c.score,
            status: "draft".to_string(),
            rationale: c.rationale,
        });
    }
    Ok(out)
}

/// Compute idea candidates at runtime WITHOUT persisting to DB. Same scoring
/// logic as `generate()` — returns fresh analysis from current corpus state.
pub async fn analyze(
    db: &Db,
    bm25: &Bm25,
    videos: &[VideoRow],
    weights: &Weights,
    niche: Option<&str>,
    top_n: usize,
) -> Result<Vec<ComputedIdea>, TubeforgeError> {
    let keywords: Vec<String> = db
        .list_keywords()
        .await?
        .into_iter()
        .map(|k| k.keyword)
        .collect();
    let centrality = graph::build(db, videos).await?;
    let videos_by_id: HashMap<&str, &VideoRow> =
        videos.iter().map(|v| (v.video_id.as_str(), v)).collect();

    // Candidate videos: top BM25 title matches per tracked keyword + the
    // highest-scoring stored videos (LLD §8.2: "high-scoring competitor
    // titles (top BM25 neighborhoods) + keywords").
    let mut candidates: Vec<(String, String)> = Vec::new(); // (video_id, keyword)
    let mut seen: HashSet<String> = HashSet::new();
    for kw in &keywords {
        for (vid, _) in bm25.matches(FIELD_TITLE, kw).into_iter().take(NEIGHBORHOOD) {
            if seen.insert(vid.clone()) {
                candidates.push((vid, kw.clone()));
            }
        }
    }
    let mut all_scores = db.all_scores().await?;
    all_scores.truncate(10);
    for s in all_scores {
        if seen.insert(s.video_id.clone()) {
            let kw = best_keyword(&s.video_id, videos, &keywords);
            candidates.push((s.video_id, kw));
        }
    }

    let scores: HashMap<String, f64> = db
        .all_scores()
        .await?
        .into_iter()
        .map(|s| (s.video_id, s.seo_score))
        .collect();

    let fit_tokens: HashSet<String> = keywords
        .iter()
        .flat_map(|k| util::tokens(k))
        .chain(niche.into_iter().flat_map(util::tokens))
        .collect();

    let mut out: Vec<ComputedIdea> = Vec::new();
    for (video_id, kw) in candidates {
        let Some(video) = videos_by_id.get(video_id.as_str()) else {
            continue;
        };
        let tags: Vec<String> = serde_json::from_str(&video.tags).unwrap_or_default();

        let seo_total = scores.get(&video_id).copied().unwrap_or_else(|| {
            crate::scoring::compute(
                &video.title,
                &video.description,
                &tags,
                std::slice::from_ref(&kw),
                bm25,
                weights,
                Some(&video_id),
            )
            .seo_total
        });

        // idea_fit: title-token overlap with the user's niche/keywords;
        // neutral 50 when nothing to fit against.
        let idea_fit = if fit_tokens.is_empty() {
            50.0
        } else {
            let title_tokens: HashSet<String> = util::tokens(&video.title).into_iter().collect();
            let inter = title_tokens.intersection(&fit_tokens).count();
            let union = title_tokens.len() + fit_tokens.len() - inter;
            if union == 0 {
                0.0
            } else {
                inter as f64 / union as f64 * 100.0
            }
        };

        // competitor_gap: low-centrality channel x high-demand keyword.
        let demand_matches = bm25.matches(FIELD_TITLE, &kw).len();
        let demand = (demand_matches as f64 / 20.0).min(1.0);
        let centrality = video
            .channel_id
            .as_ref()
            .and_then(|c| centrality.get(c))
            .copied()
            .unwrap_or(0.0);
        let competitor_gap = (1.0 - centrality) * 100.0 * demand;

        // Phase 6.6 comments-weight: engagement lift on the gap term (0-25% boost).
        let engagement_boost = crate::analytics::performance::engagement_ratio(
            video.view_count,
            video.like_count,
            video.comment_count,
        )
        .map(|r| ((r * 100.0) / 4.0).clamp(0.0, 1.0) * 0.25)
        .unwrap_or(0.0);
        let competitor_gap = competitor_gap * (1.0 + engagement_boost);

        let score = W_SEO * seo_total + W_FIT * idea_fit + W_GAP * competitor_gap;

        let rationale = json!({
            "seo_total": round2(seo_total),
            "idea_fit": round2(idea_fit),
            "competitor_gap": round2(competitor_gap),
            "engagement_boost": round4(engagement_boost),
            "centrality": round4(centrality),
            "demand_matches": demand_matches,
            "keyword": kw,
            "source_channel": video.channel_id,
        });

        out.push(ComputedIdea {
            title_suggestion: video.title.clone(),
            source_video: Some(video_id),
            score: round2(score),
            rationale,
        });
    }

    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out.truncate(top_n);
    Ok(out)
}

/// The tracked keyword whose tokens best match the video title ("" when
/// none — scores-only candidates).
fn best_keyword(video_id: &str, videos: &[VideoRow], keywords: &[String]) -> String {
    let Some(v) = videos.iter().find(|v| v.video_id == video_id) else {
        return String::new();
    };
    let title: HashSet<String> = util::tokens(&v.title).into_iter().collect();
    if title.is_empty() {
        return String::new();
    }
    let mut best = String::new();
    let mut best_hits = 0usize;
    for kw in keywords {
        let kw_tokens: HashSet<String> = util::tokens(kw).into_iter().collect();
        let hits = kw_tokens.intersection(&title).count();
        if hits > best_hits {
            best_hits = hits;
            best = kw.clone();
        }
    }
    best
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}
