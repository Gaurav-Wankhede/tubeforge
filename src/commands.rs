//! One file per CLI subcommand (LLD §2 commands/).
//! Phase 1 scope: init, ingest, refresh, score, reindex, backup, quota.
//! Phase 2 additions: ideas, keywords, scorecard, health, alerts.
//! Phase 3 additions: thumbnail render, thumbnail list-templates.
//! Phase 3 workstream B: check availability, export, filmot get.
//!
//! TubeForge is a **direct CLI** for agents — there is no MCP server. Agents
//! invoke `tubeforge <cmd> --json` themselves (JSON envelope + exit codes).

pub mod alerts;
pub mod analyze;
pub mod availability;
pub mod backup;
pub mod comments;
pub mod export;
pub mod filmot;
pub mod forecast;
pub mod gaps;
pub mod greedy;
pub mod health;
pub mod ideas;
pub mod ingest;
pub mod init;
pub mod keywords;
pub mod metadata;
pub mod prompt;
pub mod quota;
pub mod refresh;
pub mod reindex;
pub mod rpc;
pub mod score;
pub mod scorecard;
pub mod serve;
pub mod tags;
pub mod thumbnail;
pub mod transcript;
pub mod videos;
