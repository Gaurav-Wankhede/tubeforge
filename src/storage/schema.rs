//! Schema version + meta-key ledger for the tfdb-backed storage layer.
//!
//! The tfdb engine (crate::tfdb) defines the full 22-table schema itself
//! (`tfdb_schema::all`), so there are no embedded SQL migrations. `SCHEMA_VERSION`
//! is recorded in the `meta` table on every open and reported by
//! `Db::user_version` / health for API parity with the legacy turso layer.

/// Current schema version (mirrors the recorded `meta.schema_version`).
pub const SCHEMA_VERSION: i64 = 9;

/// meta keys used by the ledger (LLD §3.1 comment block).
pub const META_KEYS: [&str; 6] = [
    "schema_version",
    "quota_videos_list_used",
    "quota_videos_list_date",
    "last_backup_at",
    "last_reindex_at",
    "settings_json",
];
