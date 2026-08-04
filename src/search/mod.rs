//! Search layer (LLD §2, §3.2): tantivy BM25 index.
//!
//! **Dependency rule:** this is the ONLY module that imports `tantivy`.
//! BM25 is computed in Rust via tantivy — never the engine's FTS (ADR-2).
//!
//! Field names mirror LLD §3.2. Lifecycle: `IndexWriter` per ingest batch
//! (add/delete by `video_id`, one commit), `Reader` reload for scoring
//! queries, full rebuild via `rebuild` (`reindex` command).

pub mod bm25;
pub mod index;

pub use index::{
    new_index, open_or_create, rebuild, upsert, VideoDoc, FIELD_CHANNEL_ID, FIELD_DESCRIPTION,
    FIELD_PUBLISHED_AT, FIELD_TAGS, FIELD_TITLE, FIELD_VIDEO_ID,
};

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::collector::TopDocs;
    use tantivy::query::QueryParser;
    use tantivy::{doc, schema::STORED};

    /// Gate item 5: tantivy compiles/pins and returns a positive BM25 score
    /// for a trivial document.
    #[test]
    fn tantivy_pin_bm25_positive() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut builder = tantivy::schema::Schema::builder();
        let title = builder.add_text_field(FIELD_TITLE, tantivy::schema::TEXT | STORED);
        let schema = builder.build();

        let index = tantivy::Index::create_in_dir(dir.path(), schema).expect("create index");
        let mut writer = index.writer(50_000_000).expect("writer");
        writer
            .add_document(doc![title => "rust database engineering guide"])
            .expect("add doc");
        writer.commit().expect("commit");

        let reader = index.reader().expect("reader");
        reader.reload().expect("reload");
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(&index, vec![title]);
        let query = parser.parse_query("database").expect("parse query");
        let collector = TopDocs::with_limit(5).order_by_score();

        let hits = searcher.search(&query, &collector).expect("search");
        assert_eq!(hits.len(), 1, "expected exactly one hit");
        let (score, _doc_address) = hits[0];
        assert!(
            score > 0.0,
            "BM25 score must be positive for a matching doc, got {score}"
        );

        let miss = parser.parse_query("unrelatedterm").expect("parse miss");
        let miss_hits = searcher
            .search(&miss, &collector)
            .expect("search miss");
        assert_eq!(miss_hits.len(), 0, "no hit expected for non-matching term");
    }

    /// `new_index` helper is usable and yields an openable index.
    #[test]
    fn new_index_helper_creates_searchable_index() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = new_index(&dir.path().join("index")).expect("new_index");
        let reader = index.reader().expect("reader");
        let searcher = reader.searcher();
        assert_eq!(searcher.num_docs(), 0, "fresh index has no documents");
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
            schema.get_field(f).unwrap_or_else(|_| panic!("missing field {f}"));
        }
    }
}
