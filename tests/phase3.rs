//! Phase 3 workstream B integration tests: migration 003 (privacy_status),
//! export (dir + zip round-trips), health privacy census, and the
//! no-key error contracts of `check availability` / `filmot get`.

use std::path::Path;

use tubeforge::commands::availability;
use tubeforge::commands::export::{self, ExportFormat};
use tubeforge::commands::filmot;
use tubeforge::config::Config;
use tubeforge::storage::Db;

fn test_config(dir: &Path) -> Config {
    Config {
        db_path: dir.join("tubeforge.db"),
        data_dir: dir.join("data"),
        backup_dir: dir.join("backups"),
        backup_keep: 10,
        log_level: "info".to_string(),
        youtube_api_key: Some("test-key".to_string()),
        quota_warn_at: 90,
        chromium_dir: dir.join("chromium"),
        ytdlp_path: "yt-dlp".into(),
        ytdlp_enabled: false,
        ytdlp_client: None,
        ytdlp_js_runtime: None,
        own_channel: None,
    }
}

/// Seed every exportable table with deterministic rows (2 videos, 1 channel,
/// 1 tracked keyword + 1 ranking, 1 idea, 1 alert, 1 score).
async fn seed_export_data(db: &mut Db) {
    let at = "2026-07-01T00:00:00Z";
    {
        let mut batch = db.begin_batch().await.expect("batch");
        batch
            .upsert_channel(&tubeforge::storage::db::ChannelRow {
                channel_id: "UCa1b2c3d4e5f6g7h8i9j0kLM".into(),
                handle: Some("@Fixture".into()),
                title: "Fixture Channel".into(),
                country: Some("US".into()),
                subscriber_count: Some(1000),
                video_count: Some(2),
                source: "rss".into(),
                fetched_at: at.into(),
                updated_at: at.into(),
                ..Default::default()
            })
            .await
            .expect("channel");
        for (id, published) in [
            ("bbb222ccc33", "2026-07-02T00:00:00Z"),
            ("aaa111bbb22", "2026-07-01T00:00:00Z"),
        ] {
            batch
                .upsert_video(&tubeforge::storage::db::VideoRow {
                    video_id: id.into(),
                    channel_id: Some("UCa1b2c3d4e5f6g7h8i9j0kLM".into()),
                    title: format!("Video {id}"),
                    description: "desc".into(),
                    tags: r#"["a","b"]"#.into(),
                    published_at: published.into(),
                    view_count: Some(42),
                    source: "rss".into(),
                    fetched_at: at.into(),
                    updated_at: at.into(),
                    ..Default::default()
                })
                .await
                .expect("video");
        }
        batch.commit().await.expect("commit");
    }
    db.add_keywords(&["rust".to_string()], Some("dev"))
        .await
        .expect("keyword");
    db.upsert_ranking("rust", at, Some("aaa111bbb22"), Some(1), None)
        .await
        .expect("ranking");
    db.upsert_idea("Idea One", "{}", 80.5, "draft", None)
        .await
        .expect("idea");
    db.insert_alert("gap", None, "test alert", "warn")
        .await
        .expect("alert");
    db.upsert_score("aaa111bbb22", 90.0, 80.0, 85.0, "{}")
        .await
        .expect("score");
}

// ---------------------------------------------------------------------------
// migration 003 (SCHEMA_VERSION 2 → 3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_migration_003_privacy_column_and_version_gate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("m003.db");

    let db = Db::open(&db_path).await.expect("open 1");
    assert_eq!(
        db.user_version().await.expect("version"),
        tubeforge::storage::schema::SCHEMA_VERSION
    );
    assert_eq!(
        db.meta_get("schema_version")
            .await
            .expect("meta")
            .as_deref(),
        Some(
            tubeforge::storage::schema::SCHEMA_VERSION
                .to_string()
                .as_str()
        )
    );
    // The new nullable column exists.
    let cols = table_cols(&db, "videos").await;
    assert!(
        cols.contains(&"privacy_status".to_string()),
        "privacy_status missing"
    );

    // Reopen: the version gate keeps 003 from re-running (no duplicate-column
    // error — ALTER TABLE ADD COLUMN has no IF NOT EXISTS here).
    let db2 = Db::open(&db_path).await.expect("open 2");
    assert_eq!(
        db2.user_version().await.expect("version"),
        tubeforge::storage::schema::SCHEMA_VERSION
    );
    assert!(table_cols(&db2, "videos")
        .await
        .contains(&"privacy_status".to_string()));
    // Column is nullable: a minimal insert still works.
    let mut db2 = db2;
    {
        let mut batch = db2.begin_batch().await.expect("batch");
        batch
            .upsert_video(&tubeforge::storage::db::VideoRow {
                video_id: "y1y1y1y1y1y".into(),
                title: "t".into(),
                published_at: "2026-01-01T00:00:00Z".into(),
                fetched_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                source: "rss".into(),
                ..Default::default()
            })
            .await
            .expect("minimal insert");
        batch.commit().await.expect("commit");
    }
    // And a full VideoRow upsert through the repository carries it.
    let mut v = db2
        .get_video("y1y1y1y1y1y")
        .await
        .expect("get")
        .expect("row");
    v.privacy_status = Some("unlisted".to_string());
    let mut batch = db2.begin_batch().await.expect("batch");
    batch.upsert_video(&v).await.expect("upsert");
    batch.commit().await.expect("commit");
    let back = db2
        .get_video("y1y1y1y1y1y")
        .await
        .expect("get")
        .expect("row");
    assert_eq!(back.privacy_status.as_deref(), Some("unlisted"));
}

