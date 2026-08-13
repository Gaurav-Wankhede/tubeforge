//! Storage layer (LLD §2).
//!
//! **Dependency rule:** this is the ONLY module that imports `turso`.
//! Engine: Turso Database, WAL journal mode ONLY (never MVCC, ADR-5).

pub mod backup;
pub mod db;
pub mod db_tf;
pub mod schema;

pub use db::{Db, KeywordResearchRow};
