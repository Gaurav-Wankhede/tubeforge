//! Storage layer (LLD §2).
//!
//! **Dependency rule:** the live `Db` is backed by the from-scratch tfdb
//! engine (`crate::tfdb`), re-exported through `db`. The legacy turso SQL
//! dependency was removed entirely; tfdb is WAL + CRC + checkpoint (ADR-5).

pub mod backup;
pub mod db;
pub mod db_tf;
pub mod schema;

pub use db::{Db, KeywordResearchRow};
