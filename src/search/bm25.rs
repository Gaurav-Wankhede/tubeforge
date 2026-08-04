//! BM25 query construction + score retrieval (LLD §3.2 query surface).
//!
//! Phase 1 basic mode: `corpus_resonance` answers "how well does this text
//! (title) rank against the corpus in field X" — the raw BM25 maximum over
//! matching docs, excluding the video itself when scoring a stored video.
//! The full weighted keyword engine arrives in Phase 2.

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::Field;
use tantivy::{Index, IndexReader, Searcher};

use crate::error::TubeforgeError;

/// BM25 access over a single tantivy index (cheap to open; Reader reloads
/// between ingest batches).
pub struct Bm25 {
    index: Index,
    reader: IndexReader,
}

/// Reasonable upper bound for corpus scans: the index tops out at ~10k docs
/// in v1 (HLD §10), so collecting all hits and taking the max is fine.
const COLLECT_LIMIT: usize = 10_000;

impl Bm25 {
    pub fn open(index: Index) -> Result<Self, TubeforgeError> {
        let reader = index.reader().map_err(index_err)?;
        Ok(Bm25 { index, reader })
    }

    /// Refresh the reader after an ingest batch committed new segments.
    pub fn reload(&mut self) -> Result<(), TubeforgeError> {
        self.reader.reload().map_err(index_err)
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
        if query.trim().is_empty() {
            return 0.0;
        }
        let schema = self.index.schema();
        let field = match schema.get_field(field_name) {
            Ok(f) => f,
            Err(_) => return 0.0,
        };
        let parsed = match QueryParser::for_index(&self.index, vec![field]).parse_query(query) {
            Ok(q) => q,
            Err(_) => return 0.0,
        };

        let searcher = self.reader.searcher();
        let hits = match searcher.search(
            &parsed,
            &TopDocs::with_limit(COLLECT_LIMIT).order_by_score(),
        ) {
            Ok(h) => h,
            Err(_) => return 0.0,
        };

        let video_id_field = schema.get_field(crate::search::index::FIELD_VIDEO_ID).ok();
        let mut best = 0.0f32;
        for (score, addr) in hits {
            if let Some(vf) = video_id_field {
                if let Some(exclude) = exclude_video_id {
                    if doc_video_id(&searcher, vf, addr).as_deref() == Some(exclude) {
                        continue;
                    }
                }
            }
            best = best.max(score);
        }
        best as f64
    }

    /// All docs matching `query` in `field_name` with their BM25 scores.
    pub fn matches(&self, field_name: &str, query: &str) -> Vec<(String, f32)> {
        let schema = self.index.schema();
        let field = match schema.get_field(field_name) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let parsed = match QueryParser::for_index(&self.index, vec![field]).parse_query(query) {
            Ok(q) => q,
            Err(_) => return Vec::new(),
        };
        let searcher = self.reader.searcher();
        let Ok(hits) = searcher.search(
            &parsed,
            &TopDocs::with_limit(COLLECT_LIMIT).order_by_score(),
        ) else {
            return Vec::new();
        };
        let video_id_field = schema.get_field(crate::search::index::FIELD_VIDEO_ID).ok();
        hits.into_iter()
            .map(|(score, addr)| {
                let id = video_id_field
                    .and_then(|vf| doc_video_id(&searcher, vf, addr))
                    .unwrap_or_default();
                (id, score)
            })
            .collect()
    }

    pub fn num_docs(&self) -> u64 {
        self.reader.searcher().num_docs()
    }
}

fn doc_video_id(searcher: &Searcher, field: Field, addr: tantivy::DocAddress) -> Option<String> {
    use tantivy::schema::document::Value;
    let doc: tantivy::TantivyDocument = searcher.doc(addr).ok()?;
    doc.get_first(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn index_err(e: tantivy::TantivyError) -> TubeforgeError {
    TubeforgeError::Index { detail: e.to_string() }
}
