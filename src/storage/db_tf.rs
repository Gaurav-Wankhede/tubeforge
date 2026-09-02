//! tfdb-backed repository (migration of the legacy turso `Db`).
//!
//! Same public API as the legacy `db.rs`, but built on `crate::tfdb` (the
//! from-scratch engine) instead of turso. The engine is synchronous, so each
//! method wraps its work in an `async` block to keep the historical
//! `.await`-based call sites compiling unchanged. Interior mutability
//! (`Arc<Mutex<Engine>>`) lets every method keep the legacy `&self` receiver
//! even though tfdb writes need `&mut Engine`.
//!
//! Storage conventions mirrored from `db.rs`:
//! - JSON columns (tags, topic_categories, rationale, components, topics,
//!   points, properties, suggested_tags, related_keywords, top_entities) are
//!   stored as `Value::Json` but exposed to callers as JSON *strings*.
//! - Auto-increment ids (alerts, ideas, tags, ingest_log, keyword_research)
//!   are assigned in Rust via `next_id` since tfdb has no AUTOINCREMENT.
//! - Composite keys (keyword_rankings, edges, channel_snapshots, video_tags,
//!   competitor_tags) are folded into a single tfdb pk column.
//!
//! Interior mutability (`Arc<Mutex<Engine>>`) lets every method keep the legacy
//! `&self` receiver even though tfdb writes need `&mut Engine`.

// tfdb is synchronous; the async methods here only exist to keep the legacy
// `.await`-based call sites compiling unchanged. None of them await internally.
#![allow(clippy::unused_async)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::error::TubeforgeError;
use crate::storage::schema::SCHEMA_VERSION;
use crate::tfdb::store::{Engine, Row, Value};

/// A live tfdb-backed database connection.
///
/// The engine lives behind `Arc<Mutex<Engine>>` so a `Db` is `Send + Sync`
/// and cheaply `Clone` — the serve layer shares one `Db` across `tokio::spawn`
/// tasks, and a clone is a second handle to the same in-memory engine.
#[derive(Clone)]
pub struct Db {
    pub engine: Arc<Mutex<Engine>>,
    pub path: PathBuf,
}

// ---------------------------------------------------------------------------
// Row types (identical to db.rs — callers must not change).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ChannelRow {
    pub channel_id: String,
    pub handle: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub country: Option<String>,
    pub subscriber_count: Option<i64>,
    pub video_count: Option<i64>,
    pub source: String,
    pub etag: Option<String>,
    pub fetched_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct VideoRow {
    pub video_id: String,
    pub channel_id: Option<String>,
    pub title: String,
    pub description: String,
    pub tags: String,
    pub category_id: Option<String>,
    pub duration_sec: Option<i64>,
    pub published_at: String,
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub thumb_url: Option<String>,
    pub source: String,
    pub fetched_at: String,
    pub updated_at: String,
    pub recording_date: Option<String>,
    pub recording_location_name: Option<String>,
    pub recording_lat: Option<f64>,
    pub recording_lng: Option<f64>,
    pub topic_categories: String,
    pub privacy_status: Option<String>,
}

