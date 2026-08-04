//! Turso connection wrapper (LLD §2 storage/db.rs).
//!
//! Real API of turso 0.7.2 (verified against crate source, Aug 3 2026):
//! - `Builder::new_local(path).experimental_*(..).build().await` -> `Database`
//! - `db.connect()` -> `Connection` (sync call; async statements)
//! - `conn.execute(sql, params).await`, `conn.query(sql, params).await`
//! - `conn.prepare(sql).await` -> `Statement` with `.query()/.execute()/.query_row()`
//! - `conn.transaction().await` / `transaction_with_behavior(..)` -> `Transaction`
//!   (requires `&mut Connection`; `commit()`, `rollback()`, derefs to Connection)
//! - `rows.next().await` -> `Option<Row>`; `row.get_value(idx)` -> `Value`
//!
//! Notes on the engine:
//! - `PRAGMA journal_mode=WAL` returns a row, so it must be run as a QUERY
//!   (`conn.execute` errors with Misuse "unexpected row during execution").
//! - `VACUUM INTO` requires the engine's `vacuum` experimental feature
//!   (enabled via `Builder::experimental_vacuum(true)`).
//! - FTS index method requires `experimental_index_method(true)`; the crate's
//!   `fts` cargo feature is on by default in turso 0.7.2.
//! - Params: homogeneous const arrays/Vecs, heterogeneous tuples (≤16) and
//!   the `params!` macro all work; `Option<T>` binds as NULL.
//!
//! **Dependency rule:** this is the ONLY module in the crate that imports
//! `turso`. All SQL a domain module needs goes through the repository methods
//! here (`get_video`, `Batch::upsert_video`, ...), so ingest.rs never touches
//! the engine crate.

use std::path::{Path, PathBuf};

use turso::params;
use turso::transaction::{Transaction, TransactionBehavior};
use turso::Value;

use super::schema::{MIGRATIONS, SCHEMA_VERSION};
use crate::error::{storage_err, TubeforgeError};

/// A live connection to the Turso database.
pub struct Db {
    pub conn: turso::Connection,
    pub path: PathBuf,
}

// ---------------------------------------------------------------------------
// Row types (LLD §3.1) — the only shape domain modules deal with.
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
    pub tags: String, // JSON array string (LLD §3.1)
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
    /// `recordingDetails.recordingDate` (RFC3339, date-only; C1).
    pub recording_date: Option<String>,
    /// `recordingDetails.location.locationDescription` (C1).
    pub recording_location_name: Option<String>,
    pub recording_lat: Option<f64>,
    pub recording_lng: Option<f64>,
    /// `topicDetails.topicCategories` — JSON array of category URLs (C2);
    /// labels are derived at read time, never stored twice.
    pub topic_categories: String,
}

/// One row of the `scores` table (LLD §3.1).
#[derive(Debug, Clone, Default)]
pub struct ScoreRow {
    pub video_id: String,
    pub seo_score: f64,
    pub geo_score: f64,
    pub total_score: f64,
    pub components: String, // flat JSON, seo + geo keys (LLD §7.5)
    pub computed_at: String,
}

/// One row of the `keywords` table.
#[derive(Debug, Clone, Default)]
pub struct KeywordRow {
    pub keyword: String,
    pub niche: Option<String>,
    pub created_at: String,
}

/// One row of the `keyword_rankings` table (position NULL = not ranked;
/// topics = derived topic labels of the winning video, JSON array).
#[derive(Debug, Clone, Default)]
pub struct RankingRow {
    pub keyword: String,
    pub checked_at: String,
    pub video_id: Option<String>,
    pub position: Option<i64>,
    pub topics: Option<String>,
}

/// One row of the `edges` table (competitor graph).
#[derive(Debug, Clone, Default)]
pub struct EdgeRow {
    pub from_channel: String,
    pub to_channel: String,
    pub weight: f64,
    pub source: String, // overlap | manual
}

/// One row of the `ideas` table.
#[derive(Debug, Clone, Default)]
pub struct IdeaRow {
    pub idea_id: i64,
    pub title_suggestion: String,
    pub rationale: String, // JSON: signals that fired
    pub score: f64,
    pub status: String, // draft | saved | discarded
    pub source_video: Option<String>,
    pub created_at: String,
}

/// One row of the `alerts` table.
#[derive(Debug, Clone, Default)]
pub struct AlertRow {
    pub alert_id: i64,
    pub kind: String, // brand | gap | quota | integrity
    pub channel_id: Option<String>,
    pub message: String,
    pub severity: String, // info | warn | critical
    pub created_at: String,
    pub read_at: Option<String>,
}

/// One row of the `ingest_log` table (most recent first).
#[derive(Debug, Clone, Default)]
pub struct IngestLogRow {
    pub batch_id: String,
    pub item: String,
    pub status: String,
    pub detail: Option<String>,
    pub at: String,
}

const CHANNEL_COLS: &str = "channel_id, handle, title, description, avatar_url, country, \
                            subscriber_count, video_count, source, etag, fetched_at, updated_at";
