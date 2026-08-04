//! One file per CLI subcommand (LLD §2 commands/).
//! Phase 1 scope: init, ingest, refresh, score, reindex, backup, quota, mcp.
//! Phase 2 additions: ideas, keywords, scorecard, health, alerts.
//! Phase 3 additions: thumbnail render, thumbnail list-templates.
//! Phase 3 workstream B: check availability, export, filmot get.

pub mod alerts;
pub mod availability;
pub mod backup;
pub mod export;
pub mod filmot;
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
pub mod serve;
pub mod thumbnail;
