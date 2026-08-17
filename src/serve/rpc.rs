//! JSON-RPC 2.0 — the shared streaming analysis protocol.
//!
//! A single method surface is served over **two transports**:
//!   - WebSocket at `/ws` (dashboard/frontend, see `serve/web/ws.rs`)
//!   - **stdio** via `tubeforge rpc` (agent harnesses, see `serve/stdio.rs`)
//!
//! `dispatch()` is transport-agnostic: handlers push `RpcResponse`s into an
//! `UnboundedSender<String>` channel, and the transport task forwards each
//! serialized response to the client/agent.
//!
//! Message flow:
//!   Client → Server: {"id":"req-1","method":"ideas.analyze","params":{}}
//!   Server → Client: {"id":"req-1","type":"progress","progress":0.3,"message":"..."}
//!   Server → Client: {"id":"req-1","type":"result","data":{...}}
//!
//! Server can also push notifications:
//!   Server → Client: {"type":"notification","event":"ingest.completed","data":{...}}

use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::StreamExt;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::TubeforgeError;
use crate::serve::web::{self, Response, ServeState};
use crate::serve::AppState;
use crate::storage::db::{Db, VideoRow};

/// SEO component keys in canonical display order (matches `scoring::seo`).
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

/// GEO component keys in canonical display order (matches `scoring::geo`).
const GEO_COMPONENT_KEYS: [&str; 7] = [
    "entity_coverage",
    "qa_phrasing",
    "list_phrasing",
    "conversational",
    "metadata_complete",
    "location_signal",
    "topic_relevance",
];

/// Transport-agnostic RPC output channel. The WebSocket loop and the stdio
/// JSON-RPC bridge (`serve::stdio`) both feed this channel; the transport
/// task forwards each serialized `RpcResponse` to the client/agent.
pub(crate) type RpcSender = tokio::sync::mpsc::UnboundedSender<String>;

/// Client → Server request.
#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: HashMap<String, Value>,
}

/// Server → Client response envelope.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcResponse {
    Progress {
        id: Value,
        progress: f32,
        message: String,
    },
    Result {
        id: Value,
        data: Value,
    },
    Error {
        id: Value,
        error: RpcError,
    },
    Notification {
        event: String,
        data: Value,
    },
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

/// Handle a WebSocket upgrade — one client, many streaming RPC requests.
pub fn ws_handler(
    req: &mut http::Request<hyper::body::Incoming>,
    state: Arc<ServeState>,
) -> Option<Response> {
    let app_state: &AppState = state.get()?;
    let app_state = app_state.clone();
    web::ws::upgrade(req, move |socket| {
        tokio::spawn(handle_socket(app_state, socket));
    })
}

async fn handle_socket(state: AppState, socket: web::ws::WebSocket) {
    let (mut sink, mut receiver) = web::ws::split(socket);

    // RPC output is pushed into a channel; one forwarder task drains it into
    // the WebSocket sink. Keeps the handler dispatch transport-agnostic so the
    // same method surface also runs over stdio (`serve::stdio`).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let forwarder = tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if sink
                .send(web::ws::Message::Text(text.into()))
                .await
                .is_err()
            {
                break; // client disconnected
            }
        }
    });

    // Process messages sequentially per connection (preserves ordering).
    while let Some(Ok(msg)) = receiver.next().await {
        let web::ws::Message::Text(text) = msg else {
            continue;
        };

        let req: RpcRequest = match serde_json::from_str(text.as_str()) {
            Ok(r) => r,
            Err(e) => {
                let err = RpcResponse::Error {
                    id: Value::Null,
                    error: RpcError {
                        code: -32700,
                        message: format!("parse error: {e}"),
                    },
                };
                let _ = send(&tx, &err).await;
                continue;
            }
        };

        // Process sequentially with OWNED state/sender (clones) so the future
        // is 'static + Send for axum's on_upgrade. Some handlers (e.g.
        // analysis.refresh) do live yt-dlp work on a dedicated std thread.
        // Clone into named owned locals so no borrow of the loop-local `state`
        // is held across the dispatch await (which must be Send).
        let owned_state = state.clone();
        let owned_sender = tx.clone();
        dispatch(owned_state, owned_sender, req).await;
    }

    drop(tx); // end the forwarder once the client closes
    let _ = forwarder.await;
}

/// Serialize a response and push it into the transport channel. A closed
/// channel means the client/agent has disconnected — treated as non-fatal.
pub(crate) async fn send(sender: &RpcSender, res: &RpcResponse) -> Result<(), TubeforgeError> {
    let json = serde_json::to_string(res).map_err(|e| TubeforgeError::Storage {
        code: "RPC_SERIALIZE".into(),
        message: e.to_string(),
    })?;
    let _ = sender.send(json);
    Ok(())
}

async fn progress(sender: &RpcSender, id: &Value, pct: f32, msg: &str) {
    let res = RpcResponse::Progress {
        id: id.clone(),
        progress: pct,
        message: msg.to_string(),
    };
    let _ = send(sender, &res).await;
}

