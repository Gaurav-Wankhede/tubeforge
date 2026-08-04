//! tantivy index lifecycle (LLD §3.2).
//!
//! Location `<data>/index/`; rebuildable via `rebuild` (reindex command) —
//! the index is never part of backups.

use std::path::Path;

use tantivy::schema::{DateOptions, Schema, STORED, STRING, TEXT};
use tantivy::{DateTime, Index, IndexWriter, Term};

use crate::error::TubeforgeError;

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

/// The canonical index schema (LLD §3.2).
pub fn schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field(FIELD_VIDEO_ID, STRING | STORED);
    builder.add_text_field(FIELD_CHANNEL_ID, STRING | STORED);
    builder.add_text_field(FIELD_TITLE, TEXT | STORED);
    builder.add_text_field(FIELD_DESCRIPTION, TEXT);
    builder.add_text_field(FIELD_TAGS, TEXT);
    builder.add_date_field(FIELD_PUBLISHED_AT, DateOptions::default().set_indexed());
    builder.build()
}

/// Build a fresh index directory at `dir` (overwrites nothing; errors if the
/// dir already holds an index).
pub fn new_index(dir: &Path) -> Result<Index, TubeforgeError> {
    // tantivy's MmapDirectory does not create the target dir itself.
    std::fs::create_dir_all(dir).map_err(|e| TubeforgeError::Index {
        detail: format!("create index dir {}: {e}", dir.display()),
    })?;
    Index::create_in_dir(dir, schema()).map_err(|e| TubeforgeError::Index {
        detail: format!("create index at {}: {e}", dir.display()),
    })
}

/// Open an existing index or create a fresh one at `dir`.
pub fn open_or_create(dir: &Path) -> Result<Index, TubeforgeError> {
    match Index::open_in_dir(dir) {
        Ok(index) => Ok(index),
        Err(_) => new_index(dir),
    }
}

/// Delete the doc for `video_id` (if present) and add the new one. Caller
/// commits. One `IndexWriter` per batch (LLD §3.2 lifecycle).
pub fn upsert(writer: &mut IndexWriter, fields: &Schema, doc: &VideoDoc) -> Result<(), TubeforgeError> {
    let video_id = fields.get_field(FIELD_VIDEO_ID).map_err(index_err)?;
    let _deleted = writer.delete_term(Term::from_field_text(video_id, &doc.video_id));

    let mut d = tantivy::doc![];
    d.add_text(video_id, &doc.video_id);
    if let Some(cid) = &doc.channel_id {
        let channel_id = fields.get_field(FIELD_CHANNEL_ID).map_err(index_err)?;
        d.add_text(channel_id, cid);
    }
    d.add_text(fields.get_field(FIELD_TITLE).map_err(index_err)?, &doc.title);
    d.add_text(fields.get_field(FIELD_DESCRIPTION).map_err(index_err)?, &doc.description);
    d.add_text(fields.get_field(FIELD_TAGS).map_err(index_err)?, doc.tags.join(" "));
    if let Some(ts) = doc.published_at {
        d.add_date(
            fields.get_field(FIELD_PUBLISHED_AT).map_err(index_err)?,
            DateTime::from_timestamp_secs(ts),
        );
    }
    writer.add_document(d).map_err(index_err)?;
    Ok(())
}

/// Full rebuild from the `videos` table contents: truncate the dir, create a
/// fresh index, add every doc, commit. Idempotent (LLD §3.2 recovery path).
pub fn rebuild(dir: &Path, docs: &[VideoDoc]) -> Result<usize, TubeforgeError> {
    if dir.exists() {
        for entry in std::fs::read_dir(dir).map_err(|e| TubeforgeError::Index {
            detail: format!("read index dir {}: {e}", dir.display()),
        })? {
            let p = entry.map_err(|e| TubeforgeError::Index {
                detail: format!("index dir entry: {e}"),
            })?.path();
            if p.is_dir() {
                std::fs::remove_dir_all(&p).map_err(|e| TubeforgeError::Index {
                    detail: format!("remove {}: {e}", p.display()),
                })?;
            } else {
                std::fs::remove_file(&p).map_err(|e| TubeforgeError::Index {
                    detail: format!("remove {}: {e}", p.display()),
                })?;
            }
        }
    }

    let index = new_index(dir)?;
    let fields = index.schema();
    let mut writer = index.writer(50_000_000).map_err(index_err)?;
    for doc in docs {
        upsert(&mut writer, &fields, doc)?;
    }
    writer.commit().map_err(index_err)?;
    Ok(docs.len())
}

fn index_err(e: tantivy::TantivyError) -> TubeforgeError {
    TubeforgeError::Index { detail: e.to_string() }
}
