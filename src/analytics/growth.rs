//! Growth analysis layer — the OWN channel's command center.
//!
//! Purpose: transform the raw DB (which `discover`/`ingest` populate) into
//! **precise, chart-ready analysis** for growing the user's own channel.
//! This is the analysis layer the frontend consumes — it NEVER emits raw DB
//! records. Everything returned is a computed insight:
//!   - own-channel overview (stats + growth trend + vs-competitor medians)
//!   - next-video recommendation (forecast-ranked topic + auto title/desc/tags)
//!   - keyword opportunity (chart-ready series per topic)
//!   - tag intelligence (own tags vs competitor tag gaps)
//!
//! Contract: the frontend shows charts and recommendations ONLY. The raw
//! rows live in the DB (fed by `discover`) and never reach the browser.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::analytics::content::{self, DraftInput};
use crate::analytics::forecast::{self, Point};
use crate::error::TubeforgeError;
use crate::storage::db::{ChannelRow, ScoreRow, VideoRow};
use crate::storage::{Db, KeywordResearchRow};

/// Chart-ready point (label + numeric value) — the only shape the frontend
/// receives for a series. No raw identifiers, no internal fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartPoint {
    pub label: String,
    pub value: f64,
}

/// Own-channel overview: computed stats + growth trend + competitor medians.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnOverview {
    pub channel_name: String,
    pub subscriber_count: i64,
    pub video_count: usize,
    pub total_views: i64,
    pub avg_views: f64,
    pub avg_score: f64,
    pub best_video_title: String,
    /// Growth trend (from channel_snapshots total_views) — chart-ready.
    pub growth: Vec<ChartPoint>,
    /// Competitor medians for comparison (computed, no raw rows).
    pub competitor: CompetitorMedians,
    /// Tag-gap headline: top tags competitors use that we don't.
    pub tag_gaps: Vec<TagGapInsight>,
}

/// Computed competitor medians — aggregated, never raw competitor rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitorMedians {
    pub subscriber_median: f64,
    pub avg_views_median: f64,
    pub score_median: f64,
    pub channel_count: usize,
}

/// One tag-gap insight: a tag competitors use that we don't.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagGapInsight {
    pub tag: String,
    pub competitor_usage: i64,
    pub our_usage: i64,
    pub opportunity: f64,
}

/// The single most valuable "what to make next" recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextVideoRecommendation {
    pub topic: String,
    pub verdict: String,
    pub next_opportunity: f64,
    pub opportunity_score: f64,
    pub competition_score: f64,
    pub volume_label: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub reliability: String,
    /// VidIQ-style View Prediction tier (Very High/High/Medium/Low).
    pub prediction: String,
    /// Plain-language "make THIS because...".
    pub why: String,
}

/// One keyword's chart-ready opportunity series (for the opportunity chart).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordOpportunity {
    pub keyword: String,
    pub opportunity: f64,
    pub competition: f64,
    pub volume: String,
    pub verdict: String,
    pub trend: Vec<ChartPoint>,
}