/// Route a request to the correct handler based on method name. Takes OWNED
/// state/sender (clones) so the returned future is 'static + Send for axum's
/// on_upgrade — handlers that await long operations (yt-dlp refresh) need this.
/// Shared by the WebSocket loop and the stdio JSON-RPC bridge.
pub(crate) async fn dispatch(state: AppState, sender: RpcSender, req: RpcRequest) {
    // Move the params out into a local owned by `dispatch`'s own future scope.
    // Passing `&req.params` (a borrow of the caller's loop-local `req`) across
    // the awaits makes the future non-Send for axum's `on_upgrade` — owning
    // the params here fixes it.
    let RpcRequest { id, method, params } = req;
    let result = match method.as_str() {
        "dashboard.overview" => handle_dashboard(&state, &sender, &params).await,
        "ideas.analyze" => handle_ideas(&state, &sender, &params).await,
        "keywords.list" => handle_keywords_list(&state, &sender, &params).await,
        "keywords.trending" => handle_keywords_trending(&state, &sender, &params).await,
        "scores.list" => handle_scores_list(&state, &sender, &params).await,
        "scores.detail" => handle_scores_detail(&state, &sender, &params).await,
        "scores.backfill" => handle_scores_backfill(&state, &sender, &params).await,
        "videos.list" => handle_videos_list(&state, &sender, &params).await,
        "videos.detail" => handle_videos_detail(&state, &sender, &params).await,
        "scorecard.get" => handle_scorecard(&state, &sender, &params).await,
        "health.get" => handle_health(&state, &sender, &params).await,
        "gaps.get" => handle_gaps(&state, &sender, &params).await,
        "tags.cloud" => handle_tags_cloud(&state, &sender, &params).await,
        "tags.gaps" => handle_tags_gaps(&state, &sender, &params).await,
        "analysis.overview" => handle_analysis_overview(&state, &sender, &params).await,
        "analysis.next-video" => handle_analysis_next_video(&state, &sender, &params).await,
        "analysis.keywords" => handle_analysis_keywords(&state, &sender, &params).await,
        "analysis.refresh" => handle_analysis_refresh(&state, &sender, &params).await,
        "alerts.list" => handle_alerts_list(&state, &sender, &params).await,
        "audit.get" => handle_audit(&state, &sender, &params).await,
        "channels.snapshots" => handle_channels_snapshots(&state, &sender, &params).await,
        _ => Err(TubeforgeError::Usage(format!("unknown method: {method}"))),
    };

    match result {
        Ok(data) => {
            let res = RpcResponse::Result { id, data };
            let _ = send(&sender, &res).await;
        }
        Err(e) => {
            let res = RpcResponse::Error {
                id,
                error: RpcError {
                    code: -32603,
                    message: e.to_string(),
                },
            };
            let _ = send(&sender, &res).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Individual handlers — each computes at runtime and streams progress
// ---------------------------------------------------------------------------

use crate::analytics::keywords::trend_rows;
use crate::analytics::reports;

/// A runtime-scoring context: opens the BM25 index once and reuses it across
/// many videos in a single request. Scoring is computed FRESH from the current
/// corpus + tracked keywords — never read from a stale stored score.
struct RuntimeScorer {
    bm25: Option<crate::search::bm25::Bm25>,
    weights: crate::scoring::weights::Weights,
    keywords: Vec<String>,
}

impl RuntimeScorer {
    /// Open the index + weights from `data_dir`. `keywords` are fetched by
    /// the caller (async, off the executor). Returns `None` bm25 when the
    /// index is unavailable — callers fall back to stored scores (graceful).
    fn new(data_dir: &std::path::Path, keywords: Vec<String>) -> RuntimeScorer {
        let weights = crate::scoring::weights::Weights::from_env()
            .unwrap_or_else(|_| crate::scoring::weights::Weights::defaults());
        let bm25 = crate::search::open_or_create(&data_dir.join("index"))
            .ok()
            .and_then(|index| crate::search::bm25::Bm25::open(index).ok());
        RuntimeScorer {
            bm25,
            weights,
            keywords,
        }
    }

    /// Compute the full SEO+GEO score for a stored video at runtime.
    fn score(&self, video: &VideoRow) -> Option<crate::scoring::ScoreResult> {
        let bm25 = self.bm25.as_ref()?;
        let tags: Vec<String> = serde_json::from_str(&video.tags).unwrap_or_default();
        let meta = crate::scoring::geo::GeoMeta {
            published_at: video.published_at.clone(),
            recording_date: video.recording_date.clone(),
            recording_location_name: video.recording_location_name.clone(),
            recording_lat: video.recording_lat,
            recording_lng: video.recording_lng,
            topic_categories: serde_json::from_str(&video.topic_categories).unwrap_or_default(),
        };
        Some(crate::scoring::compute_with_meta(
            &video.title,
            &video.description,
            &tags,
            &self.keywords,
            bm25,
            &self.weights,
            Some(&video.video_id),
            &meta,
        ))
    }

    /// Build a score map (video_id → seo_total) that prefers a FRESH runtime
    /// score for videos lacking a stored one, and reuses the stored value for
    /// the rest. This keeps the fast path AND guarantees newly-collected
    /// videos always carry fresh analysis — never a stale/zero placeholder.
    async fn runtime_seo_scores(
        &self,
        db: &Db,
        videos: &[VideoRow],
    ) -> std::collections::HashMap<String, f64> {
        use std::collections::{HashMap, HashSet};
        let stored: HashSet<String> = db
            .all_scores()
            .await
            .ok()
            .map(|s| s.into_iter().map(|r| r.video_id).collect())
            .unwrap_or_default();
        let mut out: HashMap<String, f64> = HashMap::new();
        for v in videos {
            if stored.contains(&v.video_id) {
                continue; // already scored — fast path
            }
            if let Some(r) = self.score(v) {
                let _ = db
                    .upsert_score(
                        &v.video_id,
                        r.seo_total,
                        r.geo_total,
                        r.total,
                        &r.components_flat.to_string(),
                    )
                    .await;
                out.insert(v.video_id.clone(), r.seo_total);
            }
        }
        out
    }
}

/// Phase 6.6 performance-half payload for one video: VPH, engagement,
/// retention (from the stored yt-dlp heatmap) and trending flag. Mirrors the
/// HTTP `score_detail_api` helper so the RPC schema stays identical.
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

async fn handle_dashboard(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.2, "Reading health report...").await;
    let h = reports::health(&state.db, 7).await?;

    progress(sender, &Value::Null, 0.6, "Reading counts...").await;
    let counts = json!({
        "videos": h["counts"]["videos"],
        "channels": h["counts"]["channels"],
        "scores": h["counts"]["scores"],
        "ideas": h["counts"]["ideas"],
        "alerts": h["counts"]["alerts"],
        "keywords": h["counts"]["keywords"],
    });

    // Include KG build status + stats (internal enhancement).
    let kg = crate::serve::kg_status(state).await;

    progress(sender, &Value::Null, 0.9, "Finalizing...").await;
    Ok(json!({
        "counts": counts,
        "kg_built": kg.built,
        "kg_stats": {
            "entities": kg.entity_count,
            "relations": kg.relation_count,
            "communities": kg.community_count,
        },
        "integrity": h["integrity"],
        "quota": h["quota"],
        "last_ingest": h["last_ingest"],
        "stale_channels": h["stale_channels"],
        "index": h["index"],
    }))
}

async fn handle_ideas(
    state: &AppState,
    sender: &RpcSender,
    params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.1, "Loading videos from corpus...").await;
    let videos = state.db.all_videos().await?;

    if videos.is_empty() {
        return Ok(json!({
            "ideas": [],
            "note": "no videos in database — run `tubeforge ingest` first",
        }));
    }

    progress(sender, &Value::Null, 0.3, "Opening BM25 index...").await;
    let index_dir = state.data_dir.join("index");
    let index = crate::search::open_or_create(&index_dir)?;
    let bm25 = crate::search::bm25::Bm25::open(index)?;

    progress(
        sender,
        &Value::Null,
        0.5,
        "Loading keywords & building competitor graph...",
    )
    .await;
    let weights = crate::scoring::weights::Weights::from_env()?;

    let top_n: usize = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(25)
        .clamp(1, 200) as usize;
    let niche = params.get("niche").and_then(|v| v.as_str());

    progress(
        sender,
        &Value::Null,
        0.7,
        "Scoring candidates (SEO + fit + gap)...",
    )
    .await;
    let ideas =
        crate::analytics::ideas::analyze(&state.db, &bm25, &videos, &weights, niche, top_n).await?;

    progress(sender, &Value::Null, 0.9, "Formatting results...").await;
    let items: Vec<Value> = ideas
        .iter()
        .enumerate()
        .map(|(i, c)| {
            json!({
                "id": i + 1,
                "title": c.title_suggestion,
                "rationale": c.rationale,
                "score": c.score,
                "source_video": c.source_video,
            })
        })
        .collect();

    // Generate graph-based ideas from KG (if available).
    let graph_ideas = compute_graph_ideas(state).await;

    Ok(json!({
        "ideas": items,
        "generated_at": crate::util::now_rfc3339(),
        "corpus_size": videos.len(),
        "graph_ideas": graph_ideas,
    }))
}

/// Compute graph-based video ideas (internal KG enhancement).
/// Returns `Value::Null` when KG is not available.
async fn compute_graph_ideas(state: &AppState) -> Value {
    let kg = match crate::serve::get_kg(state).await {
        Some(kg) => kg,
        None => return Value::Null,
    };
    let own_channel = state.own_channel.as_deref();
    let ideas = crate::analytics::graph_aware::generate_graph_ideas(&kg, own_channel, 5);
    let ideas_json: Vec<Value> = ideas
        .into_iter()
        .map(|(title, score, rationale)| {
            json!({
                "title": title,
                "score": score,
                "rationale": rationale,
                "source": "knowledge_graph",
            })
        })
        .collect();
    Value::Array(ideas_json)
}

async fn handle_keywords_list(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.4, "Loading keyword rankings...").await;
    let rankings = state.db.list_rankings().await?;
    let trends = trend_rows(&rankings);

    // Map trend_rows → frontend Keyword schema. trend_rows emits
    // `latest_position`, `delta`, `snapshots`; the frontend expects
    // `rank`, `trend`, `sparkline` (number[]).
    let items: Vec<Value> = trends
        .iter()
        .map(|t| {
            let delta = t["delta"].as_i64();
            let trend = match delta {
                Some(d) if d < 0 => "rising",
                Some(d) if d > 0 => "declining",
                _ => "stable",
            };
            let sparkline: Vec<Value> = t["snapshots"]
                .as_array()
                .map(|snaps| {
                    snaps
                        .iter()
                        .filter_map(|s| s["position"].as_i64())
                        .map(|p| json!(p))
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "keyword": t["keyword"],
                "rank": t["latest_position"],
                "trend": trend,
                "sparkline": sparkline,
            })
        })
        .collect();

    Ok(json!({ "keywords": items, "total": items.len() }))
}

