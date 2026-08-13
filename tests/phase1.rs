//! Phase 1 integration tests (LLD §12): wiremock fixtures only, NO real
//! network in automated tests.
//!
//! Coverage map (LLD §12):
//! - §5.1 RSS parse + ETag 304        p1_rss_parse_from_fixture, p1_backup_guard_304
//! - §5.2 oEmbed parse + handle       p1_oembed_parse_and_handle_extraction
//! - §5.3 API batching ≤50, 403       p1_api_batching_and_quota_ledger, p1_api_403_quota_error
//! - §5.4 quota ledger + rollover     p1_api_batching_and_quota_ledger, p1_quota_rollover_resets
//! - §6.2 upsert idempotency/preced.  p1_ingest_channels_idempotent, p1_source_precedence,
//!   p1_oembed_link_ingest_placeholder_nullable
//! - §6.3 backup guard                p1_backup_guard_snapshot_no_backup_304
//! - §3.2/§9.2 reindex                p1_reindex_rebuilds_after_corruption
//! - §7.2 basic score envelope        p1_score_draft_envelope_shape
//! - §9.3 migrations                  p1_migration_full_schema, p1_migration_phase0_upgrade

use serde_json::json;
use std::path::Path;
use std::time::Duration;
use tubeforge::commands::{reindex, score};
use tubeforge::config::Config;
use tubeforge::error::TubeforgeError;
use tubeforge::fetch::api::{iso8601_duration_to_secs, ApiClient};
use tubeforge::fetch::oembed::{handle_from_author_url, OEmbed};
use tubeforge::fetch::quota;
use tubeforge::fetch::rss::{self, RssFeed};
use tubeforge::fetch::FetchClients;
use tubeforge::ingest::{self, IngestOptions};
use tubeforge::search::bm25::Bm25;
use tubeforge::search::open_or_create;
use tubeforge::search::FIELD_TITLE;
use tubeforge::storage::db::VideoRow;
use tubeforge::storage::Db;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CHANNEL_ID: &str = "UCa1b2c3d4e5f6g7h8i9j0kLM";
const ETAG: &str = "W/\"abc123\"";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("fixture exists")
}

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

fn clients(mock: &MockServer) -> FetchClients {
    FetchClients::for_test(&mock.uri(), Duration::from_secs(5)).expect("clients")
}

fn opts(use_api: bool) -> IngestOptions {
    IngestOptions {
        use_api,
        no_backup: false,
    }
}

fn snapshot_count(cfg: &Config) -> usize {
    std::fs::read_dir(&cfg.backup_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    let fname = e.file_name();
                    let n = fname.to_string_lossy();
                    n.starts_with("tubeforge-") && n.ends_with(".db")
                })
                .count()
        })
        .unwrap_or(0)
}

/// Register the standard RSS feed mock (200 with ETag).
async fn mock_rss(mock: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/feeds/videos.xml"))
        .and(query_param("channel_id", CHANNEL_ID))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(fixture("rss_feed.xml"))
                .insert_header("ETag", ETAG),
        )
        .mount(mock)
        .await;
}

// ---------------------------------------------------------------------------
// §5.1 RSS parse
// ---------------------------------------------------------------------------

#[test]
fn p1_rss_parse_from_fixture() {
    let feed: RssFeed = rss::parse_feed(&fixture("rss_feed.xml")).expect("parse");

    assert_eq!(feed.channel_id.as_deref(), Some(CHANNEL_ID));
    assert_eq!(feed.channel_title.as_deref(), Some("Fixture Channel"));
    assert_eq!(feed.entries.len(), 3);

    let v = &feed.entries[0];
    assert_eq!(v.video_id, "aaa111bbb22");
    assert_eq!(v.title, "Rust Database Engineering Guide");
    assert_eq!(v.link, "https://www.youtube.com/watch?v=aaa111bbb22");
    assert_eq!(v.published, "2026-07-15T10:00:00+00:00");
    assert_eq!(v.updated, "2026-07-15T10:05:00+00:00");
    assert_eq!(
        v.description,
        "Learn how to build a database in Rust & understand the storage engine internals."
    );
    assert_eq!(
        v.thumbnail_url.as_deref(),
        Some("https://i.ytimg.com/vi/aaa111bbb22/mqdefault.jpg")
    );
    assert_eq!(v.rating_count, Some(12));
    assert_eq!(v.star_rating, Some(4.75));
    assert_eq!(v.views, Some(12345));

    assert_eq!(feed.entries[1].views, Some(9876));
    assert_eq!(feed.entries[2].video_id, "eee333fff44");
}

