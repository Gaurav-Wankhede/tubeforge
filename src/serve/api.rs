//! JSON API handlers for the TubeForge SPA frontend (PRD §5.4 — API
//! wire for the standalone SPA that replaces the HTMX dashboard).
//!
//! All endpoints return `application/json`. Route tree is mounted under
//! `/api/` by `api_routes()`.  No CSRF gate — the SPA runs on loopback
//! and sends no cookies (stateless JSON, Authorization header when auth
//! is added later).

use std::collections::HashMap;

use http::StatusCode;
use crate::serve::web::{get, post, Json, Path, Query, Router, State};
use serde_json::{json, Value};

use super::AppState;
use crate::analytics::keywords::trend_rows;
use crate::analytics::reports;
use crate::analytics::tags;
use crate::error::TubeforgeError;
use crate::storage::db::Db;

pub mod analysis;

// ---------------------------------------------------------------------------
// SEO / GEO component keys (hardcoded — match LLD §7.2/§7.3 constants
// in the parent module; not imported to keep this module self-contained).
// ---------------------------------------------------------------------------

const SEO_COMPONENT_KEYS: [&str; 15] = [
    "keyword_title",
    "title_front",
    "title_length",
    "title_hooks",
    "title_40_chars",
    "keyword_desc",
    "desc_first150",
    "desc_first2lines",
    "desc_length",
    "desc_structure",
    "tags_relevance",
    "tags_quality",
    "keyword_tags",
    "hashtag_count",
    "keyword_triple",
];

const GEO_COMPONENT_KEYS: [&str; 7] = [
    "entity_coverage",
    "qa_phrasing",
    "list_phrasing",
    "conversational",
    "metadata_complete",
    "location_signal",
    "topic_relevance",
];

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the JSON API router. All routes are under `/api/`.
pub fn api_routes() -> Router {
    Router::new()
        .route("/api/healthz", get(healthz_api))
        .route("/api/counts", get(counts_api))
        .route("/api/trends", get(trends_api))
        .route("/api/alerts", get(alerts_api))
        .route("/api/alerts/read", post(alerts_read_api))
        .route("/api/alerts/clear", post(alerts_clear_api))
        .route("/api/scores", get(scores_api))
        .route("/api/scores/{id}", get(score_detail_api))
        .route("/api/videos", get(videos_api))
        .route("/api/videos/{id}", get(video_detail_api))
        .route("/api/ideas/analyze", get(ideas_analyze_api))
        .route("/api/keywords", get(keywords_api))
        .route("/api/keywords/trending", get(keywords_trending_api))
        .route("/api/keywords/inspect", get(keywords_inspect_api))
        .route("/api/keywords/history", get(keywords_history_api))
        .route("/api/scorecard", get(scorecard_api))
        .route("/api/audit", get(audit_api))
        .route("/api/audit/{id}", get(audit_channel_api))
        .route("/api/health", get(health_api))
        .route("/api/channels/{id}/snapshots", get(channel_snapshots_api))
        .route("/api/gaps", get(gaps_api))
        .route("/api/gaps/outliers", get(gaps_outliers_api))
        .route("/api/gaps/coverage", get(gaps_coverage_api))
        .route("/api/transcripts", get(transcripts_api))
        .route("/api/transcripts/{id}", get(transcript_api))
        .route("/api/comments/{id}", get(comments_api))
        .route("/api/tags", get(tags_cloud_api))
        .route("/api/tags/gaps", get(tags_gaps_api))
        .route("/api/tags/video/{id}", get(video_tags_api))
        .route("/api/tags/competitor/{id}", get(competitor_tags_api))
        .merge(analysis::analysis_routes())
        .fallback(api_not_found)
}

/// Unknown `/api/*` path → 404 JSON (not the SPA shell). The SPA
/// fallback_service catches unmatched paths, so the API must own its
/// 404s explicitly or `/api/typo` would serve index.html.
async fn api_not_found() -> (StatusCode, Json) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "not_found" })),
    )
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/healthz — liveness probe.
async fn healthz_api() -> (StatusCode, Json) {
    (StatusCode::OK, Json(json!({"ok": true})))
}

/// GET /api/counts — aggregate entity counts from the health report.
///
/// Enhanced with `kg_built` and `kg_stats` when the Knowledge Graph is available.
async fn counts_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let stale = super::stale_days();
    let h = reports::health(&st.db, stale).await.map_err(api_err)?;
    let counts = &h["counts"];
    let tags = st.db.count_tags().await.map_err(api_err)?;

    // KG status (non-blocking — returns zeros if KG not built)
    let kg = super::kg_status(&st).await;

    Ok(Json(json!({
        "videos":    counts["videos"].as_i64().unwrap_or(0),
        "channels":  counts["channels"].as_i64().unwrap_or(0),
        "tags":      tags,
        "ideas":     counts["ideas"].as_i64().unwrap_or(0),
        "alerts":    counts["alerts"].as_i64().unwrap_or(0),
        "keywords":  counts["keyword_rankings"].as_i64().unwrap_or(0),
        "kg_built":  kg.built,
        "kg_stats": {
            "entities":   kg.entity_count,
            "relations":  kg.relation_count,
            "communities": kg.community_count,
        },
    })))
}

