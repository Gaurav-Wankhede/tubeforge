//! Analytics modules (LLD §8): competitor graph + PageRank, Next Ideas,
//! keyword rank tracking, and reports (scorecard / health / alerts).
//!
//! Dependency rule: analytics goes through the `storage`/`search` APIs only —
//! never turso or tantivy directly.

pub mod actions;
pub mod audit;
pub mod bandit;
pub mod content;
pub mod forecast;
pub mod gaps;
pub mod graph;
pub mod graph_aware;
pub mod graph_viz;
pub mod growth;
pub mod history_tracker;
pub mod ideas;
pub mod keywords;
pub mod kg;
pub mod kg_algorithms;
pub mod kg_builder;
pub mod kg_retriever;
pub mod performance;
pub mod query;
pub mod reports;
pub mod research;
pub mod tags;
pub mod topic_generator;