// ---------------------------------------------------------------------------
// §5.2 oEmbed parse + @handle extraction
// ---------------------------------------------------------------------------

#[test]
fn p1_oembed_parse_and_handle_extraction() {
    let o: OEmbed = serde_json::from_str(&fixture("oembed.json")).expect("parse");
    assert_eq!(o.title.as_deref(), Some("Example Video Title"));
    assert_eq!(o.author_name.as_deref(), Some("Example Channel"));
    assert_eq!(
        o.thumbnail_url.as_deref(),
        Some("https://i.ytimg.com/vi/aaa111bbb22/hqdefault.jpg")
    );
    assert_eq!(o.handle().as_deref(), Some("@examplechannel"));

    // Legacy /channel/UC... author URLs carry no handle.
    assert_eq!(
        handle_from_author_url("https://www.youtube.com/channel/UCa1b2c3d4e5f6g7h8i9j0k"),
        None
    );
    assert_eq!(
        handle_from_author_url("https://www.youtube.com/@weird_name"),
        Some("@weird_name".to_string())
    );
    assert_eq!(handle_from_author_url("not a url"), None);
}

#[test]
fn p1_api_duration_parse() {
    assert_eq!(iso8601_duration_to_secs("PT12M34S"), Some(754));
    assert_eq!(iso8601_duration_to_secs("P1DT2H"), Some(93_600));
    assert_eq!(iso8601_duration_to_secs("PT8M5S"), Some(485));
    assert_eq!(iso8601_duration_to_secs("garbage"), None);
}

// ---------------------------------------------------------------------------
// §5.3 / §5.4 API batching + quota ledger + rollover
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_api_batching_and_quota_ledger() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/videos"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("api_videos_list.json")))
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let db = Db::open(&dir.path().join("test.db"))
        .await
        .expect("open db");
    let api = ApiClient::new(&clients(&mock), "test-key");

    // 60 ids → 2 calls (50 + 10).
    let ids: Vec<String> = (0..60).map(|i| format!("vid{:010}", i)).collect();
    let items = api.fetch_videos(&db, &ids).await.expect("fetch");
    assert!(!items.is_empty(), "fixture items returned");

    let reqs = mock.received_requests().await.expect("reqs");
    let api_calls: Vec<_> = reqs.iter().filter(|r| r.url.path() == "/videos").collect();
    assert_eq!(api_calls.len(), 2, "60 ids must batch into exactly 2 calls");
    let ids_first = api_calls[0]
        .url
        .query_pairs()
        .find(|(k, _)| k == "id")
        .expect("id param")
        .1
        .to_string();
    let ids_second = api_calls[1]
        .url
        .query_pairs()
        .find(|(k, _)| k == "id")
        .expect("id param")
        .1
        .to_string();
    assert_eq!(
        ids_first.split(',').count(),
        50,
        "first call is a full batch"
    );
    assert_eq!(
        ids_second.split(',').count(),
        10,
        "second call has the remainder"
    );

    // Ledger: 1 unit per call (2 calls).
    let (used, date) = quota::used(&db).await.expect("ledger");
    assert_eq!(used, 2);
    assert_eq!(date, quota::today_pt());

    // Field mapping: views are strings in the API, durations parsed.
    let rich = items
        .iter()
        .find(|i| i.video_id == "aaa111bbb22")
        .expect("item");
    assert_eq!(rich.view_count, Some(123_456));
    assert_eq!(rich.like_count, Some(7_890));
    assert_eq!(rich.duration_sec, Some(754));
    assert_eq!(rich.tags, vec!["rust", "database", "storage", "tutorial"]);
    assert_eq!(rich.channel_id.as_deref(), Some(CHANNEL_ID));
}

#[tokio::test]
async fn p1_quota_rollover_resets() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/videos"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("api_videos_list.json")))
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let db = Db::open(&dir.path().join("test.db"))
        .await
        .expect("open db");

    // Seed a stale bucket: 5 units used YESTERDAY (midnight PT rollover).
    let yesterday = (chrono::Utc::now() - chrono::Duration::days(1))
        .with_timezone(&chrono_tz::America::Los_Angeles)
        .format("%Y-%m-%d")
        .to_string();
    db.meta_set("quota_videos_list_used", "5")
        .await
        .expect("seed used");
    db.meta_set("quota_videos_list_date", &yesterday)
        .await
        .expect("seed date");

    let api = ApiClient::new(&clients(&mock), "test-key");
    api.fetch_videos(&db, &["aaa111bbb22".to_string()])
        .await
        .expect("fetch");

    let (used, date) = quota::used(&db).await.expect("ledger");
    assert_eq!(used, 1, "stale bucket resets before recording");
    assert_eq!(date, quota::today_pt());
}