const VIDEO_COLS: &str = "video_id, channel_id, title, description, tags, category_id, \
                          duration_sec, published_at, view_count, like_count, comment_count, \
                          thumb_url, source, fetched_at, updated_at, recording_date, \
                          recording_location_name, recording_lat, recording_lng, \
                          topic_categories";

// ---------------------------------------------------------------------------
// Open / migrate / integrity
// ---------------------------------------------------------------------------

impl Db {
    /// Open (creating if needed) the database at `path`, ensure WAL journal
    /// mode, apply migrations, and run `integrity_check`.
    pub async fn open(path: &Path) -> Result<Db, TubeforgeError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| storage_err("IO", format!("create dir {}: {e}", parent.display())))?;
            }
        }

        let db_handle = turso::Builder::new_local(path.to_str().unwrap_or(":memory:"))
            .experimental_vacuum(true)
            .experimental_index_method(true)
            .build()
            .await
            .map_err(|e| storage_err("OPEN", e))?;
        let conn = db_handle.connect().map_err(|e| storage_err("CONNECT", e))?;

        let mut db = Db { conn, path: path.to_path_buf() };

        // WAL is the locked journal mode (ADR-5). Run as a query: PRAGMA
        // journal_mode returns one row with the resulting mode.
        let mode = db.journal_mode().await?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(TubeforgeError::Storage {
                code: "JOURNAL_MODE".to_string(),
                message: format!("expected WAL journal mode, got {mode}"),
            });
        }

        db.migrate().await?;
        db.integrity_check().await?;
        Ok(db)
    }

    /// Apply pending migrations (LLD §9.3) in one transaction. Migration 001
    /// is idempotent and is always applied, so Phase 0 databases (meta-only)
    /// gain the full v1 schema in place; migration 002 is version-gated
    /// (v1 → v2 adds the recordingDetails/topicDetails columns). Version
    /// markers are bumped only when the recorded `user_version` is behind
    /// `SCHEMA_VERSION`.
    pub async fn migrate(&mut self) -> Result<(), TubeforgeError> {
        let current = self.user_version().await?;

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(|e| storage_err("BEGIN", e))?;

        for m in MIGRATIONS {
            if m.version > current || m.idempotent {
                tx.execute_batch(m.sql)
                    .await
                    .map_err(|e| storage_err("MIGRATE", e))?;
            }
        }

        if current < SCHEMA_VERSION {
            // PRAGMA user_version = N: in turso 0.7.2 this SET form returns NO
            // rows (unlike SQLite), so it must be executed, not queried.
            tx.execute(format!("PRAGMA user_version = {SCHEMA_VERSION}"), ())
                .await
                .map_err(|e| storage_err("USER_VERSION", e))?;
            set_meta_key(&tx, "schema_version", &SCHEMA_VERSION.to_string()).await?;
        }

        tx.commit()
            .await
            .map_err(|e| storage_err("COMMIT", e))?;
        Ok(())
    }

    /// `PRAGMA user_version` (row-returning read; goes through `query`).
    pub async fn user_version(&self) -> Result<i64, TubeforgeError> {
        query_i64(&self.conn, "PRAGMA user_version").await
    }

    /// `PRAGMA journal_mode` (run as query; returns a row).
    pub async fn journal_mode(&self) -> Result<String, TubeforgeError> {
        query_text(&self.conn, "PRAGMA journal_mode").await
    }

    /// `PRAGMA integrity_check`; on failure returns `Integrity` (exit 5).
    pub async fn integrity_check(&self) -> Result<(), TubeforgeError> {
        let mut stmt = self
            .conn
            .prepare("PRAGMA integrity_check")
            .await
            .map_err(|e| storage_err("INTEGRITY", e))?;
        let mut rows = stmt
            .query(())
            .await
            .map_err(|e| storage_err("INTEGRITY", e))?;
        let mut problems: Vec<String> = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| storage_err("INTEGRITY", e))? {
            if let Value::Text(msg) = row.get_value(0).map_err(|e| storage_err("INTEGRITY", e))? {
                if msg != "ok" {
                    problems.push(msg);
                }
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(TubeforgeError::Integrity {
                detail: problems.join("; "),
            })
        }
    }

    /// Consistent single-file snapshot via `VACUUM INTO 'dest'` (LLD §9.1).
    /// Requires the engine `vacuum` experimental feature (Builder flag).
    pub async fn vacuum_into(&self, dest: &Path) -> Result<(), TubeforgeError> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| storage_err("IO", format!("create dir {}: {e}", parent.display())))?;
        }
        let sql = format!(
            "VACUUM INTO '{}'",
            dest.to_str().unwrap_or_default().replace('\'', "''")
        );
        self.conn
            .execute(sql, ())
            .await
            .map_err(|e| storage_err("VACUUM_INTO", e))?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // meta key/value
    // -----------------------------------------------------------------------

    /// Read a meta key (None when absent).
    pub async fn meta_get(&self, key: &str) -> Result<Option<String>, TubeforgeError> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM meta WHERE key = ?1")
            .await
            .map_err(|e| storage_err("META_GET", e))?;
        let row = stmt
            .query([key])
            .await
            .map_err(|e| storage_err("META_GET", e))?
            .next()
            .await
            .map_err(|e| storage_err("META_GET", e))?;
        match row {
            Some(r) => match r.get_value(0).map_err(|e| storage_err("META_GET", e))? {
                Value::Text(v) => Ok(Some(v)),
                _ => Ok(None),
            },
            None => Ok(None),
        }
    }

    /// Upsert a meta key on the live connection.
    pub async fn meta_set(&self, key: &str, value: &str) -> Result<(), TubeforgeError> {
        set_meta_key(&self.conn, key, value).await
    }

    // -----------------------------------------------------------------------
    // Repository reads (single-connection sequential — no writers pending)
    // -----------------------------------------------------------------------

    pub async fn get_channel(&self, id: &str) -> Result<Option<ChannelRow>, TubeforgeError> {
        let sql = format!("SELECT {CHANNEL_COLS} FROM channels WHERE channel_id = ?1");
        query_row_owned(&self.conn, &sql, [id], channel_from_values).await
    }

    pub async fn get_video(&self, id: &str) -> Result<Option<VideoRow>, TubeforgeError> {
        let sql = format!("SELECT {VIDEO_COLS} FROM videos WHERE video_id = ?1");
        query_row_owned(&self.conn, &sql, [id], video_from_values).await
    }

    pub async fn all_channels(&self) -> Result<Vec<ChannelRow>, TubeforgeError> {
        let sql = format!("SELECT {CHANNEL_COLS} FROM channels ORDER BY channel_id");
        query_rows_owned(&self.conn, &sql, (), channel_from_values).await
    }

    pub async fn all_videos(&self) -> Result<Vec<VideoRow>, TubeforgeError> {
        let sql = format!("SELECT {VIDEO_COLS} FROM videos ORDER BY video_id");
        query_rows_owned(&self.conn, &sql, (), video_from_values).await
    }

    /// Insert an alert (LLD §8.4); used by ingest on quota degradation.
    pub async fn insert_alert(
        &self,
        kind: &str,
        channel_id: Option<&str>,
        message: &str,
        severity: &str,
    ) -> Result<(), TubeforgeError> {
        let at = crate::util::now_rfc3339();
        self.conn
            .execute(
                "INSERT INTO alerts (kind, channel_id, message, severity, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params!(
                    kind,
                    channel_id,
                    message,
                    severity,
                    at
                ),
            )
            .await
            .map_err(|e| storage_err("ALERT", e))?;
        Ok(())
    }

    /// Row count helper used by tests and health checks.
    pub async fn count(&self, sql: &str) -> Result<i64, TubeforgeError> {
        let mut stmt = self
            .conn
            .prepare(sql)
            .await
            .map_err(|e| storage_err("COUNT", e))?;
        let row = stmt
            .query_row(())
            .await
            .map_err(|e| storage_err("COUNT", e))?;
        match row.get_value(0).map_err(|e| storage_err("COUNT", e))? {
            Value::Integer(n) => Ok(n),
            _ => Err(TubeforgeError::Storage {
                code: "COUNT".to_string(),
                message: "expected integer result".to_string(),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Phase 2 repository reads/writes (single statements, one writer — no
    // transaction spans multiple writes here; callers own batch semantics).
    // -----------------------------------------------------------------------

    /// Upsert the persisted score row for one video (LLD §6.4, §7.5).
    pub async fn upsert_score(
        &self,
        video_id: &str,
        seo: f64,
        geo: f64,
        total: f64,
        components: &str,
    ) -> Result<(), TubeforgeError> {
        self.conn
            .execute(
                "INSERT INTO scores (video_id, seo_score, geo_score, total_score, \
                                     components, computed_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(video_id) DO UPDATE SET \
                   seo_score = excluded.seo_score, geo_score = excluded.geo_score, \
                   total_score = excluded.total_score, components = excluded.components, \
                   computed_at = excluded.computed_at",
                params!(
                    video_id,
                    seo,
                    geo,
                    total,
                    components,
                    crate::util::now_rfc3339()
                ),
            )
            .await
            .map_err(|e| storage_err("UPSERT_SCORE", e))?;
        Ok(())
    }

    pub async fn get_score(&self, video_id: &str) -> Result<Option<ScoreRow>, TubeforgeError> {
        let sql = "SELECT video_id, seo_score, geo_score, total_score, components, computed_at \
                   FROM scores WHERE video_id = ?1";
        query_row_owned(&self.conn, sql, [video_id], score_from_values).await
    }

    /// All score rows ordered by total_score DESC (scorecard/ideas inputs).
    pub async fn all_scores(&self) -> Result<Vec<ScoreRow>, TubeforgeError> {
        let sql = "SELECT video_id, seo_score, geo_score, total_score, components, computed_at \
                   FROM scores ORDER BY total_score DESC";
        query_rows_owned(&self.conn, sql, (), score_from_values).await
    }

    /// Insert tracked keywords (INSERT OR IGNORE — idempotent). Returns the
    /// number of keywords newly added.
    pub async fn add_keywords(
        &self,
        keywords: &[String],
        niche: Option<&str>,
    ) -> Result<usize, TubeforgeError> {
        let at = crate::util::now_rfc3339();
        let mut added = 0;
        for kw in keywords {
            if kw.trim().is_empty() {
                continue;
            }
            let n = self
                .conn
                .execute(
                    "INSERT OR IGNORE INTO keywords (keyword, niche, created_at) \
                     VALUES (?1, ?2, ?3)",
                    params!(kw.trim(), niche, at.as_str()),
                )
                .await
                .map_err(|e| storage_err("ADD_KEYWORD", e))?;
            added += n as usize;
        }
        Ok(added)
    }

    pub async fn list_keywords(&self) -> Result<Vec<KeywordRow>, TubeforgeError> {
        let sql = "SELECT keyword, niche, created_at FROM keywords ORDER BY keyword";
        query_rows_owned(&self.conn, sql, (), keyword_from_values).await
    }

    /// One `keyword_rankings` snapshot row; the PK is (keyword, checked_at),
    /// so a same-instant recheck overwrites instead of duplicating. `topics`
    /// is the JSON array of derived topic labels of the winning video.
    pub async fn upsert_ranking(
        &self,
        keyword: &str,
        checked_at: &str,
        video_id: Option<&str>,
        position: Option<i64>,
        topics: Option<&str>,
    ) -> Result<(), TubeforgeError> {
        self.conn
            .execute(
                "INSERT INTO keyword_rankings (keyword, checked_at, video_id, position, topics) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(keyword, checked_at) DO UPDATE SET \
                   video_id = excluded.video_id, position = excluded.position, \
                   topics = excluded.topics",
                params!(keyword, checked_at, video_id, position, topics),
            )
            .await
            .map_err(|e| storage_err("RANKING", e))?;
        Ok(())
    }

    /// All snapshots ordered by keyword, then checked_at ascending — Rust
    /// computes trends/deltas from this (LLD §8.3: lag/lead unavailable).
    pub async fn list_rankings(&self) -> Result<Vec<RankingRow>, TubeforgeError> {
        let sql = "SELECT keyword, checked_at, video_id, position, topics \
                   FROM keyword_rankings ORDER BY keyword, checked_at";
        query_rows_owned(&self.conn, sql, (), ranking_from_values).await
    }

    pub async fn list_edges(&self) -> Result<Vec<EdgeRow>, TubeforgeError> {
        let sql = "SELECT from_channel, to_channel, weight, source FROM edges \
                   ORDER BY from_channel, to_channel";
        query_rows_owned(&self.conn, sql, (), edge_from_values).await
    }

    /// Upsert one edge. A `manual` edge is never overwritten by an `overlap`
    /// upsert (the conflict target keeps the manual row).
    pub async fn upsert_edge(
        &self,
        from_channel: &str,
        to_channel: &str,
        weight: f64,
        source: &str,
    ) -> Result<(), TubeforgeError> {
        self.conn
            .execute(
                "INSERT INTO edges (from_channel, to_channel, weight, source) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(from_channel, to_channel) DO UPDATE SET \
                   weight = excluded.weight, source = excluded.source \
                 WHERE edges.source = 'overlap'",
                params!(from_channel, to_channel, weight, source),
            )
            .await
            .map_err(|e| storage_err("EDGE", e))?;
        Ok(())
    }

    /// Drop the auto-suggested overlap edges before re-syncing (the graph
    /// cache is derived data — rebuildable, LLD §8.1). Manual edges survive.
    pub async fn delete_overlap_edges(&self) -> Result<usize, TubeforgeError> {
        let n = self
            .conn
            .execute("DELETE FROM edges WHERE source = 'overlap'", ())
            .await
            .map_err(|e| storage_err("EDGE_DELETE", e))?;
        Ok(n as usize)
    }

    /// Insert a Next Idea (draft) or refresh an existing row for the same
    /// (title_suggestion, source_video). Returns the row id.
    pub async fn upsert_idea(
        &self,
        title_suggestion: &str,
        rationale: &str,
        score: f64,
        status: &str,
        source_video: Option<&str>,
    ) -> Result<i64, TubeforgeError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT idea_id FROM ideas \
                 WHERE title_suggestion = ?1 AND (?2 IS NULL AND source_video IS NULL \
                   OR source_video = ?2)",
            )
            .await
            .map_err(|e| storage_err("IDEA", e))?;
        let mut rows = stmt
            .query(params!(title_suggestion, source_video))
            .await
            .map_err(|e| storage_err("IDEA", e))?;
        let existing = match rows.next().await.map_err(|e| storage_err("IDEA", e))? {
            Some(r) => match r.get_value(0).map_err(|e| storage_err("IDEA", e))? {
                Value::Integer(id) => Some(id),
                _ => None,
            },
            None => None,
        };

        if let Some(id) = existing {
            self.conn
                .execute(
                    "UPDATE ideas SET rationale = ?1, score = ?2, status = ?3 \
                     WHERE idea_id = ?4",
                    params!(rationale, score, status, id),
                )
                .await
                .map_err(|e| storage_err("IDEA_UPDATE", e))?;
            Ok(id)
        } else {
            let at = crate::util::now_rfc3339();
            self.conn
                .execute(
                    "INSERT INTO ideas (title_suggestion, rationale, score, status, \
                                        source_video, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params!(title_suggestion, rationale, score, status, source_video, at.as_str()),
                )
                .await
                .map_err(|e| storage_err("IDEA_INSERT", e))?;
            // Rowid of the last insert on this connection.
            query_i64(&self.conn, "SELECT last_insert_rowid()").await
        }
    }

    /// Ideas ordered by score DESC; `status` filters when given.
    pub async fn list_ideas(
        &self,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<IdeaRow>, TubeforgeError> {
        let sql = "SELECT idea_id, title_suggestion, rationale, score, status, source_video, \
                   created_at FROM ideas \
                   WHERE (?1 IS NULL OR status = ?1) ORDER BY score DESC, idea_id DESC LIMIT ?2";
        query_rows_owned(&self.conn, sql, params!(status, limit as i64), idea_from_values).await
    }

    /// Mark a set of idea ids with a status (draft|saved|discarded).
    pub async fn set_idea_statuses(&self, ids: &[i64], status: &str) -> Result<usize, TubeforgeError> {
        let mut marked = 0;
        for id in ids {
            marked += self
                .conn
                .execute(
                    "UPDATE ideas SET status = ?1 WHERE idea_id = ?2",
                    params!(status, *id),
                )
                .await
                .map_err(|e| storage_err("IDEA_STATUS", e))? as usize;
        }
        Ok(marked)
    }

    /// Newest alerts first (limit 0 = all).
    pub async fn list_alerts(&self, limit: usize) -> Result<Vec<AlertRow>, TubeforgeError> {
        let sql = "SELECT alert_id, kind, channel_id, message, severity, created_at, read_at \
                   FROM alerts ORDER BY created_at DESC, alert_id DESC LIMIT ?1";
        query_rows_owned(&self.conn, sql, params!(limit as i64), alert_from_values).await
    }

    /// True when an identical alert (kind, channel_id, message) already exists —
    /// used to keep rule evaluation idempotent across runs (LLD §8.4).
    pub async fn alert_exists(
        &self,
        kind: &str,
        channel_id: Option<&str>,
        message: &str,
    ) -> Result<bool, TubeforgeError> {
        let sql = "SELECT count(*) FROM alerts \
                   WHERE kind = ?1 AND message = ?2 \
                     AND ((?3 IS NULL AND channel_id IS NULL) OR channel_id = ?3)";
        let mut stmt = self
            .conn
            .prepare(sql)
            .await
            .map_err(|e| storage_err("ALERT_EXISTS", e))?;
        let row = stmt
            .query(params!(kind, message, channel_id))
            .await
            .map_err(|e| storage_err("ALERT_EXISTS", e))?
            .next()
            .await
            .map_err(|e| storage_err("ALERT_EXISTS", e))?;
        match row {
            Some(r) => match r.get_value(0).map_err(|e| storage_err("ALERT_EXISTS", e))? {
                Value::Integer(n) => Ok(n > 0),
                _ => Ok(false),
            },
            None => Ok(false),
        }
    }

    pub async fn mark_alerts_read(&self) -> Result<usize, TubeforgeError> {
        let n = self
            .conn
            .execute(
                "UPDATE alerts SET read_at = ?1 WHERE read_at IS NULL",
                params!(crate::util::now_rfc3339()),
            )
            .await
            .map_err(|e| storage_err("ALERT_READ", e))?;
        Ok(n as usize)
    }

    pub async fn clear_alerts(&self) -> Result<usize, TubeforgeError> {
        let n = self
            .conn
            .execute("DELETE FROM alerts", ())
            .await
            .map_err(|e| storage_err("ALERT_CLEAR", e))?;
        Ok(n as usize)
    }

    /// Channel ids marked as competitors (added_at ascending, then id).
    pub async fn list_competitors(&self) -> Result<Vec<String>, TubeforgeError> {
        let sql = "SELECT channel_id FROM competitors ORDER BY added_at, channel_id";
        query_rows_owned(&self.conn, sql, (), |v| match &v[0] {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        })
        .await
    }

    /// Most recent ingest_log row (LLD §8.4 health: last ingest).
    pub async fn last_ingest(&self) -> Result<Option<IngestLogRow>, TubeforgeError> {
        let sql = "SELECT batch_id, item, status, detail, at FROM ingest_log \
                   ORDER BY at DESC, rowid DESC LIMIT 1";
        query_row_owned(&self.conn, sql, (), ingest_log_from_values).await
    }

    // -----------------------------------------------------------------------
    // Batch writes (LLD §6.3): fetch-all, then ONE transaction, then one
    // writer. The whole batch rolls back on any failure.
    // -----------------------------------------------------------------------

    /// Begin the single batch write transaction (Immediate so the backup
    /// guard ordering can never race).
    pub async fn begin_batch(&mut self) -> Result<Batch<'_>, TubeforgeError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(|e| storage_err("BEGIN", e))?;
        Ok(Batch { tx })
    }
}

/// The write side of one ingest batch. Owns the engine transaction; commit
/// persists everything, drop without commit rolls back (engine default).
pub struct Batch<'a> {
    tx: Transaction<'a>,
}

impl Batch<'_> {
    pub async fn upsert_channel(&mut self, c: &ChannelRow) -> Result<(), TubeforgeError> {
        self.tx
            .execute(
                "INSERT INTO channels (channel_id, handle, title, description, avatar_url, \
                                       country, subscriber_count, video_count, source, etag, \
                                       fetched_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
                 ON CONFLICT(channel_id) DO UPDATE SET \
                   handle = excluded.handle, title = excluded.title, \
                   description = excluded.description, avatar_url = excluded.avatar_url, \
                   country = excluded.country, subscriber_count = excluded.subscriber_count, \
                   video_count = excluded.video_count, source = excluded.source, \
                   etag = excluded.etag, fetched_at = excluded.fetched_at, \
                   updated_at = excluded.updated_at",
                params!(
                    c.channel_id.as_str(),
                    c.handle.as_deref(),
                    c.title.as_str(),
                    c.description.as_deref(),
                    c.avatar_url.as_deref(),
                    c.country.as_deref(),
                    c.subscriber_count,
                    c.video_count,
                    c.source.as_str(),
                    c.etag.as_deref(),
                    c.fetched_at.as_str(),
                    c.updated_at.as_str()
                ),
            )
            .await
            .map_err(|e| storage_err("UPSERT_CHANNEL", e))?;
        Ok(())
    }

    pub async fn upsert_video(&mut self, v: &VideoRow) -> Result<(), TubeforgeError> {
        self.tx
            .execute(
                "INSERT INTO videos (video_id, channel_id, title, description, tags, \
                                     category_id, duration_sec, published_at, view_count, \
                                     like_count, comment_count, thumb_url, source, fetched_at, \
                                     updated_at, recording_date, recording_location_name, \
                                     recording_lat, recording_lng, topic_categories) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
                         ?16, ?17, ?18, ?19, ?20) \
                 ON CONFLICT(video_id) DO UPDATE SET \
                   channel_id = excluded.channel_id, title = excluded.title, \
                   description = excluded.description, tags = excluded.tags, \
                   category_id = excluded.category_id, duration_sec = excluded.duration_sec, \
                   published_at = excluded.published_at, view_count = excluded.view_count, \
                   like_count = excluded.like_count, comment_count = excluded.comment_count, \
                   thumb_url = excluded.thumb_url, source = excluded.source, \
                   fetched_at = excluded.fetched_at, updated_at = excluded.updated_at, \
                   recording_date = excluded.recording_date, \
                   recording_location_name = excluded.recording_location_name, \
                   recording_lat = excluded.recording_lat, \
                   recording_lng = excluded.recording_lng, \
                   topic_categories = excluded.topic_categories",
                params!(
                    v.video_id.as_str(),
                    v.channel_id.as_deref(),
                    v.title.as_str(),
                    v.description.as_str(),
                    v.tags.as_str(),
                    v.category_id.as_deref(),
                    v.duration_sec,
                    v.published_at.as_str(),
                    v.view_count,
                    v.like_count,
                    v.comment_count,
                    v.thumb_url.as_deref(),
                    v.source.as_str(),
                    v.fetched_at.as_str(),
                    v.updated_at.as_str(),
                    v.recording_date.as_deref(),
                    v.recording_location_name.as_deref(),
                    v.recording_lat,
                    v.recording_lng,
                    v.topic_categories.as_str()
                ),
            )
            .await
            .map_err(|e| storage_err("UPSERT_VIDEO", e))?;
        Ok(())
    }

    /// One `ingest_log` row (LLD §6.4): status ok | skipped | failed.
    pub async fn log_ingest(
        &mut self,
        batch_id: &str,
        item: &str,
        status: &str,
        detail: Option<&str>,
    ) -> Result<(), TubeforgeError> {
        self.tx
            .execute(
                "INSERT INTO ingest_log (batch_id, item, status, detail, at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params!(batch_id, item, status, detail, crate::util::now_rfc3339()),
            )
            .await
            .map_err(|e| storage_err("INGEST_LOG", e))?;
        Ok(())
    }

    pub async fn commit(self) -> Result<(), TubeforgeError> {
        self.tx
            .commit()
            .await
            .map_err(|e| storage_err("COMMIT", e))
    }
}

