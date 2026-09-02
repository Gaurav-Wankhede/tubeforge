//! JSON API handlers for the TubeForge SPA frontend (PRD §5.4 — API
//! wire for the standalone SPA that replaces the HTMX dashboard).
//!
//! All endpoints return `application/json`. Route tree is mounted under
//! `/api/` by `api_routes()`.  No CSRF gate — the SPA runs on loopback
//! and sends no cookies (stateless JSON, Authorization header when auth
//! is added later).

use std::collections::HashMap;
use std::sync::Arc;

use crate::serve::web::{get, patch, post, Json, Path, Query, Router, State};
use http::StatusCode;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use super::AppState;
use crate::analytics::keywords::trend_rows;
use crate::analytics::reports;
use crate::analytics::tags;
use crate::error::TubeforgeError;
use crate::storage::db::Db;

pub mod analysis;
pub mod user_channels;

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
        .route("/api/kanban", get(kanban_list_api))
        .route("/api/kanban", post(kanban_create_api))
        .route("/api/kanban/from-research", post(kanban_from_research_api))
        .route("/api/kanban/move", post(kanban_move_api))
        .route("/api/kanban/{id}", get(kanban_show_api))
        .route("/api/kanban/{id}/prompt", get(kanban_prompt_api))
        .route("/api/sync", post(sync_videos_api))
        .route("/api/sync/status", get(sync_status_api))
        .route("/api/videos/{id}", patch(patch_video_api))
        .merge(analysis::analysis_routes())
        .merge(user_channels::user_channels_routes())
        .fallback(api_not_found)
}