#[tokio::test]
async fn p1_api_403_quota_error() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/videos"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error": {"code": 403, "message": "quota exceeded",
                    "errors": [{"reason": "quotaExceeded"}]}})),
        )
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let db = Db::open(&dir.path().join("test.db"))
        .await
        .expect("open db");
    let api = ApiClient::new(&clients(&mock), "test-key");

    let err = api
        .fetch_videos(&db, &["aaa111bbb22".to_string()])
        .await
        .expect_err("403 must error");
    assert!(
        matches!(
            err,
            TubeforgeError::Quota {
                endpoint: tubeforge::error::Endpoint::VideosList,
                ..
            }
        ),
        "403 quotaExceeded maps to the Quota error (exit 4), got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// §6.2 / §6.3 Ingest: idempotency, precedence, backup guard
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_ingest_channels_idempotent() {
    let mock = MockServer::start().await;
    mock_rss(&mock).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");

    let s1 = ingest::ingest_channels(
        &cfg,
        &clients(&mock),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &opts(false),
    )
    .await
    .expect("first ingest");
    assert_eq!(s1.channels_added, 1);
    assert_eq!(s1.videos_added, 3);
    assert!(
        s1.snapshot.is_some(),
        "first ingest backs up before writing"
    );

    // Run twice → identical state (LLD §12 idempotency).
    let s2 = ingest::ingest_channels(
        &cfg,
        &clients(&mock),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &opts(false),
    )
    .await
    .expect("second ingest");
    assert_eq!(s2.channels_added, 0);
    assert_eq!(s2.videos_added, 0);
    assert_eq!(s2.channels_skipped, 1);
    assert_eq!(s2.videos_skipped, 3);
    assert!(s2.snapshot.is_none(), "no changes → no backup");

    assert_eq!(db.count("SELECT count(*) FROM channels").await.unwrap(), 1);
    assert_eq!(db.count("SELECT count(*) FROM videos").await.unwrap(), 3);
    let row = db
        .get_video("aaa111bbb22")
        .await
        .expect("video")
        .expect("exists");
    assert_eq!(row.source, "rss");
    assert_eq!(row.title, "Rust Database Engineering Guide");
    assert_eq!(row.channel_id.as_deref(), Some(CHANNEL_ID));
    assert_eq!(row.view_count, Some(12345), "rss views");

    assert_eq!(
        snapshot_count(&cfg),
        1,
        "one snapshot total across both runs"
    );
}

#[tokio::test]
async fn p1_source_precedence_api_over_rss_never_downgrade() {
    let mock = MockServer::start().await;
    mock_rss(&mock).await;
    Mock::given(method("GET"))
        .and(path("/videos"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("api_videos_list.json")))
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");

    // RSS then API → API wins (rich data wins).
    let s = ingest::ingest_channels(
        &cfg,
        &clients(&mock),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &opts(true),
    )
    .await
    .expect("ingest with api");
    assert_eq!(s.api, "ok");
    assert_eq!(s.videos_added, 3);

    let row = db
        .get_video("aaa111bbb22")
        .await
        .expect("video")
        .expect("exists");
    assert_eq!(row.source, "api");
    assert_eq!(row.title, "Rust Database Engineering Guide (API)");
    assert_eq!(
        row.description,
        "Rich API description with details about storage engines and B-trees."
    );
    assert_eq!(row.duration_sec, Some(754));
    assert_eq!(row.view_count, Some(123_456));
    let tags: Vec<String> = serde_json::from_str(&row.tags).unwrap();
    assert_eq!(tags, vec!["rust", "database", "storage", "tutorial"]);

    // Now RSS carries a DIFFERENT title — the API data must NOT downgrade.
    let rss_v2 = fixture("rss_feed.xml").replace(
        "<title>Rust Database Engineering Guide</title>",
        "<title>RSS Only Title</title>",
    );
    Mock::given(method("GET"))
        .and(path("/feeds/videos.xml"))
        .and(query_param("channel_id", CHANNEL_ID))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(rss_v2)
                .insert_header("ETag", ETAG),
        )
        .mount(&mock)
        .await;

    let s2 = ingest::ingest_channels(
        &cfg,
        &clients(&mock),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &opts(false),
    )
    .await
    .expect("re-ingest rss only");
    assert_eq!(
        s2.videos_skipped, 3,
        "all rss rows lose precedence → skipped"
    );
    assert_eq!(s2.videos_updated, 0);

    let row = db
        .get_video("aaa111bbb22")
        .await
        .expect("video")
        .expect("exists");
    assert_eq!(
        row.title, "Rust Database Engineering Guide (API)",
        "api data retained"
    );
    assert_eq!(row.source, "api");
}