// ---------------------------------------------------------------------------
// Value mapping helpers
// ---------------------------------------------------------------------------

fn channel_from_values(v: &[Value]) -> ChannelRow {
    fn s(v: &Value) -> Option<String> {
        match v {
            Value::Text(t) => Some(t.clone()),
            Value::Null => None,
            _ => Some(format!("{v:?}")),
        }
    }
    fn i(v: &Value) -> Option<i64> {
        match v {
            Value::Integer(n) => Some(*n),
            _ => None,
        }
    }
    fn t(v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }
    ChannelRow {
        channel_id: t(&v[0]),
        handle: s(&v[1]),
        title: t(&v[2]),
        description: s(&v[3]),
        avatar_url: s(&v[4]),
        country: s(&v[5]),
        subscriber_count: i(&v[6]),
        video_count: i(&v[7]),
        source: t(&v[8]),
        etag: s(&v[9]),
        fetched_at: t(&v[10]),
        updated_at: t(&v[11]),
    }
}

fn video_from_values(v: &[Value]) -> VideoRow {
    fn s(v: &Value) -> Option<String> {
        match v {
            Value::Text(t) => Some(t.clone()),
            Value::Null => None,
            _ => Some(format!("{v:?}")),
        }
    }
    fn i(v: &Value) -> Option<i64> {
        match v {
            Value::Integer(n) => Some(*n),
            _ => None,
        }
    }
    fn f(v: &Value) -> Option<f64> {
        match v {
            Value::Real(n) => Some(*n),
            Value::Integer(n) => Some(*n as f64),
            _ => None,
        }
    }
    fn t(v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }
    VideoRow {
        video_id: t(&v[0]),
        channel_id: s(&v[1]),
        title: t(&v[2]),
        description: t(&v[3]),
        tags: t(&v[4]),
        category_id: s(&v[5]),
        duration_sec: i(&v[6]),
        published_at: t(&v[7]),
        view_count: i(&v[8]),
        like_count: i(&v[9]),
        comment_count: i(&v[10]),
        thumb_url: s(&v[11]),
        source: t(&v[12]),
        fetched_at: t(&v[13]),
        updated_at: t(&v[14]),
        recording_date: s(&v[15]),
        recording_location_name: s(&v[16]),
        recording_lat: f(&v[17]),
        recording_lng: f(&v[18]),
        topic_categories: t(&v[19]),
    }
}

