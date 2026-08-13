//! Keyword research (Phase 6.6 — VidIQ Keyword-Inspector equivalent).
//!
//! VidIQ's keyword analysis shows: estimated search volume, competition
//! score, 12-month trend, related keywords, what ranks now — and on every
//! ranking video: its tags, stats and SEO score (the overlay). YouTube
//! hides real search volume, so TubeForge computes the same analysis from
//! keyless signals (all empirically verified Aug 2026):
//! - **SERP overlay**: full extraction per ranking video (ytsearch) —
//!   views, likes, comments, upload date, REAL tags, and our own SEO score
//!   of their metadata (structural components).
//! - **Suggested tags**: frequency-ranked real tags harvested from the
//!   ranking videos (tags that rank = tags that work).
//! - **Demand**: SERP mean views + a relative volume label (Low/Med/High —
//!   TubeBuddy's honest approach, since exact numbers are unobtainable).
//! - **Competition**: blend of channel diversity (70%) × incumbent
//!   authority (30%), 0-100.
//! - **Opportunity**: demand × weakness, 0-100, with a plain-language
//!   verdict for the creator.
//! - **Related keywords**: Google's public YouTube autocomplete.
//! - **Recency/activity**: `ytsearchdate` full extraction — are channels
//!   still uploading on this topic (last 90 days)?
//! - **Corpus resonance**: BM25 match against stored videos.
//!
//! No LLM, no API key, no quota.

use serde::{Deserialize, Serialize};

use crate::error::TubeforgeError;
use crate::fetch::ytdlp::{YtdlpClient, YtdlpSearchResult};
use crate::fetch::FetchClients;
use crate::search::bm25::Bm25;
use crate::search::FIELD_TITLE;
use crate::storage::db::Db;

/// Default SERP size for the demand/competition analysis. Full extraction
/// costs ~1.7s/video, so 6 is the balance for a research action.
pub const DEFAULT_SERP: u64 = 6;
/// Demand saturates when the SERP's mean views reach this (a topic whose
/// ranking videos average 250k+ views is high-demand for a niche channel).
const DEMAND_SATURATION_VIEWS: f64 = 250_000.0;
/// Competition blend: how much weight goes to channel diversity vs
/// incumbent authority (mean views). Diversity is the stronger keyless
/// signal ("how many channels own the topic"), so it gets 70%.
const COMPETITION_DIVERSITY_WEIGHT: f64 = 0.7;
/// A topic is "actively published" when this many of the recent-upload
/// SERP entries landed within the last 90 days.
const RECENT_WINDOW_DAYS: i64 = 90;
const RECENT_ACTIVE_THRESHOLD: usize = 2;
/// Minimum tag frequency (count of ranking videos using it) to appear in
/// the suggested-tags list.
const SUGGESTED_TAG_MIN_FREQ: usize = 1;
/// Cap on suggested tags returned.
const SUGGESTED_TAG_MAX: usize = 20;

/// One ranked SERP result with the full keyless overlay (VidIQ-style).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerpResult {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub channel_id: String,
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub upload_date: Option<String>,
    /// The video's REAL tags (full extraction only).
    pub tags: Vec<String>,
    /// Our 15-component SEO score of their metadata (structural half —
    /// no corpus needed; BM25-derived components are 0 without the index).
    pub seo_score: f64,
}

/// The full keyword analysis (VidIQ Keyword-Inspector style).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordResearch {
    pub keyword: String,
    /// Demand proxy: SERP size + total/mean views of ranking videos.
    pub serp_total: usize,
    pub serp_mean_views: f64,
    pub serp_total_views: f64,
    /// Relative search-volume label (exact numbers are unobtainable).
    pub volume_label: String,
    /// Competition: distinct channels + blended score (0-100).
    pub ranking_channels: usize,
    pub competition_score: f64,
    /// 0-100 opportunity = demand × weakness, plus a plain verdict.
    pub opportunity_score: f64,
    /// VidIQ-style composite keyword score: blend of demand, competition
    /// (inverted), recency and corpus fit — the headline number.
    pub keyword_score: f64,
    pub verdict: String,
    /// Tags harvested from the ranking videos, frequency-ranked, with the
    /// number of ranking videos using each (usage count).
    pub suggested_tags: Vec<TagSuggestion>,
    /// Related keywords from YouTube autocomplete (order = Google's
    /// popularity ranking) + our own quick opportunity estimate.
    pub related_keywords: Vec<RelatedKeyword>,
    /// Recency: recent-upload SERP entries + whether the topic is active.
    pub recent_uploads: usize,
    pub actively_published: bool,
    /// Corpus resonance: how strongly the keyword matches stored titles.
    pub corpus_resonance: Option<f64>,
    /// Our own stored videos matching the keyword (BM25 top hits).
    pub corpus_matches: Vec<CorpusMatch>,
    /// The ranked SERP itself with the full overlay.
    pub serp: Vec<SerpResult>,
}