/// Build the own-channel overview. `own` is the own channel id from config.
pub async fn own_overview(db: &Db, own: &str) -> Result<OwnOverview, TubeforgeError> {
    let channels = db.all_channels().await?;
    let videos = db.all_videos().await?;
    let scores = db.all_scores().await?;
    let comp_ids: std::collections::HashSet<String> =
        db.list_competitors().await?.into_iter().collect();

    // Our channel + videos.
    let own_channel = channels
        .iter()
        .find(|c| c.channel_id == own)
        .cloned()
        .unwrap_or_else(|| crate::storage::db::ChannelRow {
            channel_id: own.to_string(),
            title: "My Channel".to_string(),
            ..Default::default()
        });
    let own_videos: Vec<&VideoRow> = videos
        .iter()
        .filter(|v| v.channel_id.as_deref() == Some(own))
        .collect();
    let video_count = own_videos.len();
    let total_views: i64 = own_videos.iter().filter_map(|v| v.view_count).sum();
    let avg_views = if video_count > 0 {
        total_views as f64 / video_count as f64
    } else {
        0.0
    };
    let own_video_ids: std::collections::HashSet<&str> =
        own_videos.iter().map(|v| v.video_id.as_str()).collect();
    let own_scores: Vec<&ScoreRow> = scores
        .iter()
        .filter(|s| own_video_ids.contains(s.video_id.as_str()))
        .collect();
    let avg_score = if own_scores.is_empty() {
        0.0
    } else {
        own_scores.iter().map(|s| s.total_score).sum::<f64>() / own_scores.len() as f64
    };
    let best_video_title = own_videos
        .iter()
        .max_by_key(|v| v.view_count.unwrap_or(0))
        .map(|v| v.title.clone())
        .unwrap_or_else(|| "—".to_string());

    // Growth trend from channel_snapshots (total_views over time).
    let mut growth: Vec<ChartPoint> = Vec::new();
    if let Ok(snaps) = db.channel_snapshots(own).await {
        for (at, _subs, _vids, views) in snaps {
            let label = at.chars().take(10).collect(); // YYYY-MM-DD
            growth.push(ChartPoint {
                label,
                value: views.unwrap_or(0) as f64,
            });
        }
    }

    // Competitor medians (computed across competitors only — never raw rows).
    let comp_channels: Vec<&ChannelRow> = channels
        .iter()
        .filter(|c| comp_ids.contains(&c.channel_id))
        .collect();
    let mut subs: Vec<i64> = comp_channels
        .iter()
        .filter_map(|c| c.subscriber_count)
        .collect();
    let mut avg_views_all: Vec<f64> = Vec::new();
    let mut scores_all: Vec<f64> = Vec::new();
    for v in &videos {
        if let Some(cid) = &v.channel_id {
            if comp_ids.contains(cid) {
                if let Some(c) = &v.view_count {
                    avg_views_all.push(*c as f64);
                }
            }
        }
    }
    for s in &scores {
        if let Some(v) = videos.iter().find(|v| v.video_id == s.video_id) {
            if v.channel_id
                .as_deref()
                .map(|c| comp_ids.contains(c))
                .unwrap_or(false)
            {
                scores_all.push(s.total_score);
            }
        }
    }
    subs.sort_unstable();
    avg_views_all.sort_by(|a, b| a.total_cmp(b));
    scores_all.sort_by(|a, b| a.total_cmp(b));
    let median = |v: &mut [f64]| {
        if v.is_empty() {
            return 0.0;
        }
        let m = v.len() / 2;
        if v.len() % 2 == 1 {
            v[m]
        } else {
            (v[m - 1] + v[m]) / 2.0
        }
    };
    let sub_f: Vec<f64> = subs.into_iter().map(|x| x as f64).collect();
    let competitor = CompetitorMedians {
        subscriber_median: median(&mut sub_f.clone()),
        avg_views_median: median(&mut avg_views_all),
        score_median: median(&mut scores_all),
        channel_count: comp_channels.len(),
    };

    // Tag gaps: competitor tags we don't use (computed from competitor_tag_stats).
    let tag_gaps = tag_gaps_for(db, own).await?;

    Ok(OwnOverview {
        channel_name: own_channel.title,
        subscriber_count: own_channel.subscriber_count.unwrap_or(0),
        video_count,
        total_views,
        avg_views,
        avg_score,
        best_video_title,
        growth,
        competitor,
        tag_gaps,
    })
}

/// Tag intelligence: tags competitors use that our channel doesn't.
async fn tag_gaps_for(db: &Db, own: &str) -> Result<Vec<TagGapInsight>, TubeforgeError> {
    let ours: std::collections::HashSet<String> = db
        .all_videos()
        .await?
        .iter()
        .filter(|v| v.channel_id.as_deref() == Some(own))
        .flat_map(|v| serde_json::from_str::<Vec<String>>(&v.tags).unwrap_or_default())
        .map(|t| t.to_lowercase())
        .collect();
    // Aggregate competitor tag usage from the raw videos (computed, no rows out).
    let comp_ids: std::collections::HashSet<String> =
        db.list_competitors().await?.into_iter().collect();
    let mut usage: HashMap<String, i64> = HashMap::new();
    for v in db.all_videos().await? {
        if v.channel_id
            .as_deref()
            .map(|c| comp_ids.contains(c))
            .unwrap_or(false)
        {
            for t in serde_json::from_str::<Vec<String>>(&v.tags).unwrap_or_default() {
                let t = t.trim().to_lowercase();
                if !t.is_empty() {
                    *usage.entry(t).or_insert(0) += 1;
                }
            }
        }
    }
    let total_comp_videos: i64 = usage.values().sum::<i64>().max(1);
    let mut gaps: Vec<TagGapInsight> = usage
        .into_iter()
        .filter(|(t, _)| !ours.contains(t))
        .map(|(tag, usage)| TagGapInsight {
            tag: tag.clone(),
            competitor_usage: usage,
            our_usage: 0,
            opportunity: (usage as f64 / total_comp_videos as f64 * 100.0).min(100.0),
        })
        .collect();
    gaps.sort_by_key(|b| std::cmp::Reverse(b.competitor_usage));
    gaps.truncate(20);
    Ok(gaps)
}

