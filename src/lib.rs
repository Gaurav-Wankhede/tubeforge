//! TubeForge — local-first YouTube SEO/GEO growth engine.
//!
//! Phase 2: full SEO/GEO scoring engine (weights config), analytics modules
//! (competitor graph + PageRank, Next Ideas, keyword rank tracking, scorecard
//! / health / alerts). Module layout per LLD §2.
//! Dependency rule: `storage` is the only module importing `turso`;
//! `search` is the only module importing `tantivy`.

pub mod analytics;
pub mod categories;
pub mod cli;
pub mod commands;
pub mod config;
pub mod error;
pub mod export;
pub mod fetch;
pub mod ingest;
pub mod output;
pub mod playbook;
pub mod scoring;
pub mod search;
pub mod serve;
pub mod storage;
pub mod tfdb;
pub mod util;

// Re-export the flexible query API for convenience
pub use analytics::query;
