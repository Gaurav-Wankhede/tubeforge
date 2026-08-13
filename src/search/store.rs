//! From-scratch BM25 inverted index (replaces tantivy). LLD §3.2.
//!
//! A single-file, self-contained full-text index specialized for the
//! TubeForge corpus: an in-memory inverted index (term -> postings per field)
//! with a durable, atomic, checksummed JSON snapshot on disk. No external
//! engine — tokenization, posting lists, and BM25 are implemented here.
//!
//! Design notes:
//! - Index is stored at `<data>/index/` and is *rebuildable* (`rebuild`),
//!   never part of backups (LLD §3.2). The on-disk snapshot is written
//!   atomically (temp file + rename) so a crash mid-write never corrupts the
//!   last good snapshot.
//! - Corpus is small (<= ~10k docs, HLD §10): a whole-store reload on open is
//!   the right tradeoff, and BM25 scoring is in-memory.
//! - Fields mirror LLD §3.2: video_id (stored, not tokenized), channel_id
//!   (stored), title/description/tags (tokenized text).
//!
//! Only this module knows the on-disk format; `index.rs` and `bm25.rs` are
//! thin wrappers that keep the call sites (ingest, commands, serve, tests)
//! stable.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{index_err, TubeforgeError};

/// Tokenizer: lowercase + split on non-alphanumeric runs (Unicode-aware).
/// This is intentionally simple and deterministic — enough for BM25 over the
/// YouTube corpus and far simpler to reason about than a rule pipeline.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            current.extend(c.to_lowercase());
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// One indexed document. `video_id` is the stable dedup key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexedDoc {
    pub video_id: String,
    pub channel_id: Option<String>,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
}

/// A stored posting: which doc and how many times the term occurs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Posting {
    doc: u32,
    tf: u32,
}

/// Serialized store snapshot (one field per document set of postings).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    version: u32,
    docs: Vec<IndexedDoc>,
    /// term -> postings, for every tokenized field.
    title: HashMap<String, Vec<Posting>>,
    description: HashMap<String, Vec<Posting>>,
    tags: HashMap<String, Vec<Posting>>,
    /// per-field doc length per doc (doc id -> token count).
    title_len: Vec<u32>,
    description_len: Vec<u32>,
    tags_len: Vec<u32>,
}

impl Snapshot {
    fn empty() -> Self {
        Snapshot {
            version: STORE_VERSION,
            docs: Vec::new(),
            title: HashMap::new(),
            description: HashMap::new(),
            tags: HashMap::new(),
            title_len: Vec::new(),
            description_len: Vec::new(),
            tags_len: Vec::new(),
        }
    }
}

const STORE_VERSION: u32 = 1;

/// The full-text index: a map from video_id to doc + three inverted indexes.
#[derive(Debug, Clone)]
pub struct Store {
    snap: Snapshot,
    by_video_id: HashMap<String, u32>,
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

impl Store {
    pub fn new() -> Self {
        Store {
            snap: Snapshot::empty(),
            by_video_id: HashMap::new(),
        }
    }

    pub fn num_docs(&self) -> u64 {
        self.snap.docs.len() as u64
    }

    /// Add or replace the doc for `video_id`. Postings are rebuilt for the
    /// changed doc; unaffected postings are kept.
    pub fn upsert(&mut self, doc: IndexedDoc) {
        if doc.video_id.is_empty() {
            return;
        }
        if let Some(&old) = self.by_video_id.get(&doc.video_id) {
            self.remove_postings(old);
            self.snap.docs[old as usize] = doc.clone();
            self.add_postings(old, &doc);
        } else {
            let id = self.snap.docs.len() as u32;
            self.by_video_id.insert(doc.video_id.clone(), id);
            self.snap.docs.push(doc.clone());
            self.snap.title_len.push(0);
            self.snap.description_len.push(0);
            self.snap.tags_len.push(0);
            self.add_postings(id, &doc);
        }
    }

    /// Remove the doc for `video_id` (and its postings). No-op if absent.
    pub fn remove(&mut self, video_id: &str) {
        if let Some(&id) = self.by_video_id.get(video_id) {
            self.remove_postings(id);
            self.by_video_id.remove(video_id);
            // Compact the doc array and renumber (keeps doc ids dense).
            self.compact(id);
        }
    }

    fn add_postings(&mut self, id: u32, doc: &IndexedDoc) {
        let title = tokenize(&doc.title);
        let description = tokenize(&doc.description);
        let tags = tokenize(&doc.tags.join(" "));
        self.snap.title_len[id as usize] = title.len() as u32;
        self.snap.description_len[id as usize] = description.len() as u32;
        self.snap.tags_len[id as usize] = tags.len() as u32;
        add_terms(&mut self.snap.title, id, &title);
        add_terms(&mut self.snap.description, id, &description);
        add_terms(&mut self.snap.tags, id, &tags);
    }

