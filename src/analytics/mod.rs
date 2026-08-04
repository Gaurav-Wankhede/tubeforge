//! Analytics modules (LLD §8): competitor graph + PageRank, Next Ideas,
//! keyword rank tracking, and reports (scorecard / health / alerts).
//!
//! Dependency rule: analytics goes through the `storage`/`search` APIs only —
//! never turso or tantivy directly.

pub mod graph;
pub mod ideas;
pub mod keywords;
pub mod reports;
