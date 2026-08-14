//! `forecast` + `suggest` (LLD §8 + forecast layer).
//!
//! - `forecast [KEYWORD]` — future-facing analysis over stored keyword-research
//!   history. Extrapolates opportunity/competition/views over time →
//!   rising/flat/falling verdict + next-period estimate. When no keyword is
//!   given, forecasts every researched keyword and ranks by the next
//!   opportunity estimate (the "which topic to make next" signal).
//! - `suggest <TOPIC>` — auto-draft Title / Description / Tags for a future
//!   video from the topic's research + forecast + the SEO/GEO score model.

use serde_json::{json, Value};

use crate::analytics::content::{self, DraftInput};
use crate::analytics::forecast::{self, Forecast, Point, TrendVerdict};
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::{Db, KeywordResearchRow};

/// `forecast [KEYWORD] [--horizon DAYS] [--channels]`.
pub async fn run_forecast(
    cfg: &Config,
    keyword: Option<&str>,
    horizon: u64,
    with_channels: bool,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let rows = db.keyword_research_all().await?;

    // Group by keyword.
    let mut by_kw: std::collections::BTreeMap<String, Vec<KeywordResearchRow>> =
        std::collections::BTreeMap::new();
    for r in rows {
        by_kw.entry(r.keyword.clone()).or_default().push(r);
    }

    // Optional filter to one keyword.
    let keys: Vec<String> = if let Some(k) = keyword {
        vec![k.to_string()]
    } else {
        by_kw.keys().cloned().collect()
    };

    let mut results: Vec<Value> = Vec::new();
    for k in keys {
        let Some(history) = by_kw.get(&k) else {
            continue;
        };
        let f = forecast_keyword(history, horizon as f64);
        let latest = history.last().cloned();

        let mut item = json!({
            "keyword": k,
            "points": history.len(),
        });
        if let Some(f) = &f {
            item["verdict"] = json!(verdict_str(&f.verdict));
            item["next_opportunity"] = json!(f.next_estimate);
            item["slope_per_day"] = json!(f.slope_per_day);
            item["pct_over_horizon"] = json!(f.pct_over_horizon);
            item["reliability"] = json!(reliability_str(&f.reliability));
            item["t_statistic"] = json!(f.t_statistic);
            item["r_squared"] = json!(f.r_squared);
        }
        if let Some(l) = &latest {
            item["latest_opportunity"] = json!(l.opportunity_score);
            item["latest_competition"] = json!(l.competition_score);
            item["volume"] = json!(l.volume_label);
        }
        results.push(item);
    }

    // Rank by next opportunity estimate desc when forecasting all.
    if keyword.is_none() {
        results.sort_by(|a, b| {
            let ao = a["next_opportunity"].as_f64().unwrap_or(0.0);
            let bo = b["next_opportunity"].as_f64().unwrap_or(0.0);
            bo.partial_cmp(&ao).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    let mut out = json!({
        "forecasted": results.len(),
        "horizon_days": horizon,
        "note": "reliability is honest: most short histories are LOW — re-collect over days/weeks for durable trends",
        "results": results,
    });

    // Channel-growth forecasting (needs channel_snapshots history — build it
    // by running `tubeforge refresh` over time). When TUBEFORGE_OWN_CHANNEL is
    // set, the OWN channel is flagged and compared against the competitor set.
    if with_channels {
        let own = cfg.own_channel.as_deref();
        let channels = db.all_channels().await?;
        let mut channel_results: Vec<Value> = Vec::new();
        for c in &channels {
            let snaps = db.channel_snapshots(&c.channel_id).await?;
            // snaps: (at, subscriber_count, video_count, total_views)
            let f = forecast_channel(&snaps, horizon as f64);
            let is_own = own.map(|o| o == c.channel_id.as_str()).unwrap_or(false);
            let mut item = json!({
                "channel_id": c.channel_id,
                "channel": c.title,
                "is_own": is_own,
            });
            if let Some(f) = f {
                item["verdict"] = json!(verdict_str(&f.verdict));
                item["next_total_views"] = f.next_estimate.into();
                item["slope_per_day"] = f.slope_per_day.into();
                item["pct_over_horizon"] = f.pct_over_horizon.into();
                item["reliability"] = json!(reliability_str(&f.reliability));
                item["points"] = f.points.into();
            }
            channel_results.push(item);
        }
        channel_results.sort_by(|a, b| {
            let ao = a["pct_over_horizon"].as_f64().unwrap_or(0.0);
            let bo = b["pct_over_horizon"].as_f64().unwrap_or(0.0);
            bo.partial_cmp(&ao).unwrap_or(std::cmp::Ordering::Equal)
        });
        let own_count = channel_results
            .iter()
            .filter(|r| r["is_own"] == true)
            .count();
        out["channels"] = json!({
            "forecasted": channel_results.len(),
            "own_channel": own,
            "own_flagged": own_count > 0,
            "note": if own.is_some() {
                "own channel flagged `is_own:true` for growth targeting — compare its growth against the competitors above"
            } else {
                "needs channel_snapshots history — run `tubeforge refresh` repeatedly over days/weeks; set TUBEFORGE_OWN_CHANNEL to flag your own channel"
            },
            "results": channel_results,
        });
    }

    Ok(out)
}

/// Forecast channel growth from `channel_snapshots` (total_views over time).
#[allow(clippy::type_complexity)]
fn forecast_channel(
    snaps: &[(String, Option<i64>, Option<i64>, Option<i64>)],
    horizon: f64,
) -> Option<Forecast> {
    let mut points: Vec<Point> = Vec::with_capacity(snaps.len());
    let mut first: Option<chrono::DateTime<chrono::Utc>> = None;
    for (at, _subs, _vids, views) in snaps {
        let views = (*views)? as f64;
        let at = chrono::DateTime::parse_from_rfc3339(at)
            .map(|d| d.with_timezone(&chrono::Utc))
            .ok()?;
        let base = first.get_or_insert(at);
        let days = (at - *base).num_minutes() as f64 / 60.0 / 24.0;
        points.push(Point { days, value: views });
    }
    forecast::forecast(&points, horizon)
}

/// `suggest <TOPIC> [--horizon DAYS]`.
pub async fn run_suggest(cfg: &Config, topic: &str, horizon: u64) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let history = db.keyword_research_history(topic).await?;
    if history.is_empty() {
        return Ok(json!({
            "topic": topic,
            "error": "topic not researched — run `tubeforge keywords discover \"<topic>\"` first",
        }));
    }

    let latest = history.last().unwrap();
    let f = forecast_keyword(&history, horizon as f64);

    let suggested_tags: Vec<String> =
        serde_json::from_str(&latest.suggested_tags).unwrap_or_default();
    let related: Vec<String> = serde_json::from_str(&latest.related_keywords).unwrap_or_default();

    let input = DraftInput {
        topic: topic.to_string(),
        volume_label: Some(latest.volume_label.clone()),
        opportunity_score: Some(latest.opportunity_score),
        competition_score: Some(latest.competition_score),
        serp_mean_views: Some(latest.serp_mean_views),
        verdict: f.as_ref().map(|f| verdict_str(&f.verdict).to_string()),
        suggested_tags,
        related_keywords: related,
    };
    let draft = content::generate(&input);

    let mut out = json!({
        "topic": draft.topic,
        "demand_angle": draft.demand_angle,
        "title": draft.title,
        "description": draft.description,
        "tags": draft.tags,
    });
    if let Some(f) = &f {
        out["forecast"] = json!({
            "verdict": verdict_str(&f.verdict),
            "next_opportunity": f.next_estimate,
            "reliability": reliability_str(&f.reliability),
            "points": f.points,
        });
    }
    Ok(out)
}

/// Forecast one keyword from its research history (opportunity_score over time).
fn forecast_keyword(history: &[KeywordResearchRow], horizon: f64) -> Option<Forecast> {
    let mut points: Vec<Point> = Vec::with_capacity(history.len());
    let mut first: Option<chrono::DateTime<chrono::Utc>> = None;
    for r in history {
        let at = chrono::DateTime::parse_from_rfc3339(&r.at)
            .map(|d| d.with_timezone(&chrono::Utc))
            .ok();
        if let Some(at) = at {
            let base = first.get_or_insert(at);
            let days = (at - *base).num_minutes() as f64 / 60.0 / 24.0;
            points.push(Point {
                days,
                value: r.opportunity_score,
            });
        }
    }
    forecast::forecast(&points, horizon)
}

fn verdict_str(v: &TrendVerdict) -> &'static str {
    match v {
        TrendVerdict::Rising => "rising",
        TrendVerdict::Flat => "flat",
        TrendVerdict::Falling => "falling",
    }
}

fn reliability_str(r: &forecast::Reliability) -> &'static str {
    match r {
        forecast::Reliability::Low => "low",
        forecast::Reliability::Medium => "medium",
        forecast::Reliability::High => "high",
    }
}
