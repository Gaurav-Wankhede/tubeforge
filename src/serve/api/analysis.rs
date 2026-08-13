//! Analysis endpoints — the OWN channel's growth command center.
//!
//! These return **computed, chart-ready analysis only** — never raw DB
//! records. The frontend consumes these to show charts + precise
//! recommendations for growing the user's own channel.
//!
//! Routes (mounted under `/api/analysis/`):
//!   GET /api/analysis/topic?q=<topic>  — precise topic analysis (scan + gap + packaging)
//!   GET /api/analysis/overview       — own channel stats + growth + vs-competitors
//!   GET /api/analysis/next-video     — the single "make this next" recommendation
//!   GET /api/analysis/keywords       — chart-ready keyword opportunity (top 25)
//!   GET /api/analysis/tags           — tag gaps (competitor tags we don't use)

use http::StatusCode;
use crate::serve::web::{get, Json, Query, Router, State};
use serde_json::{json, Value};
use std::collections::HashMap;

use super::AppState;
use crate::fetch::FetchClients;
use crate::storage::db::Db;

/// Build the analysis router (mounted by `api_routes` under `/api/analysis`).
pub fn analysis_routes() -> Router {
    Router::new()
        .route("/api/analysis/topic", get(topic_api))
        .route("/api/analysis/overview", get(overview_api))
        .route("/api/analysis/next-video", get(next_video_api))
        .route("/api/analysis/keywords", get(keywords_api))
        .route("/api/analysis/tags", get(tags_api))
        .route("/api/analysis/graph", get(graph_svg_api))
}

/// GET /api/analysis/topic?q=<topic>&serp=N — precise topic analysis for the
/// own channel. Scans the topic in realtime, identifies the demand-supply gap,
/// and auto-drafts title/description/tags. Returns ONLY computed analysis +
/// chart data — never raw DB rows.
async fn topic_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let topic = params.get("q").cloned().unwrap_or_default();
    if topic.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing ?q= topic" })),
        ));
    }
    let serp: u64 = params.get("serp").and_then(|v| v.parse().ok()).unwrap_or(6);

    let ytdlp = st.ytdlp.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "yt-dlp disabled — set TUBEFORGE_YTDLP_ENABLED=true" })),
        )
    })?;
    let clients = FetchClients::new().map_err(api_err)?;

    // NOTE: corpus resonance (tantivy index) is intentionally skipped in the
    // API handler — holding `&Bm25` (tantivy IndexReader, !Sync) across the
    // inspect await makes the handler future non-Send for axum. The CLI
    // (`tubeforge analyze`) provides corpus resonance; the API serves the
    // live SERP + gap + packaging signals.
    let research = crate::analytics::research::inspect(&st.db, None, ytdlp, &clients, &topic, serp)
        .await
        .map_err(api_err)?;

    // Persist (feeds future analysis) — non-fatal. Clone into locals first so
    // `research` is not borrowed across the awaits (the handler future must
    // remain `Send` for axum — `&[SerpResult]` borrowed from a local is not).
    let serp_rows = research.serp.clone();
    let snapshot = (
        research.keyword.clone(),
        research.volume_label.clone(),
        research.serp_total,
        research.serp_mean_views,
        research.ranking_channels,
        research.competition_score,
        research.opportunity_score,
        research.actively_published,
        serde_json::to_string(&research.suggested_tags).unwrap_or_else(|_| "[]".to_string()),
        serde_json::to_string(&research.related_keywords).unwrap_or_else(|_| "[]".to_string()),
    );
    let db_path = st.db.path.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let Ok(rt) = rt else { return };
        rt.block_on(async move {
            match Db::open(&db_path).await {
                Ok(db) => {
                    if let Err(e) = crate::storage::db::persist_serp_db(&db, &serp_rows).await {
                        tracing::warn!(err = %e, "analysis: persist serp failed");
                    }
                    if let Err(e) = crate::analytics::tags::analyze_competitors(&db).await {
                        tracing::warn!(err = %e, "analysis: analyze competitors failed");
                    }
                }
                Err(e) => tracing::warn!(err = %e, "analysis: open db failed"),
            }
        });
    });
    let _ = st
        .db
        .upsert_keyword_research(
            &snapshot.0,
            &crate::util::now_rfc3339(),
            &snapshot.1,
            snapshot.2 as i64,
            snapshot.3,
            snapshot.4 as i64,
            snapshot.5,
            snapshot.6,
            snapshot.7,
            &snapshot.8,
            &snapshot.9,
        )
        .await;

    // Auto-draft packaging.
    let input = crate::analytics::content::DraftInput {
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
    let draft = crate::analytics::content::generate(&input);

    // Gap analysis.
    let demand = research.serp_mean_views;
    let weakness = 1.0 - research.competition_score / 100.0;
    let gap_score = ((demand / 100_000.0).min(1.0) * weakness * 100.0).min(100.0);
    let gap_type = if research.opportunity_score >= 70.0 {
        "underserved — high demand, few channels own this"
    } else if research.opportunity_score >= 40.0 {
        "contested — solid demand, win with a sharper angle"
    } else {
        "saturated — enter only with a clearly differentiated angle"
    };

    // Ranking chart (top videos by views — chart-ready, no raw ids exposed).
    let ranking_chart: Vec<Value> = research
        .serp
        .iter()
        .take(6)
        .enumerate()
        .map(|(i, r)| {
            json!({
                "position": i + 1,
                "title": r.title,
                "channel": r.channel,
                "views": r.view_count.unwrap_or(0),
                "seo_score": r.seo_score,
            })
        })
        .collect();

    let body = json!({
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
        "gap": {
            "score": (gap_score * 100.0).round() / 100.0,
            "type": gap_type,
            "demand_views": demand,
            "supply_videos": research.serp_total,
        },
        "ranking_chart": ranking_chart,
        "packaging": {
            "title": draft.title,
            "description": draft.description,
            "tags": draft.tags,
        },
        "suggested_tags": research.suggested_tags,
        "related_keywords": research.related_keywords,
    });

    Ok(Json(body))
}

