//! Custom full-text index lifecycle (LLD §3.2) — replaces tantivy.
//!
//! Location `<data>/index/`; rebuildable via `rebuild` (`reindex` command) —
//! the index is never part of backups. The index is an in-memory inverted
//! index (`store::Store`) with an atomic single-file JSON snapshot
//! (`index.json`) for durability across process restarts.
//!
//! The public surface mirrors the old tantivy call sites so ingest.rs,
//! commands, serve, and tests need only the minimal edits described in the
//! store module: `Index::schema()`, `Index::writer()`, `index::upsert`,
//! `Writer::commit()`, `new_index`, `open_or_create`, `rebuild`.

use std::path::Path;
use std::sync::{Arc, RwLock};

use super::store::{field_from_name, Store};
use crate::error::{index_err, TubeforgeError};

/// Field names mirror LLD §3.2.
pub const FIELD_VIDEO_ID: &str = "video_id";
pub const FIELD_CHANNEL_ID: &str = "channel_id";
pub const FIELD_TITLE: &str = "title";
pub const FIELD_DESCRIPTION: &str = "description";
pub const FIELD_TAGS: &str = "tags";
pub const FIELD_PUBLISHED_AT: &str = "published_at";

/// A document ready for the index (built from a `videos` row).
#[derive(Debug, Clone, Default)]
pub struct VideoDoc {
    pub video_id: String,
    pub channel_id: Option<String>,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    /// Seconds since epoch; None when the source had no date (oEmbed).
    pub published_at: Option<i64>,
}

impl VideoDoc {
    fn to_indexed(&self) -> super::store::IndexedDoc {
        super::store::IndexedDoc {
            video_id: self.video_id.clone(),
            channel_id: self.channel_id.clone(),
            title: self.title.clone(),
            description: self.description.clone(),
            tags: self.tags.clone(),
        }
    }
}

/// Schema mirror: resolves field names to a `FieldName` for query building.
/// Call sites historically held a `Schema` and passed it to `upsert`; we keep
/// the shape (a cheap, clonable handle) to avoid churn.
#[derive(Debug, Clone, Default)]
pub struct Schema;

impl Schema {
    pub fn get_field(&self, name: &str) -> Result<super::store::FieldName, TubeforgeError> {
        field_from_name(name).ok_or_else(|| {
            index_err(format!("unknown field {name:?} (expected title/description/tags)"))
        })
    }
}

/// The full-text index: a shared in-memory store + its snapshot path.
#[derive(Clone)]
pub struct Index {
    store: Arc<RwLock<Store>>,
    path: Arc<Path>,
}

impl Index {
    /// Shared handle to the in-memory store (used by `Bm25`).
    pub(crate) fn store_handle(&self) -> Arc<RwLock<Store>> {
        Arc::clone(&self.store)
    }

    /// Snapshot directory (used by `Bm25::reload`).
    pub(crate) fn path_handle(&self) -> Arc<Path> {
        Arc::clone(&self.path)
    }

    pub fn schema(&self) -> Schema {
        Schema
    }

    /// Open a writer over this index. Multiple writers share the same store;
    /// `commit` persists it atomically.
    pub fn writer(&self, _memory_budget: usize) -> IndexWriter {
        IndexWriter {
            store: Arc::clone(&self.store),
            path: Arc::clone(&self.path),
        }
    }

    pub fn num_docs(&self) -> u64 {
        self.store.read().unwrap_or_else(|p| p.into_inner()).num_docs()
    }
}

/// An index writer: accumulates upserts in memory, persists on `commit`.
pub struct IndexWriter {
    store: Arc<RwLock<Store>>,
    path: Arc<Path>,
}

impl IndexWriter {
    pub fn delete_term(&mut self, video_id: &str) {
        if let Ok(mut s) = self.store.write() {
            s.remove(video_id);
        }
    }

    pub fn add_document(&mut self, doc: VideoDoc) -> Result<(), TubeforgeError> {
        let mut s = self
            .store
            .write()
            .map_err(|e| index_err(format!("index lock poisoned: {e}")))?;
        s.upsert(doc.to_indexed());
        Ok(())
    }

    /// Persist the in-memory state to the snapshot file atomically.
    pub fn commit(self) -> Result<(), TubeforgeError> {
        let s = self
            .store
            .read()
            .map_err(|e| index_err(format!("index lock poisoned: {e}")))?;
        s.persist(&self.path.join(super::store::Store::file_name()))?;
        Ok(())
    }
}

/// Add (or replace) the doc for `video_id` and persist. Kept for callers that
/// want a one-shot upsert without managing a writer (mirrors old `upsert`).
pub fn upsert(writer: &mut IndexWriter, _fields: &Schema, doc: &VideoDoc) -> Result<(), TubeforgeError> {
    writer.add_document(doc.clone())
}

/// Build a fresh index at `dir` (overwrites nothing; errors if the dir
/// already holds an index).
pub fn new_index(dir: &Path) -> Result<Index, TubeforgeError> {
    if dir.join(super::store::Store::file_name()).exists() {
        return Err(index_err(format!(
            "index already exists at {} — remove it or open instead",
            dir.display()
        )));
    }
    let store = Store::new();
    store.persist(&dir.join(super::store::Store::file_name()))?;
    Ok(Index {
        store: Arc::new(RwLock::new(store)),
        path: Arc::from(dir),
    })
}

/// Open an existing index or create a fresh one at `dir`.
pub fn open_or_create(dir: &Path) -> Result<Index, TubeforgeError> {
    let store = Store::load(&dir.join(super::store::Store::file_name()))?;
    Ok(Index {
        store: Arc::new(RwLock::new(store)),
        path: Arc::from(dir),
    })
}

/// Full rebuild from the `videos` table contents: write a fresh snapshot from
/// `docs`. Idempotent (LLD §3.2 recovery path). Returns the doc count.
pub fn rebuild(dir: &Path, docs: &[VideoDoc]) -> Result<usize, TubeforgeError> {
    std::fs::create_dir_all(dir).map_err(|e| index_err(format!(
        "create index dir {}: {e}",
        dir.display()
    )))?;
    let store = super::store::write_store(dir, &docs.iter().map(VideoDoc::to_indexed).collect::<Vec<_>>())?;
    let n = store.num_docs();
    std::fs::remove_file(dir.join("meta.json")).ok();
    Ok(n as usize)
}