async fn handle_keywords_trending(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.5, "Loading trending keywords...").await;
    let rankings = state.db.list_rankings().await?;
    let trends = trend_rows(&rankings);

    let trending: Vec<Value> = trends
        .iter()
        .filter(|t| t["delta"].as_i64().map(|d| d < 0).unwrap_or(false))
        .take(15)
        .map(|t| {
            json!({
                "keyword": t["keyword"],
                "rank": t["latest_position"],
            })
        })
        .collect();

    Ok(json!({ "trending": trending, "total": trending.len() }))
}

async fn handle_scores_list(
    state: &AppState,
    sender: &RpcSender,
    params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.2, "Loading videos...").await;
    let videos = state.db.all_videos().await?;

    progress(sender, &Value::Null, 0.6, "Loading stored scores...").await;
    let channels = state.db.all_channels().await?;

    let channel_title: HashMap<&str, &str> = channels
        .iter()
        .map(|c| (c.channel_id.as_str(), c.title.as_str()))
        .collect();

    let q = params.get("q").and_then(|v| v.as_str()).unwrap_or("");
    let ql = q.trim().to_lowercase();

    let stored: HashMap<String, crate::storage::db::ScoreRow> = state
        .db
        .all_scores()
        .await?
        .into_iter()
        .map(|s| (s.video_id.clone(), s))
        .collect();

    progress(sender, &Value::Null, 0.8, "Assembling score list...").await;
    let mut items: Vec<Value> = videos
        .iter()
        .filter(|v| ql.is_empty() || v.title.to_lowercase().contains(&ql))
        .map(|v| {
            // List view uses STORED scores (fast path). Scores are kept fresh
            // by the SERP pipeline (on search) and the score/ingest pipelines
            // (on collection). The DETAIL view computes the full breakdown at
            // runtime for the freshest possible number.
            let s = stored.get(&v.video_id);
            let (seo, geo, total) = s
                .map(|s| (s.seo_score, s.geo_score, s.total_score))
                .unwrap_or((0.0, 0.0, 0.0));
            json!({
                "video_id":        v.video_id,
                "title":           v.title,
                "channel_name":    v.channel_id.as_deref()
                    .and_then(|cid| channel_title.get(cid).copied())
                    .unwrap_or("—"),
                "overall_score":   total,
                "freshness_score": seo,
                "authority_score": geo,
                "published_at":    v.published_at,
                "views":           v.view_count.unwrap_or(0),
            })
        })
        .collect();

    items.sort_by(|a, b| {
        let sa = a["overall_score"].as_f64().unwrap_or(0.0);
        let sb = b["overall_score"].as_f64().unwrap_or(0.0);
        sb.total_cmp(&sa)
    });

    Ok(json!({ "scores": items, "total": items.len() }))
}

