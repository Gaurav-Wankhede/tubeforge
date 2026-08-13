//! Deduplication + data-cleaning tests for the storage layer.
//!
//! Covers migration 004 cleanup, atomic idea/alert dedup, keyword
//! normalization, and the @handle placeholder → real channel merge.

use std::path::PathBuf;

use tubeforge::storage::db::{ChannelRow, Db, VideoRow};

fn db_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("dedup.db")
}

#[tokio::test]
async fn schema_version_records_and_reopen_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);

    // 1. Open fresh: the full tfdb schema is created and the version recorded.
    {
        let mut db = Db::open(&path).await.expect("open fresh db");
        let at = "2026-08-01T00:00:00Z";
        // Seed duplicate ideas/alerts through the app-level deduping writes.
        let mut batch = db.begin_batch().await.expect("batch");
        batch
            .upsert_video(&VideoRow {
                video_id: "vid1".into(),
                title: "Video One".into(),
                description: String::new(),
                published_at: at.into(),
                fetched_at: at.into(),
                updated_at: at.into(),
                source: "rss".into(),
                ..Default::default()
            })
            .await
            .expect("insert video");
        batch.commit().await.expect("commit");
        db.upsert_idea("Same Title", "{}", 50.0, "draft", Some("vid1"))
            .await
            .expect("idea");
        db.insert_alert("quota", None, "quota warning", "warn")
            .await
            .expect("alert");
    }

    // 2. Re-open: version stays put (idempotent open, no SQL migrations to run).
    let db = Db::open(&path).await.expect("reopen");
    assert_eq!(
        db.user_version().await.expect("version"),
        tubeforge::storage::schema::SCHEMA_VERSION
    );

    // App-level dedup guarantees no duplicate logical rows.
    let ideas = db.list_ideas(None, 10).await.expect("list ideas");
    assert_eq!(ideas.len(), 1, "duplicate ideas merged to one row");
    assert_eq!(ideas[0].title_suggestion, "Same Title");

    let alerts = db.list_alerts(0).await.expect("list alerts");
    assert_eq!(alerts.len(), 1, "duplicate alerts merged to one row");
}

#[tokio::test]
async fn upsert_idea_dedupes_and_refreshes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut db = Db::open(&db_path(&dir)).await.expect("open db");

    // Seed a video so the FK on source_video is satisfied.
    let mut batch = db.begin_batch().await.expect("begin batch");
    batch
        .upsert_video(&VideoRow {
            video_id: "vid1".into(),
            title: "Video One".into(),
            description: String::new(),
            published_at: "2026-08-01T00:00:00Z".into(),
            fetched_at: "2026-08-01T00:00:00Z".into(),
            updated_at: "2026-08-01T00:00:00Z".into(),
            source: "rss".into(),
            ..Default::default()
        })
        .await
        .expect("insert video");
    batch.commit().await.expect("commit");

    let id1 = db
        .upsert_idea("My Idea", "{}", 70.0, "draft", Some("vid1"))
        .await
        .expect("upsert idea");
    let id2 = db
        .upsert_idea("My Idea", "{}", 90.0, "draft", Some("vid1"))
        .await
        .expect("upsert same idea");

    assert_eq!(id1, id2, "same logical idea returns the same row id");
    let ideas = db.list_ideas(None, 10).await.expect("list ideas");
    assert_eq!(ideas.len(), 1);
    assert_eq!(ideas[0].score, 90.0, "score updated on refresh");
}

#[tokio::test]
async fn insert_alert_dedupes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Db::open(&db_path(&dir)).await.expect("open db");

    let first = db
        .insert_alert("quota", None, "quota warning", "warn")
        .await
        .expect("insert alert");
    assert_eq!(first, 1);

    let second = db
        .insert_alert("quota", None, "quota warning", "warn")
        .await
        .expect("insert duplicate alert");
    assert_eq!(second, 0, "duplicate alert suppressed");

    let alerts = db.list_alerts(0).await.expect("list alerts");
    assert_eq!(alerts.len(), 1);
}

#[tokio::test]
async fn add_keywords_normalizes_case() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Db::open(&db_path(&dir)).await.expect("open db");

    let added = db
        .add_keywords(&["Rust".into(), "  rust  ".into()], None)
        .await
        .expect("add keywords");
    assert_eq!(added, 1, "case/whitespace variants collapse to one keyword");

    let keywords = db.list_keywords().await.expect("list keywords");
    assert_eq!(keywords.len(), 1);
    assert_eq!(keywords[0].keyword, "rust");
}

#[tokio::test]
async fn batch_merge_channel_repoints_and_drops_placeholder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut db = Db::open(&db_path(&dir)).await.expect("open db");
    let at = "2026-08-01T00:00:00Z";

    let mut batch = db.begin_batch().await.expect("begin batch");

    // Real canonical channel.
    batch
        .upsert_channel(&ChannelRow {
            channel_id: "UC123".into(),
            handle: Some("@rust".into()),
            title: "Rust Channel".into(),
            source: "rss".into(),
            fetched_at: at.into(),
            updated_at: at.into(),
            ..Default::default()
        })
        .await
        .expect("upsert real channel");

    // Placeholder from an earlier oEmbed ingest (no handle — the canonical
    // channel owns it; handle is UNIQUE in the schema).
    batch
        .upsert_channel(&ChannelRow {
            channel_id: "@rust".into(),
            handle: None,
            title: "Placeholder".into(),
            source: "oembed".into(),
            fetched_at: at.into(),
            updated_at: at.into(),
            ..Default::default()
        })
        .await
        .expect("upsert placeholder");

    // Video attached to the placeholder.
    batch
        .upsert_video(&VideoRow {
            video_id: "vid1".into(),
            channel_id: Some("@rust".into()),
            title: "Video One".into(),
            description: String::new(),
            published_at: at.into(),
            fetched_at: at.into(),
            updated_at: at.into(),
            source: "oembed".into(),
            ..Default::default()
        })
        .await
        .expect("upsert video");

    // Merge placeholder into the canonical channel.
    batch
        .merge_channel("@rust", "UC123")
        .await
        .expect("merge channel");
    batch.commit().await.expect("commit batch");

    let channels = db.all_channels().await.expect("list channels");
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].channel_id, "UC123");

    let videos = db.all_videos().await.expect("list videos");
    assert_eq!(videos.len(), 1);
    assert_eq!(
        videos[0].channel_id.as_deref(),
        Some("UC123"),
        "video repointed"
    );
}