fn score_from_values(v: &[Value]) -> ScoreRow {
    fn t(v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }
    fn f(v: &Value) -> f64 {
        match v {
            Value::Real(n) => *n,
            Value::Integer(n) => *n as f64,
            _ => 0.0,
        }
    }
    ScoreRow {
        video_id: t(&v[0]),
        seo_score: f(&v[1]),
        geo_score: f(&v[2]),
        total_score: f(&v[3]),
        components: t(&v[4]),
        computed_at: t(&v[5]),
    }
}

fn keyword_from_values(v: &[Value]) -> KeywordRow {
    fn s(v: &Value) -> Option<String> {
        match v {
            Value::Text(t) => Some(t.clone()),
            _ => None,
        }
    }
    fn t(v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }
    KeywordRow {
        keyword: t(&v[0]),
        niche: s(&v[1]),
        created_at: t(&v[2]),
    }
}

fn ranking_from_values(v: &[Value]) -> RankingRow {
    fn s(v: &Value) -> Option<String> {
        match v {
            Value::Text(t) => Some(t.clone()),
            _ => None,
        }
    }
    fn i(v: &Value) -> Option<i64> {
        match v {
            Value::Integer(n) => Some(*n),
            _ => None,
        }
    }
    fn t(v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }
    RankingRow {
        keyword: t(&v[0]),
        checked_at: t(&v[1]),
        video_id: s(&v[2]),
        position: i(&v[3]),
        topics: s(&v[4]),
    }
}