async fn handle_scores_detail(
    state: &AppState,
    sender: &RpcSender,
    params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| TubeforgeError::Usage("missing id".into()))?;

    progress(sender, &Value::Null, 0.2, "Loading video metadata...").await;
    let videos = state.db.all_videos().await?;
    let video = videos
        .iter()
        .find(|v| v.video_id == id)
        .ok_or_else(|| TubeforgeError::Usage(format!("video {id} not found")))?;
    let title = video.title.clone();

    // Compute the FULL SEO + GEO breakdown FRESH at runtime.
    progress(sender, &Value::Null, 0.5, "Scoring at runtime...").await;
    let keywords: Vec<String> = state
        .db
        .list_keywords()
        .await?
        .into_iter()
        .map(|k| k.keyword)
        .collect();
    let scorer = RuntimeScorer::new(&state.data_dir, keywords);

    // Prefer a fresh runtime score; fall back to the stored breakdown.
    let (seo_total, geo_total, total, seo_components, geo_components) = match scorer.score(video) {
        Some(r) => {
            let mut seo = HashMap::new();
            if let Value::Object(m) = &r.seo_components {
                for (k, v) in m {
                    if let Some(f) = v.as_f64() {
                        seo.insert(k.clone(), f);
                    }
                }
            }
            let mut geo = HashMap::new();
            if let Value::Object(m) = &r.geo_components {
                for (k, v) in m {
                    if let Some(f) = v.as_f64() {
                        geo.insert(k.clone(), f);
                    }
                }
            }
            (r.seo_total, r.geo_total, r.total, seo, geo)
        }
        None => {
            let score = state
                .db
                .all_scores()
                .await?
                .into_iter()
                .find(|s| s.video_id == id);
            let mut seo = HashMap::new();
            let mut geo = HashMap::new();
            if let Some(s) = &score {
                let components: Value = serde_json::from_str(&s.components).unwrap_or(Value::Null);
                for k in &SEO_COMPONENT_KEYS {
                    if let Some(v) = components.get(*k).and_then(|v| v.as_f64()) {
                        seo.insert(k.to_string(), v);
                    }
                }
                for k in &GEO_COMPONENT_KEYS {
                    if let Some(v) = components.get(*k).and_then(|v| v.as_f64()) {
                        geo.insert(k.to_string(), v);
                    }
                }
                (s.seo_score, s.geo_score, s.total_score, seo, geo)
            } else {
                (0.0, 0.0, 0.0, seo, geo)
            }
        }
    };

    progress(sender, &Value::Null, 0.9, "Finalizing score breakdown...").await;

    // Compute graph-aware scores from the Knowledge Graph (internal enhancement).
    let graph_scores = compute_graph_scores_for_video(state, id, video).await;

    Ok(json!({
        "video_id":       id,
        "title":          title,
        "seo_total":      seo_total,
        "geo_total":      geo_total,
        "total":          total,
        "seo_components": seo_components,
        "geo_components": geo_components,
        "graph_scores":   graph_scores,
        "performance":    performance_for(&state.db, id).await,
    }))
}

