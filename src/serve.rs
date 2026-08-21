//! HTMX dashboard server (PRD §5.4 deferred item — delivered as
//! `tubeforge serve`).
//!
//! Local-first design contract (documented in README + here):
//! - **Loopback only** (`127.0.0.1` by default; `localhost`/`::1` allowed).
//!   Single-user, no auth — the CSRF origin guard (see `csrf.rs`) is the
//!   only cross-origin protection.
//! - **Single writer** (LLD §10): this server opens ONE Db connection and
//!   only mutates via the CLI code paths (idea status, alert read/clear).
//!   Running `serve` concurrently with writing CLI commands is unsupported;
//!   concurrent READERS are fine (WAL).
//! - **stdout purity** (LLD §4.2): `serve` is long-running and never emits
//!   the JSON envelope — the listening line goes to stderr.
//! - Mutations ride the same repository methods the CLI uses
//!   (`set_idea_statuses`, `mark_alerts_read`, `clear_alerts`) — no new
//!   write logic is duplicated here.
//!
//! Charts are server-rendered inline SVG (`svg.rs`) — no JS chart library
//! (PRD §11 open question resolved).

pub mod api;
pub mod csrf;
pub mod rpc;
pub mod stdio;
pub mod svg;
pub mod templates;
pub mod web;

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use askama::Template;
use futures::Stream;
use serde_json::Value;

use crate::analytics::keywords::trend_rows;
use crate::analytics::reports;
use crate::config::Config;
use crate::error::{storage_err, TubeforgeError};
use crate::fetch::ytdlp::YtdlpClient;
use crate::storage::db::{Db, IdeaRow};
use http::{HeaderMap, StatusCode};
use svg::sparkline;
use templates::*;
use web::sse::{Event, KeepAlive, Sse};
use web::{
    get, post, Headers, Html, IntoResponse, Path, Query, ReqUri, Response, Router, ServeState,
    State,
};

/// The 15 SEO component keys in canonical display order (LLD §7.2 +
/// Phase 6.6 vidIQ additions, matches `scoring::seo::SeoComponents::values`).
pub const SEO_COMPONENT_KEYS: [&str; 15] = [
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

/// The 7 GEO component keys in canonical display order (LLD §7.3, matches
/// `scoring::geo::GeoComponents`).
pub const GEO_COMPONENT_KEYS: [&str; 7] = [
    "entity_coverage",
    "qa_phrasing",
    "list_phrasing",
    "conversational",
    "metadata_complete",
    "location_signal",
    "topic_relevance",
];

/// Idea statuses shared with `commands::ideas` (draft|saved|discarded).
const IDEA_STATUSES: [&str; 3] = ["draft", "saved", "discarded"];

/// Row cap for the scores page (keep simple: top-N + title filter).
const SCORES_LIMIT: usize = 100;
/// Alert list cap (mirrors `commands::alerts::LIST_LIMIT`).
const ALERTS_LIMIT: usize = 100;
/// Idea list cap for the page.
const IDEAS_LIMIT: usize = 500;

/// SSE tick: re-read the counts every 5s, send only on change.
const SSE_TICK: Duration = Duration::from_secs(5);
/// SSE comment heartbeat (`: ping`) so proxies/browsers keep the
/// connection alive while the counts are quiet.
const SSE_HEARTBEAT: Duration = Duration::from_secs(15);

/// Shared server state: one Db connection (opened at startup) plus the
/// actually-bound `host:port` used by the CSRF origin guard.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub bind: String,
    /// yt-dlp client for keyless research endpoints (`keywords inspect`).
    /// None when TUBEFORGE_YTDLP_ENABLED is off — the endpoint degrades
    /// with a clear error instead of a silent empty result.
    pub ytdlp: Option<YtdlpClient>,
    /// Data root — the tantivy index (`<data>/index`) lives here, needed
    /// by the keyword-research corpus-resonance half.
    pub data_dir: PathBuf,
    /// The channel we are GROWING (our own channel). Set from
    /// `TUBEFORGE_OWN_CHANNEL`; the scorecard/audit APIs flag it `is_own`
    /// so the dashboard can compare our channel against competitors.
    pub own_channel: Option<String>,
    /// Lazily-loaded Knowledge Graph. `None` until first KG-dependent
    /// endpoint is hit, then `Some(kg)` for the lifetime of the server.
    /// Wrapped in Arc<Mutex<>> for safe shared access across handlers.
    pub kg: Arc<std::sync::Mutex<Option<crate::analytics::kg::KnowledgeGraph>>>,
}

// ---------------------------------------------------------------------------
// Knowledge Graph lazy loading (shared by the HTTP API and the WebSocket RPC)
// ---------------------------------------------------------------------------