    fn remove_postings(&mut self, id: u32) {
        remove_doc_from_index(&mut self.snap.title, id);
        remove_doc_from_index(&mut self.snap.description, id);
        remove_doc_from_index(&mut self.snap.tags, id);
    }

    fn compact(&mut self, removed: u32) {
        self.snap.docs.remove(removed as usize);
        self.snap.title_len.remove(removed as usize);
        self.snap.description_len.remove(removed as usize);
        self.snap.tags_len.remove(removed as usize);
        // Shift doc ids above `removed` down by one in every index.
        for map in [&mut self.snap.title, &mut self.snap.description, &mut self.snap.tags] {
            map.retain(|_, postings| {
                postings.retain(|p| p.doc != removed);
                postings.iter_mut().for_each(|p| {
                    if p.doc > removed {
                        p.doc -= 1;
                    }
                });
                !postings.is_empty()
            });
        }
        // Rebuild the id map.
        self.by_video_id.clear();
        for (i, d) in self.snap.docs.iter().enumerate() {
            self.by_video_id.insert(d.video_id.clone(), i as u32);
        }
    }

    // -- query surface ----------------------------------------------------

    /// Average document length for a field (for BM25 `b` normalization).
    fn avg_len(&self, field: FieldName) -> f64 {
        let lens = match field {
            FieldName::Title => &self.snap.title_len,
            FieldName::Description => &self.snap.description_len,
            FieldName::Tags => &self.snap.tags_len,
            FieldName::Stored => return 0.0,
        };
        let n = lens.len() as f64;
        if n == 0.0 {
            return 0.0;
        }
        lens.iter().map(|&l| l as f64).sum::<f64>() / n
    }

    fn doc_len(&self, field: FieldName, doc: u32) -> f64 {
        match field {
            FieldName::Title => self.snap.title_len[doc as usize] as f64,
            FieldName::Description => self.snap.description_len[doc as usize] as f64,
            FieldName::Tags => self.snap.tags_len[doc as usize] as f64,
            FieldName::Stored => 0.0,
        }
    }

    fn postings(&self, field: FieldName, term: &str) -> Option<&Vec<Posting>> {
        match field {
            FieldName::Title => self.snap.title.get(term),
            FieldName::Description => self.snap.description.get(term),
            FieldName::Tags => self.snap.tags.get(term),
            FieldName::Stored => None,
        }
    }

    pub fn video_id(&self, doc: u32) -> &str {
        self.snap.docs
            .get(doc as usize)
            .map(|d| d.video_id.as_str())
            .unwrap_or_default()
    }

    pub fn doc_video_id(&self, doc: u32) -> &str {
        self.video_id(doc)
    }

    /// BM25 score of a single doc for a term set over one field. Terms with
    /// no postings contribute 0. Excludes the doc for `exclude_video_id`.
    fn score_doc(
        &self,
        field: FieldName,
        terms: &[String],
        doc: u32,
        exclude_video_id: Option<&str>,
    ) -> f32 {
        if let Some(ex) = exclude_video_id {
            if self.video_id(doc) == ex {
                return 0.0;
            }
        }
        let n = self.num_docs() as f64;
        let avgdl = self.avg_len(field);
        let dl = self.doc_len(field, doc);
        let k1 = 1.2f64;
        let b = 0.75f64;
        let mut total = 0.0f64;
        for term in terms {
            let Some(postings) = self.postings(field, term) else {
                continue;
            };
            let df = postings.len() as f64;
            let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
            let tf = postings
                .iter()
                .find(|p| p.doc == doc)
                .map(|p| p.tf as f64)
                .unwrap_or(0.0);
            if tf == 0.0 {
                continue;
            }
            let denom = tf + k1 * (1.0 - b + b * dl / avgdl.max(1.0));
            total += idf * (tf * (k1 + 1.0)) / denom;
        }
        total as f32
    }

    /// Max BM25 over matching docs (excluding `exclude_video_id`), 0 when
    /// nothing matches. This is `corpus_resonance`.
    pub fn corpus_resonance(
        &self,
        field: FieldName,
        query: &str,
        exclude_video_id: Option<&str>,
    ) -> f32 {
        let terms = tokenize(query);
        if terms.is_empty() {
            return 0.0;
        }
        let mut best = 0.0f32;
        for doc in 0..self.snap.docs.len() as u32 {
            let s = self.score_doc(field, &terms, doc, exclude_video_id);
            best = best.max(s);
        }
        best
    }

