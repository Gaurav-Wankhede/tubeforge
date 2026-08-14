//! `gaps` + `outliers` (Phase 6.5): competitor gap mining, pure-Rust.
//!
//! - `outliers` — Method A: videos at ≥3x their channel's mean views.
//! - `gaps` — the full report: outliers + coverage map (Method C) +
//!   freshness + format gaps, with demand×weakness scores.
//!
//! No LLM involved: the CLI emits the JSON envelope (agents) or a markdown
//! report (`--markdown`) that feeds the AI prompt bundles downstream.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::analytics::gaps;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::storage::Db;

/// Build the channel_id → title map from the channels table.
async fn channel_names(
    db: &Db,
    videos: &[crate::storage::db::VideoRow],
) -> Result<HashMap<String, String>, TubeforgeError> {
    let mut names: HashMap<String, String> = db
        .all_channels()
        .await?
        .into_iter()
        .map(|c| (c.channel_id, c.title))
        .collect();
    // Fallback: any channel missing from the table (shouldn't happen) gets
    // its id as the display name.
    for v in videos {
        if let Some(cid) = &v.channel_id {
            names.entry(cid.clone()).or_insert_with(|| cid.clone());
        }
    }
    Ok(names)
}

/// `outliers`: videos at ≥3x channel mean views (Method A).
pub async fn run_outliers(cfg: &Config, channels: &[String]) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let mut videos = db.all_videos().await?;
    if !channels.is_empty() {
        videos.retain(|v| {
            v.channel_id
                .as_ref()
                .map(|c| channels.iter().any(|w| w == c))
                .unwrap_or(false)
        });
    }
    let names = channel_names(&db, &videos).await?;
    let rows: Vec<Value> = gaps::outliers(&videos, &names)
        .iter()
        .map(|o| {
            json!({
                "video_id": o.video_id,
                "title": o.title,
                "channel_id": o.channel_id,
                "channel": o.channel_name,
                "views": o.views,
                "channel_mean": o.channel_mean,
                "multiple": o.multiple,
            })
        })
        .collect();
    Ok(json!({ "outliers": rows, "total": rows.len() }))
}

/// `gaps`: full gap report (outliers + coverage + freshness + format).
pub async fn run_gaps(
    cfg: &Config,
    channels: &[String],
    markdown: bool,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let mut videos = db.all_videos().await?;
    if !channels.is_empty() {
        videos.retain(|v| {
            v.channel_id
                .as_ref()
                .map(|c| channels.iter().any(|w| w == c))
                .unwrap_or(false)
        });
    }
    if videos.is_empty() {
        return Ok(json!({
            "outliers": [],
            "topics": [],
            "freshness_gaps": [],
            "format_gaps": [],
            "note": "no videos in database — run `tubeforge ingest` first",
        }));
    }
    let names = channel_names(&db, &videos).await?;
    let report = gaps::report(&videos, &names).await?;

    if markdown {
        return Ok(Value::String(markdown_report(&report)));
    }

    let outliers: Vec<Value> = report
        .outliers
        .iter()
        .map(|o| {
            json!({
                "video_id": o.video_id,
                "title": o.title,
                "channel": o.channel_name,
                "views": o.views,
                "channel_mean": o.channel_mean,
                "multiple": o.multiple,
            })
        })
        .collect();
    let topics: Vec<Value> = report
        .topics
        .iter()
        .take(50)
        .map(|t| {
            json!({
                "topic": t.topic,
                "videos": t.videos,
                "channels": t.channels,
                "mean_views": t.mean_views,
                "newest_at": t.newest_at,
                "no_short": t.no_short,
                "score": t.score,
            })
        })
        .collect();

    Ok(json!({
        "outliers": outliers,
        "topics": topics,
        "freshness_gaps": report.freshness_gaps,
        "format_gaps": report.format_gaps,
        "note": "agent-ready — feed topics into `tubeforge prompt` bundles for AI mining",
    }))
}

/// Markdown gap report (human/agent readable; the `prompt` bundles consume
/// the JSON envelope instead).
fn markdown_report(r: &gaps::GapReport) -> String {
    let mut out = String::new();
    out.push_str("# Competitor Gap Report\n\n");

    out.push_str("## Outliers (≥3x channel mean — proven demand)\n\n");
    if r.outliers.is_empty() {
        out.push_str("_None._\n\n");
    } else {
        out.push_str("| Video | Channel | Views | ×mean |\n|---|---|---|---|\n");
        for o in &r.outliers {
            out.push_str(&format!(
                "| {} | {} | {} | {:.1}x |\n",
                o.title, o.channel_name, o.views, o.multiple
            ));
        }
        out.push('\n');
    }

    out.push_str("## Coverage map (top topics by gap score)\n\n");
    if r.topics.is_empty() {
        out.push_str("_None._\n\n");
    } else {
        out.push_str("| Topic | Videos | Channels | Mean views | Short? | Score |\n|---|---|---|---|---|---|\n");
        for t in r.topics.iter().take(30) {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                t.topic,
                t.videos,
                t.channels,
                t.mean_views as i64,
                if t.no_short { "no" } else { "yes" },
                t.score
            ));
        }
        out.push('\n');
    }

    out.push_str("## Freshness gaps (coverage older than 1y)\n\n");
    if r.freshness_gaps.is_empty() {
        out.push_str("_None._\n\n");
    } else {
        for t in &r.freshness_gaps {
            out.push_str(&format!("- {t}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Format gaps (no Short version)\n\n");
    if r.format_gaps.is_empty() {
        out.push_str("_None._\n\n");
    } else {
        for t in &r.format_gaps {
            out.push_str(&format!("- {t}\n"));
        }
        out.push('\n');
    }

    out
}