#[tokio::test]
async fn p1_oembed_link_ingest_placeholder_nullable() {
    let mock = MockServer::start().await;
    // Link 1: author URL carries @handle → placeholder channel "@examplechannel".
    Mock::given(method("GET"))
        .and(path("/oembed"))
        .and(query_param(
            "url",
            "https://www.youtube.com/watch?v=aaa111bbb22",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("oembed.json")))
        .mount(&mock)
        .await;
    // Link 2: legacy /channel/ URL → NO handle → video.channel_id stays NULL.
    Mock::given(method("GET"))
        .and(path("/oembed"))
        .and(query_param(
            "url",
            "https://www.youtube.com/watch?v=nohandle00000",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "type": "video", "version": "1.0",
            "title": "No Handle Video",
            "author_name": "Legacy Channel",
            "author_url": "https://www.youtube.com/channel/UCa1b2c3d4e5f6g7h8i9j0k",
            "provider_name": "YouTube",
            "thumbnail_url": "https://i.ytimg.com/vi/nohandle00000/hqdefault.jpg",
        })))
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");

    let s = ingest::ingest_links(
        &cfg,
        &clients(&mock),
        &mut db,
        &["aaa111bbb22".to_string(), "nohandle00000".to_string()],
        &opts(false),
    )
    .await
    .expect("ingest links");
    assert_eq!(s.videos_added, 2);

    // Placeholder channel keyed by @handle, source oembed.
    let ph = db
        .get_channel("@examplechannel")
        .await
        .expect("placeholder")
        .expect("exists");
    assert_eq!(ph.title, "Example Channel");
    assert_eq!(ph.source, "oembed");
    assert_eq!(ph.handle.as_deref(), Some("@examplechannel"));

    let with_handle = db
        .get_video("aaa111bbb22")
        .await
        .expect("v1")
        .expect("exists");
    assert_eq!(with_handle.channel_id.as_deref(), Some("@examplechannel"));
    assert_eq!(with_handle.source, "oembed");
    assert_eq!(with_handle.title, "Example Video Title");

    let no_handle = db
        .get_video("nohandle00000")
        .await
        .expect("v2")
        .expect("exists");
    assert_eq!(
        no_handle.channel_id, None,
        "no handle → nullable channel_id is NULL"
    );

    // No real UC... channel row was created for the placeholder.
    assert!(db.get_channel(CHANNEL_ID).await.unwrap().is_none());
}

#[tokio::test]
async fn p1_backup_guard_snapshot_no_backup_304() {
    let mock = MockServer::start().await;
    mock_rss(&mock).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");

    // 1. Ingest with writes → snapshot created.
    ingest::ingest_channels(
        &cfg,
        &clients(&mock),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &opts(false),
    )
    .await
    .expect("ingest");
    assert_eq!(snapshot_count(&cfg), 1, "write batch creates a snapshot");

    // 2. --no-backup ingest with writes → NO new snapshot.
    let s = ingest::ingest_channels(
        &cfg,
        &clients(&mock),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &IngestOptions {
            use_api: false,
            no_backup: true,
        },
    )
    .await
    .expect("ingest no-backup");
    assert_eq!(s.snapshot, None, "--no-backup reports no snapshot");
    assert_eq!(
        snapshot_count(&cfg),
        1,
        "no snapshot written under --no-backup"
    );

    // 3. refresh with 304 (ETag matched) → no writes → NO snapshot.
    Mock::given(method("GET"))
        .and(path("/feeds/videos.xml"))
        .and(query_param("channel_id", CHANNEL_ID))
        .and(header("If-None-Match", ETAG))
        .respond_with(ResponseTemplate::new(304))
        .mount(&mock)
        .await;
    // Re-register the 200 mock so non-matching refreshes still work elsewhere:
    mock_rss(&mock).await;

    let r = ingest::refresh_channels(&cfg, &clients(&mock), &mut db, &[], &opts(false))
        .await
        .expect("refresh");
    assert_eq!(r.channels_skipped, 1, "304 → skipped");
    assert_eq!(r.snapshot, None);
    assert_eq!(snapshot_count(&cfg), 1, "304 refresh creates no snapshot");
}