/// Compute graph-aware scores for a video (internal KG enhancement).
/// Returns `Value::Null` when KG is not available (graceful degradation).
async fn compute_graph_scores_for_video(
    state: &AppState,
    video_id: &str,
    video: &VideoRow,
) -> Value {
    let kg = match crate::serve::get_kg(state).await {
        Some(kg) => kg,
        None => return Value::Null,
    };
    let channel_id = video.channel_id.as_deref();
    let keywords: Vec<String> = serde_json::from_str::<Vec<String>>(&video.tags)
        .unwrap_or_default()
        .into_iter()
        .take(5)
        .collect();
    let scores =
        crate::analytics::graph_aware::compute_graph_scores(&kg, video_id, channel_id, &keywords);
    json!({
        "tag_authority":       scores.tag_authority,
        "topic_dominance":     scores.topic_dominance,
        "keyword_competition": scores.keyword_competition,
    })
}

/// Score every video that currently lacks a stored score, streaming progress.
/// This is the one-time backfill so the fast list view has fresh stored
/// scores for all videos (not just the ones collected via search).
async fn handle_scores_backfill(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.05, "Loading videos...").await;
    let videos = state.db.all_videos().await?;
    let keywords: Vec<String> = state
        .db
        .list_keywords()
        .await?
        .into_iter()
        .map(|k| k.keyword)
        .collect();
    let scorer = RuntimeScorer::new(&state.data_dir, keywords);

    let stored: std::collections::HashSet<String> = state
        .db
        .all_scores()
        .await?
        .into_iter()
        .map(|s| s.video_id)
        .collect();

    let unscored: Vec<&VideoRow> = videos
        .iter()
        .filter(|v| !stored.contains(&v.video_id))
        .collect();
    let total = unscored.len();
    if total == 0 {
        return Ok(json!({ "scored": 0, "total": 0, "done": true }));
    }

    progress(
        sender,
        &Value::Null,
        0.1,
        &format!("Scoring {total} videos at runtime..."),
    )
    .await;
    let mut scored = 0usize;
    for (i, v) in unscored.iter().enumerate() {
        if let Some(r) = scorer.score(v) {
            let _ = state
                .db
                .upsert_score(
                    &v.video_id,
                    r.seo_total,
                    r.geo_total,
                    r.total,
                    &r.components_flat.to_string(),
                )
                .await;
            scored += 1;
        }
        // Stream progress every 10 videos (avoid flooding the socket).
        if i % 10 == 0 {
            let pct = 0.10 + (i as f32 / total as f32) * 0.85;
            progress(sender, &Value::Null, pct, &format!("Scored {i}/{total}")).await;
        }
    }
    progress(sender, &Value::Null, 0.95, "Backfill complete").await;

    Ok(json!({ "scored": scored, "total": total, "done": true }))
}

async fn handle_videos_list(
    state: &AppState,
    sender: &RpcSender,
    params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.4, "Loading videos...").await;
    let videos = state.db.all_videos().await?;

    let q = params.get("q").and_then(|v| v.as_str()).unwrap_or("");
    let ql = q.trim().to_lowercase();
    let page = params
        .get("page")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .max(1);
    let page_size = params
        .get("page_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(20)
        .clamp(1, 200);

    let mut items: Vec<Value> = videos
        .iter()
        .filter(|v| ql.is_empty() || v.title.to_lowercase().contains(&ql))
        .map(|v| {
            json!({
                "video_id": v.video_id,
                "title": v.title,
                "channel_id": v.channel_id,
                "view_count": v.view_count,
                "like_count": v.like_count,
                "comment_count": v.comment_count,
                "published_at": v.published_at,
                "thumb_url": v.thumb_url,
                "tags": v.tags,
            })
        })
        .collect();

    items.sort_by(|a, b| {
        let va = a["view_count"].as_i64().unwrap_or(0);
        let vb = b["view_count"].as_i64().unwrap_or(0);
        vb.cmp(&va)
    });

    let total = items.len();
    let start = ((page - 1) * page_size) as usize;
    let page_items: Vec<Value> = items
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();

    Ok(json!({
        "items": page_items,
        "total": total,
        "page": page,
        "page_size": page_size,
    }))
}

async fn handle_videos_detail(
    state: &AppState,
    sender: &RpcSender,
    params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| TubeforgeError::Usage("missing id".into()))?;

    progress(sender, &Value::Null, 0.5, "Loading video detail...").await;
    let videos = state.db.all_videos().await?;
    let video = videos
        .iter()
        .find(|v| v.video_id == id)
        .ok_or_else(|| TubeforgeError::Usage(format!("video {id} not found")))?;

    Ok(json!({
        "video_id": video.video_id,
        "title": video.title,
        "description": video.description,
        "channel_id": video.channel_id,
        "view_count": video.view_count,
        "like_count": video.like_count,
        "comment_count": video.comment_count,
        "published_at": video.published_at,
        "duration_sec": video.duration_sec,
        "category_id": video.category_id,
        "thumb_url": video.thumb_url,
        "tags": video.tags,
    }))
}