/// Forecast one keyword's opportunity over its history (chart-ready trend).
fn forecast_keyword(
    history: &[&KeywordResearchRow],
    horizon: f64,
) -> Option<(forecast::Forecast, Vec<ChartPoint>)> {
    let mut points: Vec<Point> = Vec::with_capacity(history.len());
    let mut first: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut trend: Vec<ChartPoint> = Vec::with_capacity(history.len());
    for r in history {
        if let Ok(at) =
            chrono::DateTime::parse_from_rfc3339(&r.at).map(|d| d.with_timezone(&chrono::Utc))
        {
            let base = first.get_or_insert(at);
            let days = (at - *base).num_minutes() as f64 / 60.0 / 24.0;
            points.push(Point {
                days,
                value: r.opportunity_score,
            });
            trend.push(ChartPoint {
                label: at.format("%m-%d").to_string(),
                value: r.opportunity_score,
            });
        }
    }
    forecast::forecast(&points, horizon).map(|f| (f, trend))
}

fn verdict_str(v: &forecast::TrendVerdict) -> &'static str {
    match v {
        forecast::TrendVerdict::Rising => "rising",
        forecast::TrendVerdict::Flat => "flat",
        forecast::TrendVerdict::Falling => "falling",
    }
}

fn reliability_str(r: &forecast::Reliability) -> &'static str {
    match r {
        forecast::Reliability::Low => "low",
        forecast::Reliability::Medium => "medium",
        forecast::Reliability::High => "high",
    }
}