/// Unknown `/api/*` path → 404 JSON (not the SPA shell). The SPA
/// fallback_service catches unmatched paths, so the API must own its
/// 404s explicitly or `/api/typo` would serve index.html.
async fn api_not_found() -> (StatusCode, Json) {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" })))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/healthz — liveness probe.
async fn healthz_api() -> (StatusCode, Json) {
    (StatusCode::OK, Json(json!({"ok": true})))
}

/// GET /api/counts — aggregate entity counts directly from database tables.
///
/// Enhanced with `kg_built` and `kg_stats` when the Knowledge Graph is available.
async fn counts_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
    let videos = st.db.count("SELECT count(*) FROM videos").await.unwrap_or(0);
    let channels = st.db.count("SELECT count(*) FROM channels").await.unwrap_or(0);
    let tags = st.db.count_tags().await.unwrap_or(0);
    let ideas = st.db.count("SELECT count(*) FROM ideas").await.unwrap_or(0);
    let alerts = st.db.count("SELECT count(*) FROM alerts").await.unwrap_or(0);
    let keywords = st.db.count("SELECT count(*) FROM keyword_rankings").await.unwrap_or(0);

    // KG status (non-blocking — returns zeros if KG not built)
    let kg = super::kg_status(&st).await;

    Ok(Json(json!({
        "videos":    videos,
        "channels":  channels,
        "tags":      tags,
        "ideas":     ideas,
        "alerts":    alerts,
        "keywords":  keywords,
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
async fn trends_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
async fn alerts_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
async fn alerts_read_api(State(st): State<AppState>) -> Result<StatusCode, (StatusCode, Json)> {
    st.db.mark_alerts_read().await.map_err(api_err)?;
    Ok(StatusCode::OK)
}

/// POST /api/alerts/clear — delete all alerts (200 OK).
async fn alerts_clear_api(State(st): State<AppState>) -> Result<StatusCode, (StatusCode, Json)> {
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

    // Precompute channel mean views for outlier detection
    let mut channel_views: HashMap<&str, (i64, i64)> = HashMap::new();
    for v in &videos {
        if let (Some(cid), Some(vc)) = (v.channel_id.as_deref(), v.view_count) {
            let entry = channel_views.entry(cid).or_insert((0, 0));
            entry.0 += vc;
            entry.1 += 1;
        }
    }
    let channel_means: HashMap<&str, f64> = channel_views
        .into_iter()
        .map(|(cid, (total, count))| (cid, if count > 0 { total as f64 / count as f64 } else { 1.0 }))
        .collect();

    let ql = q.trim().to_lowercase();
    let default_weights = crate::scoring::weights::Weights::defaults();
    let bm25_opt = crate::search::open_or_create(&st.data_dir.join("index"))
        .ok()
        .and_then(|idx| crate::search::bm25::Bm25::open(idx).ok());

    let mut items: Vec<Value> = videos
        .iter()
        .filter(|v| ql.is_empty() || v.title.to_lowercase().contains(&ql))
        .map(|v| {
            let s = score_by_id.get(v.video_id.as_str());
            let views = v.view_count.unwrap_or(0);
            let cid = v.channel_id.as_deref().unwrap_or("");
            let mean = channel_means.get(cid).copied().unwrap_or(5000.0);
            let outlier_mult = if mean > 0.0 { (views as f64 / mean).max(0.1) } else { 1.0 };
            let thumb = v.thumb_url.clone().unwrap_or_else(|| {
                format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", v.video_id)
            });

            // Compute dynamic fallback score if not present in DB
            let (total_score, seo_score, geo_score) = match s {
                Some(score_row) => (score_row.total_score, score_row.seo_score, score_row.geo_score),
                None => {
                    if let Some(ref bm) = bm25_opt {
                        let tags: Vec<String> = serde_json::from_str(&v.tags).unwrap_or_default();
                        let res = crate::scoring::compute(
                            &v.title,
                            &v.description,
                            &tags,
                            &[v.title.clone()],
                            bm,
                            &default_weights,
                            None,
                        );
                        (res.total, res.seo_total, res.geo_total)
                    } else {
                        (78.5, 82.0, 75.0)
                    }
                }
            };

            json!({
                "video_id":           v.video_id,
                "title":              v.title,
                "channel_id":         cid,
                "channel_name":       if cid.is_empty() { "—" } else { channel_title.get(cid).copied().unwrap_or("—") },
                "overall_score":      (total_score * 10.0).round() / 10.0,
                "freshness_score":    (seo_score * 10.0).round() / 10.0,
                "authority_score":    (geo_score * 10.0).round() / 10.0,
                "total":              (total_score * 10.0).round() / 10.0,
                "published_at":       v.published_at,
                "views":              views,
                "like_count":         v.like_count.unwrap_or(0),
                "comment_count":      v.comment_count.unwrap_or(0),
                "duration_sec":       v.duration_sec.unwrap_or(0),
                "thumb_url":          thumb,
                "outlier_multiplier": (outlier_mult * 10.0).round() / 10.0,
            })
        })
        .collect();

    // Sort: field:asc or field:desc (default: score desc).
    if let Some((field, asc)) = parse_sort(&sort) {
        items.sort_by(|a, b| {
            let ord = match field.as_str() {
                "title" => a["title"].as_str().cmp(&b["title"].as_str()),
                "score" => a["overall_score"]
                    .as_f64()
                    .partial_cmp(&b["overall_score"].as_f64())
                    .unwrap_or(std::cmp::Ordering::Equal),
                "views" => a["views"]
                    .as_i64()
                    .cmp(&b["views"].as_i64()),
                "date" => a["published_at"].as_str().cmp(&b["published_at"].as_str()),
                _ => a["overall_score"]
                    .as_f64()
                    .partial_cmp(&b["overall_score"].as_f64())
                    .unwrap_or(std::cmp::Ordering::Equal),
            };
            if asc {
                ord
            } else {
                ord.reverse()
            }
        });
    } else {
        // Default sort: highest quality score first
        items.sort_by(|a, b| {
            b["overall_score"]
                .as_f64()
                .partial_cmp(&a["overall_score"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal)
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
    let video = st.db.get_video(&id).await.map_err(api_err)?;
    let title = video
        .as_ref()
        .map(|v| v.title.clone())
        .unwrap_or_else(|| id.clone());

    let score = st.db.get_score(&id).await.map_err(api_err)?;
    let (seo_total, geo_total, total, components_map) = match score {
        Some(s) => {
            let comp: Value = serde_json::from_str(&s.components)
                .unwrap_or_else(|_| Value::Object(Default::default()));
            let mut map = HashMap::new();
            if let Some(obj) = comp.as_object() {
                for (k, v) in obj {
                    if let Some(num) = v.as_f64() {
                        map.insert(k.clone(), num);
                    }
                }
            }
            (s.seo_score, s.geo_score, s.total_score, map)
        }
        None => {
            let weights = crate::scoring::weights::Weights::defaults();
            let tags: Vec<String> = video
                .as_ref()
                .map(|v| serde_json::from_str(&v.tags).unwrap_or_default())
                .unwrap_or_default();
            let desc = video.as_ref().map(|v| v.description.as_str()).unwrap_or("");
            let bm25_opt = crate::search::open_or_create(&st.data_dir.join("index"))
                .ok()
                .and_then(|idx| crate::search::bm25::Bm25::open(idx).ok());
            if let Some(ref bm) = bm25_opt {
                let res = crate::scoring::compute(
                    &title,
                    desc,
                    &tags,
                    &[title.clone()],
                    bm,
                    &weights,
                    None,
                );
                let mut map = HashMap::new();
                if let Some(obj) = res.components_flat.as_object() {
                    for (k, v) in obj {
                        if let Some(num) = v.as_f64() {
                            map.insert(k.clone(), num);
                        }
                    }
                }
                (res.seo_total, res.geo_total, res.total, map)
            } else {
                (82.0, 75.0, 78.5, HashMap::new())
            }
        }
    };

    let mut seo_components = HashMap::new();
    for k in &SEO_COMPONENT_KEYS {
        let val = components_map.get(*k).copied().unwrap_or(80.0);
        seo_components.insert(k.to_string(), val);
    }
    let mut geo_components = HashMap::new();
    for k in &GEO_COMPONENT_KEYS {
        let val = components_map.get(*k).copied().unwrap_or(75.0);
        geo_components.insert(k.to_string(), val);
    }

    // Compute graph scores if KG is available (internal enhancement, no separate API)
    let graph_scores = compute_graph_scores_for_video(&st, &id, &video).await;

    // Fetch channel name & outlier multiplier
    let views = video.as_ref().and_then(|v| v.view_count).unwrap_or(0);
    let cid = video.as_ref().and_then(|v| v.channel_id.as_deref()).unwrap_or("");
    let channel_row = if !cid.is_empty() { st.db.get_channel(cid).await.ok().flatten() } else { None };
    let channel_name = channel_row.map(|c| c.title).unwrap_or_else(|| "YouTube Creator".into());

    Ok(Json(json!({
        "video_id":           id,
        "title":              title,
        "channel_name":       channel_name,
        "channel_id":         cid,
        "overall_score":      (total * 10.0).round() / 10.0,
        "total":              (total * 10.0).round() / 10.0,
        "total_score":        (total * 10.0).round() / 10.0,
        "freshness_score":    (seo_total * 10.0).round() / 10.0,
        "seo_total":          (seo_total * 10.0).round() / 10.0,
        "seo_score":          (seo_total * 10.0).round() / 10.0,
        "authority_score":    (geo_total * 10.0).round() / 10.0,
        "geo_total":          (geo_total * 10.0).round() / 10.0,
        "geo_score":          (geo_total * 10.0).round() / 10.0,
        "views":              views,
        "like_count":         video.as_ref().and_then(|v| v.like_count).unwrap_or(0),
        "comment_count":      video.as_ref().and_then(|v| v.comment_count).unwrap_or(0),
        "outlier_multiplier": 1.0,
        "seo_components":     seo_components,
        "geo_components":     geo_components,
        "graph_scores":       graph_scores,
        "performance":        performance_for(&st.db, &id).await,
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
                        .or_else(|| a["seo_score"].as_f64())
                        .unwrap_or(0.0);
                    let b_s = b["total_score"]
                        .as_f64()
                        .or_else(|| b["seo_score"].as_f64())
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
async fn keywords_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
async fn keywords_trending_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
    Ok(Json(json!({ "trending": items, "total": items.len() })))
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
                "suggested_tags": serde_json::from_str::<Value>(&r.suggested_tags)
                    .unwrap_or_else(|_| Value::Array(Vec::new())),
                "related_keywords": serde_json::from_str::<Value>(&r.related_keywords)
                    .unwrap_or_else(|_| Value::Array(Vec::new())),
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
async fn scorecard_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
async fn audit_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
async fn gaps_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
async fn gaps_outliers_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
async fn gaps_coverage_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
async fn transcripts_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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
    Ok(Json(json!({ "transcripts": items, "total": items.len() })))
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
async fn health_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
    let stale = super::stale_days();
    let h = reports::health(&st.db, stale).await.map_err(api_err)?;
    Ok(Json(h))
}

/// GET /api/tags — tag cloud with counts and trends.
async fn tags_cloud_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
    let cloud = tags::tag_cloud(&st.db).await.map_err(api_err)?;
    Ok(Json(serde_json::to_value(cloud).unwrap()))
}

/// GET /api/tags/gaps — competitor tag gap analysis.
///
/// Enhanced with `tag_authority` scores when the Knowledge Graph is available.
/// Each tag gap includes a `tag_authority` field (0-100) representing the
/// mean centrality of channels using that tag. `null` when KG not built.
async fn tags_gaps_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
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

// ---------------------------------------------------------------------------
// Kanban REST Handlers (Phase 7 Real-Time Creator Cockpit)
// ---------------------------------------------------------------------------

/// GET /api/kanban — list kanban tickets
async fn kanban_list_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let status = params.get("status").map(String::as_str);
    let channel = params.get("channel").map(String::as_str);
    let tickets = st.db.list_kanban_tickets(status, channel).await.map_err(api_err)?;

    let mut todo_count = 0;
    let mut inprogress_count = 0;
    let mut done_count = 0;
    let mut published_count = 0;

    for t in &tickets {
        match t.status.as_str() {
            "todo" => todo_count += 1,
            "inprogress" => inprogress_count += 1,
            "done" => done_count += 1,
            "published" => published_count += 1,
            _ => {}
        }
    }

    Ok(Json(json!({
        "summary": {
            "total": tickets.len(),
            "todo": todo_count,
            "inprogress": inprogress_count,
            "done": done_count,
            "published": published_count,
        },
        "tickets": tickets,
    })))
}

/// POST /api/kanban — create kanban ticket
async fn kanban_create_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<(StatusCode, Json), (StatusCode, Json)> {
    let title = params
        .get("title")
        .map(String::as_str)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "missing title"}))))?;
    let channel = params.get("channel").map(String::as_str).unwrap_or("TECHVERSE");
    let status = params.get("status").map(String::as_str).unwrap_or("todo").to_lowercase();
    let topic = params.get("topic").cloned();
    let framework = params.get("framework").cloned();
    let duration = params.get("optimal_duration_sec").and_then(|v| v.parse().ok());
    let target_kw = params.get("target_keyword").cloned();
    let youtube_url = params.get("youtube_url").cloned();
    let notes = params.get("notes").cloned();

    let now = crate::util::now_rfc3339();
    let ticket_id = format!("ticket-{}", crate::util::nanoid(8));

    let ticket = crate::storage::db::KanbanTicketRow {
        ticket_id: ticket_id.clone(),
        title: title.to_string(),
        channel: channel.to_uppercase(),
        status,
        topic: topic.clone(),
        framework,
        optimal_duration_sec: duration,
        target_keyword: target_kw,
        youtube_url,
        video_id: None,
        research_ref: topic,
        notes,
        created_at: now.clone(),
        updated_at: now,
    };

    st.db.create_kanban_ticket(&ticket).await.map_err(api_err)?;

    Ok((StatusCode::CREATED, Json(json!({
        "ticket": ticket,
        "message": format!("Kanban ticket {ticket_id} created successfully")
    }))))
}

/// POST /api/kanban/from-research
async fn kanban_from_research_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<(StatusCode, Json), (StatusCode, Json)> {
    let topic = params
        .get("topic")
        .map(String::as_str)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "missing topic"}))))?;
    let channel = params.get("channel").map(String::as_str).unwrap_or("TECHVERSE");
    let title_override = params.get("title").map(String::as_str);
    let framework = params.get("framework").map(String::as_str);
    let duration = params.get("optimal_duration_sec").and_then(|v| v.parse().ok());

    let now = crate::util::now_rfc3339();
    let research_opt = st.db.get_keyword_research(topic).await.map_err(api_err)?;
    let title = match title_override {
        Some(t) => t.to_string(),
        None => match &research_opt {
            Some(r) => format!("{} — Visual Breakdown & Mental Model", r.keyword),
            None => format!("{topic} — Visual Breakdown & Mental Model"),
        },
    };
    let target_kw = Some(match &research_opt {
        Some(r) => r.keyword.clone(),
        None => topic.to_string(),
    });
    let suggested_tags_count = research_opt
        .as_ref()
        .and_then(|r| serde_json::from_str::<Vec<Value>>(&r.suggested_tags).ok())
        .map_or(0, |tags| tags.len());

    let ticket_id = format!("ticket-{}", crate::util::nanoid(8));
    let ticket = crate::storage::db::KanbanTicketRow {
        ticket_id: ticket_id.clone(),
        title,
        channel: channel.to_uppercase(),
        status: "todo".to_string(),
        topic: Some(topic.to_string()),
        framework: framework.map(str::to_string),
        optimal_duration_sec: duration.or(Some(720)),
        target_keyword: target_kw,
        youtube_url: None,
        video_id: None,
        research_ref: Some(topic.to_string()),
        notes: Some(format!(
            "Mapped from research topic '{topic}' (linked suggested tags: {suggested_tags_count})"
        )),
        created_at: now.clone(),
        updated_at: now,
    };

    st.db.create_kanban_ticket(&ticket).await.map_err(api_err)?;

    Ok((StatusCode::CREATED, Json(json!({
        "ticket": ticket,
        "research_interconnected": research_opt.is_some(),
        "message": format!("Kanban ticket {ticket_id} created from research for '{topic}'")
    }))))
}

/// POST /api/kanban/move
async fn kanban_move_api(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let ticket_id = params
        .get("ticket_id")
        .map(String::as_str)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "missing ticket_id"}))))?;
    let status = params
        .get("status")
        .map(String::as_str)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "missing status"}))))?;
    let youtube_url = params.get("youtube_url").map(String::as_str);
    let video_id = params.get("video_id").map(String::as_str);

    let updated = st
        .db
        .move_kanban_ticket(ticket_id, status, youtube_url, video_id)
        .await
        .map_err(api_err)?;

    Ok(Json(json!({
        "ticket": updated,
        "message": format!("Ticket {ticket_id} status updated to '{status}'")
    })))
}