/// GET /api/trends — real views-over-time series aggregated from
/// `channel_snapshots` (subscriber/video/view history written on every
/// refresh). Aggregates total_views across all channels per snapshot date,
/// so the Dashboard's "Views (30 days)" chart shows actual growth history.
async fn trends_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let channels = st.db.all_channels().await.map_err(api_err)?;
    let mut by_date: std::collections::BTreeMap<String, (i64, i64, i64)> =
        std::collections::BTreeMap::new();
    for c in &channels {
        let rows = st
            .db
            .channel_snapshots(&c.channel_id)
            .await
            .map_err(api_err)?;
        for (at, subs, vids, views) in rows {
            let e = by_date.entry(at.clone()).or_insert((0, 0, 0));
            e.0 += subs.unwrap_or(0);
            e.1 += vids.unwrap_or(0);
            e.2 += views.unwrap_or(0);
        }
    }

    let points: Vec<Value> = by_date
        .into_iter()
        .map(|(date, (subs, vids, views))| {
            json!({
                "date": date,
                "views": views,
                "subscribers": subs,
                "videos": vids,
            })
        })
        .collect();
    Ok(Json(json!(points)))
}

/// GET /api/alerts — newest alerts first.
async fn alerts_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let alerts = st.db.list_alerts(100).await.map_err(api_err)?;
    let items: Vec<Value> = alerts
        .iter()
        .map(|a| {
            json!({
                "id":         a.alert_id,
                "kind":       a.kind,
                "message":    a.message,
                "severity":   a.severity,
                "created_at": a.created_at,
                "read":       a.read_at.is_some(),
            })
        })
        .collect();
    Ok(Json(json!(items)))
}

/// POST /api/alerts/read — mark all alerts read (200 OK).
async fn alerts_read_api(
    State(st): State<AppState>,
) -> Result<StatusCode, (StatusCode, Json)> {
    st.db.mark_alerts_read().await.map_err(api_err)?;
    Ok(StatusCode::OK)
}

/// POST /api/alerts/clear — delete all alerts (200 OK).
async fn alerts_clear_api(
    State(st): State<AppState>,
) -> Result<StatusCode, (StatusCode, Json)> {
    st.db.clear_alerts().await.map_err(api_err)?;
    Ok(StatusCode::OK)
}

/// GET /api/scores — joined video+scores+channels, filterable and sortable.
/// Query params: `?q=` (title search), `?sort=field:asc|desc`.
async fn scores_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let q = params.get("q").cloned().unwrap_or_default();
    let sort = params.get("sort").cloned().unwrap_or_default();

    let videos = st.db.all_videos().await.map_err(api_err)?;
    let scores = st.db.all_scores().await.map_err(api_err)?;
    let channels = st.db.all_channels().await.map_err(api_err)?;

    let channel_title: HashMap<&str, &str> = channels
        .iter()
        .map(|c| (c.channel_id.as_str(), c.title.as_str()))
        .collect();
    let score_by_id: HashMap<&str, &crate::storage::db::ScoreRow> =
        scores.iter().map(|s| (s.video_id.as_str(), s)).collect();

    let ql = q.trim().to_lowercase();
    let mut items: Vec<Value> = videos
        .iter()
        .filter(|v| ql.is_empty() || v.title.to_lowercase().contains(&ql))
        .map(|v| {
            let s = score_by_id.get(v.video_id.as_str());
            json!({
                "video_id":        v.video_id,
                "title":           v.title,
                "channel_name":    v.channel_id.as_deref()
                    .and_then(|cid| channel_title.get(cid).copied())
                    .unwrap_or("—"),
                "overall_score":   s.map(|s| s.total_score).unwrap_or(0.0),
                "freshness_score": s.map(|s| s.seo_score).unwrap_or(0.0),
                "authority_score": s.map(|s| s.geo_score).unwrap_or(0.0),
                "published_at":    v.published_at,
                "views":           v.view_count.unwrap_or(0),
            })
        })
        .collect();

    // Sort: field:asc or field:desc (default: score desc).
    if let Some((field, asc)) = parse_sort(&sort) {
        items.sort_by(|a, b| {
            let ord = match field.as_str() {
                "overall_score" | "score" => compare_f64(
                    a["overall_score"].as_f64().unwrap_or(0.0),
                    b["overall_score"].as_f64().unwrap_or(0.0),
                ),
                "views" => compare_i64(a["views"].as_i64(), b["views"].as_i64()),
                "published_at" | "date" => {
                    a["published_at"].as_str().cmp(&b["published_at"].as_str())
                }
                "title" => a["title"].as_str().cmp(&b["title"].as_str()),
                _ => std::cmp::Ordering::Equal,
            };
            if asc {
                ord
            } else {
                ord.reverse()
            }
        });
    } else {
        items.sort_by(|a, b| {
            compare_f64(
                b["overall_score"].as_f64().unwrap_or(0.0),
                a["overall_score"].as_f64().unwrap_or(0.0),
            )
        });
    }

    Ok(Json(json!(items)))
}