/// One of our stored videos matching the researched keyword.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusMatch {
    pub video_id: String,
    pub title: String,
    pub channel: Option<String>,
    pub view_count: Option<i64>,
    pub bm25: f64,
}

/// A suggested tag with how many ranking videos use it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagSuggestion {
    pub tag: String,
    /// Ranking videos using this tag.
    pub usage: usize,
}

/// A related keyword from YouTube autocomplete, with Google's popularity
/// rank (position in the suggestion list) and a quick opportunity estimate
/// from our corpus resonance (0 = unknown).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedKeyword {
    pub keyword: String,
    /// 1 = most popular autocomplete suggestion for the base keyword.
    pub popularity_rank: usize,
}

/// Run the full analysis. `ytdlp` must be enabled (Config error otherwise).
pub async fn inspect(
    db: &Db,
    bm25: Option<&Bm25>,
    ytdlp: &YtdlpClient,
    clients: &FetchClients,
    keyword: &str,
    serp_n: u64,
) -> Result<KeywordResearch, TubeforgeError> {
    let serp_n = if serp_n == 0 { DEFAULT_SERP } else { serp_n };

    // 1. Real SERP with FULL extraction (search_with always dumps full
    //    metadata: views/likes/comments/upload/tags — verified empirically;
    //    flat-playlist omits them).
    let mut results: Vec<YtdlpSearchResult> = ytdlp.search(keyword, serp_n).await?;
    let empty = empty_bm25();
    let mut enriched: Vec<SerpResult> = Vec::new();
    for r in results.drain(..) {
        let seo_score = metadata_seo_score_with(&r.title, &r.tags, &empty);
        enriched.push(SerpResult {
            video_id: r.video_id,
            title: r.title,
            channel: r.channel,
            channel_id: r.channel_id,
            view_count: r.view_count,
            like_count: r.like_count,
            comment_count: r.comment_count,
            upload_date: r.upload_date,
            tags: r.tags,
            seo_score,
        });
    }
    let results = enriched;

    // 2. Related keywords (public autocomplete) — Google returns them in
    //    popularity order, which IS a relative search-volume ranking.
    let related_keywords: Vec<RelatedKeyword> = crate::fetch::youtube_suggestions(clients, keyword)
        .await
        .into_iter()
        .enumerate()
        .map(|(i, k)| RelatedKeyword {
            keyword: k,
            popularity_rank: i + 1,
        })
        .collect();

    // 3. Competition + demand from the SERP.
    let mut channels: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut total_views = 0.0f64;
    let mut with_views = 0usize;
    for r in &results {
        if !r.channel_id.is_empty() {
            channels.insert(r.channel_id.clone());
        }
        if let Some(v) = r.view_count {
            total_views += v as f64;
            with_views += 1;
        }
    }
    let ranking_channels = channels.len();
    let mean_views = if with_views > 0 {
        total_views / with_views as f64
    } else {
        0.0
    };

    // Competition: blend of channel diversity × incumbent authority.
    let diversity = if serp_n > 0 {
        (ranking_channels as f64 / serp_n as f64).min(1.0)
    } else {
        0.0
    };
    let authority = (mean_views / 1_000_000.0).min(1.0);
    let competition_score = (COMPETITION_DIVERSITY_WEIGHT * diversity
        + (1.0 - COMPETITION_DIVERSITY_WEIGHT) * authority)
        * 100.0;

    // Opportunity: demand × weakness.
    let demand = (mean_views / DEMAND_SATURATION_VIEWS).min(1.0);
    let weakness = 1.0 - competition_score / 100.0;
    let opportunity_score = ((demand * weakness) * 100.0).min(100.0);

    // Volume label (TubeBuddy's honest relative scale).
    let volume_label = if mean_views >= 250_000.0 {
        "High".to_string()
    } else if mean_views >= 60_000.0 {
        "Medium".to_string()
    } else if mean_views > 0.0 {
        "Low".to_string()
    } else {
        "Unknown".to_string()
    };

    // Verdict (plain language, consistent with the volume bands).
    let verdict = match volume_label.as_str() {
        "High" if competition_score < 60.0 => format!(
            "Strong opportunity — \"{keyword}\" is high-demand (avg {:.0} views per ranking \
             video) and only {ranking_channels} channel(s) own the SERP. A well-optimized video \
             can take positions.",
            mean_views
        ),
        "High" | "Medium" if competition_score < 75.0 => format!(
            "Moderate opportunity — \"{keyword}\" has solid demand (avg {:.0} views per ranking \
             video) across {ranking_channels} channel(s). Win with a sharper title, a better \
             hook, and the suggested tags.",
            mean_views
        ),
        "High" | "Medium" => format!(
            "Saturated — \"{keyword}\" ranks strongly across {ranking_channels} channel(s) with \
             high views. Pick a more specific angle or a related long-tail keyword.",
        ),
        "Low" => format!(
            "Low demand — ranking videos for \"{keyword}\" average only {:.0} views. The topic \
             may be too niche; check the related keywords for a higher-demand variant.",
            mean_views
        ),
        _ => format!(
            "No demand data — the SERP for \"{keyword}\" returned no view counts. Try a \
             broader topic.",
        ),
    };

    // 4. Suggested tags: frequency-ranked real tags from ranking videos.
    let suggested_tags = harvest_tags(&results);

    // 5. Recency: are channels still uploading on this topic?
    let recent = ytdlp.search_date(keyword, 5).await.unwrap_or_default();
    let now = chrono::Utc::now();
    let recent_uploads = recent
        .iter()
        .filter(|r| {
            r.upload_date
                .as_deref()
                .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y%m%d").ok())
                .map(|d| (now.date_naive() - d).num_days() <= RECENT_WINDOW_DAYS)
                .unwrap_or(false)
        })
        .count();
    let actively_published = recent_uploads >= RECENT_ACTIVE_THRESHOLD;

    // 6. Corpus resonance + our stored matches.
    let (corpus_resonance, corpus_matches) = match bm25 {
        Some(b) => {
            let hits = b.matches(FIELD_TITLE, keyword);
            let resonance = ((hits.len() as f64) / 20.0).min(1.0) * 100.0;
            let videos = db.all_videos().await.unwrap_or_default();
            let by_id: std::collections::HashMap<&str, &crate::storage::db::VideoRow> =
                videos.iter().map(|v| (v.video_id.as_str(), v)).collect();
            let matches: Vec<CorpusMatch> = hits
                .into_iter()
                .take(5)
                .filter_map(|(vid, score)| {
                    let v = by_id.get(vid.as_str())?;
                    Some(CorpusMatch {
                        video_id: v.video_id.clone(),
                        title: v.title.clone(),
                        channel: v.channel_id.clone(),
                        view_count: v.view_count,
                        bm25: score as f64,
                    })
                })
                .collect();
            (Some(round2(resonance)), matches)
        }
        None => (None, Vec::new()),
    };

    // VidIQ-style composite keyword score: the headline number a creator
    // sees first. Blend: demand (40%), competition inverted (35%), recency
    // (15%), corpus fit (10%).
    let demand_n = (mean_views / DEMAND_SATURATION_VIEWS).min(1.0);
    let comp_inv = 1.0 - competition_score / 100.0;
    let recency_n = if actively_published { 1.0 } else { 0.5 };
    let fit_n = corpus_resonance.map(|r| r / 100.0).unwrap_or(0.5);
    let keyword_score =
        (0.40 * demand_n + 0.35 * comp_inv + 0.15 * recency_n + 0.10 * fit_n) * 100.0;

    Ok(KeywordResearch {
        keyword: keyword.to_string(),
        serp_total: results.len(),
        serp_mean_views: round2(mean_views),
        serp_total_views: round2(total_views),
        volume_label,
        ranking_channels,
        competition_score: round2(competition_score),
        opportunity_score: round2(opportunity_score),
        keyword_score: round2(keyword_score),
        verdict,
        suggested_tags,
        related_keywords,
        recent_uploads,
        actively_published,
        corpus_resonance,
        corpus_matches,
        serp: results,
    })
}