/// Get the Knowledge Graph, loading it on first access.
///
/// The KG is built lazily: the first KG-dependent endpoint triggers
/// `load_or_build`. Subsequent calls reuse the cached in-memory graph.
/// Returns `None` if the KG cannot be built (graceful degradation).
///
/// Async-safe: called from within a tokio async context (axum handlers and
/// the RPC socket loop). Uses a double-checked locking pattern to avoid
/// holding the mutex across the await point.
pub async fn get_kg(st: &AppState) -> Option<crate::analytics::kg::KnowledgeGraph> {
    // Fast path: already loaded
    {
        if let Ok(guard) = st.kg.lock() {
            if let Some(kg) = guard.as_ref() {
                return Some(kg.clone());
            }
        }
    }
    // Slow path: load or build (outside the lock to avoid holding it during I/O)
    let kg = match crate::analytics::kg_builder::load_or_build(&st.db).await {
        Ok(kg) if !kg.is_empty() => kg,
        _ => return None,
    };
    // Store in the mutex
    {
        if let Ok(mut guard) = st.kg.lock() {
            *guard = Some(kg.clone());
        }
    }
    Some(kg)
}

/// KG build status for the counts endpoint.
pub struct KgStatus {
    pub built: bool,
    pub entity_count: usize,
    pub relation_count: usize,
    pub community_count: usize,
}

/// Get KG status without triggering a full build.
pub async fn kg_status(st: &AppState) -> KgStatus {
    // Check if already loaded
    if let Ok(guard) = st.kg.lock() {
        if let Some(kg) = guard.as_ref() {
            return KgStatus {
                built: true,
                entity_count: kg.node_count(),
                relation_count: kg.edge_count(),
                community_count: kg.communities.len(),
            };
        }
    }
    // Not loaded — check if KG tables have data (built but not loaded)
    let entity_count = st.db.kg_entity_count().await.unwrap_or(0);
    let relation_count = st.db.kg_relation_count().await.unwrap_or(0);
    let community_count = st.db.kg_community_count().await.unwrap_or(0);
    KgStatus {
        built: entity_count > 0,
        entity_count,
        relation_count,
        community_count,
    }
}