/// The own channel id from state; returns a 400 error JSON when unset.
fn own_or_err(st: &AppState) -> Result<String, (StatusCode, Json)> {
    st.own_channel.clone().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "own channel not configured — set TUBEFORGE_OWN_CHANNEL in .env",
            })),
        )
    })
}

/// GET /api/analysis/overview — own channel analysis (charts + insights).
async fn overview_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let own = own_or_err(&st)?;
    let overview = crate::analytics::growth::own_overview(&st.db, &own)
        .await
        .map_err(api_err)?;
    Ok(Json(serde_json::to_value(overview).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("serialize: {e}") })),
        )
    })?))
}

/// GET /api/analysis/next-video — the best "make this next" recommendation.
async fn next_video_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let rec =
        crate::analytics::growth::next_video_recommendation(&st.db, 7.0, st.own_channel.as_deref())
            .await
            .map_err(api_err)?;
    Ok(Json(json!({ "recommendation": rec })))
}

/// GET /api/analysis/keywords — chart-ready keyword opportunity (top 25).
async fn keywords_api(
    State(st): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let horizon: f64 = params
        .get("horizon")
        .and_then(|v| v.parse().ok())
        .unwrap_or(7.0);
    let opps = crate::analytics::growth::keyword_opportunities(&st.db, horizon)
        .await
        .map_err(api_err)?;
    Ok(Json(
        json!({ "opportunities": opps, "horizon_days": horizon }),
    ))
}

/// GET /api/analysis/tags — tag-gap intelligence for the own channel.
async fn tags_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let own = own_or_err(&st)?;
    let overview = crate::analytics::growth::own_overview(&st.db, &own)
        .await
        .map_err(api_err)?;
    Ok(Json(json!({ "tag_gaps": overview.tag_gaps })))
}

/// Map a `TubeforgeError` into a `(StatusCode, Json)` error envelope.
fn api_err(e: crate::error::TubeforgeError) -> (StatusCode, Json) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": e.to_string()})),
    )
}

/// GET /api/analysis/graph — Knowledge Graph visualization as SVG.
///
/// Returns the force-directed graph rendered as SVG. The KG is loaded
/// lazily on first access. Returns 503 with a clear message if the KG
/// has not been built yet.
async fn graph_svg_api(
    State(st): State<super::AppState>,
) -> Result<(StatusCode, [(&'static str, &'static str); 1], String), (StatusCode, Json)> {
    // Get or load the KG
    let kg = match super::super::get_kg(&st).await {
        Some(kg) => kg,
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "Knowledge Graph not built. Run `tubeforge kg build` first.",
                    "kg_built": false,
                })),
            ));
        }
    };

    // Build visual graph with default physics params
    let params = crate::analytics::graph_viz::PhysicsParams::default();
    let visual = crate::analytics::graph_viz::build_visual_graph(&kg, &params);
    let svg = crate::analytics::graph_viz::render_svg(&visual);

    Ok((
        StatusCode::OK,
        [(http::header::CONTENT_TYPE.as_str(), "image/svg+xml")],
        svg,
    ))
}