/// Rank all researched keywords by forecast next-opportunity and return the
/// chart-ready opportunity list (computed, never raw rows).
pub async fn keyword_opportunities(
    db: &Db,
    horizon: f64,
) -> Result<Vec<KeywordOpportunity>, TubeforgeError> {
    let rows = db.keyword_research_all().await?;
    let mut by_kw: std::collections::BTreeMap<String, Vec<&KeywordResearchRow>> =
        std::collections::BTreeMap::new();
    for r in &rows {
        by_kw.entry(r.keyword.clone()).or_default().push(r);
    }
    let mut out: Vec<KeywordOpportunity> = Vec::new();
    for (kw, history) in by_kw {
        let latest = history.last();
        let (verdict, trend) = match forecast_keyword(&history, horizon) {
            Some((f, t)) => (verdict_str(&f.verdict).to_string(), t),
            None => ("no-history".to_string(), Vec::new()),
        };
        let opp = latest.map(|r| r.opportunity_score).unwrap_or(0.0);
        let comp = latest.map(|r| r.competition_score).unwrap_or(0.0);
        let vol = latest.map(|r| r.volume_label.clone()).unwrap_or_default();
        out.push(KeywordOpportunity {
            keyword: kw,
            opportunity: opp,
            competition: comp,
            volume: vol,
            verdict,
            trend,
        });
    }
    out.sort_by(|a, b| {
        b.opportunity
            .partial_cmp(&a.opportunity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(25);
    Ok(out)
}

/// Build a ranked LIST of "make this next" recommendations (top `limit`),
/// each with auto-drafted title/description/tags.
///
/// `own_channel` (when set) is used to EXCLUDE topics the user's channel has
/// already covered — so creating a recommended video moves the system to the
/// next best topic instead of re-suggesting the same one forever.
pub async fn next_video_recommendations(
    db: &Db,
    horizon: f64,
    own_channel: Option<&str>,
    limit: usize,
) -> Result<Vec<NextVideoRecommendation>, TubeforgeError> {
    let rows = db.keyword_research_all().await?;
    let mut by_kw: std::collections::BTreeMap<String, Vec<&KeywordResearchRow>> =
        std::collections::BTreeMap::new();
    for r in &rows {
        by_kw.entry(r.keyword.clone()).or_default().push(r);
    }

    // Token sets of the user's own video titles — a topic is "already covered"
    // when its tokens overlap any of them.
    let covered_tokens: std::collections::HashSet<String> = if let Some(own) = own_channel {
        let mut set = std::collections::HashSet::new();
        if let Ok(videos) = db.all_videos().await {
            for v in videos
                .iter()
                .filter(|v| v.channel_id.as_deref() == Some(own))
            {
                set.extend(crate::util::tokens(&v.title));
            }
        }
        set
    } else {
        std::collections::HashSet::new()
    };
    let is_covered = |kw: &str| -> bool { topic_is_covered(kw, &covered_tokens) };

    // Rank by next-opportunity estimate. When there are too few snapshots to
    // forecast (< MIN_POINTS → forecast_keyword is None), fall back to the
    // latest REAL measured opportunity so a once-researched keyword can still
    // be recommended — never silently drop it to 0.0. Skip already-covered
    // topics so the system always points at genuinely new content.
    let mut ranked: Vec<(f64, String, &Vec<&KeywordResearchRow>)> = Vec::new();
    for (kw, history) in &by_kw {
        if is_covered(kw) {
            continue;
        }
        let next_opp = forecast_keyword(history, horizon)
            .and_then(|(f, _)| f.next_estimate)
            .or_else(|| history.last().map(|r| r.opportunity_score))
            .unwrap_or(0.0);
        ranked.push((next_opp, kw.clone(), history));
    }
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut out: Vec<NextVideoRecommendation> = Vec::new();
    for (next_opp, topic, history) in ranked.into_iter().take(limit) {
        let latest = history.last().unwrap();
        let (verdict, reliability) = match forecast_keyword(history, horizon) {
            Some((f, _)) => (
                verdict_str(&f.verdict).to_string(),
                reliability_str(&f.reliability).to_string(),
            ),
            None => ("no-history".to_string(), "low".to_string()),
        };

        // Auto-draft packaging.
        let suggested_tags: Vec<String> =
            serde_json::from_str(&latest.suggested_tags).unwrap_or_default();
        let related: Vec<String> =
            serde_json::from_str(&latest.related_keywords).unwrap_or_default();
        let input = DraftInput {
            topic: topic.clone(),
            volume_label: Some(latest.volume_label.clone()),
            opportunity_score: Some(latest.opportunity_score),
            competition_score: Some(latest.competition_score),
            serp_mean_views: Some(latest.serp_mean_views),
            verdict: Some(verdict.clone()),
            suggested_tags,
            related_keywords: related,
        };
        let draft = content::generate(&input);

        let prediction = crate::analytics::actions::view_prediction(
            latest.opportunity_score,
            // idea_fit: 50 neutral when no keywords; use opportunity as a
            // coarse fit proxy when we lack a dedicated fit signal here.
            latest.opportunity_score,
        );
        let why = crate::analytics::actions::why_make(
            latest.opportunity_score,
            latest.competition_score,
            &latest.volume_label,
            latest.serp_total as usize,
        );

        out.push(NextVideoRecommendation {
            topic: topic.clone(),
            verdict,
            next_opportunity: next_opp,
            opportunity_score: latest.opportunity_score,
            competition_score: latest.competition_score,
            volume_label: latest.volume_label.clone(),
            title: draft.title,
            description: draft.description,
            tags: draft.tags,
            reliability,
            prediction: prediction.to_string(),
            why,
        });
    }
    Ok(out)
}

/// Single "make this next" recommendation (highest-ranked) — kept for the
/// HTTP `/api/analysis/next-video` contract. `None` when no candidate exists.
pub async fn next_video_recommendation(
    db: &Db,
    horizon: f64,
    own_channel: Option<&str>,
) -> Result<Option<NextVideoRecommendation>, TubeforgeError> {
    Ok(next_video_recommendations(db, horizon, own_channel, 1)
        .await?
        .into_iter()
        .next())
}

/// True when a topic's significant tokens already appear in the user's own
/// video titles (≥2 token hits). Used to exclude topics the channel has
/// already covered so the next-video recommendation never repeats itself.
/// An empty `covered` set means "nothing covered yet" → always false.
fn topic_is_covered(topic: &str, covered: &std::collections::HashSet<String>) -> bool {
    if covered.is_empty() {
        return false;
    }
    let hits = crate::util::tokens(topic)
        .into_iter()
        .filter(|t| covered.contains(t))
        .count();
    hits >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(words: &[&str]) -> std::collections::HashSet<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn covered_when_two_token_hits() {
        // Own titles already contain "rust" and "ownership".
        let covered = set(&["rust", "ownership", "borrowing", "lifetime"]);
        assert!(topic_is_covered("rust ownership", &covered));
        assert!(topic_is_covered("ownership in rust explained", &covered));
    }

    #[test]
    fn not_covered_when_fewer_than_two_hits() {
        let covered = set(&["rust", "async"]);
        // Only one token ("rust") matches — a related but different topic.
        assert!(!topic_is_covered("rust server deployment", &covered));
        assert!(!topic_is_covered("async database", &covered));
    }

    #[test]
    fn not_covered_when_no_hits() {
        let covered = set(&["rust", "ownership"]);
        assert!(!topic_is_covered("react state management", &covered));
    }

    #[test]
    fn not_covered_when_no_own_videos() {
        // Empty set → nothing is considered covered.
        assert!(!topic_is_covered(
            "rust ownership",
            &std::collections::HashSet::new()
        ));
    }
}