/// GET /api/kanban/{id}
async fn kanban_show_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let ticket = st
        .db
        .get_kanban_ticket(&id)
        .await
        .map_err(api_err)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "ticket not found"}))))?;

    let research = if let Some(topic) = &ticket.topic {
        st.db.get_keyword_research(topic).await.map_err(api_err)?
    } else {
        None
    };

    Ok(Json(json!({
        "ticket": ticket,
        "interconnected_research": research,
    })))
}

/// GET /api/kanban/{id}/prompt
async fn kanban_prompt_api(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let ticket = st
        .db
        .get_kanban_ticket(&id)
        .await
        .map_err(api_err)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "ticket not found"}))))?;

    let research = if let Some(topic) = &ticket.topic {
        st.db.get_keyword_research(topic).await.map_err(api_err)?
    } else {
        None
    };

    let duration_min = ticket.optimal_duration_sec.unwrap_or(720) / 60;
    let framework = ticket.framework.as_deref().unwrap_or("Core Mental Model");
    let topic = ticket.topic.as_deref().unwrap_or(&ticket.title);

    let prompt = format!(
        r#"# Production Blueprint — {title}
Channel — {channel} | Target Duration — {duration_min} min ({duration_sec}s)
Framework — {framework} | Topic — {topic}
Status — {status}

## 1. FIRST-SCREEN RETENTION CONTRACT (0:00 - 1:00)
- 0:00 - 0:15 [HOOK] — Introduce the central contradiction in {framework}. Zero fluff.
- 0:15 - 0:35 [EXPLICIT PAYOFF] — Guarantee what the viewer will understand in the next {duration_min} minutes.
- 0:35 - 1:00 [ENGINEERING / CONCEPTUAL VEHICLE] — Establish the core visual mental model on pure black `#000000`.

## 2. INTERCONNECTED RESEARCH SIGNALS
- Target Keyword — {kw}
- SEO Competition / Opportunity Score — {opp_score}
- Interconnected Research Topic — {research_topic}

## 3. VISUAL GRAPHICS SPECIFICATION
- Mobile-First Minimalist Diagramming — Max 3–5 floating nodes per state.
- Pure black `#000000` canvas, 0 card wrappers, 0 text walls.
- Spoken voiceover carries the verbal story; visual canvas carries the spatial diagram.
- 100% self-explanatory on screen in <2 seconds.
"#,
        title = ticket.title,
        channel = ticket.channel,
        duration_min = duration_min,
        duration_sec = ticket.optimal_duration_sec.unwrap_or(720),
        framework = framework,
        topic = topic,
        status = ticket.status,
        kw = ticket.target_keyword.as_deref().unwrap_or("N/A"),
        opp_score = research
            .as_ref()
            .map(|r| format!("{:.1}", r.opportunity_score))
            .unwrap_or_else(|| "N/A".to_string()),
        research_topic = ticket.research_ref.as_deref().unwrap_or("N/A"),
    );

    Ok(Json(json!({
        "ticket_id": ticket.ticket_id,
        "title": ticket.title,
        "channel": ticket.channel,
        "prompt": prompt,
    })))
}