/// Locate the frontend SPA distribution directory, searching:
/// 1. User data directory (`<data_dir>/frontend/dist` or `<data_dir>/dist`)
/// 2. Executable parent directory (`<exe_dir>/frontend/dist` or `<exe_dir>/dist`)
/// 3. Current working directory (`frontend/dist`)
/// 4. Compile-time manifest directory (`CARGO_MANIFEST_DIR/frontend/dist`)
fn find_spa_dist(data_dir: &std::path::Path) -> Option<PathBuf> {
    let in_data = data_dir.join("frontend/dist");
    if in_data.exists() && in_data.join("index.html").exists() {
        return Some(in_data);
    }
    let in_data_dist = data_dir.join("dist");
    if in_data_dist.exists() && in_data_dist.join("index.html").exists() {
        return Some(in_data_dist);
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let in_exe_frontend = exe_dir.join("frontend/dist");
            if in_exe_frontend.exists() && in_exe_frontend.join("index.html").exists() {
                return Some(in_exe_frontend);
            }
            let in_exe_dist = exe_dir.join("dist");
            if in_exe_dist.exists() && in_exe_dist.join("index.html").exists() {
                return Some(in_exe_dist);
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let in_cwd = cwd.join("frontend/dist");
        if in_cwd.exists() && in_cwd.join("index.html").exists() {
            return Some(in_cwd);
        }
    }
    let in_manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
    if in_manifest.exists() && in_manifest.join("index.html").exists() {
        return Some(in_manifest);
    }
    None
}

/// Build the dashboard router plus the shared `ServeState` that carries the
/// `AppState` into handlers.
///
/// Route ownership:
/// - `/api/*`, `/events`, `/healthz`, `/static/*` are shared (both UIs use them).
/// - When `frontend/dist` exists, the React SPA owns the root page routes and
///   the legacy HTMX pages move under `/legacy/*`.
/// - Without a SPA build, the HTMX pages keep the root routes (original behavior).
pub fn app(state: AppState) -> (Router, Arc<ServeState>) {
    let spa_dist_opt = find_spa_dist(&state.data_dir);
    let spa_enabled = spa_dist_opt.is_some();

    let mut router = Router::new()
        .merge(api::api_routes())
        .route("/events", get(events))
        .ws(rpc::ws_handler)
        .route("/healthz", get(healthz))
        .route("/static/htmx.min.js", get(htmx_js))
        .route("/static/sse.js", get(sse_js));

    if let Some(spa_dist) = spa_dist_opt {
        router = router
            .route("/legacy", get(home))
            .route("/legacy/scores", get(scores_page))
            .route("/legacy/scores/{id}", get(score_detail))
            .route("/legacy/ideas", get(ideas_page))
            .route("/legacy/ideas/{id}/{status}", post(ideas_status))
            .route("/legacy/alerts", get(alerts_page))
            .route("/legacy/alerts/read", post(alerts_read))
            .route("/legacy/alerts/clear", post(alerts_clear))
            .route("/legacy/keywords", get(keywords_page))
            .route("/legacy/scorecard", get(scorecard_page))
            .route("/legacy/health", get(health_page))
            // SPA owns every unmatched path: client-side routing serves
            // index.html, real files under dist/ are served directly.
            .spa_fallback(spa_dist.clone(), spa_dist.join("index.html"));
    } else {
        router = router
            .route("/", get(home))
            .route("/scores", get(scores_page))
            .route("/scores/{id}", get(score_detail))
            .route("/ideas", get(ideas_page))
            .route("/ideas/{id}/{status}", post(ideas_status))
            .route("/alerts", get(alerts_page))
            .route("/alerts/read", post(alerts_read))
            .route("/alerts/clear", post(alerts_clear))
            .route("/keywords", get(keywords_page))
            .route("/scorecard", get(scorecard_page))
            .route("/health", get(health_page))
            .fallback(not_found);
    }

    (router, ServeState::new(state))
}

/// `tubeforge serve`: open one Db, bind the listener, print the listening
/// line to STDERR (stdout purity contract), serve until Ctrl-C.
pub async fn run(cfg: &Config, host: &str, port: u16) -> Result<(), TubeforgeError> {
    if !is_loopback_host(host) {
        return Err(TubeforgeError::Usage(format!(
            "serve binds loopback only (single-user, no auth): got host {host:?} — \
             use 127.0.0.1, localhost or ::1"
        )));
    }
    let db = Db::open(&cfg.db_path).await?;
    let listener = tokio::net::TcpListener::bind((host, port))
        .await
        .map_err(|e| storage_err("BIND", format!("{host}:{port}: {e}")))?;
    let addr = listener.local_addr().map_err(|e| storage_err("BIND", e))?;

    // stdout purity (LLD §4.2): the only line this server prints goes to
    // stderr. `serve` never emits the JSON envelope.
    eprintln!(
        "tubeforge serve: http://{addr} — loopback only; single-writer database \
         (do not run writing commands concurrently); Ctrl-C to stop"
    );

    let ytdlp = YtdlpClient::new(
        cfg.ytdlp_path.clone(),
        cfg.ytdlp_enabled,
        cfg.ytdlp_client.clone(),
        cfg.ytdlp_js_runtime.clone(),
    )
    .ok();
    let state = AppState {
        db: Arc::new(db),
        bind: addr.to_string(),
        ytdlp,
        data_dir: cfg.data_dir.clone(),
        own_channel: cfg.own_channel.clone(),
        kg: Arc::new(std::sync::Mutex::new(None)),
    };
    let (router, serve_state) = app(state);
    web::serve(listener, Arc::new(router), serve_state)
        .await
        .map_err(|e| storage_err("SERVE", e))
}

/// Ctrl-C → clean shutdown (graceful: in-flight requests drain first).
fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

async fn home(State(st): State<AppState>) -> Response {
    let counts = counts(&st).await;
    let alerts = match st.db.list_alerts(5).await {
        Ok(a) => alert_views(&a),
        Err(_) => Vec::new(),
    };
    let ideas = match st.db.list_ideas(None, 5).await {
        Ok(i) => idea_views(&i),
        Err(_) => Vec::new(),
    };
    let views_chart = view_bars(&st).await.unwrap_or_default();
    let seo_chart = score_histogram(&st).await;

    let counts_html = match render(CountsTemplate { ..counts }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    let alerts_html = match render(AlertsListTemplate {
        alerts: &alerts,
        unread: alerts.iter().filter(|a| !a.read).count(),
    }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    let ideas_html = match render(IdeaRowsFragment { rows: &ideas }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    let body = match render(HomeTemplate {
        counts_html: &counts_html,
        alerts_html: &alerts_html,
        ideas_html: &ideas_html,
        views_chart: &views_chart,
        seo_chart: &seo_chart,
    }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    page_impl("TubeForge — Dashboard", "home", &body)
}

/// `GET /events`: Server-Sent Events stream that replaces the old 30s
/// polling fragment. One `counts` event is pushed immediately on connect,
/// then every `SSE_TICK` the counts are re-read and re-sent only when the
/// values changed (cheap equality on the template values, no re-render for
/// identical state). `KeepAlive` writes a `: ping` comment every
/// `SSE_HEARTBEAT`. The stream ends cleanly on disconnect: the response
/// body is dropped, which cancels the unfold future (no spawned task, no
/// DB guard held across awaits — reads go through the async turso API).
async fn events(State(st): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = futures::stream::unfold(
        (st, None, true),
        |(st, mut last, mut first): (AppState, Option<CountsTemplate>, bool)| async move {
            loop {
                // `first` lives in the unfold state, not the closure body:
                // it must survive across emits so the SSE_TICK delay applies
                // AFTER an event is sent too, not only between no-change reads.
                if !first {
                    tokio::time::sleep(SSE_TICK).await;
                }
                first = false;
                let counts = counts(&st).await;
                if last.as_ref() == Some(&counts) {
                    continue;
                }
                match render(counts.clone()) {
                    Ok(html) => {
                        let event = Event::default().event("counts").data(html);
                        last = Some(counts);
                        return Some((Ok::<_, Infallible>(event), (st, last, first)));
                    }
                    Err(e) => tracing::warn!("events: counts fragment render failed: {e}"),
                }
            }
        },
    );
    Sse::new(stream).keep_alive(KeepAlive::new().interval(SSE_HEARTBEAT).text("ping"))
}

async fn scores_page(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let filter = q.get("q").cloned().unwrap_or_default();
    let (rows, total) = match score_rows(&st, &filter, SCORES_LIMIT).await {
        Ok(v) => v,
        Err(e) => return internal(e),
    };
    let body = match render(ScoresTemplate {
        rows: &rows,
        q: &filter,
        total,
        limit: SCORES_LIMIT,
    }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    page_impl("TubeForge — Scores", "scores", &body)
}

/// Row-expand fragment: the 17 component values for one video.
async fn score_detail(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let row = match st.db.get_score(&id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return match render(ScoreDetailTemplate {
                video_id: &id,
                title: "not scored",
                seo_total: "—",
                geo_total: "—",
                total: "—",
                seo_components: &[],
                geo_components: &[],
                missing: true,
            }) {
                Ok(s) => html(s).into_response(),
                Err(e) => internal(e),
            }
        }
        Err(e) => return internal(e),
    };
    let title = st
        .db
        .get_video(&id)
        .await
        .ok()
        .flatten()
        .map(|v| v.title)
        .unwrap_or_else(|| id.clone());

    let components: Value =
        serde_json::from_str(&row.components).unwrap_or(Value::Object(Default::default()));
    let seo: Vec<(String, String)> = SEO_COMPONENT_KEYS
        .iter()
        .map(|k| (k.to_string(), component_value(&components, k)))
        .collect();
    let geo: Vec<(String, String)> = GEO_COMPONENT_KEYS
        .iter()
        .map(|k| (k.to_string(), component_value(&components, k)))
        .collect();

    match render(ScoreDetailTemplate {
        video_id: &id,
        title: &title,
        seo_total: &fmt_score(row.seo_score),
        geo_total: &fmt_score(row.geo_score),
        total: &fmt_score(row.total_score),
        seo_components: &seo,
        geo_components: &geo,
        missing: false,
    }) {
        Ok(s) => html(s).into_response(),
        Err(e) => internal(e),
    }
}

async fn ideas_page(State(st): State<AppState>) -> Response {
    let rows = match st.db.list_ideas(None, IDEAS_LIMIT).await {
        Ok(i) => idea_views(&i),
        Err(e) => return internal(e),
    };
    let body = match render(IdeasTemplate { rows: &rows }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    page_impl("TubeForge — Ideas", "ideas", &body)
}

/// POST /ideas/{id}/{status}: mark an idea (same path the CLI uses), then
/// return the updated row fragment for `hx-swap="outerHTML"`.
async fn ideas_status(
    State(st): State<AppState>,
    Path((id, status)): Path<(i64, String)>,
    Headers(headers): Headers,
) -> Response {
    if let Err(resp) = csrf_guard(&headers, &st) {
        return resp.into_response();
    }
    if !IDEA_STATUSES.contains(&status.as_str()) {
        return (
            StatusCode::NOT_FOUND,
            Html(format!("unknown status {status:?}")),
        )
            .into_response();
    }
    if let Err(e) = st.db.set_idea_statuses(&[id], &status).await {
        return internal(e);
    }
    let row = match st.db.list_ideas(None, IDEAS_LIMIT).await {
        Ok(rows) => rows.into_iter().find(|r| r.idea_id == id),
        Err(e) => return internal(e),
    };
    match row {
        Some(r) => {
            let views = idea_views(&[r]);
            match render(IdeaRowTemplate { row: &views[0] }) {
                Ok(s) => html(s).into_response(),
                Err(e) => internal(e),
            }
        }
        None => (StatusCode::NOT_FOUND, Html("idea not found".to_string())).into_response(),
    }
}

async fn keywords_page(State(st): State<AppState>) -> Response {
    let rankings = match st.db.list_rankings().await {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    let trends = trend_rows(&rankings);
    let mut rows = Vec::with_capacity(trends.len());
    for t in &trends {
        let snapshots = t["snapshots"].as_array().cloned().unwrap_or_default();
        let spark = sparkline(
            &snapshots
                .iter()
                .map(|s| s["position"].as_i64())
                .collect::<Vec<_>>(),
        );
        let latest = t["latest_position"].as_i64();
        let previous = t["previous_position"].as_i64();
        let delta = t["delta"].as_i64();
        let topics: Vec<String> = t["snapshots"]
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|last| last["topics"].as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let delta_class = match delta {
            Some(d) if d < 0 => "up",
            Some(d) if d > 0 => "down",
            _ => "flat",
        };
        rows.push(KeywordTrendView {
            keyword: t["keyword"].as_str().unwrap_or("").to_string(),
            latest: fmt_position(latest),
            previous: fmt_position(previous),
            delta: match delta {
                Some(d) if d < 0 => format!("−{}", -d),
                Some(d) => format!("+{d}"),
                None => "—".to_string(),
            },
            delta_class: delta_class.to_string(),
            topics: topics.join(", "),
            spark,
        });
    }
    let body = match render(KeywordsTemplate {
        rows: &rows,
        checked: rankings.len(),
    }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    page_impl("TubeForge — Keywords", "keywords", &body)
}

async fn alerts_page(State(st): State<AppState>) -> Response {
    let alerts = match st.db.list_alerts(ALERTS_LIMIT).await {
        Ok(a) => alert_views(&a),
        Err(e) => return internal(e),
    };
    let unread = alerts.iter().filter(|a| !a.read).count();
    let panel_html = match render(AlertsPanelTemplate {
        alerts: &alerts,
        unread,
        count: alerts.len(),
    }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    let body = match render(AlertsPageTemplate {
        panel_html: &panel_html,
    }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    page_impl("TubeForge — Alerts", "alerts", &body)
}

/// POST /alerts/read: mark every alert read (CLI `--mark-read` path), then
/// return the fresh panel fragment (buttons AND list — so the buttons'
/// disabled state can't go stale).
async fn alerts_read(State(st): State<AppState>, Headers(headers): Headers) -> Response {
    if let Err(resp) = csrf_guard(&headers, &st) {
        return resp.into_response();
    }
    if let Err(e) = st.db.mark_alerts_read().await {
        return internal(e);
    }
    alerts_panel_fragment(&st).await
}

/// POST /alerts/clear: delete all alerts (CLI `alerts clear` path), then
/// return the (now empty) panel fragment.
async fn alerts_clear(State(st): State<AppState>, Headers(headers): Headers) -> Response {
    if let Err(resp) = csrf_guard(&headers, &st) {
        return resp.into_response();
    }
    if let Err(e) = st.db.clear_alerts().await {
        return internal(e);
    }
    alerts_panel_fragment(&st).await
}

/// Fresh alerts panel (action buttons + list) after a mutation.
async fn alerts_panel_fragment(st: &AppState) -> Response {
    match st.db.list_alerts(ALERTS_LIMIT).await {
        Ok(a) => {
            let views = alert_views(&a);
            let unread = views.iter().filter(|a| !a.read).count();
            match render(AlertsPanelTemplate {
                alerts: &views,
                unread,
                count: views.len(),
            }) {
                Ok(s) => html(s).into_response(),
                Err(e) => internal(e),
            }
        }
        Err(e) => internal(e),
    }
}

async fn scorecard_page(State(st): State<AppState>) -> Response {
    let card = match reports::scorecard(&st.db, &[]).await {
        Ok(c) => c,
        Err(e) => return internal(e),
    };
    let compared = card["compared"].as_u64().unwrap_or(0) as usize;
    let mut rows = Vec::new();
    if let Some(channels) = card["channels"].as_array() {
        for c in channels {
            rows.push(scorecard_view(c, false));
        }
    }
    let median = scorecard_view(&card["median"], true);
    let body = match render(ScorecardTemplate {
        rows: &rows,
        median,
        compared,
    }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    page_impl("TubeForge — Scorecard", "scorecard", &body)
}

async fn health_page(State(st): State<AppState>) -> Response {
    let stale_days = stale_days();
    let h = match reports::health(&st.db, stale_days).await {
        Ok(h) => h,
        Err(e) => return internal(e),
    };

    let counts: Vec<(String, i64)> = h["counts"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_i64().unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default();

    let stale: Vec<(String, String, String)> = h["stale_channels"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| {
                    (
                        s["channel_id"].as_str().unwrap_or("").to_string(),
                        s["title"].as_str().unwrap_or("").to_string(),
                        s["fetched_at"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let integrity = h["integrity"].as_str().unwrap_or("").to_string();
    let quota_date = h["quota"]["date"].as_str().unwrap_or("—").to_string();
    let last_ingest = h["last_ingest"]["at"].as_str().unwrap_or("—").to_string();
    let index_last = h["index"]["last_reindex_at"]
        .as_str()
        .unwrap_or("—")
        .to_string();
    let body = match render(HealthTemplate {
        counts: &counts,
        quota_used: &h["quota"]["videos_list_used"]
            .as_u64()
            .unwrap_or(0)
            .to_string(),
        quota_limit: &h["quota"]["daily_limit"].as_u64().unwrap_or(0).to_string(),
        quota_date: &quota_date,
        integrity_ok: integrity == "ok",
        integrity: &integrity,
        last_ingest: &last_ingest,
        index_fresh: h["index"]["fresh"].as_bool().unwrap_or(false),
        index_last: &index_last,
        stale: &stale,
        stale_days,
        engagement_complete: &h["metadata_completeness"]["engagement_complete"]
            .as_f64()
            .unwrap_or(0.0)
            .to_string(),
        disabled_videos: h["metadata_completeness"]["disabled_metrics"]["videos"]
            .as_i64()
            .unwrap_or(0),
        disabled_view: h["metadata_completeness"]["disabled_metrics"]["view_count"]
            .as_i64()
            .unwrap_or(0),
        disabled_like: h["metadata_completeness"]["disabled_metrics"]["like_count"]
            .as_i64()
            .unwrap_or(0),
        disabled_comment: h["metadata_completeness"]["disabled_metrics"]["comment_count"]
            .as_i64()
            .unwrap_or(0),
        privacy_unlisted: h["privacy"]["unlisted"].as_i64().unwrap_or(0),
        privacy_private: h["privacy"]["private"].as_i64().unwrap_or(0),
    }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    page_impl("TubeForge — Health", "health", &body)
}

/// Plain-text liveness endpoint for the agent contract / curl checks.
async fn healthz() -> Response {
    (StatusCode::OK, "ok").into_response()
}

/// Vendored htmx (offline-first; no CDN). Pinned in `static/htmx.min.js`.
async fn htmx_js() -> Response {
    (
        StatusCode::OK,
        [(
            http::header::CONTENT_TYPE.as_str(),
            "application/javascript",
        )],
        include_str!("../static/htmx.min.js"),
    )
        .into_response()
}

/// Vendored htmx SSE extension (offline-first; no CDN). Pinned in
/// `static/sse.js` — the v2.0.9 release asset is not published, so this is
/// the htmx-2.x source from htmx-extensions `main` (sse-connect/sse-swap
/// attribute syntax).
async fn sse_js() -> Response {
    (
        StatusCode::OK,
        [(
            http::header::CONTENT_TYPE.as_str(),
            "application/javascript",
        )],
        include_str!("../static/sse.js"),
    )
        .into_response()
}

async fn not_found(ReqUri(uri): ReqUri) -> Response {
    let path = uri.path().to_string();
    let body = match render(NotFoundTemplate { path: &path }) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    match render_base("TubeForge — 404", "", &body) {
        Ok(html) => (StatusCode::NOT_FOUND, Html(html)).into_response(),
        Err(e) => internal(e),
    }
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// Full-page wrapper: base layout + already-rendered inner HTML.
fn page_impl(title: &str, active: &str, body: &str) -> Response {
    match render_base(title, active, body) {
        Ok(html) => Html(html).into_response(),
        Err(e) => internal(e),
    }
}

fn render_base(title: &str, active: &str, body: &str) -> Result<String, TubeforgeError> {
    render(BaseTemplate {
        title,
        active,
        content: body,
    })
}

fn html(s: String) -> Html {
    Html(s)
}

fn render(t: impl Template) -> Result<String, TubeforgeError> {
    t.render()
        .map_err(|e| crate::error::storage_err("TEMPLATE", e))
}

fn internal(err: TubeforgeError) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Html(format!(
            "<h1>500 — internal error</h1><pre>{}</pre>",
            svg::esc(&err.to_string())
        )),
    )
        .into_response()
}

/// POST-only CSRF gate: see `csrf::origin_allowed` for the documented policy.
/// The Err payload is kept small (StatusCode + static message) to satisfy
/// clippy's `result_large_err`.
fn csrf_guard(headers: &HeaderMap, st: &AppState) -> Result<(), (StatusCode, &'static str)> {
    if csrf::origin_allowed(headers, &st.bind) {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "forbidden: Origin/Referer mismatch"))
    }
}

fn stale_days() -> u32 {
    match std::env::var("TUBEFORGE_STALE_DAYS") {
        Ok(v) => v.parse().unwrap_or(reports::DEFAULT_STALE_DAYS),
        Err(_) => reports::DEFAULT_STALE_DAYS,
    }
}

/// Health-card grid (shared by the page and the SSE stream).
async fn counts(st: &AppState) -> CountsTemplate {
    let h = reports::health(&st.db, stale_days()).await;
    let integrity = h
        .as_ref()
        .map(|h| h["integrity"].as_str().unwrap_or("").to_string())
        .unwrap_or_else(|e| format!("FAILED: {e}"));
    CountsTemplate {
        videos: count_of(&h, "videos"),
        channels: count_of(&h, "channels"),
        scores: count_of(&h, "scores"),
        ideas: count_of(&h, "ideas"),
        quota_used: h
            .as_ref()
            .ok()
            .and_then(|h| h["quota"]["videos_list_used"].as_u64())
            .unwrap_or(0),
        quota_limit: h
            .as_ref()
            .ok()
            .and_then(|h| h["quota"]["daily_limit"].as_u64())
            .unwrap_or(0),
        integrity_ok: integrity == "ok",
        integrity,
        last_ingest: h
            .as_ref()
            .ok()
            .and_then(|h| h["last_ingest"]["at"].as_str().map(str::to_string))
            .unwrap_or_else(|| "—".to_string()),
        stale: h
            .as_ref()
            .ok()
            .and_then(|h| h["stale_channels"].as_array())
            .map(|a| a.len() as i64)
            .unwrap_or(0),
        index_fresh: h
            .as_ref()
            .ok()
            .and_then(|h| h["index"]["fresh"].as_bool())
            .unwrap_or(false),
    }
}

fn count_of(h: &Result<Value, TubeforgeError>, key: &str) -> i64 {
    h.as_ref()
        .ok()
        .and_then(|h| h["counts"][key].as_i64())
        .unwrap_or(0)
}

/// Views-per-channel horizontal bars (dashboard home chart).
async fn view_bars(st: &AppState) -> Result<String, TubeforgeError> {
    let videos = st.db.all_videos().await?;
    let channels = st.db.all_channels().await?;
    let mut by_channel: HashMap<String, (String, i64)> = HashMap::new();
    for v in &videos {
        if let Some(cid) = &v.channel_id {
            let title = channels
                .iter()
                .find(|c| &c.channel_id == cid)
                .map(|c| c.title.clone())
                .unwrap_or_else(|| cid.clone());
            let e = by_channel.entry(cid.clone()).or_insert((title, 0));
            e.1 += v.view_count.unwrap_or(0);
        }
    }
    let mut items: Vec<(String, i64)> = by_channel.into_values().collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.1));
    items.truncate(8);
    Ok(svg::views_bars(&items))
}

/// SEO total score distribution (dashboard home chart).
async fn score_histogram(st: &AppState) -> String {
    match st.db.all_scores().await {
        Ok(scores) => svg::score_histogram(&scores.iter().map(|s| s.seo_score).collect::<Vec<_>>()),
        Err(_) => String::new(),
    }
}

/// Joined video+scores table rows, filtered by `q` (title, case-insensitive)
/// and capped at `limit`. Scored videos first (total DESC), then unscored.
async fn score_rows(
    st: &AppState,
    q: &str,
    limit: usize,
) -> Result<(Vec<ScoreRowView>, usize), TubeforgeError> {
    let videos = st.db.all_videos().await?;
    let scores = st.db.all_scores().await?;
    let channels = st.db.all_channels().await?;
    let channel_title: HashMap<&str, &str> = channels
        .iter()
        .map(|c| (c.channel_id.as_str(), c.title.as_str()))
        .collect();

    let ql = q.trim().to_lowercase();
    let filtered: Vec<_> = videos
        .iter()
        .filter(|v| ql.is_empty() || v.title.to_lowercase().contains(&ql))
        .collect();
    let total = filtered.len();

    let score_by_id: HashMap<&str, &crate::storage::db::ScoreRow> =
        scores.iter().map(|s| (s.video_id.as_str(), s)).collect();

    let mut scored: Vec<&crate::storage::db::VideoRow> = Vec::new();
    let mut unscored: Vec<&crate::storage::db::VideoRow> = Vec::new();
    for v in filtered {
        if score_by_id.contains_key(v.video_id.as_str()) {
            scored.push(v);
        } else {
            unscored.push(v);
        }
    }
    scored.sort_by(|a, b| {
        let sa = score_by_id[a.video_id.as_str()].total_score;
        let sb = score_by_id[b.video_id.as_str()].total_score;
        sb.total_cmp(&sa)
    });
    scored.extend(unscored);
    scored.truncate(limit);

    let rows = scored
        .iter()
        .map(|v| {
            let has_score = score_by_id.contains_key(v.video_id.as_str());
            let s = score_by_id.get(v.video_id.as_str());
            ScoreRowView {
                video_id: v.video_id.clone(),
                title: v.title.clone(),
                channel: v
                    .channel_id
                    .as_deref()
                    .and_then(|cid| channel_title.get(cid).copied())
                    .unwrap_or("—")
                    .to_string(),
                category: v
                    .category_id
                    .as_deref()
                    .map(|cid| {
                        crate::categories::category_name(cid)
                            .unwrap_or(cid)
                            .to_string()
                    })
                    .unwrap_or_else(|| "—".to_string()),
                seo: s
                    .map(|s| fmt_score(s.seo_score))
                    .unwrap_or_else(|| "—".to_string()),
                geo: s
                    .map(|s| fmt_score(s.geo_score))
                    .unwrap_or_else(|| "—".to_string()),
                total: s
                    .map(|s| fmt_score(s.total_score))
                    .unwrap_or_else(|| "—".to_string()),
                has_score,
            }
        })
        .collect();
    Ok((rows, total))
}

fn idea_views(rows: &[IdeaRow]) -> Vec<IdeaRowView> {
    rows.iter()
        .map(|i| IdeaRowView {
            id: i.idea_id,
            title: i.title_suggestion.clone(),
            score: format!("{:.1}", i.score),
            status: i.status.clone(),
            source: i.source_video.clone().unwrap_or_else(|| "—".to_string()),
            created: i.created_at.clone(),
        })
        .collect()
}

fn alert_views(rows: &[crate::storage::db::AlertRow]) -> Vec<AlertRowView> {
    rows.iter()
        .map(|a| AlertRowView {
            id: a.alert_id,
            kind: a.kind.clone(),
            severity: a.severity.clone(),
            channel_id: a.channel_id.clone().unwrap_or_else(|| "—".to_string()),
            message: a.message.clone(),
            created_at: a.created_at.clone(),
            read: a.read_at.is_some(),
        })
        .collect()
}

fn scorecard_view(c: &Value, is_median: bool) -> ScorecardRowView {
    let num = |k: &str| c[k].as_f64().unwrap_or(0.0);
    let round = |k: &str| format!("{:.2}", num(k));
    ScorecardRowView {
        channel_id: c["channel_id"].as_str().unwrap_or("median").to_string(),
        title: c["title"].as_str().unwrap_or("Median").to_string(),
        videos: c["videos"].as_u64().unwrap_or(0).to_string(),
        total_views: c["total_views"].as_u64().unwrap_or(0).to_string(),
        views_growth: c["views_growth"]
            .as_f64()
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "—".to_string()),
        avg_title_len: c["avg_title_len"]
            .as_f64()
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| round("avg_title_len")),
        digit_ratio: c["digit_ratio"]
            .as_f64()
            .map(|v| format!("{:.0}%", v * 100.0))
            .unwrap_or_else(|| round("digit_ratio")),
        howto_ratio: c["howto_ratio"]
            .as_f64()
            .map(|v| format!("{:.0}%", v * 100.0))
            .unwrap_or_else(|| round("howto_ratio")),
        tag_overlap: round("tag_overlap"),
        centrality: round("centrality"),
        seo_avg: c["seo"]["avg"]
            .as_f64()
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "—".to_string()),
        seo_median: c["seo"]["median"]
            .as_f64()
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "—".to_string()),
        seo_min: c["seo"]["min"]
            .as_f64()
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "—".to_string()),
        seo_max: c["seo"]["max"]
            .as_f64()
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "—".to_string()),
        scored: c["seo"]["scored"].as_u64().unwrap_or(0).to_string(),
        is_median,
    }
}

fn component_value(components: &Value, key: &str) -> String {
    components
        .get(key)
        .and_then(|v| v.as_f64())
        .map(fmt_score)
        .unwrap_or_else(|| "—".to_string())
}

fn fmt_score(v: f64) -> String {
    format!("{v:.1}")
}

fn fmt_position(p: Option<i64>) -> String {
    match p {
        Some(n) => format!("#{n}"),
        None => "—".to_string(),
    }
}

/// Small helper template for the home page's top-ideas teaser (rendered
/// separately, embedded via `|safe`).
#[derive(Template)]
#[template(path = "dashboard/home_ideas.html")]
struct IdeaRowsFragment<'a> {
    rows: &'a [IdeaRowView],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts_only() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("::1"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.5"));
        assert!(!is_loopback_host("example.com"));
    }

    #[test]
    fn component_keys_cover_22() {
        assert_eq!(SEO_COMPONENT_KEYS.len(), 15);
        assert_eq!(GEO_COMPONENT_KEYS.len(), 7);
    }

    #[test]
    fn component_value_renders_or_dash() {
        let c = serde_json::json!({"keyword_title": 82.5, "location_signal": 0.0});
        assert_eq!(component_value(&c, "keyword_title"), "82.5");
        assert_eq!(component_value(&c, "location_signal"), "0.0");
        assert_eq!(component_value(&c, "missing_key"), "—");
    }

    #[test]
    fn position_formatting() {
        assert_eq!(fmt_position(Some(3)), "#3");
        assert_eq!(fmt_position(None), "—");
    }

    #[test]
    fn counts_equality_detects_change() {
        let c = |videos: i64| CountsTemplate {
            videos,
            channels: 2,
            scores: 1,
            ideas: 1,
            quota_used: 100,
            quota_limit: 10_000,
            integrity_ok: true,
            integrity: "ok".to_string(),
            last_ingest: "2026-08-01T00:00:00Z".to_string(),
            stale: 0,
            index_fresh: true,
        };
        assert_eq!(c(2), c(2), "identical values are unchanged");
        assert_ne!(c(2), c(3), "a single count change must be detected");
        assert_ne!(
            c(2),
            CountsTemplate {
                integrity_ok: false,
                ..c(2)
            }
        );
    }
}