async fn table_cols(db: &Db, table: &str) -> Vec<String> {
    db.columns(table).await.expect("columns")
}

// ---------------------------------------------------------------------------
// export: dir + zip round-trips
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_export_dir_files_and_manifest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");
    seed_export_data(&mut db).await;

    let out_dir = dir.path().join("export");
    let data = export::run(&cfg, &out_dir, ExportFormat::Dir)
        .await
        .expect("export dir");
    assert_eq!(data["format"], "dir");
    assert_eq!(data["archive"], serde_json::Value::Null);

    // All 10 files exist.
    for f in [
        "videos.csv",
        "channels.csv",
        "tags.csv",
        "keywords.csv",
        "keyword_rankings.csv",
        "videos.json",
        "ideas.json",
        "alerts.json",
        "scores.json",
        "manifest.json",
    ] {
        assert!(out_dir.join(f).is_file(), "missing {f}");
    }

    // videos.csv header verbatim + deterministic video_id order.
    let videos_csv = std::fs::read_to_string(out_dir.join("videos.csv")).expect("read");
    let mut lines = videos_csv.lines();
    assert_eq!(
        lines.next().expect("header"),
        "Video ID,Title,Channel ID,Channel Title,Description,Published,Views,Likes,Comments,\
         Duration,Category ID,Category Name,Language,Tags,Source,Privacy Status,Recording Date,\
         Recording Location,Topic Categories"
    );
    assert!(
        lines.next().expect("row1").starts_with("aaa111bbb22,"),
        "rows ordered by video_id"
    );
    assert!(lines.next().expect("row2").starts_with("bbb222ccc33,"));

    // channels.csv / tags.csv / keywords.csv.
    let channels_csv = std::fs::read_to_string(out_dir.join("channels.csv")).expect("read");
    assert!(
        channels_csv.starts_with("Channel ID,Title,Handle,Subscribers,Video Count,Country\n"),
        "channels header: {channels_csv}"
    );
    assert!(channels_csv.contains("UCa1b2c3d4e5f6g7h8i9j0kLM"));
    let tags_csv = std::fs::read_to_string(out_dir.join("tags.csv")).expect("read");
    assert!(tags_csv.starts_with("Tag,Video Count,First Used,Last Used\n"));
    assert!(
        tags_csv.contains("a,2,"),
        "tag a on both videos: {tags_csv}"
    );
    let keywords_csv = std::fs::read_to_string(out_dir.join("keywords.csv")).expect("read");
    assert!(keywords_csv.starts_with("Keyword,Niche,Created At\n"));
    assert!(keywords_csv.contains("rust,dev,"));

    // JSON arrays parse and carry the rows.
    let videos_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("videos.json")).expect("read"))
            .expect("videos.json");
    assert_eq!(videos_json["rows"].as_array().expect("array").len(), 2);
    assert_eq!(videos_json["rows"][0]["video_id"], "aaa111bbb22");
    assert_eq!(
        videos_json["_manifest"]["schema_version"],
        tubeforge::storage::schema::SCHEMA_VERSION
    );
    let ideas_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("ideas.json")).expect("read"))
            .expect("ideas.json");
    assert_eq!(ideas_json["rows"][0]["title_suggestion"], "Idea One");
    let alerts_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("alerts.json")).expect("read"))
            .expect("alerts.json");
    assert_eq!(alerts_json["rows"][0]["kind"], "gap");
    let scores_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("scores.json")).expect("read"))
            .expect("scores.json");
    assert_eq!(scores_json["rows"][0]["total_score"], 85.0);

    // manifest carries counts + schema_version.
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out_dir.join("manifest.json")).expect("read"),
    )
    .expect("manifest");
    assert_eq!(manifest["format"], "tubeforge-export");
    assert_eq!(
        manifest["schema_version"],
        tubeforge::storage::schema::SCHEMA_VERSION
    );
    assert_eq!(manifest["counts"]["videos"], 2);
    assert_eq!(manifest["counts"]["channels"], 1);
    assert_eq!(manifest["counts"]["tags"], 2);
    assert_eq!(manifest["counts"]["keywords"], 1);
    assert_eq!(manifest["counts"]["keyword_rankings"], 1);
    assert_eq!(manifest["counts"]["ideas"], 1);
    assert_eq!(manifest["counts"]["alerts"], 1);
    assert_eq!(manifest["counts"]["scores"], 1);
    assert_eq!(manifest["files"].as_array().expect("files").len(), 10);
}