async fn handle_scorecard(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(
        sender,
        &Value::Null,
        0.3,
        "Building competitor scorecard...",
    )
    .await;
    let rows = reports::scorecard(&state.db, &[]).await?;

    // Enhance with channel centrality from KG (if available).
    let kg = crate::serve::get_kg(state).await;
    let centrality_map: HashMap<String, f64> = kg
        .as_ref()
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

    let mut channels: Vec<Value> = Vec::new();
    if let Some(arr) = rows["channels"].as_array() {
        for c in arr {
            let cid = c["channel_id"].as_str().unwrap_or("");
            let mut ch = c.clone();
            ch["centrality"] = centrality_map
                .get(cid)
                .map(|s| json!(s))
                .unwrap_or(Value::Null);
            channels.push(ch);
        }
    }

    Ok(json!({
        "rows": channels,
        "median": rows["median"],
        "compared": rows["compared"],
        "kg_built": !centrality_map.is_empty(),
    }))
}

async fn handle_channels_snapshots(
    state: &AppState,
    sender: &RpcSender,
    params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| TubeforgeError::Usage("missing id".into()))?;

    progress(sender, &Value::Null, 0.4, "Loading channel snapshots...").await;
    let channel = state
        .db
        .get_channel(id)
        .await?
        .ok_or_else(|| TubeforgeError::Usage(format!("channel {id} not found")))?;
    let rows = state.db.channel_snapshots(id).await?;
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

    Ok(json!({
        "channel_id": id,
        "channel_name": channel.title,
        "snapshots": points,
    }))
}

async fn handle_health(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.5, "Running health checks...").await;
    let h = reports::health(&state.db, 7).await?;
    Ok(h)
}

async fn handle_gaps(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.2, "Loading videos...").await;
    let videos = state.db.all_videos().await?;
    let channels = state.db.all_channels().await?;

    let channel_names: std::collections::HashMap<String, String> = channels
        .iter()
        .map(|c| (c.channel_id.clone(), c.title.clone()))
        .collect();

    progress(
        sender,
        &Value::Null,
        0.5,
        "Mining outliers & coverage gaps...",
    )
    .await;
    let report = crate::analytics::gaps::report(&videos, &channel_names).await?;

    // Actionable reframe: the topics YOU should win (high demand, few
    // covering channels) with a concrete angle — not competitor video noise.
    let opportunities = crate::analytics::actions::gap_opportunities(&report.topics, 20);

    // Compute graph-based content gaps from KG (if available).
    let graph_gaps = compute_graph_gaps(state).await;

    Ok(json!({
        "opportunities": opportunities,
        "outliers": report.outliers,
        "topics": report.topics,
        "freshness_gaps": report.freshness_gaps,
        "format_gaps": report.format_gaps,
        "graph_gaps": graph_gaps,
    }))
}

/// Compute graph-based content gaps (internal KG enhancement).
/// Returns `Value::Null` when KG is not available.
async fn compute_graph_gaps(state: &AppState) -> Value {
    let kg = match crate::serve::get_kg(state).await {
        Some(kg) => kg,
        None => return Value::Null,
    };
    let own_channel = state.own_channel.as_deref();
    let gaps = crate::analytics::graph_aware::find_content_gaps(&kg, own_channel);
    let gaps_json: Vec<Value> = gaps
        .into_iter()
        .take(10)
        .map(|(topic_id, score)| {
            let display_name = kg
                .get_entity(&topic_id)
                .map(|e| e.display_name.clone())
                .unwrap_or_else(|| topic_id.clone());
            json!({
                "topic": display_name,
                "topic_id": topic_id,
                "opportunity_score": score,
            })
        })
        .collect();
    Value::Array(gaps_json)
}

async fn handle_tags_cloud(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.5, "Building tag cloud...").await;
    let cloud = crate::analytics::tags::tag_cloud(&state.db).await?;
    Ok(serde_json::to_value(cloud)?)
}

async fn handle_tags_gaps(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    let own = state.own_channel.as_deref().unwrap_or("");
    progress(sender, &Value::Null, 0.5, "Analyzing tag gaps...").await;
    let gaps = crate::analytics::tags::tag_gaps(&state.db, own).await?;

    // Enhance each tag gap with authority from KG (if available).
    let kg = crate::serve::get_kg(state).await;
    let enhanced: Vec<Value> = gaps
        .into_iter()
        .map(|gap| {
            let authority = kg.as_ref().map(|kg| {
                crate::analytics::graph_aware::compute_tag_authority_by_name(kg, &gap.tag)
            });
            json!({
                "tag": gap.tag,
                "competitor_usage": gap.competitor_usage,
                "your_usage": gap.your_usage,
                "opportunity_score": gap.opportunity_score,
                "tag_authority": authority,
            })
        })
        .collect();

    Ok(json!({
        "gaps": enhanced,
        "kg_built": kg.is_some(),
    }))
}

