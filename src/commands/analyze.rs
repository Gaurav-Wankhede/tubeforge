//! `analyze "<topic>"` (LLD §8 + growth layer): precise topic analysis for the
//! OWN channel. Scans the topic in realtime (yt-dlp SERP), identifies the
//! demand-supply gap, and auto-drafts title/description/tags. Returns ONLY
//! computed analysis — never raw DB rows. This is the "what should I make next
//! and how should I package it" command.

use serde_json::{json, Value};

use crate::analytics::content::{self, DraftInput};
use crate::analytics::research::inspect;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::ytdlp::YtdlpClient;
use crate::fetch::FetchClients;
use crate::search::bm25::Bm25;
use crate::search::open_or_create;
use crate::storage::Db;

/// `analyze "<topic>" [--serp N]`: precise analysis — scan + gap + packaging.
pub async fn run(cfg: &Config, topic: &str, serp: u64) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;

    let ytdlp = YtdlpClient::new(
        cfg.ytdlp_path.clone(),
        cfg.ytdlp_enabled,
        cfg.ytdlp_client.clone(),
        cfg.ytdlp_js_runtime.clone(),
    )?;
    let clients = FetchClients::new()?;

    let bm25 = open_or_create(&cfg.index_dir())
        .ok()
        .and_then(|index| Bm25::open(index).ok());

    // 1. Realtime SERP scan of the topic.
    let research = inspect(&db, bm25.as_ref(), &ytdlp, &clients, topic, serp).await?;

    // 2. Persist SERP videos + tags (feeds future analysis).
    crate::storage::db::persist_serp_db(&db, &research.serp).await?;
    let _ = crate::analytics::tags::analyze_competitors(&db).await;
    let _ = db
        .upsert_keyword_research(
            &research.keyword,
            &crate::util::now_rfc3339(),
            &research.volume_label,
            research.serp_total as i64,
            research.serp_mean_views,
            research.ranking_channels as i64,
            research.competition_score,
            research.opportunity_score,
            research.actively_published,
            &serde_json::to_string(&research.suggested_tags).unwrap_or_else(|_| "[]".to_string()),
            &serde_json::to_string(&research.related_keywords).unwrap_or_else(|_| "[]".to_string()),
        )
        .await;

    // 3. Auto-draft packaging for the own channel.
    let input = DraftInput {
        topic: research.keyword.clone(),
        volume_label: Some(research.volume_label.clone()),
        opportunity_score: Some(research.opportunity_score),
        competition_score: Some(research.competition_score),
        serp_mean_views: Some(research.serp_mean_views),
        verdict: Some(research.verdict.clone()),
        suggested_tags: research
            .suggested_tags
            .iter()
            .map(|t| t.tag.clone())
            .collect(),
        related_keywords: research
            .related_keywords
            .iter()
            .map(|r| r.keyword.clone())
            .collect(),
    };
    let draft = content::generate(&input);

    // 4. Compute the gap: what ranks but is underserved.
    let gap = compute_gap(&research);

    // 5. Build the ranking chart data (top videos by views — chart-ready).
    let ranking_chart: Vec<Value> = research
        .serp
        .iter()
        .take(6)
        .map(|r| {
            json!({
                "position": research.serp.iter().position(|x| x.video_id == r.video_id).map(|i| i + 1).unwrap_or(0),
                "title": r.title,
                "channel": r.channel,
                "views": r.view_count.unwrap_or(0),
                "seo_score": r.seo_score,
            })
        })
        .collect();

    Ok(json!({
        "topic": research.keyword,
        "verdict": research.verdict,
        "scores": {
            "opportunity": research.opportunity_score,
            "competition": research.competition_score,
            "keyword_score": research.keyword_score,
        },
        "volume": research.volume_label,
        "demand": {
            "serp_total": research.serp_total,
            "avg_views_per_ranking_video": research.serp_mean_views,
            "actively_published": research.actively_published,
        },
        "gap": gap,
        "ranking_chart": ranking_chart,
        "packaging": {
            "title": draft.title,
            "description": draft.description,
            "tags": draft.tags,
        },
        "suggested_tags": research.suggested_tags,
        "related_keywords": research.related_keywords,
    }))
}

/// Compute the demand-supply gap for the topic.
fn compute_gap(research: &crate::analytics::research::KeywordResearch) -> Value {
    let demand = research.serp_mean_views;
    let _supply = research.serp_total as f64;
    let weakness = 1.0 - research.competition_score / 100.0;
    let gap_score = ((demand / 100_000.0).min(1.0) * weakness * 100.0).min(100.0);

    let gap_type = if research.opportunity_score >= 70.0 {
        "underserved — high demand, few channels own this. Strong opening."
    } else if research.opportunity_score >= 40.0 {
        "contested — solid demand across several channels. Win with a sharper angle or better hook."
    } else {
        "saturated — many channels own this with high views. Only enter with a clearly differentiated angle."
    };

    json!({
        "score": round2(gap_score),
        "type": gap_type,
        "demand_views": demand,
        "supply_videos": research.serp_total,
        "competition_weakness": round2(weakness * 100.0),
    })
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