// ---------------------------------------------------------------------------
// §5.3 403 → fallback path through ingest (no crash)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_ingest_quota_fallback_keeps_rss_data() {
    let mock = MockServer::start().await;
    mock_rss(&mock).await;
    Mock::given(method("GET"))
        .and(path("/videos"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": 403, "message": "quota exceeded",
                      "errors": [{"reason": "quotaExceeded"}]}
        })))
        .mount(&mock)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");

    let s = ingest::ingest_channels(
        &cfg,
        &clients(&mock),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &opts(true),
    )
    .await
    .expect("ingest must not crash on quota");
    assert_eq!(s.api, "quota");
    assert_eq!(
        s.videos_added, 3,
        "RSS data kept despite API quota exhaustion"
    );
    assert_eq!(s.channels_added, 1);
    assert!(!s.alerts.is_empty(), "quota alert emitted");

    let n = db
        .count("SELECT count(*) FROM alerts")
        .await
        .expect("alerts");
    assert_eq!(n, 1);
    let row = db
        .get_video("aaa111bbb22")
        .await
        .expect("video")
        .expect("exists");
    assert_eq!(row.source, "rss");
}

// ---------------------------------------------------------------------------
// §3.2 / §9.2 reindex recovery
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_reindex_rebuilds_after_corruption() {
    let mock = MockServer::start().await;
    mock_rss(&mock).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");

    ingest::ingest_channels(
        &cfg,
        &clients(&mock),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &opts(false),
    )
    .await
    .expect("ingest");

    // Corrupt the index: nuke the whole dir.
    let idx = cfg.index_dir();
    assert!(idx.exists(), "ingest created the index");
    std::fs::remove_dir_all(&idx).expect("remove index");

    // Rebuild from the videos table (idempotent recovery path).
    let out = reindex::run(&cfg).await.expect("reindex");
    assert_eq!(out["docs"], 3);

    let bm25 = Bm25::open(open_or_create(&idx).expect("open")).expect("bm25");
    let score = bm25.corpus_resonance(FIELD_TITLE, "database", None);
    assert!(score > 0.0, "reindexed corpus answers BM25, got {score}");
    assert_eq!(bm25.num_docs(), 3);

    let meta = db.meta_get("last_reindex_at").await.expect("meta");
    assert!(meta.is_some(), "reindex stamps last_reindex_at");
}

// ---------------------------------------------------------------------------
// §7.2 basic score envelope
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_score_draft_envelope_shape() {
    let mock = MockServer::start().await;
    mock_rss(&mock).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = Db::open(&cfg.db_path).await.expect("open db");
    ingest::ingest_channels(
        &cfg,
        &clients(&mock),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &opts(false),
    )
    .await
    .expect("ingest");

    let out = score::run(
        &cfg,
        &score::ScoreInput {
            video_id: None,
            draft_title: Some("Rust Database Engineering Guide".to_string()),
            draft_desc: None,
            draft_tags: None,
        },
    )
    .await
    .expect("score");

    let seo = &out["seo"];
    for c in [
        "keyword_title",
        "keyword_desc",
        "keyword_tags",
        "title_length",
    ] {
        assert!(seo["components"][c].is_f64(), "component {c} present");
    }
    assert!(seo["total"].is_f64());
    let kt = seo["components"]["keyword_title"].as_f64().unwrap();
    assert!(kt > 0.0, "corpus contains the draft title terms: {kt}");
    assert_eq!(out["video_id"], serde_json::Value::Null);
}

// ---------------------------------------------------------------------------
// §9.3 migrations
// ---------------------------------------------------------------------------

/// All tables of LLD §3.1.
const ALL_TABLES: [&str; 11] = [
    "channels",
    "videos",
    "competitors",
    "keywords",
    "keyword_rankings",
    "scores",
    "ideas",
    "edges",
    "alerts",
    "ingest_log",
    "meta",
];