/// GET /api/scores/:id — 17-component score detail for one video.
///
/// Enhanced with `graph_scores` when the Knowledge Graph is available.
/// The `graph_scores` field is `null` when KG is not built (backward compatible).
async fn score_detail_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let score = st.db.get_score(&id).await.map_err(api_err)?;
    let score = match score {
        Some(s) => s,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "score not found"})),
            ));
        }
    };
    let video = st.db.get_video(&id).await.map_err(api_err)?;
    let title = video
        .as_ref()
        .map(|v| v.title.clone())
        .unwrap_or_else(|| id.clone());

    let components: Value =
        serde_json::from_str(&score.components).unwrap_or(Value::Object(Default::default()));

    let mut seo_components = HashMap::new();
    for k in &SEO_COMPONENT_KEYS {
        if let Some(v) = components.get(*k).and_then(|v| v.as_f64()) {
            seo_components.insert(k.to_string(), v);
        }
    }
    let mut geo_components = HashMap::new();
    for k in &GEO_COMPONENT_KEYS {
        if let Some(v) = components.get(*k).and_then(|v| v.as_f64()) {
            geo_components.insert(k.to_string(), v);
        }
    }

    // Compute graph scores if KG is available (internal enhancement, no separate API)
    let graph_scores = compute_graph_scores_for_video(&st, &id, &video).await;

    Ok(Json(json!({
        "video_id":       id,
        "title":          title,
        "seo_total":      score.seo_score,
        "geo_total":      score.geo_score,
        "total":          score.total_score,
        "seo_components": seo_components,
        "geo_components": geo_components,
        "graph_scores":   graph_scores,
        "performance":    performance_for(&st.db, &id).await,
    })))
}

/// Compute graph-aware scores for a video (internal KG enhancement).
///
/// Returns `Value::Null` when KG is not available (graceful degradation).
async fn compute_graph_scores_for_video(
    st: &AppState,
    video_id: &str,
    video: &Option<crate::storage::db::VideoRow>,
) -> Value {
    let kg = match super::get_kg(st).await {
        Some(kg) => kg,
        None => return Value::Null,
    };
    let channel_id = video.as_ref().and_then(|v| v.channel_id.as_deref());
    let keywords: Vec<String> = video
        .as_ref()
        .map(|v| {
            serde_json::from_str::<Vec<String>>(&v.tags)
                .unwrap_or_default()
                .into_iter()
                .take(5)
                .collect()
        })
        .unwrap_or_default();
    let scores =
        crate::analytics::graph_aware::compute_graph_scores(&kg, video_id, channel_id, &keywords);
    serde_json::json!({
        "tag_authority":       scores.tag_authority,
        "topic_dominance":     scores.topic_dominance,
        "keyword_competition": scores.keyword_competition,
    })
}

/// Phase 6.6 performance-half payload for one video: VPH, engagement,
/// retention (from the stored yt-dlp heatmap) and trending flag.
async fn performance_for(db: &Db, video_id: &str) -> Value {
    use crate::analytics::performance as perf;
    use chrono::Utc;

    let Ok(Some(video)) = db.get_video(video_id).await else {
        return Value::Null;
    };
    let heatmap_json = db.get_heatmap(video_id).await.unwrap_or_default();
    let heatmap: Vec<(f64, f64)> = serde_json::from_str::<Vec<Value>>(&heatmap_json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| {
            let t = p.get("start_time").and_then(Value::as_f64)?;
            let v = p.get("value").and_then(Value::as_f64)?;
            Some((t, v))
        })
        .collect();
    let signals = perf::video_signals(&video, &heatmap, Utc::now());

    // Trending needs channel context (mean VPH across the channel).
    let mut trending = signals.trending;
    if let Some(cid) = &video.channel_id {
        if let Ok(all) = db.all_videos().await {
            let mut signals_all: Vec<perf::PerformanceSignals> = all
                .iter()
                .map(|v| perf::video_signals(v, &[], Utc::now()))
                .collect();
            let mut videos_all = all;
            perf::mark_trending(&mut videos_all, &mut signals_all);
            if let Some((_, s)) = videos_all.iter().zip(signals_all.iter()).find(|(v, _)| {
                v.video_id == video_id && v.channel_id.as_deref() == Some(cid.as_str())
            }) {
                trending = s.trending;
            }
        }
    }

    json!({
        "vph": signals.vph,
        "trending": trending,
        "engagement_ratio": signals.engagement_ratio,
        "engagement_score": signals.engagement_score,
        "hook_retention": signals.hook_retention,
        "mean_retention": signals.mean_retention,
        "retention_score": signals.retention_score,
        "heatmap": heatmap.iter().map(|&(t, v)| json!({"start_time": t, "value": v})).collect::<Vec<_>>(),
    })
}

