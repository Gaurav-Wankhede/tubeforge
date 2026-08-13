//! Search layer (LLD §2, §3.2): from-scratch BM25 inverted index.
//!
//! **Dependency rule:** this is the ONLY module that imports the search
//! engine; no external full-text dependency exists. Tokenization, posting
//! lists, and BM25 scoring are implemented in `store.rs`. Field names mirror
//! LLD §3.2. Lifecycle: one `IndexWriter` per ingest batch (add/delete by
//! `video_id`, one atomic commit), `Bm25::reload` for scoring queries, full
//! rebuild via `rebuild` (`reindex` command).

pub mod bm25;
pub mod index;
pub mod store;

pub use bm25::Bm25;
pub use index::{
    new_index, open_or_create, rebuild, upsert, Index, IndexWriter, Schema, VideoDoc,
    FIELD_CHANNEL_ID, FIELD_DESCRIPTION, FIELD_PUBLISHED_AT, FIELD_TAGS, FIELD_TITLE,
    FIELD_VIDEO_ID,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Gate item 5: the custom index compiles and returns a positive BM25
    /// score for a trivial document.
    #[test]
    fn custom_index_pin_bm25_positive() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = new_index(&dir.path().join("index")).expect("new_index");
        let mut writer = index.writer(50_000_000);
        writer
            .add_document(VideoDoc {
                video_id: "abc123".to_string(),
                title: "rust database engineering guide".to_string(),
                ..Default::default()
            })
            .expect("add doc");
        writer.commit().expect("commit");

        let bm25 = Bm25::open(index).expect("bm25");
        let score = bm25.corpus_resonance(FIELD_TITLE, "database", None);
        assert!(score > 0.0, "BM25 score must be positive, got {score}");
        let miss = bm25.corpus_resonance(FIELD_TITLE, "unrelatedterm", None);
        assert_eq!(miss, 0.0, "no hit for non-matching term");
        assert_eq!(bm25.num_docs(), 1);
    }

    /// `new_index` helper is usable and yields an openable, empty index.
    #[test]
    fn new_index_helper_creates_searchable_index() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = new_index(&dir.path().join("index")).expect("new_index");
        assert_eq!(index.num_docs(), 0, "fresh index has no documents");
    }

    /// Real schema fields (LLD §3.2) all resolve on a fresh index.
    #[test]
    fn schema_has_all_lld_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = new_index(&dir.path().join("index")).expect("new_index");
        let schema = index.schema();
        for f in [
            FIELD_VIDEO_ID,
            FIELD_CHANNEL_ID,
            FIELD_TITLE,
            FIELD_DESCRIPTION,
            FIELD_TAGS,
            FIELD_PUBLISHED_AT,
        ] {
            schema
                .get_field(f)
                .unwrap_or_else(|_| panic!("missing field {f}"));
        }
    }

    /// Persistence: commit then reopen (via open_or_create) reloads docs.
    #[test]
    fn reopen_loads_committed_docs() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let index = open_or_create(&dir.path().join("index")).expect("create");
            let mut writer = index.writer(50_000_000);
            writer
                .add_document(VideoDoc {
                    video_id: "x1".to_string(),
                    title: "paseto tokens".to_string(),
                    ..Default::default()
                })
                .expect("add");
            writer.commit().expect("commit");
        }
        let reopened = open_or_create(&dir.path().join("index")).expect("reopen");
        let bm25 = Bm25::open(reopened).expect("bm25");
        assert_eq!(bm25.num_docs(), 1);
        assert!(bm25.corpus_resonance(FIELD_TITLE, "paseto", None) > 0.0);
    }
}