fn edge_from_values(v: &[Value]) -> EdgeRow {
    fn f(v: &Value) -> f64 {
        match v {
            Value::Real(n) => *n,
            Value::Integer(n) => *n as f64,
            _ => 0.0,
        }
    }
    fn t(v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }
    EdgeRow {
        from_channel: t(&v[0]),
        to_channel: t(&v[1]),
        weight: f(&v[2]),
        source: t(&v[3]),
    }
}

fn idea_from_values(v: &[Value]) -> IdeaRow {
    fn s(v: &Value) -> Option<String> {
        match v {
            Value::Text(t) => Some(t.clone()),
            _ => None,
        }
    }
    fn f(v: &Value) -> f64 {
        match v {
            Value::Real(n) => *n,
            Value::Integer(n) => *n as f64,
            _ => 0.0,
        }
    }
    fn t(v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }
    IdeaRow {
        idea_id: match &v[0] {
            Value::Integer(n) => *n,
            _ => 0,
        },
        title_suggestion: t(&v[1]),
        rationale: t(&v[2]),
        score: f(&v[3]),
        status: t(&v[4]),
        source_video: s(&v[5]),
        created_at: t(&v[6]),
    }
}

fn alert_from_values(v: &[Value]) -> AlertRow {
    fn s(v: &Value) -> Option<String> {
        match v {
            Value::Text(t) => Some(t.clone()),
            _ => None,
        }
    }
    fn t(v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }
    AlertRow {
        alert_id: match &v[0] {
            Value::Integer(n) => *n,
            _ => 0,
        },
        kind: t(&v[1]),
        channel_id: s(&v[2]),
        message: t(&v[3]),
        severity: t(&v[4]),
        created_at: t(&v[5]),
        read_at: s(&v[6]),
    }
}