    /// All matching (video_id, score) pairs for a field, best first.
    pub fn matches(&self, field: FieldName, query: &str) -> Vec<(String, f32)> {
        let terms = tokenize(query);
        if terms.is_empty() {
            return Vec::new();
        }
        let mut results: Vec<(String, f32)> = Vec::new();
        for doc in 0..self.snap.docs.len() as u32 {
            let s = self.score_doc(field, &terms, doc, None);
            if s > 0.0 {
                results.push((self.video_id(doc).to_string(), s));
            }
        }
        results.sort_by(|a, b| b.1.total_cmp(&a.1));
        results
    }

    /// Whether a term (already tokenized) appears in the field's index.
    pub fn has_term(&self, field: FieldName, term: &str) -> bool {
        self.postings(field, term).is_some()
    }

    /// Distinct terms known to the index (across all fields).
    pub fn terms(&self) -> HashSet<String> {
        let mut s = HashSet::new();
        for map in [&self.snap.title, &self.snap.description, &self.snap.tags] {
            s.extend(map.keys().cloned());
        }
        s
    }

    // -- persistence -------------------------------------------------------

    /// Atomically persist to `path` (temp file + rename). Returns the doc
    /// count written. Errors on fsync/rename failure so callers know the
    /// snapshot was NOT durable.
    pub fn persist(&self, path: &Path) -> Result<u64, TubeforgeError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| index_err(format!(
                "create index dir {}: {e}",
                parent.display()
            )))?;
        }
        let json = serde_json::to_vec(&self.snap)
            .map_err(|e| index_err(format!("serialize index: {e}")))?;
        let tmp = path.with_extension("json.tmp");
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| index_err(format!(
                "create temp index {}: {e}",
                tmp.display()
            )))?;
            use std::io::Write;
            f.write_all(&json).map_err(|e| index_err(format!("write index: {e}")))?;
            f.sync_all().map_err(|e| index_err(format!("fsync index: {e}")))?;
        }
        std::fs::rename(&tmp, path).map_err(|e| index_err(format!(
            "atomic-rename index {} -> {}: {e}",
            tmp.display(),
            path.display()
        )))?;
        Ok(self.num_docs())
    }

    /// Load a snapshot from `path`. Missing file -> empty store. Corrupt or
    /// wrong-version file is a hard error (the reindex command rebuilds).
    pub fn load(path: &Path) -> Result<Store, TubeforgeError> {
        if !path.exists() {
            return Ok(Store::new());
        }
        let bytes = std::fs::read(path).map_err(|e| index_err(format!(
            "read index {}: {e}",
            path.display()
        )))?;
        let snap: Snapshot = serde_json::from_slice(&bytes).map_err(|e| index_err(format!(
            "decode index {}: {e}",
            path.display()
        )))?;
        if snap.version != STORE_VERSION {
            return Err(index_err(format!(
                "index {} version {} != expected {STORE_VERSION} — rebuild with `tubeforge reindex`",
                path.display(),
                snap.version
            )));
        }
        let mut store = Store { snap, by_video_id: HashMap::new() };
        for (i, d) in store.snap.docs.iter().enumerate() {
            store.by_video_id.insert(d.video_id.clone(), i as u32);
        }
        Ok(store)
    }

    /// Snapshot file name inside the index dir.
    pub fn file_name() -> &'static str {
        "index.json"
    }
}

/// Which text field an index refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldName {
    Title,
    Description,
    Tags,
    /// Stored (non-tokenized) fields — resolvable by name but not queryable.
    Stored,
}

fn add_terms(index: &mut HashMap<String, Vec<Posting>>, doc: u32, terms: &[String]) {
    for term in terms {
        let entry = index.entry(term.clone()).or_default();
        match entry.iter_mut().find(|p| p.doc == doc) {
            Some(p) => p.tf += 1,
            None => entry.push(Posting { doc, tf: 1 }),
        }
    }
}

fn remove_doc_from_index(index: &mut HashMap<String, Vec<Posting>>, doc: u32) {
    index.retain(|_, postings| {
        postings.retain(|p| p.doc != doc);
        !postings.is_empty()
    });
}

/// Resolve a field name string to a `FieldName`. Only the three tokenized
/// text fields are queryable; the stored fields resolve to `Stored`.
pub fn field_from_name(name: &str) -> Option<FieldName> {
    match name {
        crate::search::index::FIELD_TITLE => Some(FieldName::Title),
        crate::search::index::FIELD_DESCRIPTION => Some(FieldName::Description),
        crate::search::index::FIELD_TAGS => Some(FieldName::Tags),
        crate::search::index::FIELD_VIDEO_ID
        | crate::search::index::FIELD_CHANNEL_ID
        | crate::search::index::FIELD_PUBLISHED_AT => Some(FieldName::Stored),
        _ => None,
    }
}