/// GET /api/videos — paginated video list with optional search and sort.
/// Query params: `?q=`, `?page=`, `?page_size=`, `?sort=field:asc|desc`.
async fn videos_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let q = params.get("q").cloned().unwrap_or_default();
    let page: usize = params
        .get("page")
        .and_then(|p| p.parse().ok())
        .unwrap_or(1)
        .max(1);
    let page_size: usize = params
        .get("page_size")
        .and_then(|p| p.parse().ok())
        .unwrap_or(20);
    let sort = params.get("sort").cloned().unwrap_or_default();

    let videos = st.db.all_videos().await.map_err(api_err)?;
    let scores = st.db.all_scores().await.map_err(api_err)?;
    let channels = st.db.all_channels().await.map_err(api_err)?;

    let channel_title: HashMap<&str, &str> = channels
        .iter()
        .map(|c| (c.channel_id.as_str(), c.title.as_str()))
        .collect();
    let score_by_id: HashMap<&str, &crate::storage::db::ScoreRow> =
        scores.iter().map(|s| (s.video_id.as_str(), s)).collect();

    let ql = q.trim().to_lowercase();
    let filtered: Vec<&crate::storage::db::VideoRow> = videos
        .iter()
        .filter(|v| ql.is_empty() || v.title.to_lowercase().contains(&ql))
        .collect();
    let total = filtered.len();

    let mut items: Vec<Value> = filtered
        .iter()
        .map(|v| {
            let s = score_by_id.get(v.video_id.as_str());
            json!({
                "video_id":       v.video_id,
                "title":          v.title,
                "channel_name":   v.channel_id.as_deref()
                    .and_then(|cid| channel_title.get(cid).copied())
                    .unwrap_or("—"),
                "channel_id":     v.channel_id.as_deref().unwrap_or(""),
                "published_at":   v.published_at,
                "view_count":     v.view_count.unwrap_or(0),
                "like_count":     v.like_count.unwrap_or(0),
                "comment_count":  v.comment_count.unwrap_or(0),
                "thumbnail_url":  v.thumb_url.as_deref().unwrap_or(""),
                "description":    v.description,
                "duration_secs":  v.duration_sec.unwrap_or(0),
                "category_id":    v.category_id.as_deref().unwrap_or(""),
                "tags":           v.tags,
                "seo_score":      s.map(|s| s.seo_score),
                "geo_score":      s.map(|s| s.geo_score),
                "total_score":    s.map(|s| s.total_score),
            })
        })
        .collect();

    if let Some((field, asc)) = parse_sort(&sort) {
        items.sort_by(|a, b| {
            let ord = match field.as_str() {
                "published_at" | "date" => {
                    a["published_at"].as_str().cmp(&b["published_at"].as_str())
                }
                "title" => a["title"].as_str().cmp(&b["title"].as_str()),
                "view_count" | "views" => {
                    compare_i64(a["view_count"].as_i64(), b["view_count"].as_i64())
                }
                "total_score" | "score" => {
                    let a_s = a["total_score"]
                        .as_f64()
                        .or(a["seo_score"].as_f64())
                        .unwrap_or(0.0);
                    let b_s = b["total_score"]
                        .as_f64()
                        .or(b["seo_score"].as_f64())
                        .unwrap_or(0.0);
                    compare_f64(a_s, b_s)
                }
                _ => std::cmp::Ordering::Equal,
            };
            if asc {
                ord
            } else {
                ord.reverse()
            }
        });
    }

    let offset = (page - 1) * page_size;
    let paged: Vec<Value> = items.into_iter().skip(offset).take(page_size).collect();

    Ok(Json(json!({
        "items":     paged,
        "total":     total,
        "page":      page,
        "page_size": page_size,
    })))
}

/// GET /api/videos/:id — full video detail with scores.
async fn video_detail_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let video = st.db.get_video(&id).await.map_err(api_err)?;
    let video = match video {
        Some(v) => v,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "video not found"})),
            ));
        }
    };
    let score = st.db.get_score(&id).await.map_err(api_err)?;
    let channels = st.db.all_channels().await.map_err(api_err)?;
    let channel_name = channels
        .iter()
        .find(|c| video.channel_id.as_deref() == Some(&c.channel_id))
        .map(|c| c.title.clone())
        .unwrap_or_else(|| "—".to_string());

    Ok(Json(json!({
        "video_id":       video.video_id,
        "title":          video.title,
        "channel_id":     video.channel_id,
        "channel_name":   channel_name,
        "published_at":   video.published_at,
        "view_count":     video.view_count.unwrap_or(0),
        "like_count":     video.like_count.unwrap_or(0),
        "comment_count":  video.comment_count.unwrap_or(0),
        "thumbnail_url":  video.thumb_url,
        "description":    video.description,
        "duration_secs":  video.duration_sec.unwrap_or(0),
        "category_id":    video.category_id,
        "tags":           video.tags,
        "seo_score":      score.as_ref().map(|s| s.seo_score),
        "geo_score":      score.as_ref().map(|s| s.geo_score),
        "total_score":    score.as_ref().map(|s| s.total_score),
        "components":     score.as_ref().and_then(|s| {
            serde_json::from_str::<Value>(&s.components).ok()
        }),
    })))
}

/// GET /api/ideas/analyze — compute fresh idea recommendations at runtime
/// from the current corpus. Opens the BM25 index, runs the full scoring
/// pipeline (SEO + idea_fit + competitor_gap + engagement_boost), returns
/// ranked ideas WITHOUT persisting. Every request reflects the latest data.
async fn ideas_analyze_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let top_n: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(25);
    let niche = params.get("niche").map(|s| s.as_str());

    let videos = st.db.all_videos().await.map_err(api_err)?;
    if videos.is_empty() {
        return Ok(Json(json!({
            "ideas": [],
            "note": "no videos in database — run `tubeforge ingest` first",
            "generated_at": crate::util::now_rfc3339(),
        })));
    }

    // Open the BM25 index (synchronous, done before any async work).
    let index_dir = st.data_dir.join("index");
    let index = crate::search::open_or_create(&index_dir).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("index: {e}") })),
        )
    })?;
    let bm25 = crate::search::bm25::Bm25::open(index).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("bm25: {e}") })),
        )
    })?;
    let weights = crate::scoring::weights::Weights::from_env().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("weights: {e}") })),
        )
    })?;

    let ideas = crate::analytics::ideas::analyze(&st.db, &bm25, &videos, &weights, niche, top_n)
        .await
        .map_err(api_err)?;

    let items: Vec<Value> = ideas
        .iter()
        .enumerate()
        .map(|(i, c)| {
            json!({
                "id":             i + 1,
                "title":          c.title_suggestion,
                "rationale":      c.rationale,
                "score":          c.score,
                "source_video":   c.source_video,
            })
        })
        .collect();

    // Generate graph-based ideas if KG is available
    let graph_ideas = compute_graph_ideas(&st, &videos).await;

    Ok(Json(json!({
        "ideas": items,
        "generated_at": crate::util::now_rfc3339(),
        "corpus_size": videos.len(),
        "graph_ideas": graph_ideas,
    })))
}