fn ingest_log_from_values(v: &[Value]) -> IngestLogRow {
    fn s(v: &Value) -> Option<String> {
        match v {
            Value::Text(t) => Some(t.clone()),
            _ => None,
        }
    }
    fn t(v: &Value) -> String {
        match v {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }
    IngestLogRow {
        batch_id: t(&v[0]),
        item: t(&v[1]),
        status: t(&v[2]),
        detail: s(&v[3]),
        at: t(&v[4]),
    }
}

/// Run a statement returning 0..1 rows and map the first row (owned).
async fn query_row_owned<F, T>(
    conn: &turso::Connection,
    sql: &str,
    params: impl turso::IntoParams,
    map: F,
) -> Result<Option<T>, TubeforgeError>
where
    F: Fn(&[Value]) -> T,
{
    let mut stmt = conn
        .prepare(sql)
        .await
        .map_err(|e| storage_err("QUERY", e))?;
    let mut rows = stmt
        .query(params)
        .await
        .map_err(|e| storage_err("QUERY", e))?;
    let mut out = None;
    while let Some(row) = rows.next().await.map_err(|e| storage_err("QUERY", e))? {
        let mut vals = Vec::with_capacity(row.column_count());
        for idx in 0..row.column_count() {
            vals.push(row.get_value(idx).map_err(|e| storage_err("QUERY", e))?);
        }
        out = Some(map(&vals));
    }
    Ok(out)
}

/// Run a statement returning 0..N rows and map each (owned).
async fn query_rows_owned<F, T>(
    conn: &turso::Connection,
    sql: &str,
    params: impl turso::IntoParams,
    map: F,
) -> Result<Vec<T>, TubeforgeError>
where
    F: Fn(&[Value]) -> T,
{
    let mut stmt = conn
        .prepare(sql)
        .await
        .map_err(|e| storage_err("QUERY", e))?;
    let mut rows = stmt
        .query(params)
        .await
        .map_err(|e| storage_err("QUERY", e))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await.map_err(|e| storage_err("QUERY", e))? {
        let mut vals = Vec::with_capacity(row.column_count());
        for idx in 0..row.column_count() {
            vals.push(row.get_value(idx).map_err(|e| storage_err("QUERY", e))?);
        }
        out.push(map(&vals));
    }
    Ok(out)
}

/// Upsert a meta key on the given connection (used inside migrations where a
/// transaction borrows the connection).
async fn set_meta_key(
    conn: &turso::Connection,
    key: &str,
    value: &str,
) -> Result<(), TubeforgeError> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )
    .await
    .map_err(|e| storage_err("META_SET", e))?;
    Ok(())
}