async fn handle_analysis_overview(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.2, "Loading videos...").await;
    let videos = state.db.all_videos().await?;
    let keywords: Vec<String> = state
        .db
        .list_keywords()
        .await?
        .into_iter()
        .map(|k| k.keyword)
        .collect();
    let scorer = RuntimeScorer::new(&state.data_dir, keywords);

    progress(
        sender,
        &Value::Null,
        0.4,
        "Scoring at runtime (gap-fill)...",
    )
    .await;
    scorer.runtime_seo_scores(&state.db, &videos).await;

    let own = state
        .own_channel
        .clone()
        .ok_or_else(|| TubeforgeError::Usage("TUBEFORGE_OWN_CHANNEL not configured".into()))?;

    progress(sender, &Value::Null, 0.6, "Computing competitor medians...").await;
    let overview = crate::analytics::growth::own_overview(&state.db, &own).await?;
    Ok(serde_json::to_value(overview)?)
}

async fn handle_analysis_next_video(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    let limit: usize = _params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(5)
        .clamp(1, 20) as usize;

    progress(
        sender,
        &Value::Null,
        0.5,
        &format!("Ranking top {limit} next videos..."),
    )
    .await;
    // Pass the own channel so topics already covered by the user's channel are
    // excluded — creating a recommended video moves on to the next best topic.
    let recs = crate::analytics::growth::next_video_recommendations(
        &state.db,
        7.0,
        state.own_channel.as_deref(),
        limit,
    )
    .await?;
    // Surface the freshness of the underlying research snapshot so the UI
    // never presents a stale recommendation as if it were computed just now.
    let research_at = state
        .db
        .keyword_research_all()
        .await?
        .iter()
        .map(|r| r.at.clone())
        .max();
    Ok(json!({ "recommendations": recs, "research_at": research_at }))
}

async fn handle_analysis_keywords(
    state: &AppState,
    sender: &RpcSender,
    params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    let horizon: f64 = params
        .get("horizon")
        .and_then(|v| v.as_f64())
        .unwrap_or(7.0);

    progress(
        sender,
        &Value::Null,
        0.5,
        "Computing keyword opportunities...",
    )
    .await;
    let opps = crate::analytics::growth::keyword_opportunities(&state.db, horizon).await?;
    // Freshness metadata — these opportunities are derived from stored
    // research snapshots; expose the newest snapshot time so the UI can flag
    // staleness instead of presenting old data as live.
    let research_at = state
        .db
        .keyword_research_all()
        .await?
        .iter()
        .map(|r| r.at.clone())
        .max();
    Ok(json!({ "opportunities": opps, "horizon_days": horizon, "research_at": research_at }))
}

