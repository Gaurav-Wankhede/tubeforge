//! Embedded SQL schema + migrations (LLD §3.1, §9.3).
//!
//! Migration 001 is the complete v1 schema (all domain tables + indexes).
//! Every statement is idempotent (`IF NOT EXISTS`) and the migration is
//! marked `idempotent: true`, so databases created by the Phase 0 skeleton
//! (which only had the `meta` table) upgrade in place on open without a
//! version bump. Migration 002 (v1 → v2) adds the C1/C2 GEO columns:
//! `recordingDetails` (recording date/location) and `topicDetails`
//! (topic category URLs) on `videos`, plus the derived topic-label snapshot
//! column `topics` on `keyword_rankings`. Migration 003 (v2 → v3) adds the
//! nullable `privacy_status` column on `videos`, written by `check
//! availability` (Phase 3 workstream B). Both later migrations are
//! version-gated (applied once per database — the gate IS the idempotency
//! guard, since this engine's `ALTER TABLE ... ADD COLUMN` has no
//! `IF NOT EXISTS` form).

/// Current schema version (mirrors `PRAGMA user_version`).
pub const SCHEMA_VERSION: i64 = 3;

/// meta keys used by the ledger (LLD §3.1 comment block).
pub const META_KEYS: [&str; 6] = [
    "schema_version",
    "quota_videos_list_used",
    "quota_videos_list_date",
    "last_backup_at",
    "last_reindex_at",
    "settings_json",
];

/// One ordered migration step (LLD §9.3).
pub struct Migration {
    pub version: i64,
    /// Idempotent migrations are applied even when the recorded version
    /// already covers them; used for migration 001 so Phase 0 databases
    /// (meta-only) gain the full schema in place. Future migrations must
    /// leave this `false` and write non-idempotent, version-gated SQL.
    pub idempotent: bool,
    pub sql: &'static str,
}

/// Ordered migration list. `init` and every DB open apply pending migrations.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        idempotent: true,
        sql: SCHEMA_SQL,
    },
    Migration {
        version: 2,
        idempotent: false,
        sql: MIGRATION_002_SQL,
    },
    Migration {
        version: 3,
        idempotent: false,
        sql: MIGRATION_003_SQL,
    },
];

/// Migration 003: `videos.privacy_status` (nullable TEXT) — the privacy
/// snapshot (`public`/`unlisted`/`private`) recorded by `check availability`
/// for videos that still exist. Applied exactly once per database
/// (version-gated by the runner).
pub const MIGRATION_003_SQL: &str = r#"
ALTER TABLE videos ADD COLUMN privacy_status TEXT;
"#;

/// Migration 002: C1 `recordingDetails` columns + C2 `topic_categories`
/// column on `videos`, and the `topics` snapshot column (derived topic
/// labels, JSON array) on `keyword_rankings`. All new columns are nullable.
/// Applied exactly once per database (version-gated by the runner).
pub const MIGRATION_002_SQL: &str = r#"
ALTER TABLE videos ADD COLUMN recording_date TEXT;
ALTER TABLE videos ADD COLUMN recording_location_name TEXT;
ALTER TABLE videos ADD COLUMN recording_lat REAL;
ALTER TABLE videos ADD COLUMN recording_lng REAL;
ALTER TABLE videos ADD COLUMN topic_categories TEXT;
ALTER TABLE keyword_rankings ADD COLUMN topics TEXT;
"#;

/// Full v1 schema (LLD §3.1). Statement text deliberately free of comments —
/// the turso_parser in 0.7.2 is happier with plain statements.
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS channels (
  channel_id        TEXT PRIMARY KEY,
  handle            TEXT UNIQUE,
  title             TEXT NOT NULL,
  description       TEXT,
  avatar_url        TEXT,
  country           TEXT,
  subscriber_count  INTEGER,
  video_count       INTEGER,
  source            TEXT NOT NULL DEFAULT 'rss',
  etag              TEXT,
  fetched_at        TEXT NOT NULL,
  updated_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS videos (
  video_id      TEXT PRIMARY KEY,
  channel_id    TEXT REFERENCES channels(channel_id) ON DELETE SET NULL,
  title         TEXT NOT NULL,
  description   TEXT NOT NULL DEFAULT '',
  tags          TEXT NOT NULL DEFAULT '[]',
  category_id   TEXT,
  duration_sec  INTEGER,
  published_at  TEXT NOT NULL,
  view_count    INTEGER,
  like_count    INTEGER,
  comment_count INTEGER,
  thumb_url     TEXT,
  embedding     BLOB,
  source        TEXT NOT NULL DEFAULT 'rss',
  fetched_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_videos_channel      ON videos(channel_id);
CREATE INDEX IF NOT EXISTS idx_videos_published    ON videos(published_at DESC);
CREATE INDEX IF NOT EXISTS idx_videos_channel_pub  ON videos(channel_id, published_at DESC);

CREATE TABLE IF NOT EXISTS competitors (
  channel_id  TEXT PRIMARY KEY REFERENCES channels(channel_id) ON DELETE CASCADE,
  label       TEXT,
  added_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS keywords (
  keyword     TEXT PRIMARY KEY,
  niche       TEXT,
  created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS keyword_rankings (
  keyword     TEXT NOT NULL REFERENCES keywords(keyword) ON DELETE CASCADE,
  checked_at  TEXT NOT NULL,
  video_id    TEXT REFERENCES videos(video_id) ON DELETE SET NULL,
  position    INTEGER,
  PRIMARY KEY (keyword, checked_at)
);

CREATE TABLE IF NOT EXISTS scores (
  video_id     TEXT PRIMARY KEY REFERENCES videos(video_id) ON DELETE CASCADE,
  seo_score    REAL NOT NULL,
  geo_score    REAL NOT NULL,
  total_score  REAL NOT NULL,
  components   TEXT NOT NULL,
  computed_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_scores_total ON scores(total_score DESC);

CREATE TABLE IF NOT EXISTS ideas (
  idea_id        INTEGER PRIMARY KEY AUTOINCREMENT,
  title_suggestion TEXT NOT NULL,
  rationale      TEXT NOT NULL,
  score          REAL NOT NULL,
  status         TEXT NOT NULL DEFAULT 'draft',
  source_video   TEXT REFERENCES videos(video_id) ON DELETE SET NULL,
  created_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS edges (
  from_channel TEXT NOT NULL REFERENCES channels(channel_id) ON DELETE CASCADE,
  to_channel   TEXT NOT NULL REFERENCES channels(channel_id) ON DELETE CASCADE,
  weight       REAL NOT NULL DEFAULT 1.0,
  source       TEXT NOT NULL,
  PRIMARY KEY (from_channel, to_channel)
);

CREATE TABLE IF NOT EXISTS alerts (
  alert_id   INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,
  channel_id TEXT REFERENCES channels(channel_id) ON DELETE CASCADE,
  message    TEXT NOT NULL,
  severity   TEXT NOT NULL DEFAULT 'info',
  created_at TEXT NOT NULL,
  read_at    TEXT
);

CREATE TABLE IF NOT EXISTS ingest_log (
  batch_id   TEXT NOT NULL,
  item       TEXT NOT NULL,
  status     TEXT NOT NULL,
  detail     TEXT,
  at         TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ingest_log_batch ON ingest_log(batch_id);

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;