/// Run a single-value-returning statement as a query and return the text of
/// the first column of the first row (drains remaining rows).
async fn query_text(conn: &turso::Connection, sql: &str) -> Result<String, TubeforgeError> {
    let mut stmt = conn
        .prepare(sql)
        .await
        .map_err(|e| storage_err("QUERY", e))?;
    let mut rows = stmt
        .query(())
        .await
        .map_err(|e| storage_err("QUERY", e))?;
    let mut first = None;
    while let Some(row) = rows.next().await.map_err(|e| storage_err("QUERY", e))? {
        if first.is_none() {
            match row.get_value(0).map_err(|e| storage_err("QUERY", e))? {
                Value::Text(t) => first = Some(t),
                Value::Integer(i) => first = Some(i.to_string()),
                Value::Real(r) => first = Some(r.to_string()),
                Value::Null => first = Some("null".to_string()),
                Value::Blob(_) => first = Some("<blob>".to_string()),
            }
        }
    }
    first.ok_or_else(|| TubeforgeError::Storage {
        code: "QUERY".to_string(),
        message: format!("statement returned no rows: {sql}"),
    })
}

/// Like `query_text` but for integer-valued statements (PRAGMA user_version).
async fn query_i64(conn: &turso::Connection, sql: &str) -> Result<i64, TubeforgeError> {
    let mut stmt = conn
        .prepare(sql)
        .await
        .map_err(|e| storage_err("QUERY", e))?;
    let mut rows = stmt
        .query(())
        .await
        .map_err(|e| storage_err("QUERY", e))?;
    let mut first = None;
    while let Some(row) = rows.next().await.map_err(|e| storage_err("QUERY", e))? {
        if first.is_none() {
            if let Value::Integer(n) = row.get_value(0).map_err(|e| storage_err("QUERY", e))? {
                first = Some(n);
            }
        }
    }
    first.ok_or_else(|| TubeforgeError::Storage {
        code: "QUERY".to_string(),
        message: format!("statement returned no integer row: {sql}"),
    })
}