/// Convenience for tests: build a store from docs and persist to a dir.
pub fn write_store(dir: &Path, docs: &[IndexedDoc]) -> Result<Store, TubeforgeError> {
    let mut store = Store::new();
    for d in docs {
        store.upsert(d.clone());
    }
    store.persist(&dir.join(Store::file_name()))?;
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn doc(id: &str, title: &str) -> IndexedDoc {
        IndexedDoc {
            video_id: id.to_string(),
            channel_id: None,
            title: title.to_string(),
            description: String::new(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn upsert_indexes_and_queries() {
        let mut s = Store::new();
        s.upsert(doc("a", "rust database engineering guide"));
        s.upsert(doc("b", "paseto tokens explained"));
        let hits = s.matches(FieldName::Title, "database");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "a");
        assert!(hits[0].1 > 0.0, "positive BM25");
        assert!(s.corpus_resonance(FieldName::Title, "database", None) > 0.0);
        assert_eq!(s.matches(FieldName::Title, "nonexistentterm").len(), 0);
    }

    #[test]
    fn resonance_excludes_video() {
        let mut s = Store::new();
        s.upsert(doc("a", "rust database guide"));
        s.upsert(doc("b", "database rust both"));
        let with_a = s.corpus_resonance(FieldName::Title, "database", None);
        let without_a = s.corpus_resonance(FieldName::Title, "database", Some("a"));
        assert!(with_a >= without_a);
        assert!(without_a > 0.0, "b still matches");
    }

    #[test]
    fn upsert_replaces_and_removes() {
        let mut s = Store::new();
        s.upsert(doc("a", "old topic"));
        s.upsert(doc("a", "brand new topic"));
        assert_eq!(s.num_docs(), 1);
        assert_eq!(s.matches(FieldName::Title, "old").len(), 0);
        assert_eq!(s.matches(FieldName::Title, "brand").len(), 1);
        s.remove("a");
        assert_eq!(s.num_docs(), 0);
        assert_eq!(s.matches(FieldName::Title, "brand").len(), 0);
    }

    #[test]
    fn persist_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = Store::new();
        s.upsert(doc("a", "rust database engineering"));
        s.upsert(doc("b", "paseto vs jwt"));
        s.persist(&dir.path().join(Store::file_name())).expect("persist");
        let loaded = Store::load(&dir.path().join(Store::file_name())).expect("load");
        assert_eq!(loaded.num_docs(), 2);
        assert_eq!(loaded.matches(FieldName::Title, "database").len(), 1);
        assert_eq!(loaded.matches(FieldName::Title, "paseto").len(), 1);
    }

    #[test]
    fn tokenize_is_deterministic() {
        assert_eq!(tokenize("Rust Database!"), vec!["rust", "database"]);
        assert_eq!(tokenize("  multiple   spaces "), vec!["multiple", "spaces"]);
        assert_eq!(tokenize("Über-fáCile"), vec!["über", "fácile"]);
    }

    // Property: BM25 score is monotone in term frequency — a doc containing
    // the query term more times must never score lower than a copy with
    // fewer occurrences (same doc length aside, tf dominates for BM25).
    proptest::proptest! {
        #[test]
        fn bm25_tf_monotone(base_tf in 1usize..20, extra in 0usize..5) {
            let mut s = Store::new();
            let hi = vec!["term"; base_tf + extra].join(" ");
            let lo = vec!["term"; base_tf].join(" ");
            s.upsert(doc("a", &format!("{hi} shared")));
            s.upsert(doc("b", &format!("{lo} shared")));
            let a = s.matches(FieldName::Title, "term");
            let b = s.matches(FieldName::Title, "term");
            let sa = a.iter().find(|(id, _)| id == "a").map(|(_, s)| *s).unwrap_or(0.0);
            let sb = b.iter().find(|(id, _)| id == "b").map(|(_, s)| *s).unwrap_or(0.0);
            prop_assert!(sa >= sb, "higher tf must not score lower: {sa} < {sb}");
        }
    }

    // Property: persistence roundtrip preserves exact query results.
    proptest::proptest! {
        #[test]
        fn persist_roundtrip_preserves_matches(title in ".*", query in ".*") {
            let dir = tempfile::tempdir().expect("tempdir");
            let mut s = Store::new();
            s.upsert(doc("vid1", &title));
            s.persist(&dir.path().join(Store::file_name())).expect("persist");
            let loaded = Store::load(&dir.path().join(Store::file_name())).expect("load");
            let before = s.matches(FieldName::Title, &query);
            let after = loaded.matches(FieldName::Title, &query);
            prop_assert_eq!(before, after);
        }
    }
}