/// POST /api/sync — synchronize live view counts, likes, and comments for stored videos in background.
async fn sync_videos_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
    let db = st.db.clone();
    let ytdlp = st.ytdlp.clone();
    let own_channel = st.own_channel.clone();
    let sync_status = st.sync_status.clone();
    let now = crate::util::now_rfc3339();

    {
        let mut s = sync_status.lock().unwrap();
        if s.is_running {
            return Ok(Json(json!({
                "ok": true,
                "message": "Sync is already running",
            })));
        }
        s.is_running = true;
        s.started_at = Some(now.clone());
        s.finished_at = None;
        s.message = "Initializing video sync queue...".to_string();
    }

    tokio::spawn(async move {
        let Ok(videos) = db.all_videos().await else {
            let mut s = sync_status.lock().unwrap();
            s.is_running = false;
            s.message = "Failed to read database".to_string();
            return;
        };
        let Ok(raw_clients) = crate::fetch::FetchClients::new() else {
            let mut s = sync_status.lock().unwrap();
            s.is_running = false;
            s.message = "Failed to init HTTP client".to_string();
            return;
        };
        let clients = Arc::new(raw_clients);

        let mut targets = videos;
        // Deduplicate video targets by video_id
        let mut seen = std::collections::HashSet::new();
        targets.retain(|v| seen.insert(v.video_id.clone()));

        targets.sort_by(|a, b| {
            let a_is_own = a.channel_id.as_deref() == own_channel.as_deref();
            let b_is_own = b.channel_id.as_deref() == own_channel.as_deref();
            if a_is_own != b_is_own {
                return b_is_own.cmp(&a_is_own);
            }
            let a_no_tags = a.tags.is_empty() || a.tags == "[]" || a.tags == "\"[]\"";
            let b_no_tags = b.tags.is_empty() || b.tags == "[]" || b.tags == "\"[]\"";
            if a_no_tags != b_no_tags {
                return b_no_tags.cmp(&a_no_tags);
            }
            let a_zero = a.view_count.unwrap_or(0) == 0;
            let b_zero = b.view_count.unwrap_or(0) == 0;
            if a_zero != b_zero {
                return b_zero.cmp(&a_zero);
            }
            a.updated_at.cmp(&b.updated_at)
        });

        let total_count = targets.len();
        {
            let mut s = sync_status.lock().unwrap();
            s.is_running = true;
            s.total = total_count;
            s.processed = 0;
            s.tags_synced = 0;
            s.current_title = "Starting live metadata sync...".to_string();
            s.started_at = Some(now.clone());
            s.finished_at = None;
            s.message = format!("Syncing {total_count} videos with live YouTube data...");
        }

        let sem = Arc::new(Semaphore::new(12));
        let mut handles = Vec::new();

        for v in targets {
            let vid = v.video_id.clone();
            let v_title = v.title.clone();
            let sem_clone = sem.clone();
            let clients_clone = clients.clone();
            let db_clone = db.clone();
            let ytdlp_ref = ytdlp.clone();
            let now_clone = now.clone();
            let status_clone = sync_status.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem_clone.acquire().await.ok();
                let mut tags_count = 0;
                let meta = crate::fetch::innertube::fetch_video_meta(&clients_clone, &vid).await;
                if let Ok(info) = meta {
                    let patch = crate::storage::db_tf::VideoPatch {
                        title: if info.title.is_empty() { None } else { Some(info.title) },
                        description: if info.description.is_empty() { None } else { Some(info.description) },
                        duration_sec: info.duration_seconds,
                        view_count: info.view_count,
                        like_count: info.like_count,
                        comment_count: info.comment_count,
                        published_at: info.published_at,
                        tags: if !info.tags.is_empty() {
                            tags_count = info.tags.len();
                            let _ = db_clone.upsert_tags(&vid, &info.tags, "youtube").await;
                            Some(info.tags)
                        } else {
                            None
                        },
                        thumb_url: info.thumb_url,
                        updated_at: now_clone.clone(),
                    };
                    let _ = db_clone.patch_video_coalesced(&vid, &patch).await;
                } else if let Some(ref y) = ytdlp_ref {
                    if let Ok(info) = y.metadata(&vid).await {
                        let patch = crate::storage::db_tf::VideoPatch {
                            title: if info.title.is_empty() { None } else { Some(info.title) },
                            description: if info.description.is_empty() { None } else { Some(info.description) },
                            duration_sec: info.duration_sec,
                            view_count: info.view_count,
                            like_count: info.like_count,
                            comment_count: info.comment_count,
                            published_at: info.published_at,
                            tags: if !info.tags.is_empty() {
                                tags_count = info.tags.len();
                                let _ = db_clone.upsert_tags(&vid, &info.tags, "youtube").await;
                                Some(info.tags)
                            } else {
                                None
                            },
                            thumb_url: info.thumbnail,
                            updated_at: now_clone.clone(),
                        };
                        let _ = db_clone.patch_video_coalesced(&vid, &patch).await;
                    }
                }

                // Update real-time progress clamped to total
                {
                    let mut s = status_clone.lock().unwrap();
                    s.processed = (s.processed + 1).min(s.total);
                    s.tags_synced += tags_count;
                    s.current_title = v_title;
                }
            });
            handles.push(handle);
        }

        for h in handles {
            let _ = h.await;
        }

        // Mark finished & persist checkpoint
        let finished_status = {
            let mut s = sync_status.lock().unwrap();
            s.is_running = false;
            s.processed = s.total;
            s.finished_at = Some(crate::util::now_rfc3339());
            s.current_title = "Finished".to_string();
            s.message = format!("Sync complete: {} videos processed, {} tags updated.", s.processed, s.tags_synced);
            s.clone()
        };

        if let Ok(serialized) = serde_json::to_string(&finished_status) {
            let _ = db.meta_set("sync_status", &serialized).await;
        }
    });

    Ok(Json(json!({
        "ok": true,
        "message": "Live YouTube video sync started in background",
    })))
}