/// Compute graph-based video ideas (internal KG enhancement).
///
/// Returns `Value::Null` when KG is not available.
async fn compute_graph_ideas(st: &AppState, _videos: &[crate::storage::db::VideoRow]) -> Value {
    let kg = match super::get_kg(st).await {
        Some(kg) => kg,
        None => return Value::Null,
    };
    let own_channel = st.own_channel.as_deref();
    let ideas = crate::analytics::graph_aware::generate_graph_ideas(&kg, own_channel, 5);
    let ideas_json: Vec<Value> = ideas
        .into_iter()
        .map(|(title, score, rationale)| {
            serde_json::json!({
                "title": title,
                "score": score,
                "rationale": rationale,
                "source": "knowledge_graph",
            })
        })
        .collect();
    Value::Array(ideas_json)
}

/// GET /api/keywords — keyword rankings with trends and sparkline data.
async fn keywords_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let rankings = st.db.list_rankings().await.map_err(api_err)?;
    let trends = trend_rows(&rankings);
    let items: Vec<Value> = trends
        .iter()
        .map(|t| {
            let sparkline: Vec<Option<i64>> = t["snapshots"]
                .as_array()
                .map(|arr| arr.iter().map(|s| s["position"].as_i64()).collect())
                .unwrap_or_default();
            json!({
                "keyword":  t["keyword"],
                "rank":     t["latest_position"],
                "trend":    t["delta"],
                "sparkline": sparkline,
            })
        })
        .collect();
    Ok(Json(json!(items)))
}

/// GET /api/keywords/trending — trending keywords: latest research snapshot
/// per keyword, ranked by opportunity score DESC (VidIQ-style). Feeds the
/// frontend's `trendingKeywords` call.
async fn keywords_trending_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let rows = st.db.keyword_trending(50).await.map_err(api_err)?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "keyword": r.keyword,
                "score": r.opportunity_score,
                "competition": r.competition_score,
                "serp_mean_views": r.serp_mean_views,
                "volume_label": r.volume_label,
                "actively_published": r.actively_published,
                "source": "youtube_search",
            })
        })
        .collect();
    Ok(Json(
        json!({ "trending": items, "total": items.len() }),
    ))
}

/// GET /api/keywords/inspect?q=<topic>&serp=N — VidIQ-style keyword
/// research: real YouTube SERP demand proxy + competition + opportunity +
/// related keywords + our own corpus matches. Keyless (yt-dlp ytsearch +
/// Google YouTube autocomplete). Requires TUBEFORGE_YTDLP_ENABLED.
async fn keywords_inspect_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let q = params.get("q").cloned().unwrap_or_default();
    if q.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "missing ?q= keyword"})),
        ));
    }
    let serp: u64 = params
        .get("serp")
        .and_then(|v| v.parse().ok())
        .unwrap_or(crate::analytics::research::DEFAULT_SERP);

    let Some(ytdlp) = &st.ytdlp else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "yt-dlp disabled — set TUBEFORGE_YTDLP_ENABLED=true in .env",
            })),
        ));
    };

    let clients = crate::fetch::FetchClients::new().map_err(api_err)?;

    // NOTE: corpus resonance (the tantivy index) is intentionally skipped in
    // the API handler — holding `&Bm25` (tantivy IndexReader, !Sync) across
    // the inspect await makes the handler future non-Send for axum. The CLI
    // (`tubeforge keywords inspect`) provides corpus resonance; the API
    // serves the live SERP + autocomplete + persistence signals.
    let r = crate::analytics::research::inspect(&st.db, None, ytdlp, &clients, &q, serp)
        .await
        .map_err(api_err)?;

    // Persist SERP videos + tags on a dedicated std thread (turso's
    // Connection is !Send, so its async work cannot run in the handler's
    // future or in a tokio::spawn; a std thread with its own runtime handle
    // sidesteps both Send bounds). Errors logged, not fatal.
    {
        let serp_rows = r.serp.clone();
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
                            tracing::warn!(err = %e, "keyword research: persist serp failed");
                        }
                        if let Err(e) = crate::analytics::tags::analyze_competitors(&db).await {
                            tracing::warn!(err = %e, "keyword research: analyze failed");
                        }
                    }
                    Err(e) => tracing::warn!(err = %e, "keyword research: open db failed"),
                }
            });
        });
    }

    // Persist the snapshot immediately (migration 008) so the SPA can chart
    // opportunity/competition/demand history over time. Values are cloned
    // first so `r` is not borrowed across the await (Send bound).
    let snapshot = (
        r.keyword.clone(),
        r.volume_label.clone(),
        r.serp_total,
        r.serp_mean_views,
        r.ranking_channels,
        r.competition_score,
        r.opportunity_score,
        r.actively_published,
        serde_json::to_string(&r.suggested_tags).unwrap_or_else(|_| "[]".to_string()),
        serde_json::to_string(&r.related_keywords).unwrap_or_else(|_| "[]".to_string()),
    );
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

    let value = serde_json::to_value(r).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("serialize: {e}") })),
        )
    })?;
    Ok(Json(value))
}

