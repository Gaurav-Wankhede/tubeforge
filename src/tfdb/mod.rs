//! TubeForge Database — a from-scratch embedded storage engine.
//!
//! A single-file, zero-config, durable, crash-safe store built in pure Rust.
//! It is not a general-purpose SQL engine; it is specialized for the
//! TubeForge data model and reusable across projects:
//!
//! - **Durability**: an append-only Write-Ahead Log (WAL) + periodic atomic
//!   checkpoint. Every transaction is fsynced on commit; a crash replays the
//!   WAL and rolls forward, so committed data is never lost and partial
//!   transactions never appear (ADR-style crash-safety).
//! - **Tables**: a key/value + typed-row model with a fixed set of columns
//!   per table (schema described in `schema.rs`).
//! - **Property graph**: typed nodes + typed, weighted edges (`graph`).
//! - **HNSW**: hierarchical navigable small-world vector index for ANN
//!   nearest-neighbour search over embeddings (`hnsw`).
//! - **BM25**: full-text scoring lives in `crate::search` (reuses the same
//!   tokenizer); the DB stores the raw fields the index is rebuilt from.
//!
//! File layout on disk:
//! - `<path>.wal`  — append-only, checksummed transaction log.
//! - `<path>.dat`  — the latest atomic checkpoint snapshot.
//!
//! `Engine` owns the current in-memory state and both files. A single write
//! lock serializes writers; reads are served from the in-memory snapshot.

pub mod graph;
pub mod hnsw;
pub mod schema;
pub mod store;

pub use schema::{Col, ColType, TableSchema};
pub use store::{Engine, EngineOptions, Tx, Value};