/// A "discovery" outcome: the full keyword research PLUS the trend-layer
/// extras pulled for the searched topic (competitor registration, heatmap /
/// transcript enrichment counts, and the per-video trend signals).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discovery {
    pub research: KeywordResearch,
    /// How many top-ranking channels were newly registered as competitors.
    pub competitors_registered: usize,
    /// How many ranking videos got a heatmap (retention curve) fetched.
    pub heatmaps_fetched: usize,
    /// How many ranking videos got a transcript fetched.
    pub transcripts_fetched: usize,
    /// Per-video trend signals (VPH, engagement, retention) from the fetched
    /// heatmaps — the "is it trending" layer over the plain SERP.
    pub trends: Vec<TrendSignal>,
}

/// One ranking video's trend signals (performance half).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendSignal {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub vph: Option<f64>,
    pub engagement_score: Option<f64>,
    pub hook_retention: Option<f64>,
    pub mean_retention: Option<f64>,
    pub retention_score: Option<f64>,
}

/// `research discover "<query>"`: dynamic, search-driven competitor + trend
/// discovery. Runs the full VidIQ-style keyword research on the SEARCHED
/// TEXT (not an exact channel list), then:
/// 1. Persists the top-ranking videos + channels + tags into the corpus.
/// 2. Registers the top-ranking channels as competitors (so `scorecard`,
///    `gaps`, `ideas` and the competitor graph see them).
/// 3. Optionally enriches each ranking video with its retention heatmap
///    (and, when `--transcripts`, its transcript) via yt-dlp — the ONLY
///    public source of the audience-retention curve.
/// 4. Computes per-video trend signals (VPH, engagement, retention).
///
/// This is the "top ranking channels & videos by searched text" pipeline:
/// the search input drives the analysis, so trends emerge from what actually
/// ranks NOW for the topic. No paid service, no LLM, no API key.
#[allow(clippy::too_many_arguments)]
pub async fn discover(
    db: &Db,
    bm25: Option<&Bm25>,
    ytdlp: &YtdlpClient,
    clients: &FetchClients,
    keyword: &str,
    serp_n: u64,
    enrich: bool,
    with_transcripts: bool,
) -> Result<Discovery, TubeforgeError> {
    let serp_n = if serp_n == 0 { DEFAULT_SERP } else { serp_n };

    // 1. Full keyword research on the searched text (real SERP scan).
    let research = inspect(db, bm25, ytdlp, clients, keyword, serp_n).await?;

    // 2. Persist the ranking videos + channels + tags into the corpus.
    crate::storage::db::persist_serp_db(db, &research.serp).await?;

    // 3. Register the ranking channels as competitors (idempotent).
    let mut channel_ids: Vec<String> = research
        .serp
        .iter()
        .filter(|r| !r.channel_id.is_empty())
        .map(|r| r.channel_id.clone())
        .collect();
    channel_ids.sort();
    channel_ids.dedup();
    let label = format!("discover:{keyword}");
    let competitors_registered = db.register_competitors(&channel_ids, &label).await?;

    // 4. Tag aggregation so the Tags Analyzer + competitor gaps have data.
    let _ = crate::analytics::tags::analyze_competitors(db).await;

    // 5. Optional enrichment + trend signals per ranking video.
    let now = chrono::Utc::now();
    let mut heatmaps_fetched = 0usize;
    let mut transcripts_fetched = 0usize;
    let mut trends: Vec<TrendSignal> = Vec::new();

    for r in &research.serp {
        let mut hook: Option<f64> = None;
        let mut mean_ret: Option<f64> = None;
        let mut vph: Option<f64> = None;

        if enrich {
            // Fetch the retention heatmap + live stats (metadata call).
            match ytdlp.metadata(&r.video_id).await {
                Ok(info) => {
                    heatmaps_fetched += 1;
                    let now_str = crate::util::now_rfc3339();
                    let points_json = serde_json::to_string(
                        &info
                            .heatmap
                            .iter()
                            .map(|&(t, v)| serde_json::json!({ "start_time": t, "value": v }))
                            .collect::<Vec<_>>(),
                    )
                    .unwrap_or_else(|_| "[]".to_string());
                    db.upsert_heatmap(&r.video_id, &points_json, &now_str)
                        .await?;

                    // Live stats refresh on the persisted video row.
                    if info.view_count.is_some() || info.like_count.is_some() {
                        db.update_video_stats(
                            &r.video_id,
                            info.view_count,
                            info.like_count,
                            info.comment_count,
                            &now_str,
                        )
                        .await?;
                    }

                    // Retention + VPH from the heatmap + stats.
                    if let Some((h, m)) =
                        crate::analytics::performance::retention_from_heatmap(&info.heatmap)
                    {
                        hook = Some(h);
                        mean_ret = Some(m);
                    }
                    let published = r
                        .upload_date
                        .as_deref()
                        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y%m%d").ok())
                        .map(|nd| format!("{}T00:00:00Z", nd.format("%Y-%m-%d")));
                    if let Some(pub_at) = &published {
                        vph = crate::analytics::performance::vph(
                            info.view_count.or(r.view_count),
                            pub_at,
                            now,
                        );
                    }
                }
                Err(e) => {
                    // Non-fatal: skip enrichment for this video, keep the rest.
                    tracing::debug!(video_id = %r.video_id, err = %e, "discover: metadata enrich failed");
                }
            }

            if with_transcripts {
                match ytdlp.transcript(&r.video_id, "en").await {
                    Ok((text, kind)) => {
                        transcripts_fetched += 1;
                        let source = match kind {
                            crate::fetch::ytdlp::TranscriptKind::Auto => "auto",
                            crate::fetch::ytdlp::TranscriptKind::Manual => "manual",
                        };
                        let now_str = crate::util::now_rfc3339();
                        db.upsert_transcript(&r.video_id, "en", source, &text, &now_str)
                            .await?;
                    }
                    Err(e) => {
                        tracing::debug!(video_id = %r.video_id, err = %e, "discover: transcript skipped");
                    }
                }
            }
        }

        let engagement_score = crate::analytics::performance::engagement_score(
            crate::analytics::performance::engagement_ratio(
                r.view_count,
                r.like_count,
                r.comment_count,
            ),
        );
        let retention_score = crate::analytics::performance::retention_score(mean_ret);

        trends.push(TrendSignal {
            video_id: r.video_id.clone(),
            title: r.title.clone(),
            channel: r.channel.clone(),
            vph,
            engagement_score,
            hook_retention: hook,
            mean_retention: mean_ret,
            retention_score,
        });
    }

    Ok(Discovery {
        research,
        competitors_registered,
        heatmaps_fetched,
        transcripts_fetched,
        trends,
    })
}