/// GET /api/keywords/history?q=<keyword> — research snapshots over time
/// (opportunity/competition/demand trend — migration 008).
async fn keywords_history_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let q = params.get("q").cloned().unwrap_or_default();
    if q.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "missing ?q= keyword"})),
        ));
    }
    let rows = st.db.keyword_research_history(&q).await.map_err(api_err)?;
    let points: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "at": r.at,
                "volume_label": r.volume_label,
                "serp_total": r.serp_total,
                "serp_mean_views": r.serp_mean_views,
                "ranking_channels": r.ranking_channels,
                "competition_score": r.competition_score,
                "opportunity_score": r.opportunity_score,
                "actively_published": r.actively_published,
                "suggested_tags": serde_json::from_str::<Value>(&r.suggested_tags).unwrap_or(Value::Array(vec![])),
                "related_keywords": serde_json::from_str::<Value>(&r.related_keywords).unwrap_or(Value::Array(vec![])),
            })
        })
        .collect();
    Ok(Json(
        json!({ "keyword": q, "snapshots": points, "total": points.len() }),
    ))
}

/// GET /api/scorecard — per-channel scorecard vs median.
///
/// Enhanced with `centrality` ranking when the Knowledge Graph is available.
/// Each row includes a `centrality` field (PageRank score, 0-1) that is
/// `null` when KG is not built (backward compatible).
async fn scorecard_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    // Comparison set = competitors + our own channel (when set), so the
    // dashboard always shows OUR channel against the competitors it must beat.
    let mut only: Vec<String> = st.db.list_competitors().await.map_err(api_err)?;
    if let Some(own) = &st.own_channel {
        if !only.contains(own) {
            only.push(own.clone());
        }
    }
    let card = reports::scorecard(&st.db, &only).await.map_err(api_err)?;
    let channels = st.db.all_channels().await.map_err(api_err)?;
    let sub_map: HashMap<&str, i64> = channels
        .iter()
        .filter_map(|c| c.subscriber_count.map(|n| (c.channel_id.as_str(), n)))
        .collect();
    let own = st.own_channel.as_deref();

    // Build centrality map from KG (if available)
    let centrality_map: HashMap<String, f64> = super::get_kg(&st)
        .await
        .map(|kg| {
            kg.centrality
                .iter()
                .filter(|(id, _)| id.starts_with("channel:"))
                .map(|(id, score)| {
                    let cid = id.strip_prefix("channel:").unwrap_or(id);
                    (cid.to_string(), *score)
                })
                .collect()
        })
        .unwrap_or_default();

    let mut rows: Vec<Value> = Vec::new();
    if let Some(arr) = card["channels"].as_array() {
        for c in arr {
            let cid = c["channel_id"].as_str().unwrap_or("");
            let videos = c["videos"].as_u64().unwrap_or(0) as f64;
            let total_views = c["total_views"].as_f64().unwrap_or(0.0);
            let avg_views = if videos > 0.0 {
                total_views / videos
            } else {
                0.0
            };
            let avg_engagement = c["seo"]["avg"].as_f64().unwrap_or(0.0);
            let overall = c["seo"]["avg"].as_f64().unwrap_or(0.0);
            let centrality = centrality_map.get(cid).copied();
            rows.push(json!({
                "channel_id":       cid,
                "channel_name":     c["title"].as_str().unwrap_or(""),
                "subscriber_count": sub_map.get(cid).copied().unwrap_or(0),
                "video_count":      c["videos"].as_u64().unwrap_or(0),
                "avg_views":        avg_views,
                "avg_engagement":   avg_engagement,
                "overall_score":    overall,
                "centrality":       centrality,
                "is_own":           own.map(|o| o == cid).unwrap_or(false),
            }));
        }
    }
    // Rank: our channel first (when present), then by overall score.
    rows.sort_by(|a, b| {
        let a_own = a["is_own"].as_bool().unwrap_or(false);
        let b_own = b["is_own"].as_bool().unwrap_or(false);
        match (a_own, b_own) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                let ao = a["overall_score"].as_f64().unwrap_or(0.0);
                let bo = b["overall_score"].as_f64().unwrap_or(0.0);
                bo.partial_cmp(&ao).unwrap_or(std::cmp::Ordering::Equal)
            }
        }
    });
    let own_flagged = rows.iter().any(|r| r["is_own"].as_bool().unwrap_or(false));
    Ok(Json(json!({
        "rows": rows,
        "own_channel": own,
        "own_flagged": own_flagged,
        "kg_built": !centrality_map.is_empty(),
    })))
}

/// GET /api/audit — VidIQ-style Channel Audit for every stored channel
/// (composite 0-100 + per-component breakdown).
async fn audit_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let audits = crate::analytics::audit::audit_all(&st.db)
        .await
        .map_err(api_err)?;
    Ok(Json(serde_json::to_value(audits).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("serialize: {e}") })),
        )
    })?))
}

