//! BM25 query construction + score retrieval (LLD §3.2 query surface).
//!
//! Implemented on the from-scratch inverted index (`store::Store`), replacing
//! tantivy. `corpus_resonance` answers "how well does this text rank against
//! the corpus in field X" — the raw BM25 maximum over matching docs, excluding
//! the video itself when scoring a stored video. `matches` returns every
//! matching (video_id, score) pair best-first.

use std::sync::{Arc, RwLock};

use super::index::Index;
use super::store::{field_from_name, FieldName, Store};
use crate::error::{index_err, TubeforgeError};

/// BM25 access over a single index (cheap to open; `reload` re-reads the
/// snapshot between ingest batches).
pub struct Bm25 {
    store: Arc<RwLock<Store>>,
    path: Arc<std::path::Path>,
}

/// Reasonable upper bound for corpus scans: the index tops out at ~10k docs
/// in v1 (HLD §10), so a linear scan + max is fine.
const COLLECT_LIMIT: usize = 10_000;

impl Bm25 {
    pub fn open(index: Index) -> Result<Self, TubeforgeError> {
        Ok(Bm25 {
            store: index.store_handle(),
            path: index.path_handle(),
        })
    }

    /// Re-read the snapshot from disk so a just-committed ingest batch is
    /// visible (mirrors tantivy's `reader.reload()`).
    pub fn reload(&mut self) -> Result<(), TubeforgeError> {
        let loaded = Store::load(&self.path.join(super::store::Store::file_name()))?;
        let mut s = self
            .store
            .write()
            .map_err(|e| index_err(format!("index lock poisoned: {e}")))?;
        *s = loaded;
        Ok(())
    }

    /// Raw BM25 of `query` over `field_name`, excluding `exclude_video_id`
    /// (its own doc) when given. 0 when the corpus is empty or the query
    /// cannot be parsed.
    pub fn corpus_resonance(
        &self,
        field_name: &str,
        query: &str,
        exclude_video_id: Option<&str>,
    ) -> f64 {
        let Some(field) = field_from_name(field_name) else {
            return 0.0;
        };
        let s = self.store.read().unwrap_or_else(|p| p.into_inner());
        let top = s.matches(field, query);
        // Respect the collection cap: only scan the top COLLECT_LIMIT hits.
        let mut best = 0.0f32;
        for (id, score) in top.into_iter().take(COLLECT_LIMIT) {
            if let Some(exclude) = exclude_video_id {
                if id == exclude {
                    continue;
                }
            }
            best = best.max(score);
        }
        best as f64
    }

    /// All docs matching `query` in `field_name` with their BM25 scores.
    pub fn matches(&self, field_name: &str, query: &str) -> Vec<(String, f32)> {
        let Some(field) = field_from_name(field_name) else {
            return Vec::new();
        };
        let s = self.store.read().unwrap_or_else(|p| p.into_inner());
        s.matches(field, query)
            .into_iter()
            .take(COLLECT_LIMIT)
            .collect()
    }

    pub fn num_docs(&self) -> u64 {
        let s = self.store.read().unwrap_or_else(|p| p.into_inner());
        s.num_docs()
    }
}

/// Resolve a field name for tests/external use.
pub fn resolve_field(name: &str) -> Option<FieldName> {
    field_from_name(name)
}

/// Parse a query into its token set (public for tests).
pub fn query_terms(query: &str) -> Vec<String> {
    super::store::tokenize(query)
}