/// GET /api/analysis/refresh-equivalent over RPC: fetch LIVE YouTube data
/// for every tracked keyword (yt-dlp ytsearch + autocomplete), persist the
/// SERP videos/research snapshots to the DB, re-score at runtime, and return
/// the refreshed overview + opportunities. This is the "one button to pull
/// realtime YouTube awareness" path — the UI calls it on demand, and the
/// results are written back so ALL tabs share the fresh data.
async fn handle_analysis_refresh(
    state: &AppState,
    sender: &RpcSender,
    params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    let ytdlp = state.ytdlp.clone().ok_or_else(|| {
        TubeforgeError::Usage("yt-dlp disabled — set TUBEFORGE_YTDLP_ENABLED=true in .env".into())
    })?;
    let db_path = state.db.path.clone();
    let data_dir = state.data_dir.clone();
    let own_channel = state.own_channel.clone();

    let keywords: Vec<String> = state
        .db
        .list_keywords()
        .await?
        .into_iter()
        .map(|k| k.keyword)
        .collect();
    if keywords.is_empty() {
        return Ok(json!({
            "refreshed": 0,
            "message": "no tracked keywords — add some first (`tubeforge keywords add <kw>`)",
        }));
    }

    // Optional override: refresh a single keyword (`?q=`) or all tracked.
    let q_override = params
        .get("q")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());
    let serp: u64 = params.get("serp").and_then(|v| v.as_u64()).unwrap_or(6);
    let targets: Vec<String> = match q_override {
        Some(q) if !q.is_empty() => vec![q],
        _ => keywords.clone(),
    };

    // Capture a real stored video of the OWN channel so the worker can fetch
    // its live subscriber count via yt-dlp (the scratch Db it uses is empty).
    let own_channel_id = state.own_channel.clone();
    let own_sample_video: Option<String> = match own_channel_id {
        Some(own) => {
            let all = state.db.all_videos().await?;
            all.iter()
                .filter(|v| v.channel_id.as_deref() == Some(own.as_str()))
                .max_by(|a, b| a.published_at.cmp(&b.published_at))
                .map(|v| v.video_id.clone())
        }
        None => None,
    };

    // The ENTIRE live-fetch + persist runs on a dedicated std thread with its
    // OWN tokio runtime. yt-dlp subprocess futures are non-Send across axum's
    // `on_upgrade` Send boundary, so we can't await them in the handler. The
    // thread owns all inputs (no references to handler locals) and does all DB
    // work through a single connection it opens itself — no second connection
    // against the server's live Db, so no "database is locked".
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(Err(TubeforgeError::Storage {
                    code: "RUNTIME".into(),
                    message: format!("build refresh runtime: {e}"),
                }));
                return;
            }
        };
        let result = rt.block_on(async move {
            // All live-fetch + persistence + re-score + overview run on this
            // worker thread using its OWN Db connection to the REAL db file.
            // The server connection is idle while this runs (the handler awaits
            // the oneshot), so turso's single-writer model is respected and no
            // "database is locked" occurs.
            let db = Db::open(&db_path).await?;
            let clients = crate::fetch::FetchClients::new()?;

            let mut refreshed = 0usize;
            for kw in &targets {
                let r = crate::analytics::research::inspect(&db, None, &ytdlp, &clients, kw, serp)
                    .await?;
                crate::storage::db::persist_serp_db(&db, &r.serp).await?;
                crate::analytics::tags::analyze_competitors(&db).await?;

                let suggested = r.suggested_tags.iter().map(|t| t.tag.clone()).collect::<Vec<_>>();
                let related = r.related_keywords.iter().map(|k| k.keyword.clone()).collect::<Vec<_>>();
                let _ = db
                    .upsert_keyword_research(
                        kw,
                        &crate::util::now_rfc3339(),
                        &r.volume_label,
                        r.serp_total as i64,
                        r.serp_mean_views,
                        r.ranking_channels as i64,
                        r.competition_score,
                        r.opportunity_score,
                        r.actively_published,
                        &serde_json::to_string(&suggested).unwrap_or_else(|_| "[]".to_string()),
                        &serde_json::to_string(&related).unwrap_or_else(|_| "[]".to_string()),
                    )
                    .await;
                refreshed += 1;
            }

            // Precise LIVE subscriber count for the OWN channel via yt-dlp.
            let mut subscribers_updated = 0usize;
            if let (Some(sample_vid), Some(own)) = (own_sample_video.as_deref(), own_channel.as_deref()) {
                if let Ok(info) = ytdlp.metadata(sample_vid).await {
                    if let Some(followers) = info.channel_follower_count {
                        let _ = db
                            .update_channel_subscribers(
                                own,
                                followers,
                                &crate::util::now_rfc3339(),
                            )
                            .await;
                        subscribers_updated = 1;
                    }
                }
            }

            // Re-score so fresh data carries forward.
            let videos = db.all_videos().await?;
            let scorer = RuntimeScorer::new(&data_dir, keywords);
            scorer.runtime_seo_scores(&db, &videos).await;

            let overview = match &own_channel {
                Some(own) => serde_json::to_value(crate::analytics::growth::own_overview(&db, own).await?)?,
                None => Value::Null,
            };
            let research_at = db
                .keyword_research_all()
                .await?
                .iter()
                .map(|r| r.at.clone())
                .max();

            Ok::<_, TubeforgeError>(json!({
                "refreshed": refreshed,
                "subscribers_updated": subscribers_updated,
                "overview": overview,
                "research_at": research_at,
                "message": format!(
                    "pulled live YouTube data for {refreshed} keyword(s); refreshed {subscribers_updated} channel subscriber count(s)"
                ),
            }))
        });
        let _ = tx.send(result);
    });

    progress(sender, &Value::Null, 0.5, "Fetching live YouTube data...").await;
    match rx.await {
        Ok(Ok(data)) => Ok(data),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(TubeforgeError::Storage {
            code: "REFRESH".into(),
            message: "refresh worker panicked".into(),
        }),
    }
}

async fn handle_alerts_list(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    progress(sender, &Value::Null, 0.5, "Loading alerts...").await;
    let alerts = state.db.list_alerts(100).await?;
    let items: Vec<Value> = alerts
        .iter()
        .map(|a| {
            json!({
                "id": a.alert_id,
                "kind": a.kind,
                "message": a.message,
                "severity": a.severity,
                "created_at": a.created_at,
                "read": a.read_at.is_some(),
            })
        })
        .collect();

    Ok(json!({ "alerts": items, "total": items.len() }))
}

async fn handle_audit(
    state: &AppState,
    sender: &RpcSender,
    _params: &HashMap<String, Value>,
) -> Result<Value, TubeforgeError> {
    let own = state
        .own_channel
        .clone()
        .ok_or_else(|| TubeforgeError::Usage("TUBEFORGE_OWN_CHANNEL not configured".into()))?;

    progress(sender, &Value::Null, 0.3, "Loading videos...").await;
    let videos = state.db.all_videos().await?;
    let channels = state.db.all_channels().await?;
    let channel = channels.iter().find(|c| c.channel_id == own);
    let channel_title = channel.map(|c| c.title.as_str()).unwrap_or("");

    // Gap-fill scores at runtime so the audit's metadata component reflects
    // fresh analysis, not stale stored values.
    let keywords: Vec<String> = state
        .db
        .list_keywords()
        .await?
        .into_iter()
        .map(|k| k.keyword)
        .collect();
    let scorer = RuntimeScorer::new(&state.data_dir, keywords);
    scorer.runtime_seo_scores(&state.db, &videos).await;

    progress(sender, &Value::Null, 0.7, "Scoring components...").await;
    let audit =
        crate::analytics::audit::audit_channel(&state.db, &videos, &own, channel_title).await?;
    // VidIQ-style: turn weak components into ranked, actionable fixes for the
    // creator's OWN channel — not competitor observations.
    let actions = crate::analytics::actions::from_audit(&audit);
    Ok(json!({ "audit": audit, "actions": actions }))
}