/// Selective patch envelope for idempotent, coalesced video updates.
#[derive(Debug, Clone, Default)]
pub struct VideoPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub duration_sec: Option<i64>,
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub published_at: Option<String>,
    pub tags: Option<Vec<String>>,
    pub thumb_url: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct ScoreRow {
    pub video_id: String,
    pub seo_score: f64,
    pub geo_score: f64,
    pub total_score: f64,
    pub components: String,
    pub computed_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct KeywordRow {
    pub keyword: String,
    pub niche: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct RankingRow {
    pub keyword: String,
    pub checked_at: String,
    pub video_id: Option<String>,
    pub position: Option<i64>,
    pub topics: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EdgeRow {
    pub from_channel: String,
    pub to_channel: String,
    pub weight: f64,
    pub source: String,
}

#[derive(Debug, Clone, Default)]
pub struct IdeaRow {
    pub idea_id: i64,
    pub title_suggestion: String,
    pub rationale: String,
    pub score: f64,
    pub status: String,
    pub source_video: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct AlertRow {
    pub alert_id: i64,
    pub kind: String,
    pub channel_id: Option<String>,
    pub message: String,
    pub severity: String,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct IngestLogRow {
    pub batch_id: String,
    pub item: String,
    pub status: String,
    pub detail: Option<String>,
    pub at: String,
}

#[derive(Debug, Clone, Default)]
pub struct TagRow {
    pub tag_id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct VideoTagRow {
    pub video_id: String,
    pub tag_id: i64,
    pub position: i64,
    pub source: String,
}

#[derive(Debug, Clone, Default)]
pub struct CompetitorTagRow {
    pub channel_id: String,
    pub tag_name: String,
    pub video_count: i64,
    pub avg_views: f64,
    pub rank: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct TranscriptRow {
    pub video_id: String,
    pub lang: String,
    pub source: String,
    pub text: String,
    pub word_count: i64,
    pub fetched_at: String,
}

#[derive(Debug, Clone)]
pub struct CommentRow {
    pub comment_id: String,
    pub video_id: String,
    pub author: String,
    pub text: String,
    pub like_count: i64,
    pub published_at: String,
    pub fetched_at: String,
}

#[derive(Debug, Clone)]
pub struct KeywordTrendingRow {
    pub keyword: String,
    pub opportunity_score: f64,
    pub competition_score: f64,
    pub serp_mean_views: f64,
    pub volume_label: String,
    pub actively_published: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeywordResearchRow {
    pub keyword: String,
    pub at: String,
    pub volume_label: String,
    pub serp_total: i64,
    pub serp_mean_views: f64,
    pub ranking_channels: i64,
    pub competition_score: f64,
    pub opportunity_score: f64,
    pub actively_published: bool,
    pub suggested_tags: String,
    pub related_keywords: String,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct UserChannelRow {
    pub channel_id: String,
    pub custom_name: Option<String>,
    pub is_primary: bool,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A raw row of the `kg_entities` table (used by the KG builder/loader).
#[derive(Debug, Clone, Default)]
pub struct KgEntityRow {
    pub entity_id: String,
    pub entity_type: String,
    pub canonical_name: String,
    pub display_name: String,
    pub properties: String,
    pub centrality: Option<f64>,
    pub community_id: Option<i64>,
    pub source: String,
    pub source_ref: String,
}

/// A raw row of the `kg_relations` table (used by the KG builder/loader).
#[derive(Debug, Clone)]
pub struct KgRelationRow {
    pub from_entity: String,
    pub to_entity: String,
    pub relation_type: String,
    pub weight: f64,
    pub source: String,
}

/// A raw row of the `kg_communities` table (used by the KG builder/loader).
#[derive(Debug, Clone)]
pub struct KgCommunityRow {
    pub community_id: i64,
    pub community_type: String,
    pub member_count: i64,
    pub top_entities: String,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Greedy bot row types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GreedyHistoryRow {
    pub research_id: i64,
    pub topic: String,
    pub researched_at: String,
    pub video_ids_json: String,
    pub video_count: i64,
    pub mean_views: f64,
    pub source: String,
    pub duration_ms: i64,
}

#[derive(Debug, Clone)]
pub struct GreedyTopicLogRow {
    pub log_id: i64,
    pub topic: String,
    pub status: String,
    pub reason: String,
    pub attempted_at: String,
}

#[derive(Debug, Clone)]
pub struct GreedySeedRow {
    pub seed_id: i64,
    pub seed: String,
    pub source: String,
    pub added_at: String,
    pub active: bool,
}

/// Parameters for inserting a greedy research history row.
pub struct GreedyHistoryInsert<'a> {
    pub topic: &'a str,
    pub researched_at: &'a str,
    pub video_ids_json: &'a str,
    pub video_count: i64,
    pub mean_views: f64,
    pub source: &'a str,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KanbanTicketRow {
    pub ticket_id: String,
    pub title: String,
    pub channel: String,
    pub status: String,
    pub topic: Option<String>,
    pub framework: Option<String>,
    pub optimal_duration_sec: Option<i64>,
    pub target_keyword: Option<String>,
    pub youtube_url: Option<String>,
    pub video_id: Option<String>,
    pub research_ref: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Value mapping helpers (tfdb Row <-> legacy row structs).
// ---------------------------------------------------------------------------

fn v_text(s: impl Into<String>) -> Value {
    Value::Text(s.into())
}

fn v_opt_text(s: Option<&str>) -> Value {
    match s {
        Some(t) => Value::Text(t.to_string()),
        None => Value::Null,
    }
}

fn v_int(i: Option<i64>) -> Value {
    match i {
        Some(n) => Value::Int(n),
        None => Value::Null,
    }
}

fn v_float(f: Option<f64>) -> Value {
    match f {
        Some(v) => Value::Float(v),
        None => Value::Null,
    }
}

/// Parse a JSON *string* into a `Value::Json`, falling back to raw text when
/// the string is not valid JSON (so round-trips never lose data).
fn v_json(s: &str) -> Value {
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(j) => Value::Json(j),
        Err(_) => Value::Text(s.to_string()),
    }
}

fn v_opt_json(s: Option<&str>) -> Value {
    match s {
        Some(s) => v_json(s),
        None => Value::Null,
    }
}

fn t(row: &Row, col: &str) -> String {
    match row.get(col) {
        Some(Value::Text(s)) => s.clone(),
        _ => String::new(),
    }
}

fn opt_s(row: &Row, col: &str) -> Option<String> {
    match row.get(col) {
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

fn i(row: &Row, col: &str) -> i64 {
    row.get(col).and_then(|v| v.as_i64()).unwrap_or(0)
}

fn opt_i(row: &Row, col: &str) -> Option<i64> {
    row.get(col).and_then(|v| v.as_i64())
}

fn f(row: &Row, col: &str) -> f64 {
    row.get(col).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

fn opt_f(row: &Row, col: &str) -> Option<f64> {
    row.get(col).and_then(|v| v.as_f64())
}

fn b(row: &Row, col: &str) -> bool {
    match row.get(col) {
        Some(Value::Bool(x)) => *x,
        Some(Value::Int(x)) => *x != 0,
        Some(Value::Text(x)) => x == "1" || x.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn json_s(row: &Row, col: &str) -> String {
    match row.get(col) {
        Some(Value::Json(j)) => j.to_string(),
        Some(Value::Text(s)) => s.clone(),
        _ => String::new(),
    }
}

fn opt_json_s(row: &Row, col: &str) -> Option<String> {
    match row.get(col) {
        Some(Value::Json(j)) => Some(j.to_string()),
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Next auto-increment id for a table whose pk is a numeric `id_col`.
fn next_id(eng: &Engine, table: &str, id_col: &str) -> Result<i64, TubeforgeError> {
    let mut max = 0i64;
    for r in eng.all(table)? {
        let n = i(&r, id_col);
        if n > max {
            max = n;
        }
    }
    Ok(max + 1)
}

fn channel_to_row(c: &ChannelRow) -> Row {
    let mut row = Row::new();
    row.insert("channel_id".to_string(), v_text(&c.channel_id));
    row.insert("handle".to_string(), v_opt_text(c.handle.as_deref()));
    row.insert("title".to_string(), v_text(&c.title));
    row.insert(
        "description".to_string(),
        v_opt_text(c.description.as_deref()),
    );
    row.insert(
        "avatar_url".to_string(),
        v_opt_text(c.avatar_url.as_deref()),
    );
    row.insert("country".to_string(), v_opt_text(c.country.as_deref()));
    row.insert("subscriber_count".to_string(), v_int(c.subscriber_count));
    row.insert("video_count".to_string(), v_int(c.video_count));
    row.insert("source".to_string(), v_text(&c.source));
    row.insert("etag".to_string(), v_opt_text(c.etag.as_deref()));
    row.insert("fetched_at".to_string(), v_text(&c.fetched_at));
    row.insert("updated_at".to_string(), v_text(&c.updated_at));
    row
}

fn channel_from_row(r: &Row) -> ChannelRow {
    ChannelRow {
        channel_id: t(r, "channel_id"),
        handle: opt_s(r, "handle"),
        title: t(r, "title"),
        description: opt_s(r, "description"),
        avatar_url: opt_s(r, "avatar_url"),
        country: opt_s(r, "country"),
        subscriber_count: opt_i(r, "subscriber_count"),
        video_count: opt_i(r, "video_count"),
        source: t(r, "source"),
        etag: opt_s(r, "etag"),
        fetched_at: t(r, "fetched_at"),
        updated_at: t(r, "updated_at"),
    }
}

fn video_to_row(v: &VideoRow) -> Row {
    let mut row = Row::new();
    row.insert("video_id".to_string(), v_text(&v.video_id));
    row.insert(
        "channel_id".to_string(),
        v_opt_text(v.channel_id.as_deref()),
    );
    row.insert("title".to_string(), v_text(&v.title));
    row.insert("description".to_string(), v_text(&v.description));
    row.insert("tags".to_string(), v_json(&v.tags));
    row.insert(
        "category_id".to_string(),
        v_opt_text(v.category_id.as_deref()),
    );
    row.insert("duration_sec".to_string(), v_int(v.duration_sec));
    row.insert("published_at".to_string(), v_text(&v.published_at));
    row.insert("view_count".to_string(), v_int(v.view_count));
    row.insert("like_count".to_string(), v_int(v.like_count));
    row.insert("comment_count".to_string(), v_int(v.comment_count));
    row.insert("thumb_url".to_string(), v_opt_text(v.thumb_url.as_deref()));
    row.insert("source".to_string(), v_text(&v.source));
    row.insert("fetched_at".to_string(), v_text(&v.fetched_at));
    row.insert("updated_at".to_string(), v_text(&v.updated_at));
    row.insert(
        "recording_date".to_string(),
        v_opt_text(v.recording_date.as_deref()),
    );
    row.insert(
        "recording_location_name".to_string(),
        v_opt_text(v.recording_location_name.as_deref()),
    );
    row.insert("recording_lat".to_string(), v_float(v.recording_lat));
    row.insert("recording_lng".to_string(), v_float(v.recording_lng));
    row.insert("topic_categories".to_string(), v_json(&v.topic_categories));
    row.insert(
        "privacy_status".to_string(),
        v_opt_text(v.privacy_status.as_deref()),
    );
    row
}

fn video_from_row(r: &Row) -> VideoRow {
    VideoRow {
        video_id: t(r, "video_id"),
        channel_id: opt_s(r, "channel_id"),
        title: t(r, "title"),
        description: t(r, "description"),
        tags: json_s(r, "tags"),
        category_id: opt_s(r, "category_id"),
        duration_sec: opt_i(r, "duration_sec"),
        published_at: t(r, "published_at"),
        view_count: opt_i(r, "view_count"),
        like_count: opt_i(r, "like_count"),
        comment_count: opt_i(r, "comment_count"),
        thumb_url: opt_s(r, "thumb_url"),
        source: t(r, "source"),
        fetched_at: t(r, "fetched_at"),
        updated_at: t(r, "updated_at"),
        recording_date: opt_s(r, "recording_date"),
        recording_location_name: opt_s(r, "recording_location_name"),
        recording_lat: opt_f(r, "recording_lat"),
        recording_lng: opt_f(r, "recording_lng"),
        topic_categories: json_s(r, "topic_categories"),
        privacy_status: opt_s(r, "privacy_status"),
    }
}

fn score_from_row(r: &Row) -> ScoreRow {
    ScoreRow {
        video_id: t(r, "video_id"),
        seo_score: f(r, "seo_score"),
        geo_score: f(r, "geo_score"),
        total_score: f(r, "total_score"),
        components: json_s(r, "components"),
        computed_at: t(r, "computed_at"),
    }
}

fn keyword_from_row(r: &Row) -> KeywordRow {
    KeywordRow {
        keyword: t(r, "keyword"),
        niche: opt_s(r, "niche"),
        created_at: t(r, "created_at"),
    }
}

fn ranking_from_row(r: &Row) -> RankingRow {
    RankingRow {
        keyword: t(r, "keyword"),
        checked_at: t(r, "checked_at"),
        video_id: opt_s(r, "video_id"),
        position: opt_i(r, "position"),
        topics: opt_json_s(r, "topics"),
    }
}

fn edge_from_row(r: &Row) -> EdgeRow {
    EdgeRow {
        from_channel: t(r, "from_channel"),
        to_channel: t(r, "to_channel"),
        weight: f(r, "weight"),
        source: t(r, "source"),
    }
}

fn idea_from_row(r: &Row) -> IdeaRow {
    IdeaRow {
        idea_id: i(r, "idea_id"),
        title_suggestion: t(r, "title_suggestion"),
        rationale: json_s(r, "rationale"),
        score: f(r, "score"),
        status: t(r, "status"),
        source_video: opt_s(r, "source_video"),
        created_at: t(r, "created_at"),
    }
}

fn alert_from_row(r: &Row) -> AlertRow {
    AlertRow {
        alert_id: i(r, "alert_id"),
        kind: t(r, "kind"),
        channel_id: opt_s(r, "channel_id"),
        message: t(r, "message"),
        severity: t(r, "severity"),
        created_at: t(r, "created_at"),
        read_at: opt_s(r, "read_at"),
    }
}

fn ingest_log_from_row(r: &Row) -> IngestLogRow {
    IngestLogRow {
        batch_id: t(r, "batch_id"),
        item: t(r, "item"),
        status: t(r, "status"),
        detail: opt_s(r, "detail"),
        at: t(r, "at"),
    }
}

fn transcript_from_row(r: &Row) -> TranscriptRow {
    TranscriptRow {
        video_id: t(r, "video_id"),
        lang: t(r, "lang"),
        source: t(r, "source"),
        text: t(r, "text"),
        word_count: i(r, "word_count"),
        fetched_at: t(r, "fetched_at"),
    }
}

fn comment_from_row(r: &Row) -> CommentRow {
    CommentRow {
        comment_id: t(r, "comment_id"),
        video_id: t(r, "video_id"),
        author: t(r, "author"),
        text: t(r, "text"),
        like_count: i(r, "like_count"),
        published_at: t(r, "published_at"),
        fetched_at: t(r, "fetched_at"),
    }
}

fn keyword_research_from_row(r: &Row) -> KeywordResearchRow {
    KeywordResearchRow {
        keyword: t(r, "keyword"),
        at: t(r, "at"),
        volume_label: t(r, "volume_label"),
        serp_total: i(r, "serp_total"),
        serp_mean_views: f(r, "serp_mean_views"),
        ranking_channels: i(r, "ranking_channels"),
        competition_score: f(r, "competition_score"),
        opportunity_score: f(r, "opportunity_score"),
        actively_published: b(r, "actively_published"),
        suggested_tags: json_s(r, "suggested_tags"),
        related_keywords: json_s(r, "related_keywords"),
    }
}

fn kanban_to_row(k: &KanbanTicketRow) -> Row {
    let mut r = Row::new();
    r.insert("ticket_id".to_string(), v_text(&k.ticket_id));
    r.insert("title".to_string(), v_text(&k.title));
    r.insert("channel".to_string(), v_text(&k.channel));
    r.insert("status".to_string(), v_text(&k.status));
    r.insert("topic".to_string(), v_opt_text(k.topic.as_deref()));
    r.insert("framework".to_string(), v_opt_text(k.framework.as_deref()));
    r.insert(
        "optimal_duration_sec".to_string(),
        v_int(k.optimal_duration_sec),
    );
    r.insert(
        "target_keyword".to_string(),
        v_opt_text(k.target_keyword.as_deref()),
    );
    r.insert(
        "youtube_url".to_string(),
        v_opt_text(k.youtube_url.as_deref()),
    );
    r.insert("video_id".to_string(), v_opt_text(k.video_id.as_deref()));
    r.insert(
        "research_ref".to_string(),
        v_opt_text(k.research_ref.as_deref()),
    );
    r.insert("notes".to_string(), v_opt_text(k.notes.as_deref()));
    r.insert("created_at".to_string(), v_text(&k.created_at));
    r.insert("updated_at".to_string(), v_text(&k.updated_at));
    r
}

fn kanban_from_row(r: &Row) -> KanbanTicketRow {
    KanbanTicketRow {
        ticket_id: t(r, "ticket_id"),
        title: t(r, "title"),
        channel: t(r, "channel"),
        status: t(r, "status"),
        topic: opt_s(r, "topic"),
        framework: opt_s(r, "framework"),
        optimal_duration_sec: opt_i(r, "optimal_duration_sec"),
        target_keyword: opt_s(r, "target_keyword"),
        youtube_url: opt_s(r, "youtube_url"),
        video_id: opt_s(r, "video_id"),
        research_ref: opt_s(r, "research_ref"),
        notes: opt_s(r, "notes"),
        created_at: t(r, "created_at"),
        updated_at: t(r, "updated_at"),
    }
}

// ---------------------------------------------------------------------------
// Db
// ---------------------------------------------------------------------------

impl Db {
    /// Open (creating if needed) the tfdb database at `path`, register every
    /// domain table, and record the schema version.
    pub async fn open(path: &Path) -> Result<Db, TubeforgeError> {
        let mut engine = Engine::open(path)?;
        for schema in crate::tfdb::tfdb_schema::all() {
            engine.create_table(schema);
        }
        let db = Db {
            engine: Arc::new(Mutex::new(engine)),
            path: path.to_path_buf(),
        };
        db.meta_set("schema_version", &SCHEMA_VERSION.to_string())
            .await?;
        Ok(db)
    }

    /// Recorded schema version (meta "schema_version"), default 0.
    pub async fn user_version(&self) -> Result<i64, TubeforgeError> {
        let v = self
            .meta_get("schema_version")
            .await?
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        Ok(v)
    }

    /// tfdb has no journal; report WAL for API parity.
    pub async fn journal_mode(&self) -> Result<String, TubeforgeError> {
        Ok("wal".to_string())
    }

    /// No-op: tfdb is crash-safe by construction (WAL + CRC + checkpoint).
    pub async fn integrity_check(&self) -> Result<(), TubeforgeError> {
        Ok(())
    }

    /// Checkpoint the in-memory database to disk and truncate WAL.
    pub async fn checkpoint(&self) -> Result<u64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.checkpoint()
    }

    pub async fn meta_get(&self, key: &str) -> Result<Option<String>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng.get("meta", key)?.and_then(|r| opt_s(&r, "value")))
    }

    pub async fn meta_set(&self, key: &str, value: &str) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut row = Row::new();
        row.insert("key".to_string(), v_text(key));
        row.insert("value".to_string(), v_text(value));
        let mut tx = eng.begin();
        tx.put("meta", row)?;
        tx.commit()?;
        Ok(())
    }

    /// Row count helper. The `sql` argument is legacy SQL; tfdb has no SQL, so
    /// we only parse the `FROM <table>` clause and count that table (the exact
    /// callers in health/scorecard use plain `SELECT count(*) FROM <table>`).
    pub async fn count(&self, sql: &str) -> Result<i64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let table = extract_from_table(sql);
        let Some(t) = table else {
            return Ok(0);
        };
        if !eng.table_exists(&t) {
            return Ok(0);
        }
        // Apply a simple `WHERE col = 'value'` filter when present (the only
        // filtered count() callers use equality on a single column).
        if let Some((col, val)) = extract_where_eq(sql) {
            let rows = eng.find_eq(&t, &col, &Value::Text(val))?;
            return Ok(rows.len() as i64);
        }
        Ok(eng.count(&t).unwrap_or(0) as i64)
    }

    pub async fn table_exists(&self, table: &str) -> Result<bool, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng.table_exists(table))
    }

    // -- repository reads --------------------------------------------------

    pub async fn get_channel(&self, id: &str) -> Result<Option<ChannelRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng.get("channels", id)?.map(|r| channel_from_row(&r)))
    }

    pub async fn get_channel_by_handle(
        &self,
        handle: &str,
    ) -> Result<Option<ChannelRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng
            .find_eq("channels", "handle", &Value::Text(handle.to_string()))?
            .first()
            .map(channel_from_row))
    }

    pub async fn get_video(&self, id: &str) -> Result<Option<VideoRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng.get("videos", id)?.map(|r| video_from_row(&r)))
    }

    pub async fn all_channels(&self) -> Result<Vec<ChannelRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<ChannelRow> = eng.all("channels")?.iter().map(channel_from_row).collect();
        rows.sort_by(|a, b| a.channel_id.cmp(&b.channel_id));
        Ok(rows)
    }

    pub async fn all_videos(&self) -> Result<Vec<VideoRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<VideoRow> = eng.all("videos")?.iter().map(video_from_row).collect();
        rows.sort_by(|a, b| a.video_id.cmp(&b.video_id));
        Ok(rows)
    }

    pub async fn get_score(&self, video_id: &str) -> Result<Option<ScoreRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng.get("scores", video_id)?.map(|r| score_from_row(&r)))
    }

    pub async fn all_scores(&self) -> Result<Vec<ScoreRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<ScoreRow> = eng.all("scores")?.iter().map(score_from_row).collect();
        rows.sort_by(|a, b| b.total_score.total_cmp(&a.total_score));
        Ok(rows)
    }

    pub async fn list_keywords(&self) -> Result<Vec<KeywordRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<KeywordRow> = eng.all("keywords")?.iter().map(keyword_from_row).collect();
        rows.sort_by(|a, b| a.keyword.cmp(&b.keyword));
        Ok(rows)
    }

    pub async fn list_rankings(&self) -> Result<Vec<RankingRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<RankingRow> = eng
            .all("keyword_rankings")?
            .iter()
            .map(ranking_from_row)
            .collect();
        rows.sort_by(|a, b| {
            a.keyword
                .cmp(&b.keyword)
                .then(a.checked_at.cmp(&b.checked_at))
        });
        Ok(rows)
    }

    pub async fn ranking_count_at(&self, checked_at: &str) -> Result<u64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng
            .find_eq(
                "keyword_rankings",
                "checked_at",
                &Value::Text(checked_at.to_string()),
            )?
            .len() as u64)
    }

    pub async fn list_edges(&self) -> Result<Vec<EdgeRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<EdgeRow> = eng.all("edges")?.iter().map(edge_from_row).collect();
        rows.sort_by(|a, b| {
            a.from_channel
                .cmp(&b.from_channel)
                .then(a.to_channel.cmp(&b.to_channel))
        });
        Ok(rows)
    }

    pub async fn list_ideas(
        &self,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<IdeaRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut items: Vec<(f64, i64, IdeaRow)> = eng
            .all("ideas")?
            .into_iter()
            .filter(|r| status.is_none_or(|s| t(r, "status") == s))
            .map(|r| {
                let id = i(&r, "idea_id");
                let score = f(&r, "score");
                (score, id, idea_from_row(&r))
            })
            .collect();
        items.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
        if limit > 0 {
            items.truncate(limit);
        }
        Ok(items.into_iter().map(|(_, _, r)| r).collect())
    }

    pub async fn all_ideas(&self) -> Result<Vec<IdeaRow>, TubeforgeError> {
        self.list_ideas(None, 0).await
    }

    pub async fn list_alerts(&self, limit: usize) -> Result<Vec<AlertRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut items: Vec<(String, i64, AlertRow)> = eng
            .all("alerts")?
            .into_iter()
            .map(|r| {
                let id = i(&r, "alert_id");
                (t(&r, "created_at"), id, alert_from_row(&r))
            })
            .collect();
        items.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        if limit > 0 {
            items.truncate(limit);
        }
        Ok(items.into_iter().map(|(_, _, r)| r).collect())
    }

    pub async fn alert_exists(
        &self,
        kind: &str,
        channel_id: Option<&str>,
        message: &str,
    ) -> Result<bool, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(alert_exists_engine(&eng, kind, channel_id, message))
    }

    pub async fn list_competitors(&self) -> Result<Vec<String>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<(String, String)> = eng
            .all("competitors")?
            .into_iter()
            .map(|r| (t(&r, "added_at"), t(&r, "channel_id")))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        Ok(rows.into_iter().map(|(_, id)| id).collect())
    }

    pub async fn last_ingest(&self) -> Result<Option<IngestLogRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut items: Vec<(String, i64)> = eng
            .all("ingest_log")?
            .into_iter()
            .map(|r| (t(&r, "at"), i(&r, "log_id")))
            .collect();
        items.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let at = items
            .first()
            .map(|(at, id)| (at.clone(), *id))
            .unwrap_or_default();
        if items.is_empty() {
            return Ok(None);
        }
        let row = eng
            .all("ingest_log")?
            .into_iter()
            .find(|r| i(r, "log_id") == at.1);
        Ok(row.map(|r| ingest_log_from_row(&r)))
    }

    pub async fn count_tags(&self) -> Result<i64, TubeforgeError> {
        Ok(self.engine.lock().unwrap().count("tags")? as i64)
    }

    pub async fn kg_entity_count(&self) -> Result<usize, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        if eng.table_exists("kg_entities") {
            Ok(eng.count("kg_entities")? as usize)
        } else {
            Ok(0)
        }
    }

    pub async fn kg_relation_count(&self) -> Result<usize, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        if eng.table_exists("kg_relations") {
            Ok(eng.count("kg_relations")? as usize)
        } else {
            Ok(0)
        }
    }

    pub async fn kg_community_count(&self) -> Result<usize, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        if eng.table_exists("kg_communities") {
            Ok(eng.count("kg_communities")? as usize)
        } else {
            Ok(0)
        }
    }

    pub async fn tag_cloud(&self) -> Result<Vec<(String, i64)>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let tags: Vec<(i64, String)> = eng
            .all("tags")?
            .into_iter()
            .map(|r| (i(&r, "tag_id"), t(&r, "name")))
            .collect();
        let mut counts: HashMap<String, i64> = HashMap::new();
        for (_, name) in &tags {
            counts.entry(name.clone()).or_insert(0);
        }
        for vt in eng.all("video_tags")? {
            let tid = i(&vt, "tag_id");
            if let Some((_, name)) = tags.iter().find(|(id, _)| *id == tid) {
                *counts.entry(name.clone()).or_insert(0) += 1;
            }
        }
        let mut out: Vec<(String, i64)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(out)
    }

    pub async fn tag_gaps(
        &self,
        own_channel_id: &str,
    ) -> Result<Vec<(String, i64, i64)>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut comp: HashMap<String, i64> = HashMap::new();
        for ct in eng.all("competitor_tags")? {
            if t(&ct, "channel_id") == own_channel_id {
                continue;
            }
            *comp.entry(t(&ct, "tag_name")).or_insert(0) += i(&ct, "video_count");
        }
        let tag_names: HashMap<i64, String> = eng
            .all("tags")?
            .into_iter()
            .map(|r| (i(&r, "tag_id"), t(&r, "name")))
            .collect();
        let video_channels: HashMap<String, String> = eng
            .all("videos")?
            .into_iter()
            .filter_map(|r| {
                match (
                    r.get("video_id").and_then(|v| v.as_text()),
                    r.get("channel_id").and_then(|v| v.as_text()),
                ) {
                    (Some(vid), Some(cid)) => Some((vid.to_string(), cid.to_string())),
                    _ => None,
                }
            })
            .collect();
        let mut own: HashMap<String, i64> = HashMap::new();
        for vt in eng.all("video_tags")? {
            let vid = t(&vt, "video_id");
            if video_channels.get(&vid).map(String::as_str) != Some(own_channel_id) {
                continue;
            }
            let tid = i(&vt, "tag_id");
            if let Some(name) = tag_names.get(&tid) {
                *own.entry(name.clone()).or_insert(0) += 1;
            }
        }
        let mut out: Vec<(String, i64, i64)> = Vec::new();
        for (tag, comp_cnt) in &comp {
            let own_cnt = own.get(tag).copied().unwrap_or(0);
            if comp_cnt > &own_cnt {
                out.push((tag.clone(), *comp_cnt, own_cnt));
            }
        }
        out.sort_by_key(|x| std::cmp::Reverse(x.1 - x.2));
        Ok(out)
    }

    pub async fn get_video_tags(
        &self,
        video_id: &str,
    ) -> Result<Vec<(String, i64, String)>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut out: Vec<(String, i64, String)> = Vec::new();
        for vt in eng.find_eq("video_tags", "video_id", &Value::Text(video_id.to_string()))? {
            let tid = i(&vt, "tag_id");
            let name = eng
                .get("tags", &tid.to_string())?
                .map(|r| t(&r, "name"))
                .unwrap_or_default();
            out.push((name, i(&vt, "position"), t(&vt, "source")));
        }
        out.sort_by_key(|x| x.1);
        Ok(out)
    }

    pub async fn get_competitor_tag_stats(
        &self,
        channel_id: &str,
    ) -> Result<Vec<(String, i64, i64)>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<(String, i64, i64)> = eng
            .find_eq(
                "competitor_tags",
                "channel_id",
                &Value::Text(channel_id.to_string()),
            )?
            .into_iter()
            .map(|r| {
                (
                    t(&r, "tag_name"),
                    i(&r, "video_count"),
                    f(&r, "avg_views") as i64,
                )
            })
            .collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
        rows.truncate(50);
        Ok(rows)
    }

    pub async fn get_transcript(
        &self,
        video_id: &str,
    ) -> Result<Option<TranscriptRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng
            .get("transcripts", video_id)?
            .map(|r| transcript_from_row(&r)))
    }

    pub async fn list_transcripts(&self) -> Result<Vec<TranscriptRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<TranscriptRow> = eng
            .all("transcripts")?
            .iter()
            .map(transcript_from_row)
            .collect();
        rows.sort_by(|a, b| b.fetched_at.cmp(&a.fetched_at));
        Ok(rows)
    }

    pub async fn list_comments(&self, video_id: &str) -> Result<Vec<CommentRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<CommentRow> = eng
            .find_eq("comments", "video_id", &Value::Text(video_id.to_string()))?
            .iter()
            .map(comment_from_row)
            .collect();
        rows.sort_by_key(|x| std::cmp::Reverse(x.like_count));
        Ok(rows)
    }

    pub async fn get_heatmap(&self, video_id: &str) -> Result<String, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng
            .get("video_heatmap", video_id)?
            .map(|r| json_s(&r, "points"))
            .unwrap_or_default())
    }

    pub async fn channel_snapshots(
        &self,
        channel_id: &str,
    ) -> Result<Vec<(String, Option<i64>, Option<i64>, Option<i64>)>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<(String, Option<i64>, Option<i64>, Option<i64>)> = eng
            .find_eq(
                "channel_snapshots",
                "channel_id",
                &Value::Text(channel_id.to_string()),
            )?
            .into_iter()
            .map(|r| {
                (
                    t(&r, "at"),
                    opt_i(&r, "subscriber_count"),
                    opt_i(&r, "video_count"),
                    opt_i(&r, "total_views"),
                )
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(rows)
    }

    pub async fn channel_total_views(&self, channel_id: &str) -> Result<i64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut total = 0i64;
        for v in eng.find_eq("videos", "channel_id", &Value::Text(channel_id.to_string()))? {
            total += i(&v, "view_count");
        }
        Ok(total)
    }

    pub async fn channel_video_count(&self, channel_id: &str) -> Result<i64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng
            .find_eq("videos", "channel_id", &Value::Text(channel_id.to_string()))?
            .len() as i64)
    }

    pub async fn keyword_research_history(
        &self,
        keyword: &str,
    ) -> Result<Vec<KeywordResearchRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<KeywordResearchRow> = eng
            .find_eq(
                "keyword_research",
                "keyword",
                &Value::Text(keyword.to_string()),
            )?
            .iter()
            .map(keyword_research_from_row)
            .collect();
        rows.sort_by(|a, b| a.at.cmp(&b.at));
        Ok(rows)
    }

    pub async fn keyword_research_all(&self) -> Result<Vec<KeywordResearchRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<KeywordResearchRow> = eng
            .all("keyword_research")?
            .iter()
            .map(keyword_research_from_row)
            .collect();
        rows.sort_by(|a, b| a.keyword.cmp(&b.keyword).then(a.at.cmp(&b.at)));
        Ok(rows)
    }

    pub async fn keyword_trending(
        &self,
        limit: usize,
    ) -> Result<Vec<KeywordTrendingRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut latest: HashMap<String, String> = HashMap::new();
        for r in eng.all("keyword_research")? {
            let kw = t(&r, "keyword");
            let at = t(&r, "at");
            if let Some(cur) = latest.get(&kw) {
                if at <= *cur {
                    continue;
                }
            }
            latest.insert(kw, at);
        }
        let mut out: Vec<KeywordTrendingRow> = Vec::new();
        for r in eng.all("keyword_research")? {
            let kw = t(&r, "keyword");
            if latest.get(&kw).map(|a| a == &t(&r, "at")).unwrap_or(false) {
                out.push(KeywordTrendingRow {
                    keyword: kw,
                    opportunity_score: f(&r, "opportunity_score"),
                    competition_score: f(&r, "competition_score"),
                    serp_mean_views: f(&r, "serp_mean_views"),
                    volume_label: t(&r, "volume_label"),
                    actively_published: b(&r, "actively_published"),
                });
            }
        }
        out.sort_by(|a, b| b.opportunity_score.total_cmp(&a.opportunity_score));
        if limit > 0 {
            out.truncate(limit);
        }
        Ok(out)
    }

    // -- repository writes (all keep the legacy `&self` receiver via Mutex) --

    pub async fn insert_alert(
        &self,
        kind: &str,
        channel_id: Option<&str>,
        message: &str,
        severity: &str,
    ) -> Result<usize, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        if alert_exists_engine(&eng, kind, channel_id, message) {
            return Ok(0);
        }
        let at = crate::util::now_rfc3339();
        let id = next_id(&eng, "alerts", "alert_id")?;
        let mut row = Row::new();
        row.insert("alert_id".to_string(), Value::Int(id));
        row.insert("kind".to_string(), v_text(kind));
        row.insert("channel_id".to_string(), v_opt_text(channel_id));
        row.insert("message".to_string(), v_text(message));
        row.insert("severity".to_string(), v_text(severity));
        row.insert("created_at".to_string(), v_text(&at));
        let mut tx = eng.begin();
        tx.put("alerts", row)?;
        tx.commit()?;
        Ok(1)
    }

    pub async fn upsert_score(
        &self,
        video_id: &str,
        seo: f64,
        geo: f64,
        total: f64,
        components: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut row = Row::new();
        row.insert("video_id".to_string(), v_text(video_id));
        row.insert("seo_score".to_string(), Value::Float(seo));
        row.insert("geo_score".to_string(), Value::Float(geo));
        row.insert("total_score".to_string(), Value::Float(total));
        row.insert("components".to_string(), v_json(components));
        row.insert(
            "computed_at".to_string(),
            v_text(crate::util::now_rfc3339()),
        );
        let mut tx = eng.begin();
        tx.put("scores", row)?;
        tx.commit()?;
        eng.checkpoint()?;
        Ok(())
    }

    pub async fn upsert_scores_batch(
        &self,
        scores: &[(String, f64, f64, f64, String)],
    ) -> Result<(), TubeforgeError> {
        let now = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let mut tx = eng.begin();
        for (video_id, seo, geo, total, components) in scores {
            let mut row = Row::new();
            row.insert("video_id".to_string(), v_text(video_id));
            row.insert("seo_score".to_string(), Value::Float(*seo));
            row.insert("geo_score".to_string(), Value::Float(*geo));
            row.insert("total_score".to_string(), Value::Float(*total));
            row.insert("components".to_string(), v_json(components));
            row.insert("computed_at".to_string(), v_text(&now));
            tx.put("scores", row)?;
        }
        tx.commit()?;
        eng.checkpoint()?;
        Ok(())
    }

    pub async fn add_keywords(
        &self,
        keywords: &[String],
        niche: Option<&str>,
    ) -> Result<usize, TubeforgeError> {
        let at = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let mut to_add: Vec<Row> = Vec::new();
        let mut pending: HashMap<String, ()> = HashMap::new();
        let mut added = 0;
        for kw in keywords {
            let kw = kw.trim().to_lowercase();
            if kw.is_empty() {
                continue;
            }
            if pending.contains_key(&kw) || eng.get("keywords", &kw)?.is_some() {
                continue;
            }
            let mut row = Row::new();
            row.insert("keyword".to_string(), v_text(&kw));
            row.insert("niche".to_string(), v_opt_text(niche));
            row.insert("created_at".to_string(), v_text(&at));
            pending.insert(kw, ());
            to_add.push(row);
            added += 1;
        }
        let mut tx = eng.begin();
        for row in to_add {
            tx.put("keywords", row)?;
        }
        tx.commit()?;
        eng.checkpoint()?;
        Ok(added)
    }

    pub async fn upsert_ranking(
        &self,
        keyword: &str,
        checked_at: &str,
        video_id: Option<&str>,
        position: Option<i64>,
        topics: Option<&str>,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let pk = format!("{keyword}\x1f{checked_at}");
        let mut row = Row::new();
        row.insert("keyword_checked_at".to_string(), v_text(&pk));
        row.insert("keyword".to_string(), v_text(keyword));
        row.insert("checked_at".to_string(), v_text(checked_at));
        row.insert("video_id".to_string(), v_opt_text(video_id));
        row.insert("position".to_string(), v_int(position));
        row.insert("topics".to_string(), v_opt_json(topics));
        let mut tx = eng.begin();
        tx.put("keyword_rankings", row)?;
        tx.commit()?;
        eng.checkpoint()?;
        Ok(())
    }

    pub async fn upsert_edge(
        &self,
        from_channel: &str,
        to_channel: &str,
        weight: f64,
        source: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let pk = format!("{from_channel}\x1f{to_channel}");
        let existing = eng.get("edges", &pk)?;
        if let Some(ex) = &existing {
            if t(ex, "source") == "manual" && source == "overlap" {
                return Ok(());
            }
        }
        let mut row = Row::new();
        row.insert("from_to".to_string(), v_text(&pk));
        row.insert("from_channel".to_string(), v_text(from_channel));
        row.insert("to_channel".to_string(), v_text(to_channel));
        row.insert("weight".to_string(), Value::Float(weight));
        row.insert("source".to_string(), v_text(source));
        let mut tx = eng.begin();
        tx.put("edges", row)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn delete_overlap_edges(&self) -> Result<usize, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let pks: Vec<String> = eng
            .all("edges")?
            .into_iter()
            .filter(|r| t(r, "source") == "overlap")
            .filter_map(|r| {
                r.get("from_to")
                    .and_then(|v| v.as_text())
                    .map(str::to_string)
            })
            .collect();
        let mut tx = eng.begin();
        for pk in &pks {
            tx.delete("edges", pk)?;
        }
        tx.commit()?;
        Ok(pks.len())
    }

    pub async fn upsert_idea(
        &self,
        title_suggestion: &str,
        rationale: &str,
        score: f64,
        status: &str,
        source_video: Option<&str>,
    ) -> Result<i64, TubeforgeError> {
        let at = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let mut existing_id: Option<i64> = None;
        for r in eng.all("ideas")? {
            if t(&r, "title_suggestion") == title_suggestion
                && opt_s(&r, "source_video") == source_video.map(str::to_string)
            {
                existing_id = Some(i(&r, "idea_id"));
                break;
            }
        }
        let id = match existing_id {
            Some(id) => id,
            None => next_id(&eng, "ideas", "idea_id")?,
        };
        let mut row = Row::new();
        row.insert("idea_id".to_string(), Value::Int(id));
        row.insert("title_suggestion".to_string(), v_text(title_suggestion));
        row.insert("rationale".to_string(), v_json(rationale));
        row.insert("score".to_string(), Value::Float(score));
        row.insert("status".to_string(), v_text(status));
        row.insert("source_video".to_string(), v_opt_text(source_video));
        row.insert("created_at".to_string(), v_text(&at));
        let mut tx = eng.begin();
        tx.put("ideas", row)?;
        tx.commit()?;
        Ok(id)
    }

    pub async fn set_privacy_status(
        &self,
        video_id: &str,
        privacy_status: Option<&str>,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let Some(mut row) = eng.get("videos", video_id)? else {
            return Ok(());
        };
        row.insert("privacy_status".to_string(), v_opt_text(privacy_status));
        row.insert("updated_at".to_string(), v_text(crate::util::now_rfc3339()));
        let mut tx = eng.begin();
        tx.put("videos", row)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn set_idea_statuses(
        &self,
        ids: &[i64],
        status: &str,
    ) -> Result<usize, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut to_write: Vec<Row> = Vec::new();
        let mut marked = 0;
        for id in ids {
            if let Some(mut row) = eng.get("ideas", &id.to_string())? {
                row.insert("status".to_string(), v_text(status));
                to_write.push(row);
                marked += 1;
            }
        }
        let mut tx = eng.begin();
        for row in to_write {
            tx.put("ideas", row)?;
        }
        tx.commit()?;
        Ok(marked)
    }

    pub async fn mark_alerts_read(&self) -> Result<usize, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let now = crate::util::now_rfc3339();
        let mut to_write: Vec<Row> = Vec::new();
        let mut marked = 0;
        for r in eng.all("alerts")? {
            if r.get("read_at")
                .map(|v| !matches!(v, Value::Text(_)))
                .unwrap_or(true)
            {
                let mut nr = r;
                nr.insert("read_at".to_string(), v_text(&now));
                to_write.push(nr);
                marked += 1;
            }
        }
        let mut tx = eng.begin();
        for row in to_write {
            tx.put("alerts", row)?;
        }
        tx.commit()?;
        Ok(marked)
    }

    pub async fn clear_alerts(&self) -> Result<usize, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let pks: Vec<String> = eng
            .all("alerts")?
            .into_iter()
            .filter_map(|r| {
                r.get("alert_id")
                    .and_then(|v| v.as_i64())
                    .map(|n| n.to_string())
            })
            .collect();
        let mut tx = eng.begin();
        for pk in &pks {
            tx.delete("alerts", pk)?;
        }
        tx.commit()?;
        Ok(pks.len())
    }

    pub async fn register_competitors(
        &self,
        channel_ids: &[String],
        label: &str,
    ) -> Result<usize, TubeforgeError> {
        let now = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let mut to_add: Vec<Row> = Vec::new();
        let mut added = 0;
        for cid in channel_ids {
            let cid = cid.trim();
            if cid.is_empty() {
                continue;
            }
            if eng.get("competitors", cid)?.is_some() {
                continue;
            }
            let mut row = Row::new();
            row.insert("channel_id".to_string(), v_text(cid));
            row.insert("label".to_string(), v_text(label));
            row.insert("added_at".to_string(), v_text(&now));
            to_add.push(row);
            added += 1;
        }
        let mut tx = eng.begin();
        for row in to_add {
            tx.put("competitors", row)?;
        }
        tx.commit()?;
        Ok(added)
    }

    pub async fn upsert_tags(
        &self,
        video_id: &str,
        tag_names: &[String],
        source: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut writes: Vec<Row> = Vec::new();
        for (pos, name) in tag_names.iter().enumerate() {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let tag_id = ensure_tag(&mut eng, name)?;
            if tag_id > 0 {
                let mut row = Row::new();
                let pk = format!("{video_id}\x1f{tag_id}");
                row.insert("video_tag_id".to_string(), v_text(&pk));
                row.insert("video_id".to_string(), v_text(video_id));
                row.insert("tag_id".to_string(), Value::Int(tag_id));
                row.insert("position".to_string(), Value::Int(pos as i64));
                row.insert("source".to_string(), v_text(source));
                writes.push(row);
            }
        }
        let mut tx = eng.begin();
        for row in writes {
            tx.put("video_tags", row)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub async fn upsert_competitor_tags(
        &self,
        channel_id: &str,
        tag_name: &str,
        video_count: i64,
        avg_views: f64,
        rank: Option<i64>,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let pk = format!("{channel_id}\x1f{tag_name}");
        let mut row = Row::new();
        row.insert("channel_tag".to_string(), v_text(&pk));
        row.insert("channel_id".to_string(), v_text(channel_id));
        row.insert("tag_name".to_string(), v_text(tag_name));
        row.insert("video_count".to_string(), Value::Int(video_count));
        row.insert("avg_views".to_string(), Value::Float(avg_views));
        row.insert("rank".to_string(), v_int(rank));
        let mut tx = eng.begin();
        tx.put("competitor_tags", row)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn upsert_transcript(
        &self,
        video_id: &str,
        lang: &str,
        source: &str,
        text: &str,
        now: &str,
    ) -> Result<(), TubeforgeError> {
        let words = text.split_whitespace().count() as i64;
        let mut eng = self.engine.lock().unwrap();
        let mut row = Row::new();
        row.insert("video_id".to_string(), v_text(video_id));
        row.insert("lang".to_string(), v_text(lang));
        row.insert("source".to_string(), v_text(source));
        row.insert("text".to_string(), v_text(text));
        row.insert("word_count".to_string(), Value::Int(words));
        row.insert("fetched_at".to_string(), v_text(now));
        let mut tx = eng.begin();
        tx.put("transcripts", row)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn clear_transcripts(&self) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let pks: Vec<String> = eng
            .all("transcripts")?
            .into_iter()
            .filter_map(|r| {
                r.get("video_id")
                    .and_then(|v| v.as_text())
                    .map(str::to_string)
            })
            .collect();
        let mut tx = eng.begin();
        for pk in &pks {
            tx.delete("transcripts", pk)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub async fn upsert_comments(
        &self,
        video_id: &str,
        comments: &[CommentRow],
        now: &str,
    ) -> Result<usize, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut tx = eng.begin();
        let mut n = 0;
        for c in comments {
            let mut row = Row::new();
            row.insert("comment_id".to_string(), v_text(&c.comment_id));
            row.insert("video_id".to_string(), v_text(video_id));
            row.insert("author".to_string(), v_text(&c.author));
            row.insert("text".to_string(), v_text(&c.text));
            row.insert("like_count".to_string(), Value::Int(c.like_count));
            row.insert("published_at".to_string(), v_text(&c.published_at));
            row.insert("fetched_at".to_string(), v_text(now));
            tx.put("comments", row)?;
            n += 1;
        }
        tx.commit()?;
        Ok(n)
    }

    pub async fn clear_comments(&self) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let pks: Vec<String> = eng
            .all("comments")?
            .into_iter()
            .filter_map(|r| {
                r.get("comment_id")
                    .and_then(|v| v.as_text())
                    .map(str::to_string)
            })
            .collect();
        let mut tx = eng.begin();
        for pk in &pks {
            tx.delete("comments", pk)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub async fn upsert_heatmap(
        &self,
        video_id: &str,
        points_json: &str,
        now: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut row = Row::new();
        row.insert("video_id".to_string(), v_text(video_id));
        row.insert("points".to_string(), v_json(points_json));
        row.insert("fetched_at".to_string(), v_text(now));
        let mut tx = eng.begin();
        tx.put("video_heatmap", row)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn upsert_channel_snapshot(
        &self,
        channel_id: &str,
        at: &str,
        subscribers: Option<i64>,
        videos: Option<i64>,
        total_views: Option<i64>,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let pk = format!("{channel_id}\x1f{at}");
        let mut row = Row::new();
        row.insert("channel_at".to_string(), v_text(&pk));
        row.insert("channel_id".to_string(), v_text(channel_id));
        row.insert("at".to_string(), v_text(at));
        row.insert("subscriber_count".to_string(), v_int(subscribers));
        row.insert("video_count".to_string(), v_int(videos));
        row.insert("total_views".to_string(), v_int(total_views));
        let mut tx = eng.begin();
        tx.put("channel_snapshots", row)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn upsert_channel_snapshot_daily(
        &self,
        channel_id: &str,
        subscribers: Option<i64>,
        videos: Option<i64>,
        total_views: Option<i64>,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let now = crate::util::now_rfc3339();
        let day = now.chars().take(10).collect::<String>();
        let at = format!("{day}T00:00:00Z");
        let subscribers = match subscribers {
            Some(s) => Some(s),
            None => eng
                .get("channels", channel_id)?
                .and_then(|c| opt_i(&c, "subscriber_count")),
        };
        let pk = format!("{channel_id}\x1f{at}");
        let mut row = Row::new();
        row.insert("channel_at".to_string(), v_text(&pk));
        row.insert("channel_id".to_string(), v_text(channel_id));
        row.insert("at".to_string(), v_text(&at));
        row.insert("subscriber_count".to_string(), v_int(subscribers));
        row.insert("video_count".to_string(), v_int(videos));
        row.insert("total_views".to_string(), v_int(total_views));
        let mut tx = eng.begin();
        tx.put("channel_snapshots", row)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn upsert_keyword_research(
        &self,
        keyword: &str,
        at: &str,
        volume_label: &str,
        serp_total: i64,
        serp_mean_views: f64,
        ranking_channels: i64,
        competition_score: f64,
        opportunity_score: f64,
        actively_published: bool,
        suggested_tags: &str,
        related_keywords: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let id = next_id(&eng, "keyword_research", "research_id")?;
        let mut row = Row::new();
        row.insert("research_id".to_string(), Value::Int(id));
        row.insert("keyword".to_string(), v_text(keyword));
        row.insert("at".to_string(), v_text(at));
        row.insert("volume_label".to_string(), v_text(volume_label));
        row.insert("serp_total".to_string(), Value::Int(serp_total));
        row.insert("serp_mean_views".to_string(), Value::Float(serp_mean_views));
        row.insert("ranking_channels".to_string(), Value::Int(ranking_channels));
        row.insert(
            "competition_score".to_string(),
            Value::Float(competition_score),
        );
        row.insert(
            "opportunity_score".to_string(),
            Value::Float(opportunity_score),
        );
        row.insert(
            "actively_published".to_string(),
            Value::Bool(actively_published),
        );
        row.insert("suggested_tags".to_string(), v_json(suggested_tags));
        row.insert("related_keywords".to_string(), v_json(related_keywords));
        let mut tx = eng.begin();
        tx.put("keyword_research", row)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn get_keyword_research(
        &self,
        keyword: &str,
    ) -> Result<Option<KeywordResearchRow>, TubeforgeError> {
        let eng = self.engine.lock().unwrap();
        let rows = eng.all("keyword_research")?;
        let matching = rows
            .into_iter()
            .filter(|r| t(r, "keyword").eq_ignore_ascii_case(keyword))
            .max_by_key(|r| t(r, "at"));
        Ok(matching.as_ref().map(keyword_research_from_row))
    }

    /// Begin the single batch write transaction.
    pub async fn begin_batch(&mut self) -> Result<Batch<'_>, TubeforgeError> {
        Ok(Batch { db: self })
    }

    // -- legacy-parity / point-update helpers (callers migrated from turso) --

    /// No-op migration: tfdb creates the full schema on open and records
    /// `schema_version`, so there are no SQL migrations to run.
    pub async fn migrate(&mut self) -> Result<(), TubeforgeError> {
        Ok(())
    }

    /// Copy a consistent checkpoint of the engine to `dest` (the tfdb analog
    /// of the legacy `VACUUM INTO` snapshot). Checkpoints the engine first so
    /// the WAL is folded into the `.dat` file, then copies it. Because
    /// `Db::open(dest)` loads `<dest>.dat`, the checkpoint is mirrored there
    /// so a snapshot round-trips through `Db::open` (backup.rs's integrity
    /// check) while `dest` itself is the standalone, prunable snapshot file.
    pub async fn vacuum_into(&self, dest: &Path) -> Result<(), TubeforgeError> {
        self.engine.lock().unwrap().checkpoint()?;
        let dat = self.path.with_extension("dat");
        std::fs::copy(&dat, dest).map_err(|e| TubeforgeError::Storage {
            code: "IO".to_string(),
            message: format!(
                "copy checkpoint {} -> {}: {e}",
                dat.display(),
                dest.display()
            ),
        })?;
        let dest_dat = dest.with_extension("dat");
        std::fs::copy(&dat, &dest_dat).map_err(|e| TubeforgeError::Storage {
            code: "IO".to_string(),
            message: format!(
                "copy checkpoint {} -> {}: {e}",
                dat.display(),
                dest_dat.display()
            ),
        })?;
        Ok(())
    }

    /// Column names of a table (the tfdb analog of `PRAGMA table_info`).
    pub async fn columns(&self, table: &str) -> Result<Vec<String>, TubeforgeError> {
        Ok(crate::tfdb::tfdb_schema::all()
            .into_iter()
            .find(|s| s.name == table)
            .map(|s| s.cols.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default())
    }

    /// Coalesce-style refresh of a video's live stats (view/like/comment):
    /// only non-None values overwrite the stored row.
    pub async fn update_video_stats(
        &self,
        video_id: &str,
        view_count: Option<i64>,
        like_count: Option<i64>,
        comment_count: Option<i64>,
        updated_at: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let Some(mut row) = eng.get("videos", video_id)? else {
            return Ok(());
        };
        if view_count.is_some() {
            row.insert("view_count".to_string(), v_int(view_count));
        }
        if like_count.is_some() {
            row.insert("like_count".to_string(), v_int(like_count));
        }
        if comment_count.is_some() {
            row.insert("comment_count".to_string(), v_int(comment_count));
        }
        row.insert("updated_at".to_string(), v_text(updated_at));
        let mut tx = eng.begin();
        tx.put("videos", row)?;
        tx.commit()?;
        Ok(())
    }

    /// Update full video metadata: title, description, duration, stats, tags, published_at, thumb_url.
    pub async fn update_video_full_metadata(
        &self,
        video_id: &str,
        title: Option<&str>,
        description: Option<&str>,
        duration_seconds: Option<i64>,
        view_count: Option<i64>,
        like_count: Option<i64>,
        comment_count: Option<i64>,
        published_at: Option<&str>,
        tags: Option<&str>,
        thumb_url: Option<&str>,
        updated_at: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let Some(mut row) = eng.get("videos", video_id)? else {
            return Ok(());
        };
        if let Some(t) = title {
            if !t.is_empty() {
                row.insert("title".to_string(), v_text(t));
            }
        }
        if let Some(d) = description {
            if !d.is_empty() {
                row.insert("description".to_string(), v_text(d));
            }
        }
        if let Some(dur) = duration_seconds {
            if dur > 0 {
                row.insert("duration_sec".to_string(), Value::Int(dur));
            }
        }
        if view_count.is_some() {
            row.insert("view_count".to_string(), v_int(view_count));
        }
        if like_count.is_some() {
            row.insert("like_count".to_string(), v_int(like_count));
        }
        if comment_count.is_some() {
            row.insert("comment_count".to_string(), v_int(comment_count));
        }
        if let Some(pub_at) = published_at {
            if !pub_at.is_empty() {
                row.insert("published_at".to_string(), v_text(pub_at));
            }
        }
        if let Some(tg) = tags {
            if !tg.is_empty() && tg != "[]" {
                row.insert("tags".to_string(), v_text(tg));
            }
        }
        if let Some(th) = thumb_url {
            if !th.is_empty() {
                row.insert("thumb_url".to_string(), v_text(th));
            }
        } else if !row.contains_key("thumb_url") {
            row.insert("thumb_url".to_string(), v_text(&format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", video_id)));
        }
        row.insert("updated_at".to_string(), v_text(updated_at));
        let mut tx = eng.begin();
        tx.put("videos", row)?;
        tx.commit()?;
        Ok(())
    }

    /// Idempotent & coalescing selective patch for a video row.
    /// Only touches columns that have actually changed from live YouTube data.
    /// Skips database transaction entirely if all incoming values match existing state (0 disk writes).
    pub async fn patch_video_coalesced(
        &self,
        video_id: &str,
        patch: &VideoPatch,
    ) -> Result<bool, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let Some(existing) = eng.get("videos", video_id)? else {
            return Err(TubeforgeError::Storage {
                code: "NOT_FOUND".to_string(),
                message: format!("Video {video_id} not found in database"),
            });
        };

        let mut changed = false;
        let mut row = existing.clone();

        // 1. Check & patch view_count
        if let Some(new_views) = patch.view_count {
            let cur_views = existing.get("view_count").and_then(|v| v.as_i64());
            if cur_views != Some(new_views) {
                row.insert("view_count".to_string(), Value::Int(new_views));
                changed = true;
            }
        }

        // 2. Check & patch like_count
        if let Some(new_likes) = patch.like_count {
            let cur_likes = existing.get("like_count").and_then(|v| v.as_i64());
            if cur_likes != Some(new_likes) {
                row.insert("like_count".to_string(), Value::Int(new_likes));
                changed = true;
            }
        }

        // 3. Check & patch comment_count
        if let Some(new_comments) = patch.comment_count {
            let cur_comments = existing.get("comment_count").and_then(|v| v.as_i64());
            if cur_comments != Some(new_comments) {
                row.insert("comment_count".to_string(), Value::Int(new_comments));
                changed = true;
            }
        }

        // 4. Check & patch duration_sec
        if let Some(new_dur) = patch.duration_sec {
            let cur_dur = existing.get("duration_sec").and_then(|v| v.as_i64());
            if cur_dur != Some(new_dur) && new_dur > 0 {
                row.insert("duration_sec".to_string(), Value::Int(new_dur));
                changed = true;
            }
        }

        // 5. Check & patch thumb_url
        if let Some(ref new_thumb) = patch.thumb_url {
            let cur_thumb = existing.get("thumb_url").and_then(|v| v.as_text());
            if cur_thumb != Some(new_thumb.as_str()) && !new_thumb.is_empty() {
                row.insert("thumb_url".to_string(), v_text(new_thumb));
                changed = true;
            }
        }

        // 6. Check & patch title
        if let Some(ref new_title) = patch.title {
            let cur_title = existing.get("title").and_then(|v| v.as_text());
            if cur_title != Some(new_title.as_str()) && !new_title.is_empty() {
                row.insert("title".to_string(), v_text(new_title));
                changed = true;
            }
        }

        // 7. Check & patch description
        if let Some(ref new_desc) = patch.description {
            let cur_desc = existing.get("description").and_then(|v| v.as_text());
            if cur_desc != Some(new_desc.as_str()) && !new_desc.is_empty() {
                row.insert("description".to_string(), v_text(new_desc));
                changed = true;
            }
        }

        // 8. Check & patch tags (coalesce/merge new tags without wiping existing)
        if let Some(ref new_tags) = patch.tags {
            let cur_tags_str = existing.get("tags").and_then(|v| v.as_text()).unwrap_or("[]");
            let mut cur_tags_set: std::collections::HashSet<String> = serde_json::from_str(cur_tags_str).unwrap_or_default();
            let mut tags_modified = false;
            for t in new_tags {
                let clean = t.trim().to_string();
                if !clean.is_empty() && cur_tags_set.insert(clean) {
                    tags_modified = true;
                }
            }
            if tags_modified {
                let mut merged_vec: Vec<String> = cur_tags_set.into_iter().collect();
                merged_vec.sort();
                if let Ok(json_str) = serde_json::to_string(&merged_vec) {
                    row.insert("tags".to_string(), v_text(&json_str));
                    changed = true;
                }
            }
        }

        // 9. If nothing changed, coalesce and return false immediately (0 disk writes!)
        if !changed {
            return Ok(false);
        }

        row.insert("updated_at".to_string(), v_text(&patch.updated_at));
        let mut tx = eng.begin();
        tx.put("videos", row)?;
        tx.commit()?;
        Ok(true)
    }

    /// Refresh a channel's subscriber count and stamp `updated_at`.
    pub async fn update_channel_subscribers(
        &self,
        channel_id: &str,
        subscriber_count: i64,
        updated_at: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let Some(mut row) = eng.get("channels", channel_id)? else {
            return Ok(());
        };
        row.insert("subscriber_count".to_string(), Value::Int(subscriber_count));
        row.insert("updated_at".to_string(), v_text(updated_at));
        let mut tx = eng.begin();
        tx.put("channels", row)?;
        tx.commit()?;
        Ok(())
    }

    /// Set a video's `tags` column (JSON string) and stamp `updated_at`.
    pub async fn set_video_tags(
        &self,
        video_id: &str,
        tags: &str,
        updated_at: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let Some(mut row) = eng.get("videos", video_id)? else {
            return Ok(());
        };
        row.insert("tags".to_string(), v_json(tags));
        row.insert("updated_at".to_string(), v_text(updated_at));
        let mut tx = eng.begin();
        tx.put("videos", row)?;
        tx.commit()?;
        Ok(())
    }

    /// Put or upsert a channel row into `channels`.
    pub async fn put_channel(&self, c: &ChannelRow) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut tx = eng.begin();
        tx.put("channels", channel_to_row(c))?;
        tx.commit()?;
        Ok(())
    }

    /// Put or upsert a video row into `videos`.
    pub async fn put_video(&self, v: &VideoRow) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut tx = eng.begin();
        tx.put("videos", video_to_row(v))?;
        tx.commit()?;
        Ok(())
    }

    /// Retrieve all user-registered channels from the `user_channels` table.
    pub async fn all_user_channels(&self) -> Result<Vec<UserChannelRow>, TubeforgeError> {
        let eng = self.engine.lock().unwrap();
        let rows = eng.all("user_channels").unwrap_or_default();
        let mut out = Vec::new();
        for r in rows {
            let channel_id = r.get("channel_id").and_then(|v| v.as_text()).unwrap_or_default().to_string();
            let custom_name = r.get("custom_name").and_then(|v| v.as_text()).map(|s| s.to_string());
            let is_primary = r.get("is_primary").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
            let notes = r.get("notes").and_then(|v| v.as_text()).map(|s| s.to_string());
            let created_at = r.get("created_at").and_then(|v| v.as_text()).unwrap_or_default().to_string();
            let updated_at = r.get("updated_at").and_then(|v| v.as_text()).unwrap_or_default().to_string();
            out.push(UserChannelRow {
                channel_id,
                custom_name,
                is_primary,
                notes,
                created_at,
                updated_at,
            });
        }
        Ok(out)
    }

    /// Get a specific user channel by ID from `user_channels`.
    pub async fn get_user_channel(&self, channel_id: &str) -> Result<Option<UserChannelRow>, TubeforgeError> {
        let eng = self.engine.lock().unwrap();
        let Some(r) = eng.get("user_channels", channel_id)? else {
            return Ok(None);
        };
        let custom_name = r.get("custom_name").and_then(|v| v.as_text()).map(|s| s.to_string());
        let is_primary = r.get("is_primary").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        let notes = r.get("notes").and_then(|v| v.as_text()).map(|s| s.to_string());
        let created_at = r.get("created_at").and_then(|v| v.as_text()).unwrap_or_default().to_string();
        let updated_at = r.get("updated_at").and_then(|v| v.as_text()).unwrap_or_default().to_string();
        Ok(Some(UserChannelRow {
            channel_id: channel_id.to_string(),
            custom_name,
            is_primary,
            notes,
            created_at,
            updated_at,
        }))
    }

    /// Insert or update a user channel in `user_channels`.
    pub async fn put_user_channel(&self, row: UserChannelRow) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert("channel_id".to_string(), v_text(&row.channel_id));
        if let Some(cn) = &row.custom_name {
            map.insert("custom_name".to_string(), v_text(cn));
        }
        map.insert("is_primary".to_string(), Value::Int(if row.is_primary { 1 } else { 0 }));
        if let Some(n) = &row.notes {
            map.insert("notes".to_string(), v_text(n));
        }
        map.insert("created_at".to_string(), v_text(&row.created_at));
        map.insert("updated_at".to_string(), v_text(&row.updated_at));
        let mut tx = eng.begin();
        tx.put("user_channels", map)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove a user channel from `user_channels`.
    pub async fn delete_user_channel(&self, channel_id: &str) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut tx = eng.begin();
        tx.delete("user_channels", channel_id)?;
        tx.commit()?;
        Ok(())
    }

    // -- knowledge-graph persistence (kg_builder) -------------------------

    /// Drop every row of the given KG tables (used for full/incremental
    /// rebuild paths — `kg_communities` is always recomputed, so it is cleared
    /// on both paths while entities/relations are only cleared on full).
    pub async fn clear_kg(&self, tables: &[&str]) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut to_delete: Vec<(String, String)> = Vec::new();
        for table in tables {
            let pk_col = crate::tfdb::tfdb_schema::all()
                .into_iter()
                .find(|s| s.name == *table)
                .map(|s| s.pk)
                .unwrap_or_default();
            for row in eng.all(table)? {
                let pk = match row.get(&pk_col) {
                    Some(Value::Text(s)) => s.clone(),
                    Some(Value::Int(n)) => n.to_string(),
                    _ => continue,
                };
                to_delete.push((table.to_string(), pk));
            }
        }
        let mut tx = eng.begin();
        for (table, pk) in &to_delete {
            tx.delete(table, pk)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// All `kg_entities` rows (for the in-memory KG loader).
    pub async fn list_kg_entities(&self) -> Result<Vec<KgEntityRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<KgEntityRow> = eng
            .all("kg_entities")?
            .iter()
            .map(|r| KgEntityRow {
                entity_id: t(r, "entity_id"),
                entity_type: t(r, "entity_type"),
                canonical_name: t(r, "canonical_name"),
                display_name: t(r, "display_name"),
                properties: json_s(r, "properties"),
                centrality: opt_f(r, "centrality"),
                community_id: opt_i(r, "community_id"),
                source: t(r, "source"),
                source_ref: t(r, "source_ref"),
            })
            .collect();
        rows.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
        Ok(rows)
    }

    /// All `kg_relations` rows (for the in-memory KG loader).
    pub async fn list_kg_relations(&self) -> Result<Vec<KgRelationRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<KgRelationRow> = eng
            .all("kg_relations")?
            .iter()
            .map(|r| KgRelationRow {
                from_entity: t(r, "from_entity"),
                to_entity: t(r, "to_entity"),
                relation_type: t(r, "relation_type"),
                weight: f(r, "weight"),
                source: t(r, "source"),
            })
            .collect();
        rows.sort_by(|a, b| {
            a.from_entity
                .cmp(&b.from_entity)
                .then(a.to_entity.cmp(&b.to_entity))
        });
        Ok(rows)
    }

    /// All `kg_communities` ids (for the in-memory KG loader).
    pub async fn list_kg_communities(&self) -> Result<Vec<i64>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut ids: Vec<i64> = eng
            .all("kg_communities")?
            .into_iter()
            .filter_map(|r| r.get("community_id").and_then(|v| v.as_i64()))
            .collect();
        ids.sort_unstable();
        Ok(ids)
    }

    pub async fn persist_kg_entity(
        &self,
        entity_id: &str,
        entity_type: &str,
        canonical_name: &str,
        display_name: &str,
        properties_json: &str,
        centrality: Option<f64>,
        community_id: Option<i64>,
        source: &str,
        source_ref: &str,
    ) -> Result<(), TubeforgeError> {
        let now = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let mut row = Row::new();
        row.insert("entity_id".to_string(), v_text(entity_id));
        row.insert("entity_type".to_string(), v_text(entity_type));
        row.insert("canonical_name".to_string(), v_text(canonical_name));
        row.insert("display_name".to_string(), v_text(display_name));
        row.insert("properties".to_string(), v_json(properties_json));
        row.insert("centrality".to_string(), v_float(centrality));
        row.insert("community_id".to_string(), v_int(community_id));
        row.insert("source".to_string(), v_text(source));
        row.insert("source_ref".to_string(), v_text(source_ref));
        row.insert("created_at".to_string(), v_text(&now));
        row.insert("updated_at".to_string(), v_text(&now));
        let mut tx = eng.begin();
        tx.put("kg_entities", row)?;
        tx.commit()?;
        Ok(())
    }

    /// Insert-or-replace all KG entities, relations, and communities in one batched transaction.
    pub async fn persist_kg_batch(
        &self,
        entities: &[KgEntityRow],
        relations: &[KgRelationRow],
        communities: &[KgCommunityRow],
    ) -> Result<(), TubeforgeError> {
        let now = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let mut tx = eng.begin();
        for e in entities {
            let mut row = Row::new();
            row.insert("entity_id".to_string(), v_text(&e.entity_id));
            row.insert("entity_type".to_string(), v_text(&e.entity_type));
            row.insert("canonical_name".to_string(), v_text(&e.canonical_name));
            row.insert("display_name".to_string(), v_text(&e.display_name));
            row.insert("properties".to_string(), v_json(&e.properties));
            row.insert(
                "centrality".to_string(),
                e.centrality.map(Value::Float).unwrap_or(Value::Null),
            );
            row.insert(
                "community_id".to_string(),
                e.community_id.map(Value::Int).unwrap_or(Value::Null),
            );
            row.insert("source".to_string(), v_text(&e.source));
            row.insert("source_ref".to_string(), v_text(&e.source_ref));
            row.insert("created_at".to_string(), v_text(&now));
            row.insert("updated_at".to_string(), v_text(&now));
            tx.put("kg_entities", row)?;
        }
        for r in relations {
            let pk = format!(
                "{}\x1f{}\x1f{}",
                r.from_entity, r.to_entity, r.relation_type
            );
            let mut row = Row::new();
            row.insert("relation_id".to_string(), v_text(&pk));
            row.insert("from_entity".to_string(), v_text(&r.from_entity));
            row.insert("to_entity".to_string(), v_text(&r.to_entity));
            row.insert("relation_type".to_string(), v_text(&r.relation_type));
            row.insert("weight".to_string(), Value::Float(r.weight));
            row.insert("source".to_string(), v_text(&r.source));
            row.insert("created_at".to_string(), v_text(&now));
            tx.put("kg_relations", row)?;
        }
        for c in communities {
            let mut row = Row::new();
            row.insert("community_id".to_string(), Value::Int(c.community_id));
            row.insert("community_type".to_string(), v_text(&c.community_type));
            row.insert("member_count".to_string(), Value::Int(c.member_count));
            row.insert("top_entities".to_string(), v_json(&c.top_entities));
            row.insert("created_at".to_string(), v_text(&c.created_at));
            row.insert("updated_at".to_string(), v_text(&c.updated_at));
            tx.put("kg_communities", row)?;
        }
        tx.commit()?;
        eng.checkpoint()?;
        Ok(())
    }

    /// Insert-or-replace one `kg_relations` row (dedup key: from/to/type).
    pub async fn persist_kg_relation(
        &self,
        from_entity: &str,
        to_entity: &str,
        relation_type: &str,
        weight: f64,
        source: &str,
    ) -> Result<(), TubeforgeError> {
        let now = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let pk = format!("{from_entity}\x1f{to_entity}\x1f{relation_type}");
        let mut row = Row::new();
        row.insert("relation_id".to_string(), v_text(&pk));
        row.insert("from_entity".to_string(), v_text(from_entity));
        row.insert("to_entity".to_string(), v_text(to_entity));
        row.insert("relation_type".to_string(), v_text(relation_type));
        row.insert("weight".to_string(), Value::Float(weight));
        row.insert("source".to_string(), v_text(source));
        row.insert("created_at".to_string(), v_text(&now));
        let mut tx = eng.begin();
        tx.put("kg_relations", row)?;
        tx.commit()?;
        Ok(())
    }

    /// Insert-or-replace one `kg_communities` row.
    pub async fn persist_kg_community(
        &self,
        community_id: i64,
        member_count: i64,
        top_entities_json: &str,
    ) -> Result<(), TubeforgeError> {
        let now = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let mut row = Row::new();
        row.insert("community_id".to_string(), Value::Int(community_id));
        row.insert("community_type".to_string(), v_text("mixed"));
        row.insert("member_count".to_string(), Value::Int(member_count));
        row.insert("top_entities".to_string(), v_json(top_entities_json));
        row.insert("created_at".to_string(), v_text(&now));
        row.insert("updated_at".to_string(), v_text(&now));
        let mut tx = eng.begin();
        tx.put("kg_communities", row)?;
        tx.commit()?;
        Ok(())
    }

    /// Point `kg_entities.community_id` for one entity.
    pub async fn update_entity_community(
        &self,
        entity_id: &str,
        community_id: i64,
    ) -> Result<(), TubeforgeError> {
        let now = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let Some(mut row) = eng.get("kg_entities", entity_id)? else {
            return Ok(());
        };
        row.insert("community_id".to_string(), Value::Int(community_id));
        row.insert("updated_at".to_string(), v_text(&now));
        let mut tx = eng.begin();
        tx.put("kg_entities", row)?;
        tx.commit()?;
        Ok(())
    }

    /// Point `kg_entities.centrality` for one entity.
    pub async fn update_entity_centrality(
        &self,
        entity_id: &str,
        centrality: f64,
    ) -> Result<(), TubeforgeError> {
        let now = crate::util::now_rfc3339();
        let mut eng = self.engine.lock().unwrap();
        let Some(mut row) = eng.get("kg_entities", entity_id)? else {
            return Ok(());
        };
        row.insert("centrality".to_string(), Value::Float(centrality));
        row.insert("updated_at".to_string(), v_text(&now));
        let mut tx = eng.begin();
        tx.put("kg_entities", row)?;
        tx.commit()?;
        Ok(())
    }

    // -- greedy bot methods ------------------------------------------------

    pub async fn insert_greedy_seed(
        &self,
        seed: &str,
        source: &str,
    ) -> Result<i64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let id = next_id(&eng, "greedy_seeds", "seed_id")?;
        let now = crate::util::now_rfc3339();
        let mut row = Row::new();
        row.insert("seed_id".to_string(), v_int(Some(id)));
        row.insert("seed".to_string(), v_text(seed));
        row.insert("source".to_string(), v_text(source));
        row.insert("added_at".to_string(), v_text(&now));
        row.insert("active".to_string(), Value::Bool(true));
        let mut tx = eng.begin();
        tx.put("greedy_seeds", row)?;
        tx.commit()?;
        Ok(id)
    }

    pub async fn deactivate_greedy_seed(&self, seed_id: i64) -> Result<bool, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let rows = eng.find_eq("greedy_seeds", "seed_id", &Value::Int(seed_id))?;
        let Some(r) = rows.into_iter().next() else {
            return Ok(false);
        };
        let mut updated = r.clone();
        updated.insert("active".to_string(), Value::Bool(false));
        let mut tx = eng.begin();
        tx.put("greedy_seeds", updated)?;
        tx.commit()?;
        Ok(true)
    }

    pub async fn list_greedy_seeds(&self) -> Result<Vec<GreedySeedRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let rows = eng.all("greedy_seeds")?;
        Ok(rows
            .into_iter()
            .map(|r| GreedySeedRow {
                seed_id: i(&r, "seed_id"),
                seed: t(&r, "seed"),
                source: t(&r, "source"),
                added_at: t(&r, "added_at"),
                active: r.get("active").and_then(|v| v.as_bool()).unwrap_or(false),
            })
            .collect())
    }

    pub async fn greedy_active_seeds(&self) -> Result<Vec<String>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let rows = eng.all("greedy_seeds")?;
        Ok(rows
            .into_iter()
            .filter(|r| r.get("active").and_then(|v| v.as_bool()).unwrap_or(false))
            .map(|r| t(&r, "seed"))
            .collect())
    }

    pub async fn greedy_active_seed_count(&self) -> Result<i64, TubeforgeError> {
        let seeds = self.greedy_active_seeds().await?;
        Ok(seeds.len() as i64)
    }

    pub async fn insert_greedy_history(
        &self,
        params: &GreedyHistoryInsert<'_>,
    ) -> Result<i64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let id = next_id(&eng, "greedy_research_history", "research_id")?;
        let mut row = Row::new();
        row.insert("research_id".to_string(), v_int(Some(id)));
        row.insert("topic".to_string(), v_text(params.topic));
        row.insert("researched_at".to_string(), v_text(params.researched_at));
        row.insert("video_ids_json".to_string(), v_text(params.video_ids_json));
        row.insert("video_count".to_string(), v_int(Some(params.video_count)));
        row.insert("mean_views".to_string(), Value::Float(params.mean_views));
        row.insert("source".to_string(), v_text(params.source));
        row.insert("duration_ms".to_string(), v_int(Some(params.duration_ms)));
        let mut tx = eng.begin();
        tx.put("greedy_research_history", row)?;
        tx.commit()?;
        Ok(id)
    }

    pub async fn greedy_history_for_topic(
        &self,
        topic: &str,
    ) -> Result<Vec<GreedyHistoryRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let rows = eng.find_eq(
            "greedy_research_history",
            "topic",
            &Value::Text(topic.to_string()),
        )?;
        let mut out: Vec<GreedyHistoryRow> = rows
            .into_iter()
            .map(|r| GreedyHistoryRow {
                research_id: i(&r, "research_id"),
                topic: t(&r, "topic"),
                researched_at: t(&r, "researched_at"),
                video_ids_json: t(&r, "video_ids_json"),
                video_count: i(&r, "video_count"),
                mean_views: f(&r, "mean_views"),
                source: t(&r, "source"),
                duration_ms: i(&r, "duration_ms"),
            })
            .collect();
        out.sort_by(|a, b| b.researched_at.cmp(&a.researched_at));
        Ok(out)
    }

    pub async fn greedy_history_count(&self) -> Result<i64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        Ok(eng.count("greedy_research_history").unwrap_or(0) as i64)
    }

    pub async fn greedy_unique_topics(&self) -> Result<i64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let rows = eng.all("greedy_research_history")?;
        let set: std::collections::HashSet<String> =
            rows.into_iter().map(|r| t(&r, "topic")).collect();
        Ok(set.len() as i64)
    }

    pub async fn insert_greedy_topic_log(
        &self,
        topic: &str,
        status: &str,
        reason: &str,
        attempted_at: &str,
    ) -> Result<i64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let id = next_id(&eng, "greedy_topic_log", "log_id")?;
        let mut row = Row::new();
        row.insert("log_id".to_string(), v_int(Some(id)));
        row.insert("topic".to_string(), v_text(topic));
        row.insert("status".to_string(), v_text(status));
        row.insert("reason".to_string(), v_text(reason));
        row.insert("attempted_at".to_string(), v_text(attempted_at));
        let mut tx = eng.begin();
        tx.put("greedy_topic_log", row)?;
        tx.commit()?;
        Ok(id)
    }

    pub async fn greedy_topic_log_count(&self, status: &str) -> Result<i64, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let rows = eng.find_eq(
            "greedy_topic_log",
            "status",
            &Value::Text(status.to_string()),
        )?;
        Ok(rows.len() as i64)
    }

    pub async fn greedy_top_competitor_tags(
        &self,
        limit: usize,
    ) -> Result<Vec<String>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<(f64, String)> = eng
            .all("competitor_tags")?
            .into_iter()
            .map(|r| (f(&r, "avg_views"), t(&r, "tag_name")))
            .collect();
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        rows.truncate(limit);
        Ok(rows.into_iter().map(|(_, name)| name).collect())
    }

    pub async fn greedy_recent_related_keywords(
        &self,
        limit: usize,
    ) -> Result<Vec<String>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut rows: Vec<(String, String)> = eng
            .all("keyword_research")?
            .into_iter()
            .map(|r| (t(&r, "at"), t(&r, "related_keywords")))
            .collect();
        rows.sort_by(|a, b| b.0.cmp(&a.0));
        rows.truncate(limit);
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (_, json_str) in &rows {
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(json_str) {
                for kw in arr {
                    if seen.insert(kw.clone()) {
                        out.push(kw);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Auto-discover seed topics from the channel's existing data.
    ///
    /// Sources (in priority order):
    ///   1. Tags the channel uses (tags + video_tags tables)
    ///   2. Competitor tags (competitor_tags table, ordered by avg_views)
    ///   3. Keywords already tracked (keywords table)
    ///   4. Suggested tags from recent keyword research
    ///   5. Related keywords from recent keyword research
    ///
    /// Deduplicates case-insensitively and caps at `limit` total seeds.
    pub async fn greedy_discover_seeds(&self, limit: usize) -> Result<Vec<String>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut seeds: Vec<String> = Vec::new();

        // 1) Tags the channel uses (from tags + video_tags join)
        let tag_names: std::collections::HashMap<i64, String> = eng
            .all("tags")?
            .into_iter()
            .map(|r| (i(&r, "tag_id"), t(&r, "name")))
            .collect();
        let mut tag_use: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
        for vt in eng.all("video_tags")? {
            let tid = i(&vt, "tag_id");
            *tag_use.entry(tid).or_insert(0) += 1;
        }
        let mut tag_vec: Vec<(i64, i64)> = tag_use.into_iter().collect();
        tag_vec.sort_by_key(|b| std::cmp::Reverse(b.1));
        for (tid, _) in &tag_vec {
            if let Some(name) = tag_names.get(tid) {
                let lower = name.to_lowercase();
                if seen.insert(lower) {
                    seeds.push(name.clone());
                }
            }
        }

        // 2) Competitor tags (top 100 by avg_views)
        let mut comp: Vec<(f64, String)> = eng
            .all("competitor_tags")?
            .into_iter()
            .map(|r| (f(&r, "avg_views"), t(&r, "tag_name")))
            .collect();
        comp.sort_by(|a, b| b.0.total_cmp(&a.0));
        comp.truncate(100);
        for (_, name) in &comp {
            let lower = name.to_lowercase();
            if seen.insert(lower) {
                seeds.push(name.clone());
            }
        }

        // 3) Keywords already tracked
        for kw in eng.all("keywords")? {
            let name = t(&kw, "keyword");
            let lower = name.to_lowercase();
            if seen.insert(lower) {
                seeds.push(name);
            }
        }

        // 4) Suggested tags from recent keyword research (last 50 rows)
        let mut research_rows: Vec<(String, String, String)> = eng
            .all("keyword_research")?
            .into_iter()
            .map(|r| {
                (
                    t(&r, "at"),
                    t(&r, "suggested_tags"),
                    t(&r, "related_keywords"),
                )
            })
            .collect();
        research_rows.sort_by(|a, b| b.0.cmp(&a.0));
        research_rows.truncate(50);
        for (_, suggested_json, related_json) in &research_rows {
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(suggested_json) {
                for tag in arr {
                    let lower = tag.to_lowercase();
                    if seen.insert(lower) {
                        seeds.push(tag);
                    }
                }
            }
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(related_json) {
                for kw in arr {
                    let lower = kw.to_lowercase();
                    if seen.insert(lower) {
                        seeds.push(kw);
                    }
                }
            }
        }

        seeds.truncate(limit);
        Ok(seeds)
    }

    // -----------------------------------------------------------------------
    // Kanban Tickets CRUD & Workflow
    // -----------------------------------------------------------------------

    pub async fn create_kanban_ticket(
        &self,
        ticket: &KanbanTicketRow,
    ) -> Result<String, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut tx = eng.begin();
        tx.put("kanban_tickets", kanban_to_row(ticket))?;
        tx.commit()?;
        Ok(ticket.ticket_id.clone())
    }

    pub async fn get_kanban_ticket(
        &self,
        ticket_id: &str,
    ) -> Result<Option<KanbanTicketRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let row = eng.get("kanban_tickets", ticket_id)?;
        Ok(row.as_ref().map(kanban_from_row))
    }

    pub async fn list_kanban_tickets(
        &self,
        status: Option<&str>,
        channel: Option<&str>,
    ) -> Result<Vec<KanbanTicketRow>, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        eng.reload()?;
        let mut tickets: Vec<KanbanTicketRow> = eng
            .all("kanban_tickets")?
            .iter()
            .map(kanban_from_row)
            .filter(|t| {
                if let Some(st) = status {
                    if !t.status.eq_ignore_ascii_case(st) {
                        return false;
                    }
                }
                if let Some(ch) = channel {
                    if !t.channel.eq_ignore_ascii_case(ch) {
                        return false;
                    }
                }
                true
            })
            .collect();
        // Sort by created_at descending
        tickets.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tickets)
    }

    pub async fn update_kanban_ticket(
        &self,
        ticket: &KanbanTicketRow,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let mut tx = eng.begin();
        tx.put("kanban_tickets", kanban_to_row(ticket))?;
        tx.commit()?;
        eng.checkpoint()?;
        Ok(())
    }

    pub async fn move_kanban_ticket(
        &self,
        ticket_id: &str,
        new_status: &str,
        youtube_url: Option<&str>,
        video_id: Option<&str>,
    ) -> Result<KanbanTicketRow, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let Some(row) = eng.get("kanban_tickets", ticket_id)? else {
            return Err(TubeforgeError::Usage(format!(
                "ticket not found: {ticket_id}"
            )));
        };
        let mut ticket = kanban_from_row(&row);
        ticket.status = new_status.to_lowercase();
        if let Some(url) = youtube_url {
            ticket.youtube_url = Some(url.to_string());
        }
        if let Some(vid) = video_id {
            ticket.video_id = Some(vid.to_string());
        }
        ticket.updated_at = crate::util::now_rfc3339();

        let mut tx = eng.begin();
        tx.put("kanban_tickets", kanban_to_row(&ticket))?;
        tx.commit()?;
        eng.checkpoint()?;
        Ok(ticket)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "each argument maps 1:1 to an optional kanban edit flag; a params struct would only relocate the list"
    )]
    pub async fn update_kanban_ticket_fields(
        &self,
        ticket_id: &str,
        title: Option<&str>,
        status: Option<&str>,
        topic: Option<&str>,
        framework: Option<&str>,
        duration: Option<i64>,
        keyword: Option<&str>,
        notes: Option<&str>,
    ) -> Result<KanbanTicketRow, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let Some(row) = eng.get("kanban_tickets", ticket_id)? else {
            return Err(TubeforgeError::Usage(format!(
                "ticket not found: {ticket_id}"
            )));
        };
        let mut ticket = kanban_from_row(&row);
        if let Some(t) = title {
            ticket.title = t.to_string();
        }
        if let Some(s) = status {
            ticket.status = s.to_lowercase();
        }
        if let Some(top) = topic {
            ticket.topic = Some(top.to_string());
        }
        if let Some(f) = framework {
            ticket.framework = Some(f.to_string());
        }
        if let Some(d) = duration {
            ticket.optimal_duration_sec = Some(d);
        }
        if let Some(k) = keyword {
            ticket.target_keyword = Some(k.to_string());
        }
        if let Some(n) = notes {
            ticket.notes = Some(n.to_string());
        }
        ticket.updated_at = crate::util::now_rfc3339();

        let mut tx = eng.begin();
        tx.put("kanban_tickets", kanban_to_row(&ticket))?;
        tx.commit()?;
        eng.checkpoint()?;
        Ok(ticket)
    }

    pub async fn delete_kanban_ticket(&self, ticket_id: &str) -> Result<bool, TubeforgeError> {
        let mut eng = self.engine.lock().unwrap();
        let exists = eng.get("kanban_tickets", ticket_id)?.is_some();
        if exists {
            let mut tx = eng.begin();
            tx.delete("kanban_tickets", ticket_id)?;
            tx.commit()?;
            eng.checkpoint()?;
        }
        Ok(exists)
    }
}