#[tokio::test]
async fn p3_export_zip_archive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");
    seed_export_data(&mut db).await;

    let out_dir = dir.path().join("zipexport");
    let data = export::run(&cfg, &out_dir, ExportFormat::Zip)
        .await
        .expect("export zip");
    assert_eq!(data["format"], "zip");
    let archive_name = data["archive"].as_str().expect("archive name");
    let archive_path = out_dir.join(archive_name);
    assert!(archive_path.is_file(), "zip exists: {archive_path:?}");

    // Read the archive back with the zip crate: 10 entries, deflate-valid.
    let file = std::fs::File::open(&archive_path).expect("open zip");
    let mut z = zip::ZipArchive::new(file).expect("parse zip");
    assert_eq!(z.len(), 10);
    let mut names: Vec<String> = (0..z.len())
        .map(|i| z.by_index(i).expect("entry").name().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "alerts.json",
            "channels.csv",
            "ideas.json",
            "keyword_rankings.csv",
            "keywords.csv",
            "manifest.json",
            "scores.json",
            "tags.csv",
            "videos.csv",
            "videos.json"
        ]
    );
    // Spot-check a decompressed entry.
    let mut videos = z.by_name("videos.csv").expect("videos.csv entry");
    let mut body = String::new();
    use std::io::Read;
    videos.read_to_string(&mut body).expect("read entry");
    assert!(body.starts_with("Video ID,Title,"));
}

// ---------------------------------------------------------------------------
// health privacy census (migration 003 surfaced in health)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_health_privacy_census() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");
    let at = "2026-07-01T00:00:00Z";
    {
        let mut batch = db.begin_batch().await.expect("batch");
        for (id, privacy) in [
            ("aaa111bbb22", "public"),
            ("bbb222ccc33", "unlisted"),
            ("ccc333ddd44", "private"),
            ("ddd444eee55", "public"),
        ] {
            batch
                .upsert_video(&tubeforge::storage::db::VideoRow {
                    video_id: id.into(),
                    title: "t".into(),
                    published_at: at.into(),
                    fetched_at: at.into(),
                    updated_at: at.into(),
                    privacy_status: Some(privacy.into()),
                    source: "rss".into(),
                    ..Default::default()
                })
                .await
                .expect("video");
        }
        batch.commit().await.expect("commit");
    }
    let h = tubeforge::analytics::reports::health(&db, 14)
        .await
        .expect("health");
    assert_eq!(h["privacy"]["unlisted"], 1);
    assert_eq!(h["privacy"]["private"], 1);
}

// ---------------------------------------------------------------------------
// opt-in contracts: clear errors when the required key is absent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_check_availability_requires_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg = test_config(dir.path());
    cfg.youtube_api_key = None;
    Db::open(&cfg.db_path).await.expect("open db");

    let err = availability::run(&cfg, &[])
        .await
        .expect_err("no key → error");
    assert!(
        matches!(err, tubeforge::error::TubeforgeError::Config(_)),
        "config error, got {err:?}"
    );
}

#[tokio::test]
async fn p3_filmot_get_requires_key() {
    // The key env is process-global; this crate's tests are the only users
    // of TUBEFORGE_FILMOT_KEY, so removing it here cannot race anything.
    std::env::remove_var(filmot::FILMOT_KEY_ENV);
    let err = filmot::run_get("dQw4w9WgXcQ")
        .await
        .expect_err("no key → error");
    assert!(
        matches!(err, tubeforge::error::TubeforgeError::Config(_)),
        "config error, got {err:?}"
    );
    assert!(
        err.to_string().contains("TUBEFORGE_FILMOT_KEY"),
        "message points at the env key: {err}"
    );
}

/// The full command path (`check availability --video-id` on an unknown id)
/// errors as a usage error, mirroring `scorecard --channel`.
#[tokio::test]
async fn p3_check_availability_unknown_id_is_usage_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    Db::open(&cfg.db_path).await.expect("open db");

    let err = availability::run(&cfg, &["notstored00000".to_string()])
        .await
        .expect_err("unknown id");
    assert!(
        matches!(err, tubeforge::error::TubeforgeError::Usage(_)),
        "usage error, got {err:?}"
    );
}