/// GET /api/audit/:id — Channel Audit for one channel.
async fn audit_channel_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let channel = st
        .db
        .get_channel(&id)
        .await
        .map_err(api_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "channel not found"})),
            )
        })?;
    let videos = st.db.all_videos().await.map_err(api_err)?;
    let audit = crate::analytics::audit::audit_channel(&st.db, &videos, &id, &channel.title)
        .await
        .map_err(api_err)?;
    Ok(Json(serde_json::to_value(audit).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("serialize: {e}") })),
        )
    })?))
}

/// GET /api/channels/:id/snapshots — channel growth history
/// (subscribers/videos/views per refresh, migration 007).
async fn channel_snapshots_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let channel = st
        .db
        .get_channel(&id)
        .await
        .map_err(api_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "channel not found"})),
            )
        })?;
    let rows = st.db.channel_snapshots(&id).await.map_err(api_err)?;
    let points: Vec<Value> = rows
        .into_iter()
        .map(|(at, subs, vids, views)| {
            json!({
                "at": at,
                "subscriber_count": subs,
                "video_count": vids,
                "total_views": views,
            })
        })
        .collect();
    Ok(Json(json!({
        "channel_id": id,
        "channel_name": channel.title,
        "snapshots": points,
    })))
}

/// Channel-id → title map for the gap report (from stored channels).
async fn gap_channel_names(db: &Db) -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::new();
    if let Ok(channels) = db.all_channels().await {
        for c in channels {
            names.insert(c.channel_id, c.title);
        }
    }
    names
}

/// GET /api/gaps — full competitor gap report (Phase 6.5).
///
/// Enhanced with `graph_gaps` when the Knowledge Graph is available.
/// Graph gaps are topics with high demand but low supply, detected via
/// community analysis. The field is `null` when KG is not built.
async fn gaps_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let videos = st.db.all_videos().await.map_err(api_err)?;
    if videos.is_empty() {
        return Ok(Json(json!({
            "outliers": [], "topics": [], "freshness_gaps": [], "format_gaps": [],
            "note": "no videos in database — run `tubeforge ingest` first",
        })));
    }
    let names = gap_channel_names(&st.db).await;
    let report = crate::analytics::gaps::report(&videos, &names)
        .await
        .map_err(api_err)?;

    let outliers: Vec<Value> = report
        .outliers
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
    let topics: Vec<Value> = report
        .topics
        .iter()
        .map(|t| {
            json!({
                "topic": t.topic,
                "videos": t.videos,
                "channels": t.channels,
                "mean_views": t.mean_views,
                "newest_at": t.newest_at,
                "no_short": t.no_short,
                "is_series": t.is_series,
                "score": t.score,
                "covering_channels": t.covering_channels,
            })
        })
        .collect();

    // Compute graph-based gaps if KG is available
    let graph_gaps = compute_graph_gaps(&st).await;

    Ok(Json(json!({
        "outliers": outliers,
        "topics": topics,
        "freshness_gaps": report.freshness_gaps,
        "format_gaps": report.format_gaps,
        "graph_gaps": graph_gaps,
    })))
}

/// Compute graph-based content gaps (internal KG enhancement).
///
/// Returns `Value::Null` when KG is not available.
async fn compute_graph_gaps(st: &AppState) -> Value {
    let kg = match super::get_kg(st).await {
        Some(kg) => kg,
        None => return Value::Null,
    };
    let own_channel = st.own_channel.as_deref();
    let gaps = crate::analytics::graph_aware::find_content_gaps(&kg, own_channel);
    let gaps_json: Vec<Value> = gaps
        .into_iter()
        .take(10)
        .map(|(topic_id, score)| {
            let display_name = kg
                .get_entity(&topic_id)
                .map(|e| e.display_name.clone())
                .unwrap_or_else(|| topic_id.clone());
            serde_json::json!({
                "topic": display_name,
                "topic_id": topic_id,
                "opportunity_score": score,
            })
        })
        .collect();
    Value::Array(gaps_json)
}

/// GET /api/gaps/outliers — videos ≥3× their channel's mean views.
async fn gaps_outliers_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let videos = st.db.all_videos().await.map_err(api_err)?;
    let names = gap_channel_names(&st.db).await;
    let rows: Vec<Value> = crate::analytics::gaps::outliers(&videos, &names)
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
    Ok(Json(json!({ "outliers": rows, "total": rows.len() })))
}

/// GET /api/gaps/coverage — topic × channel coverage matrix.
async fn gaps_coverage_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let videos = st.db.all_videos().await.map_err(api_err)?;
    let rows: Vec<Value> = crate::analytics::gaps::coverage(&videos)
        .iter()
        .map(|t| {
            json!({
                "topic": t.topic,
                "videos": t.videos,
                "channels": t.channels,
                "mean_views": t.mean_views,
                "newest_at": t.newest_at,
                "no_short": t.no_short,
                "is_series": t.is_series,
                "score": t.score,
                "covering_channels": t.covering_channels,
            })
        })
        .collect();
    Ok(Json(json!({ "topics": rows, "total": rows.len() })))
}

/// GET /api/transcripts — inventory of stored transcripts.
async fn transcripts_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let rows = st.db.list_transcripts().await.map_err(api_err)?;
    let items: Vec<Value> = rows
        .iter()
        .map(|t| {
            json!({
                "video_id": t.video_id,
                "lang": t.lang,
                "source": t.source,
                "words": t.word_count,
                "fetched_at": t.fetched_at,
            })
        })
        .collect();
    Ok(Json(
        json!({ "transcripts": items, "total": items.len() }),
    ))
}