#[tokio::test]
async fn p1_migration_full_schema() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut db = Db::open(&dir.path().join("test.db"))
        .await
        .expect("open db");
    assert_eq!(
        db.user_version().await.expect("user_version"),
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

    let mut missing = Vec::new();
    for t in ALL_TABLES {
        if !db.table_exists(t).await.expect("table_exists") {
            missing.push(t);
        }
    }
    assert!(missing.is_empty(), "missing tables: {missing:?}");

    // Nullable FK: a video with NULL channel_id is legal (oEmbed path).
    let n = db
        .count("SELECT count(*) FROM videos WHERE channel_id IS NULL")
        .await
        .unwrap();
    assert_eq!(n, 0, "empty table");
    {
        let mut batch = db.begin_batch().await.expect("batch");
        batch
            .upsert_video(&VideoRow {
                video_id: "x1x1x1x1x1x".into(),
                channel_id: None,
                title: "t".into(),
                published_at: "2026-01-01T00:00:00Z".into(),
                fetched_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                source: "rss".into(),
                ..Default::default()
            })
            .await
            .expect("insert with NULL channel_id");
        batch.commit().await.expect("commit");
    }
    assert_eq!(db.count("SELECT count(*) FROM videos").await.unwrap(), 1);
}

/// Migration 002 (v1 → v2): the recordingDetails + topicDetails columns on
/// `videos` and the `topics` snapshot column on `keyword_rankings` exist on
/// a fresh database, and re-running `migrate()` (the second open of the same
/// file) applies nothing new — the version gate keeps 002 idempotent.
#[tokio::test]
async fn p1_migration_002_columns_and_idempotency() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("m002.db");

    // First open applies 001 + 002 + 003.
    let db = Db::open(&db_path).await.expect("open 1");
    assert_eq!(
        db.user_version().await.expect("version"),
        tubeforge::storage::schema::SCHEMA_VERSION
    );
    for c in [
        "recording_date",
        "recording_location_name",
        "recording_lat",
        "recording_lng",
        "topic_categories",
    ] {
        assert!(
            table_cols(&db, "videos").await.contains(&c.to_string()),
            "videos column {c} missing"
        );
    }
    assert!(
        table_cols(&db, "keyword_rankings")
            .await
            .contains(&"topics".to_string()),
        "keyword_rankings.topics missing"
    );

    // Second open = migrate() again: must not error and must not duplicate
    // columns (ALTER TABLE ADD COLUMN has no IF NOT EXISTS — the version gate
    // is what makes the migration idempotent-safe).
    let db2 = Db::open(&db_path).await.expect("open 2 (migrate re-apply)");
    assert_eq!(
        db2.user_version().await.expect("version"),
        tubeforge::storage::schema::SCHEMA_VERSION
    );
    assert!(table_cols(&db2, "videos")
        .await
        .contains(&"recording_date".to_string()));
    assert!(table_cols(&db2, "keyword_rankings")
        .await
        .contains(&"topics".to_string()));
    // Nullable columns: a minimal insert without the new columns still works.
    let mut db2 = db2;
    {
        let mut batch = db2.begin_batch().await.expect("batch");
        batch
            .upsert_video(&VideoRow {
                video_id: "y1y1y1y1y1y".into(),
                title: "t".into(),
                published_at: "2026-01-01T00:00:00Z".into(),
                fetched_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                source: "rss".into(),
                ..Default::default()
            })
            .await
            .expect("minimal insert with nullable new columns");
        batch.commit().await.expect("commit");
    }
    assert_eq!(db2.count("SELECT count(*) FROM videos").await.unwrap(), 1);
}

/// Column names of a table via the tfdb schema (the analog of the legacy
/// `PRAGMA table_info`).
async fn table_cols(db: &Db, table: &str) -> Vec<String> {
    db.columns(table).await.expect("columns")
}

/// A fresh tfdb open creates the full schema in one step (there are no
/// version-gated SQL migrations — the schema is always complete) and records
/// the current `SCHEMA_VERSION`; reopening is idempotent.
#[tokio::test]
async fn p1_migration_phase0_upgrade() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("phase0.db");

    // Open fresh: full schema + version recorded in one step.
    let db = Db::open(&db_path).await.expect("open fresh db");
    assert_eq!(
        db.user_version().await.unwrap(),
        tubeforge::storage::schema::SCHEMA_VERSION,
        "fresh open records the current schema version"
    );
    let mut missing = Vec::new();
    for t in ALL_TABLES {
        if !db.table_exists(t).await.expect("table_exists") {
            missing.push(t);
        }
    }
    assert!(missing.is_empty(), "full schema present: {missing:?}");
    assert_eq!(
        db.meta_get("schema_version").await.unwrap().as_deref(),
        Some(
            tubeforge::storage::schema::SCHEMA_VERSION
                .to_string()
                .as_str()
        )
    );
}