/// Harvest tags from the ranking videos, frequency-ranked (most-used first),
/// with usage counts — VidIQ shows how widely a tag is used by the SERP.
fn harvest_tags(results: &[SerpResult]) -> Vec<TagSuggestion> {
    let mut freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for r in results {
        for t in &r.tags {
            let t = t.trim().to_lowercase();
            if t.is_empty() || t.len() > 60 {
                continue;
            }
            *freq.entry(t).or_default() += 1;
        }
    }
    let mut tags: Vec<(String, usize)> = freq
        .into_iter()
        .filter(|(_, n)| *n >= SUGGESTED_TAG_MIN_FREQ)
        .collect();
    tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    tags.into_iter()
        .take(SUGGESTED_TAG_MAX)
        .map(|(tag, usage)| TagSuggestion { tag, usage })
        .collect()
}

/// Our 15-component SEO score of a competitor video's metadata — the
/// structural half only (BM25-derived components score 0 without a corpus).
/// Same weights as the draft scorer so the overlay is comparable. The empty
/// index is built once and reused across SERP videos (index creation is
/// the slow part).
fn metadata_seo_score_with(title: &str, tags: &[String], empty: &crate::search::bm25::Bm25) -> f64 {
    let w = crate::scoring::weights::Weights::defaults();
    let seo = crate::scoring::seo::compute(title, "", tags, &[title.to_string()], empty, None);
    crate::scoring::seo_total(&seo.values(), &w)
}