/// GET /api/transcripts/:id — one video's transcript text.
async fn transcript_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let Some(t) = st.db.get_transcript(&id).await.map_err(api_err)? else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "no transcript for video"})),
        ));
    };
    let title = st
        .db
        .get_video(&id)
        .await
        .map_err(api_err)?
        .map(|v| v.title)
        .unwrap_or_else(|| id.clone());
    Ok(Json(json!({
        "video_id": t.video_id,
        "title": title,
        "lang": t.lang,
        "source": t.source,
        "words": t.word_count,
        "fetched_at": t.fetched_at,
        "text": t.text,
    })))
}

/// GET /api/comments/:id — stored comments for one video (by like count).
async fn comments_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let rows = st.db.list_comments(&id).await.map_err(api_err)?;
    let items: Vec<Value> = rows
        .iter()
        .map(|c| {
            json!({
                "comment_id": c.comment_id,
                "author": c.author,
                "text": c.text,
                "likes": c.like_count,
                "published_at": c.published_at,
            })
        })
        .collect();
    Ok(Json(
        json!({ "video_id": id, "comments": items, "total": items.len() }),
    ))
}

/// GET /api/health — full health report as JSON.
async fn health_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let stale = super::stale_days();
    let h = reports::health(&st.db, stale).await.map_err(api_err)?;
    Ok(Json(h))
}

/// GET /api/tags — tag cloud with counts and trends.
async fn tags_cloud_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    let cloud = tags::tag_cloud(&st.db).await.map_err(api_err)?;
    Ok(Json(serde_json::to_value(cloud).unwrap()))
}

/// GET /api/tags/gaps — competitor tag gap analysis.
///
/// Enhanced with `tag_authority` scores when the Knowledge Graph is available.
/// Each tag gap includes a `tag_authority` field (0-100) representing the
/// mean centrality of channels using that tag. `null` when KG not built.
async fn tags_gaps_api(
    State(st): State<AppState>,
) -> Result<Json, (StatusCode, Json)> {
    // Get own channel ID from config or first owned channel
    let own_channel_id = get_own_channel_id(&st.db).await.map_err(api_err)?;
    let gaps = tags::tag_gaps(&st.db, &own_channel_id)
        .await
        .map_err(api_err)?;

    // Enhance with tag authority from KG if available
    let enhanced = enhance_tag_gaps_with_authority(&st, gaps).await;
    Ok(Json(enhanced))
}

/// Enhance tag gaps with authority scores from the Knowledge Graph.
async fn enhance_tag_gaps_with_authority(st: &AppState, gaps: Vec<tags::TagGap>) -> Value {
    let kg = super::get_kg(st).await;
    let enhanced: Vec<Value> = gaps
        .into_iter()
        .map(|gap| {
            let authority = kg.as_ref().map(|kg| {
                crate::analytics::graph_aware::compute_tag_authority_by_name(kg, &gap.tag)
            });
            serde_json::json!({
                "tag": gap.tag,
                "competitor_usage": gap.competitor_usage,
                "your_usage": gap.your_usage,
                "opportunity_score": gap.opportunity_score,
                "tag_authority": authority,
            })
        })
        .collect();
    serde_json::json!({
        "gaps": enhanced,
        "kg_built": kg.is_some(),
    })
}

/// GET /api/tags/video/:id — tags for a specific video.
async fn video_tags_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let vt = tags::video_tags(&st.db, &id).await.map_err(api_err)?;
    Ok(Json(serde_json::to_value(vt).unwrap()))
}

/// GET /api/tags/competitor/:id — competitor tag stats.
async fn competitor_tags_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let ct = tags::competitor_tags(&st.db, &id).await.map_err(api_err)?;
    Ok(Json(serde_json::to_value(ct).unwrap()))
}

/// Helper: get the owner's channel ID (first channel in DB, or from config).
async fn get_own_channel_id(db: &Db) -> Result<String, TubeforgeError> {
    let channels = db.all_channels().await?;
    channels
        .first()
        .map(|c| c.channel_id.clone())
        .ok_or_else(|| TubeforgeError::Storage {
            code: "NO_CHANNELS".to_string(),
            message: "no channels in database".to_string(),
        })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a `TubeforgeError` into a `(StatusCode, Json)` error envelope.
fn api_err(e: crate::error::TubeforgeError) -> (StatusCode, Json) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": e.to_string()})),
    )
}

// ---------------------------------------------------------------------------
// Knowledge Graph lazy loading (internal-only — no separate API endpoints)
// ---------------------------------------------------------------------------

/// Parse a `field:asc` or `field:desc` sort parameter.
/// Returns `(field, is_ascending)` or `None` when empty.
fn parse_sort(sort: &str) -> Option<(String, bool)> {
    let sort = sort.trim();
    if sort.is_empty() {
        return None;
    }
    if let Some((field, dir)) = sort.split_once(':') {
        let field = field.trim().to_string();
        let asc = !dir.trim().eq_ignore_ascii_case("desc");
        Some((field, asc))
    } else {
        Some((sort.to_string(), true))
    }
}

/// Total-ordering comparison for `f64` values (treats NaN as less-than).
fn compare_f64(a: f64, b: f64) -> std::cmp::Ordering {
    a.total_cmp(&b)
}

/// Total-ordering comparison for `Option<i64>` values (None sorts last
/// in descending, first in ascending — callers handle direction).
fn compare_i64(a: Option<i64>, b: Option<i64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}