/// The write side of one ingest batch. tfdb's `Tx` offers no read access (its
/// only `&mut` borrows the engine), and `merge_channel` needs to read current
/// rows, so the batch wraps the `Db` and commits each operation in its own
/// tfdb transaction instead of holding one long-lived `Tx`.
pub struct Batch<'a> {
    db: &'a mut Db,
}

impl Batch<'_> {
    pub async fn upsert_channel(&mut self, c: &ChannelRow) -> Result<(), TubeforgeError> {
        let mut eng = self.db.engine.lock().unwrap();
        let mut tx = eng.begin();
        tx.put("channels", channel_to_row(c))?;
        tx.commit()?;
        Ok(())
    }

    pub async fn upsert_video(&mut self, v: &VideoRow) -> Result<(), TubeforgeError> {
        let mut eng = self.db.engine.lock().unwrap();
        let mut tx = eng.begin();
        tx.put("videos", video_to_row(v))?;
        tx.commit()?;
        Ok(())
    }

    pub async fn merge_channel(
        &mut self,
        old_id: &str,
        new_id: &str,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.db.engine.lock().unwrap();
        let videos = eng.find_eq("videos", "channel_id", &Value::Text(old_id.to_string()))?;
        let comp_old = eng.get("competitors", old_id)?;
        let comp_new = eng.get("competitors", new_id)?;
        let edges = eng.all("edges")?;
        let alerts = eng.all("alerts")?;
        let edge_pairs: std::collections::HashSet<(String, String)> = edges
            .iter()
            .map(|e| (t(e, "from_channel"), t(e, "to_channel")))
            .collect();

        let mut tx = eng.begin();
        for mut v in videos {
            v.insert("channel_id".to_string(), v_text(new_id));
            tx.put("videos", v)?;
        }
        if comp_old.is_some() {
            tx.delete("competitors", old_id)?;
            if comp_new.is_none() {
                if let Some(mut c) = comp_old {
                    c.insert("channel_id".to_string(), v_text(new_id));
                    tx.put("competitors", c)?;
                }
            }
        }
        for e in edges {
            let from = t(&e, "from_channel");
            let to = t(&e, "to_channel");
            if from == old_id {
                tx.delete("edges", &format!("{old_id}\x1f{to}"))?;
                if new_id != to && !edge_pairs.contains(&(new_id.to_string(), to.clone())) {
                    let mut ne = e.clone();
                    ne.insert("from_channel".to_string(), v_text(new_id));
                    ne.insert("from_to".to_string(), v_text(format!("{new_id}\x1f{to}")));
                    tx.put("edges", ne)?;
                }
            } else if to == old_id {
                tx.delete("edges", &format!("{from}\x1f{old_id}"))?;
                if from != new_id && !edge_pairs.contains(&(from.clone(), new_id.to_string())) {
                    let mut ne = e;
                    ne.insert("to_channel".to_string(), v_text(new_id));
                    ne.insert("from_to".to_string(), v_text(format!("{from}\x1f{new_id}")));
                    tx.put("edges", ne)?;
                }
            }
        }
        for a in alerts {
            if t(&a, "channel_id") == old_id {
                let mut na = a;
                na.insert("channel_id".to_string(), v_text(new_id));
                tx.put("alerts", na)?;
            }
        }
        tx.delete("channels", old_id)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn log_ingest(
        &mut self,
        batch_id: &str,
        item: &str,
        status: &str,
        detail: Option<&str>,
    ) -> Result<(), TubeforgeError> {
        let mut eng = self.db.engine.lock().unwrap();
        let id = next_id(&eng, "ingest_log", "log_id")?;
        let mut row = Row::new();
        row.insert("log_id".to_string(), Value::Int(id));
        row.insert("batch_id".to_string(), v_text(batch_id));
        row.insert("item".to_string(), v_text(item));
        row.insert("status".to_string(), v_text(status));
        row.insert("detail".to_string(), v_opt_text(detail));
        row.insert("at".to_string(), v_text(crate::util::now_rfc3339()));
        let mut tx = eng.begin();
        tx.put("ingest_log", row)?;
        tx.commit()?;
        Ok(())
    }

    pub async fn commit(self) -> Result<(), TubeforgeError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Extract the table name from a legacy `SELECT count(*) FROM <table>` string or plain table name.
fn extract_from_table(sql: &str) -> Option<String> {
    let lower = sql.trim().to_ascii_lowercase();
    if let Some(idx) = lower.find("from") {
        let rest = lower[idx + 4..].trim_start();
        let word: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if word.is_empty() {
            None
        } else {
            Some(word)
        }
    } else {
        let word: String = lower
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if word.is_empty() {
            None
        } else {
            Some(word)
        }
    }
}

/// Extract a simple `WHERE <col> = '<value>'` clause (single-column equality),
/// used by the filtered `count()` calls. Returns (col, value) without quotes.
fn extract_where_eq(sql: &str) -> Option<(String, String)> {
    let lower = sql.to_ascii_lowercase();
    let idx = lower.find("where")?;
    let clause = lower[idx + 5..].trim();
    let eq = clause.find('=')?;
    let col = clause[..eq].trim().to_string();
    if col.is_empty() || !col.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let val = clause[eq + 1..].trim();
    let val = val.trim_matches('\'').trim_matches('"').to_string();
    if val.is_empty() {
        return None;
    }
    Some((col, val))
}

fn alert_exists_engine(eng: &Engine, kind: &str, channel_id: Option<&str>, message: &str) -> bool {
    eng.all("alerts")
        .map(|rows| {
            rows.iter().any(|r| {
                t(r, "kind") == kind
                    && t(r, "message") == message
                    && opt_s(r, "channel_id") == channel_id.map(str::to_string)
            })
        })
        .unwrap_or(false)
}

/// Ensure a tag exists, returning its id (insert-or-ignore semantics).
fn ensure_tag(eng: &mut Engine, name: &str) -> Result<i64, TubeforgeError> {
    if let Some(row) = eng
        .find_eq("tags", "name", &Value::Text(name.to_string()))?
        .first()
    {
        return Ok(i(row, "tag_id"));
    }
    let id = next_id(eng, "tags", "tag_id")?;
    let mut row = Row::new();
    row.insert("tag_id".to_string(), Value::Int(id));
    row.insert("name".to_string(), v_text(name));
    let mut tx = eng.begin();
    tx.put("tags", row)?;
    tx.commit()?;
    Ok(id)
}

/// Persist SERP videos (with their real tags) into the local DB. Free function
/// mirroring `db.rs::persist_serp_db` (called from `research::inspect`).
pub async fn persist_serp_db(
    db: &Db,
    results: &[crate::analytics::research::SerpResult],
) -> Result<(), TubeforgeError> {
    let now = crate::util::now_rfc3339();
    let mut eng = db.engine.lock().unwrap();

    let mut channel_rows: Vec<Row> = Vec::new();
    let mut video_rows: Vec<Row> = Vec::new();
    let mut score_rows: Vec<Row> = Vec::new();
    let mut vt_rows: Vec<Row> = Vec::new();

    for r in results {
        if !r.channel_id.is_empty() {
            let mut chan_row = eng.get("channels", &r.channel_id)?.unwrap_or_default();
            chan_row.insert("channel_id".to_string(), v_text(&r.channel_id));
            if !r.channel.is_empty() {
                chan_row.insert("title".to_string(), v_text(&r.channel));
            }
            chan_row.insert("source".to_string(), v_text("yt-dlp"));
            if !chan_row.contains_key("fetched_at") {
                chan_row.insert("fetched_at".to_string(), v_text(&now));
            }
            chan_row.insert("updated_at".to_string(), v_text(&now));
            channel_rows.push(chan_row);
        }

        let tags_json = serde_json::to_string(&r.tags).unwrap_or_else(|_| "[]".to_string());
        let published = r
            .upload_date
            .as_deref()
            .map(|d| {
                chrono::NaiveDate::parse_from_str(d, "%Y%m%d")
                    .ok()
                    .map(|nd| format!("{}T00:00:00Z", nd.format("%Y-%m-%d")))
                    .unwrap_or_else(|| now.clone())
            })
            .unwrap_or_else(|| now.clone());
        let channel_param: Option<&str> = if r.channel_id.is_empty() {
            None
        } else {
            Some(r.channel_id.as_str())
        };
        let existing = eng.get("videos", &r.video_id)?;
        let tags = match &existing {
            Some(ex) => {
                let cur = json_s(ex, "tags");
                if cur.is_empty() || cur == "[]" {
                    tags_json.clone()
                } else {
                    cur
                }
            }
            None => tags_json.clone(),
        };

        let mut row = existing.unwrap_or_default();
        row.insert("video_id".to_string(), v_text(&r.video_id));
        if let Some(cp) = channel_param {
            row.insert("channel_id".to_string(), v_text(cp));
        }
        if !r.title.is_empty() {
            row.insert("title".to_string(), v_text(&r.title));
        }
        row.insert("tags".to_string(), v_json(&tags));
        if !row.contains_key("published_at") || row.get("published_at").and_then(|v| v.as_text()).map(|s| s.is_empty()).unwrap_or(true) {
            row.insert("published_at".to_string(), v_text(&published));
        }
        if let Some(vc) = r.view_count {
            row.insert("view_count".to_string(), Value::Int(vc));
        }
        if let Some(lc) = r.like_count {
            row.insert("like_count".to_string(), Value::Int(lc));
        }
        if let Some(cc) = r.comment_count {
            row.insert("comment_count".to_string(), Value::Int(cc));
        }
        if let Some(dur) = r.duration_sec {
            row.insert("duration_sec".to_string(), Value::Int(dur));
        }
        if let Some(ref thumb) = r.thumb_url {
            row.insert("thumb_url".to_string(), v_text(thumb));
        } else if !row.contains_key("thumb_url") {
            row.insert("thumb_url".to_string(), v_text(&format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", r.video_id)));
        }
        if let Some(ref desc) = r.description {
            if !desc.is_empty() {
                row.insert("description".to_string(), v_text(desc));
            }
        }
        row.insert("source".to_string(), v_text("yt-dlp"));
        if !row.contains_key("fetched_at") {
            row.insert("fetched_at".to_string(), v_text(&now));
        }
        row.insert("updated_at".to_string(), v_text(&now));
        video_rows.push(row);

        if r.seo_score > 0.0 {
            let mut srow = Row::new();
            srow.insert("video_id".to_string(), v_text(&r.video_id));
            srow.insert("seo_score".to_string(), Value::Float(r.seo_score));
            srow.insert("geo_score".to_string(), Value::Float(0.0));
            srow.insert("total_score".to_string(), Value::Float(r.seo_score));
            srow.insert("components".to_string(), v_json("{}"));
            srow.insert("computed_at".to_string(), v_text(&now));
            score_rows.push(srow);
        }

        for (pos, tag) in r.tags.iter().enumerate() {
            let tag = tag.trim();
            if tag.is_empty() {
                continue;
            }
            let tag_id = ensure_tag(&mut eng, tag)?;
            if tag_id > 0 {
                let mut vrow = Row::new();
                vrow.insert(
                    "video_tag_id".to_string(),
                    v_text(format!("{}\x1f{tag_id}", r.video_id)),
                );
                vrow.insert("video_id".to_string(), v_text(&r.video_id));
                vrow.insert("tag_id".to_string(), Value::Int(tag_id));
                vrow.insert("position".to_string(), Value::Int(pos as i64));
                vrow.insert("source".to_string(), v_text("youtube"));
                vt_rows.push(vrow);
            }
        }
    }

    let mut tx = eng.begin();
    for row in channel_rows {
        tx.put("channels", row)?;
    }
    for row in video_rows {
        tx.put("videos", row)?;
    }
    for row in score_rows {
        tx.put("scores", row)?;
    }
    for row in vt_rows {
        tx.put("video_tags", row)?;
    }
    tx.commit()?;
    Ok(())
}

/// Collapse duplicate videos: rows sharing (channel_id, title) merge into one
/// record, keeping the richest row. Returns (merged_groups, deleted_rows).
pub async fn dedupe_videos(db: &Db) -> Result<(usize, usize), TubeforgeError> {
    let mut eng = db.engine.lock().unwrap();
    let videos: Vec<VideoRow> = eng.all("videos")?.iter().map(video_from_row).collect();

    let mut groups: HashMap<(String, String), Vec<&VideoRow>> = HashMap::new();
    for v in &videos {
        if let Some(cid) = &v.channel_id {
            groups
                .entry((cid.clone(), v.title.clone()))
                .or_default()
                .push(v);
        }
    }

    let mut merged = 0usize;
    let mut deleted = 0usize;

    for group in groups.into_values().filter(|vs| vs.len() > 1) {
        let mut winner = group[0];
        for v in &group[1..] {
            let rank = |x: &VideoRow| {
                let desc = if x.description.trim().is_empty() {
                    0
                } else {
                    1
                };
                (desc, x.view_count.unwrap_or(0))
            };
            if rank(v) > rank(winner) {
                winner = v;
            }
        }
        let winner_video_tags: HashMap<i64, bool> = eng
            .find_eq(
                "video_tags",
                "video_id",
                &Value::Text(winner.video_id.clone()),
            )?
            .into_iter()
            .map(|r| (i(&r, "tag_id"), true))
            .collect();

        for v in &group {
            if v.video_id == winner.video_id {
                continue;
            }
            let vts = eng.find_eq("video_tags", "video_id", &Value::Text(v.video_id.clone()))?;
            let comments = eng.find_eq("comments", "video_id", &Value::Text(v.video_id.clone()))?;
            let krs = eng.find_eq(
                "keyword_rankings",
                "video_id",
                &Value::Text(v.video_id.clone()),
            )?;
            let idrs = eng.find_eq("ideas", "source_video", &Value::Text(v.video_id.clone()))?;

            let mut tx = eng.begin();
            for t in ["scores", "video_heatmap", "transcripts"] {
                tx.delete(t, &v.video_id)?;
            }
            for vt in vts {
                let tag_id = i(&vt, "tag_id");
                if winner_video_tags.contains_key(&tag_id) {
                    if let Some(tid) = vt.get("video_tag_id").and_then(|x| x.as_text()) {
                        tx.delete("video_tags", tid)?;
                    }
                } else {
                    let winner_pk = format!("{}\x1f{tag_id}", winner.video_id);
                    if let Some(tid) = vt.get("video_tag_id").and_then(|x| x.as_text()) {
                        tx.delete("video_tags", tid)?;
                    }
                    let mut nv = vt;
                    nv.insert("video_id".to_string(), v_text(&winner.video_id));
                    nv.insert("video_tag_id".to_string(), v_text(&winner_pk));
                    tx.put("video_tags", nv)?;
                }
            }
            for c in comments {
                let mut nc = c;
                nc.insert("video_id".to_string(), v_text(&winner.video_id));
                tx.put("comments", nc)?;
            }
            for kr in krs {
                let mut nk = kr;
                nk.insert("video_id".to_string(), v_text(&winner.video_id));
                tx.put("keyword_rankings", nk)?;
            }
            for idr in idrs {
                let mut ni = idr;
                ni.insert("source_video".to_string(), v_text(&winner.video_id));
                tx.put("ideas", ni)?;
            }
            tx.delete("videos", &v.video_id)?;
            tx.commit()?;
            deleted += 1;
        }
        merged += 1;
    }
    Ok((merged, deleted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Runtime::new().expect("rt").block_on(f)
    }

    fn open_test() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = block_on(Db::open(&dir.path().join("t.db"))).expect("open");
        (dir, db)
    }

    proptest::proptest! {
        #[test]
        fn add_keywords_roundtrip_preserves_all(
            keywords in proptest::collection::vec("[a-zA-Z0-9 ]{0,20}", 0..50),
        ) {
            let (_d, db) = open_test();
            let added = block_on(db.add_keywords(&keywords, Some("test"))).unwrap();
            let listed = block_on(db.list_keywords()).unwrap();

            let mut expected: Vec<String> = keywords
                .iter()
                .map(|k| k.trim().to_lowercase())
                .filter(|k| !k.is_empty())
                .collect();
            expected.sort();
            expected.dedup();

            prop_assert_eq!(added, expected.len(), "added count");
            let mut names: Vec<String> = listed.iter().map(|k| k.keyword.clone()).collect();
            names.sort();
            prop_assert_eq!(names, expected, "round-trip preserves all keywords");
            prop_assert!(
                listed.iter().all(|k| k.niche.as_deref() == Some("test")),
                "niche preserved"
            );
        }

        #[test]
        fn add_keywords_never_duplicates(
            keywords in proptest::collection::vec("[a-zA-Z ]{0,10}", 1..30),
        ) {
            let (_d, db) = open_test();
            block_on(db.add_keywords(&keywords, None)).unwrap();
            block_on(db.add_keywords(&keywords, None)).unwrap();

            let listed = block_on(db.list_keywords()).unwrap();
            let mut names: Vec<String> = listed.iter().map(|k| k.keyword.clone()).collect();
            names.sort();
            let before = names.len();
            names.dedup();
            prop_assert_eq!(names.len(), before, "no duplicate rows after double-insert");

            let mut expected: Vec<String> = keywords
                .iter()
                .map(|k| k.trim().to_lowercase())
                .filter(|k| !k.is_empty())
                .collect();
            expected.sort();
            expected.dedup();
            prop_assert_eq!(listed.len(), expected.len(), "normalized set matches");
        }
    }

    #[test]
    fn upsert_score_roundtrip() {
        let (_d, db) = open_test();
        block_on(db.upsert_score("v1", 0.5, 0.25, 0.75, "{\"seo\":1}")).unwrap();
        let s = block_on(db.get_score("v1"))
            .unwrap()
            .expect("score present");
        assert_eq!(s.video_id, "v1");
        assert_eq!(s.seo_score, 0.5);
        assert_eq!(s.geo_score, 0.25);
        assert_eq!(s.total_score, 0.75);

        block_on(db.upsert_score("v1", 0.9, 0.1, 1.0, "{}")).unwrap();
        let s2 = block_on(db.get_score("v1")).unwrap().unwrap();
        assert_eq!(s2.total_score, 1.0);
        assert!(block_on(db.get_score("missing")).unwrap().is_none());
    }

    #[test]
    fn meta_roundtrip() {
        let (_d, db) = open_test();
        assert_eq!(block_on(db.meta_get("nope")).unwrap(), None);
        block_on(db.meta_set("foo", "bar")).unwrap();
        assert_eq!(
            block_on(db.meta_get("foo")).unwrap(),
            Some("bar".to_string())
        );
        block_on(db.meta_set("foo", "baz")).unwrap();
        assert_eq!(
            block_on(db.meta_get("foo")).unwrap(),
            Some("baz".to_string())
        );
        assert_eq!(block_on(db.user_version()).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn ranking_ordered_by_keyword_then_checked_at() {
        let (_d, db) = open_test();
        block_on(db.upsert_ranking("beta", "2026-01-01", Some("v1"), Some(1), None)).unwrap();
        block_on(db.upsert_ranking("alpha", "2026-01-02", None, None, None)).unwrap();
        block_on(db.upsert_ranking("alpha", "2026-01-01", Some("v2"), Some(2), None)).unwrap();
        // Overwrite the same (keyword, checked_at) pk.
        block_on(db.upsert_ranking("alpha", "2026-01-01", Some("v3"), Some(3), None)).unwrap();

        let rows = block_on(db.list_rankings()).unwrap();
        let keys: Vec<(String, String)> = rows
            .iter()
            .map(|r| (r.keyword.clone(), r.checked_at.clone()))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("alpha".to_string(), "2026-01-01".to_string()),
                ("alpha".to_string(), "2026-01-02".to_string()),
                ("beta".to_string(), "2026-01-01".to_string()),
            ]
        );
        assert_eq!(block_on(db.ranking_count_at("2026-01-01")).unwrap(), 2);
        // Same-instant overwrite means only one row per (keyword, checked_at).
        assert_eq!(
            rows.iter()
                .filter(|r| r.keyword == "alpha" && r.checked_at == "2026-01-01")
                .count(),
            1
        );
    }

    #[test]
    fn all_videos_ordered_by_id() {
        let (_d, mut db) = open_test();
        let mut batch = block_on(db.begin_batch()).unwrap();
        for id in ["c", "a", "b"] {
            let v = VideoRow {
                video_id: id.to_string(),
                title: format!("video {id}"),
                ..Default::default()
            };
            block_on(batch.upsert_video(&v)).unwrap();
        }
        block_on(batch.commit()).unwrap();

        let ids: Vec<String> = block_on(db.all_videos())
            .unwrap()
            .into_iter()
            .map(|v| v.video_id)
            .collect();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn set_idea_statuses() {
        let (_d, db) = open_test();
        let id1 = block_on(db.upsert_idea("idea a", "[]", 1.0, "draft", None)).unwrap();
        let id2 = block_on(db.upsert_idea("idea b", "[]", 2.0, "draft", None)).unwrap();

        let marked = block_on(db.set_idea_statuses(&[id1, id2], "saved")).unwrap();
        assert_eq!(marked, 2);
        let ideas = block_on(db.all_ideas()).unwrap();
        assert!(ideas.iter().all(|i| i.status == "saved"));
        assert_eq!(
            block_on(db.set_idea_statuses(&[999_999], "discarded")).unwrap(),
            0
        );
    }

    #[test]
    fn insert_alert_dedupes() {
        let (_d, db) = open_test();
        assert_eq!(
            block_on(db.insert_alert("brand", Some("c1"), "msg", "info")).unwrap(),
            1
        );
        assert_eq!(
            block_on(db.insert_alert("brand", Some("c1"), "msg", "info")).unwrap(),
            0
        );
        assert_eq!(
            block_on(db.insert_alert("brand", Some("c2"), "msg", "info")).unwrap(),
            1
        );
        assert_eq!(
            block_on(db.insert_alert("brand", None, "msg", "info")).unwrap(),
            1
        );
        assert_eq!(
            block_on(db.insert_alert("brand", None, "msg", "info")).unwrap(),
            0
        );
        assert_eq!(block_on(db.list_alerts(0)).unwrap().len(), 3);
    }

    #[test]
    fn test_kanban_ticket_crud() {
        let (_d, db) = open_test();
        let ticket = KanbanTicketRow {
            ticket_id: "ticket-test1".to_string(),
            title: "Test Video 1".to_string(),
            channel: "BOOKVERSE".to_string(),
            status: "todo".to_string(),
            topic: Some("influence".to_string()),
            framework: Some("7 Levers".to_string()),
            optimal_duration_sec: Some(720),
            target_keyword: Some("influence summary".to_string()),
            youtube_url: None,
            video_id: None,
            research_ref: Some("influence".to_string()),
            notes: Some("test notes".to_string()),
            created_at: crate::util::now_rfc3339(),
            updated_at: crate::util::now_rfc3339(),
        };

        block_on(db.create_kanban_ticket(&ticket)).unwrap();
        let fetched = block_on(db.get_kanban_ticket("ticket-test1"))
            .unwrap()
            .expect("found");
        assert_eq!(fetched.title, "Test Video 1");
        assert_eq!(fetched.status, "todo");

        let moved = block_on(db.move_kanban_ticket(
            "ticket-test1",
            "inprogress",
            Some("https://youtube.com/watch?v=123"),
            Some("123"),
        ))
        .unwrap();
        assert_eq!(moved.status, "inprogress");
        assert_eq!(
            moved.youtube_url.as_deref(),
            Some("https://youtube.com/watch?v=123")
        );

        let list = block_on(db.list_kanban_tickets(Some("inprogress"), Some("BOOKVERSE"))).unwrap();
        assert_eq!(list.len(), 1);

        let deleted = block_on(db.delete_kanban_ticket("ticket-test1")).unwrap();
        assert!(deleted);
        assert!(block_on(db.get_kanban_ticket("ticket-test1"))
            .unwrap()
            .is_none());
    }
}