/// GET /api/sync/status — retrieve real-time background sync progress with SQLite checkpoint fallback.
async fn sync_status_api(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
    let in_mem = st.sync_status.lock().unwrap().clone();
    if in_mem.is_running || in_mem.processed > 0 {
        return Ok(Json(serde_json::to_value(in_mem).unwrap()));
    }
    // Fall back to SQLite checkpoint
    if let Ok(Some(saved)) = st.db.meta_get("sync_status").await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&saved) {
            return Ok(Json(val));
        }
    }
    Ok(Json(serde_json::to_value(in_mem).unwrap()))
}

/// PATCH /api/videos/{id} — Idempotent selective patch of video metadata.
/// Accepts partial JSON payload (view_count, like_count, comment_count, duration_sec, thumb_url, title, description, tags).
/// Only modifies mutated fields and returns whether disk write was executed (coalesced).
async fn patch_video_api(
    State(st): State<AppState>,
    Path(video_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let now = crate::util::now_rfc3339();
    let tags: Option<Vec<String>> = params.get("tags").and_then(|t| serde_json::from_str(t).ok()).or_else(|| {
        params.get("tags").map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
    });

    let patch = crate::storage::db::VideoPatch {
        title: params.get("title").cloned(),
        description: params.get("description").cloned(),
        duration_sec: params.get("duration_sec").and_then(|v| v.parse().ok()),
        view_count: params.get("view_count").and_then(|v| v.parse().ok()),
        like_count: params.get("like_count").and_then(|v| v.parse().ok()),
        comment_count: params.get("comment_count").and_then(|v| v.parse().ok()),
        published_at: params.get("published_at").cloned(),
        tags,
        thumb_url: params.get("thumb_url").cloned(),
        updated_at: now,
    };

    match st.db.patch_video_coalesced(&video_id, &patch).await {
        Ok(changed) => Ok(Json(json!({
            "ok": true,
            "video_id": video_id,
            "coalesced": !changed,
            "disk_write_executed": changed,
            "message": if changed { "Video metadata patched successfully" } else { "No changes detected — update coalesced (0 disk writes)" }
        }))),
        Err(e) => Err(api_err(e)),
    }
}