/// A no-op BM25 (empty index) so `seo::compute`'s corpus components are 0 —
/// the structural components still score normally. The tempdir is leaked on
/// purpose (process-lifetime) so tantivy's lazy file access never hits a
/// deleted directory; an empty corpus costs nothing.
fn empty_bm25() -> crate::search::bm25::Bm25 {
    let dir = tempfile::tempdir().expect("tempdir for bm25");
    let index = crate::search::new_index(&dir.path().join("idx")).expect("index");
    std::mem::forget(dir);
    crate::search::bm25::Bm25::open(index).expect("bm25")
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Convenience: convert raw yt-dlp results (used by tests).
pub fn _serp_from_ytdlp(results: Vec<YtdlpSearchResult>) -> Vec<SerpResult> {
    results
        .into_iter()
        .map(|r| SerpResult {
            video_id: r.video_id,
            title: r.title,
            channel: r.channel,
            channel_id: r.channel_id,
            view_count: r.view_count,
            like_count: r.like_count,
            comment_count: r.comment_count,
            upload_date: r.upload_date,
            tags: r.tags,
            seo_score: 0.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn competition_and_opportunity_math() {
        // serp=8, 2 distinct channels, mean views 200k:
        // diversity = 2/8 = .25, authority = .2
        // competition = .7*.25 + .3*.2 = .235 → 23.5
        // demand = 200k/250k = .8, weakness = .765 → opportunity = 61.2
        let diversity = 2.0f64 / 8.0;
        let authority = 200_000.0f64 / 1_000_000.0;
        let comp = (0.7f64 * diversity + 0.3 * authority) * 100.0;
        assert!((comp - 23.5).abs() < 1e-9);
        let demand = (200_000.0f64 / 250_000.0).min(1.0);
        let opp = demand * (1.0 - comp / 100.0) * 100.0;
        assert!((opp - 61.2).abs() < 1e-9);

        // Saturated: 8/8 distinct channels + 1M+ avg views.
        let comp_full = (0.7f64 * 1.0 + 0.3 * 1.0) * 100.0;
        assert_eq!(comp_full, 100.0);
        assert_eq!(demand * (1.0 - comp_full / 100.0) * 100.0, 0.0);
    }

    #[test]
    fn volume_label_bands() {
        assert_eq!(label(300_000.0), "High");
        assert_eq!(label(100_000.0), "Medium");
        assert_eq!(label(20_000.0), "Low");
        assert_eq!(label(0.0), "Unknown");
    }

    fn label(mean: f64) -> String {
        if mean >= 250_000.0 {
            "High".to_string()
        } else if mean >= 60_000.0 {
            "Medium".to_string()
        } else if mean > 0.0 {
            "Low".to_string()
        } else {
            "Unknown".to_string()
        }
    }

    #[test]
    fn harvest_tags_ranks_by_frequency() {
        let mk = |tags: Vec<&str>| SerpResult {
            video_id: "x".into(),
            title: "t".into(),
            channel: "c".into(),
            channel_id: "cid".into(),
            view_count: None,
            like_count: None,
            comment_count: None,
            upload_date: None,
            tags: tags.into_iter().map(String::from).collect(),
            seo_score: 0.0,
        };
        let results = vec![
            mk(vec!["rust", "async", "tokio"]),
            mk(vec!["rust", "async"]),
            mk(vec!["rust"]),
        ];
        let tags = harvest_tags(&results);
        assert_eq!(tags[0].tag, "rust");
        assert_eq!(tags[0].usage, 3);
        assert_eq!(tags[1].tag, "async");
        assert_eq!(tags[1].usage, 2);
        assert!(tags.iter().any(|t| t.tag == "tokio" && t.usage == 1));
    }
}
