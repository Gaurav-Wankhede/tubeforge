//! One file per CLI subcommand (LLD §2 commands/).
//! Phase 1 scope: init, ingest, refresh, score, reindex, backup, quota, mcp.
//! Phase 2 additions: ideas, keywords, scorecard, health, alerts.

pub mod alerts;
pub mod backup;
pub mod health;
pub mod ideas;
pub mod ingest;
pub mod init;
pub mod keywords;
pub mod mcp;
pub mod quota;
pub mod refresh;
pub mod reindex;
pub mod score;
pub mod scorecard;
