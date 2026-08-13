//! Storage repository facade.
//!
//! The `Db` and every row type live in `db_tf`, the from-scratch tfdb-backed
//! implementation. This module simply re-exports them so all existing callers
//! (`use crate::storage::db::{Db, ChannelRow, ...}`) keep compiling unchanged.
//!
//! The legacy turso-backed SQL implementation was removed wholesale: tfdb is
//! the single storage engine. All methods on `Db` are `async` (the engine is
//! synchronous; the `async` signature preserves historical `.await` call sites).

pub use super::db_tf::*;
