//! Phase 2 integration tests (LLD §12): scoring persistence round-trip,
//! ideas generation + status marking, keyword check → snapshot → trend,
//! scorecard/health/alerts end-to-end against a temp DB. Wiremock fixtures
//! only — no real network.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use serde_json::json;
use tubeforge::analytics::keywords as akeywords;
use tubeforge::cli::AlertsAction;
use tubeforge::commands::{alerts, health, ideas, score, scorecard};
use tubeforge::config::Config;
use tubeforge::ingest::{self, IngestOptions};
use tubeforge::search::bm25::Bm25;
use tubeforge::search::open_or_create;
use tubeforge::storage::Db;

use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CHANNEL_ID: &str = "UCa1b2c3d4e5f6g7h8i9j0kLM";

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
        ytdlp_path: "yt-dlp".into(),
        ytdlp_enabled: false,
        ytdlp_client: None,
        ytdlp_js_runtime: None,
        own_channel: None,
        niche_terms: Vec::new(),
    }
}

/// Register the standard RSS feed mock (200 with ETag).
async fn mock_rss(mock: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/feeds/videos.xml"))
        .and(query_param("channel_id", CHANNEL_ID))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(fixture("rss_feed.xml"))
                .insert_header("ETag", "W/\"abc123\""),
        )
        .mount(mock)
        .await;
}

/// Ingest the 3-video fixture channel (RSS, no API).
async fn seed_channel(cfg: &Config) -> Db {
    let mock = MockServer::start().await;
    mock_rss(&mock).await;
    let mut db = Db::open(&cfg.db_path).await.expect("open db");
    ingest::ingest_channels(
        cfg,
        &tubeforge::fetch::FetchClients::for_test(&mock.uri(), Duration::from_secs(5))
            .expect("clients"),
        &mut db,
        &[CHANNEL_ID.to_string()],
        &IngestOptions {
            use_api: false,
            no_backup: false,
        },
    )
    .await
    .expect("ingest");
    db
}

fn bm25(cfg: &Config) -> Bm25 {
    Bm25::open(open_or_create(&cfg.index_dir()).expect("index")).expect("bm25")
}

// ---------------------------------------------------------------------------
// §7 scoring: persistence round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_score_persistence_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let db = seed_channel(&cfg).await;

    // Ingest recomputed scores for the 3 changed videos (LLD §6.4).
    assert_eq!(db.count("SELECT count(*) FROM scores").await.unwrap(), 3);
    let row = db
        .get_score("aaa111bbb22")
        .await
        .expect("score")
        .expect("exists");
    assert!(row.seo_score > 0.0 && row.seo_score <= 100.0);
    assert!(row.geo_score >= 0.0 && row.geo_score <= 100.0);
    let comp: serde_json::Value = serde_json::from_str(&row.components).expect("components json");
    assert!(comp.get("keyword_title").is_some(), "seo key persisted");
    assert!(comp.get("entity_coverage").is_some(), "geo key persisted");

    // `score --video-id` returns the full envelope shape (LLD §4.2).
    let out = score::run(
        &cfg,
        &score::ScoreInput {
            video_id: Some("aaa111bbb22".to_string()),
            draft_title: None,
            draft_desc: None,
            draft_tags: None,
        },
    )
    .await
    .expect("score stored video");
    assert_eq!(out["video_id"], "aaa111bbb22");
    assert!(out["seo"]["total"].is_f64());
    assert!(out["geo"]["total"].is_f64());
    assert!(out["total"].is_f64());
    for c in [
        "keyword_title",
        "title_front",
        "title_length",
        "title_hooks",
        "keyword_desc",
        "desc_first150",
        "desc_structure",
        "tags_relevance",
        "tags_quality",
        "keyword_tags",
    ] {
        assert!(out["seo"]["components"][c].is_f64(), "seo component {c}");
    }
    for c in [
        "entity_coverage",
        "qa_phrasing",
        "list_phrasing",
        "conversational",
        "metadata_complete",
    ] {
        assert!(out["geo"]["components"][c].is_f64(), "geo component {c}");
    }

    // Draft flow still works and still beats the corpus on its own terms.
    let draft = score::run(
        &cfg,
        &score::ScoreInput {
            video_id: None,
            draft_title: Some("Rust Database Engineering Guide".to_string()),
            draft_desc: Some(
                "What is a database? How to build one in Rust. 0:00 intro\n- bullet".to_string(),
            ),
            draft_tags: Some("rust,database,tutorial".to_string()),
        },
    )
    .await
    .expect("score draft");
    assert_eq!(draft["video_id"], serde_json::Value::Null);
    assert!(
        draft["seo"]["components"]["keyword_title"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    assert!(
        draft["geo"]["components"]["entity_coverage"]
            .as_f64()
            .unwrap()
            > 0.0
    );
}

// ---------------------------------------------------------------------------
// §8.2 ideas: generation → status save
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_ideas_generation_and_status_save() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let db = seed_channel(&cfg).await;

    db.add_keywords(&["rust".to_string(), "database".to_string()], None)
        .await
        .expect("add keywords");

    // Generation persists drafts, ranked by score.
    let out = ideas::run(&cfg, 5, Some("rust programming"), None)
        .await
        .expect("ideas draft");
    let rows = out["ideas"].as_array().expect("ideas array");
    assert!(!rows.is_empty(), "candidate pool generated");
    for r in rows {
        assert_eq!(r["status"], "draft");
        assert!(r["rationale"].get("seo_total").is_some(), "rationale json");
    }

    // `--status saved` marks the pool (LLD §8.2 status marking).
    let out = ideas::run(&cfg, 5, None, Some("saved"))
        .await
        .expect("ideas saved");
    for r in out["ideas"].as_array().expect("ideas array") {
        assert_eq!(r["status"], "saved");
    }
    let n = db
        .count("SELECT count(*) FROM ideas WHERE status = 'saved'")
        .await
        .unwrap();
    assert_eq!(n as usize, out["ideas"].as_array().unwrap().len());

    // Invalid status is a Usage error.
    let err = ideas::run(&cfg, 5, None, Some("bogus"))
        .await
        .expect_err("bogus status");
    assert!(matches!(err, tubeforge::error::TubeforgeError::Usage(_)));
}

// ---------------------------------------------------------------------------
// §8.3 keywords: check → snapshot → trend
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_keywords_check_snapshot_trend() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let db = seed_channel(&cfg).await;

    db.add_keywords(&["database".to_string()], None)
        .await
        .expect("add");

    let videos = db.all_videos().await.expect("videos");
    let own: HashSet<String> = HashSet::new(); // no competitors yet → all videos own

    // Check 1: own video "Rust Database Engineering Guide" tops the corpus.
    let n = akeywords::check(&db, &bm25(&cfg), &videos, &own)
        .await
        .expect("check 1");
    assert_eq!(n, 1);
    let ranks = db.list_rankings().await.expect("rankings");
    assert_eq!(ranks.len(), 1);
    assert_eq!(ranks[0].position, Some(1));
    assert_eq!(ranks[0].video_id.as_deref(), Some("aaa111bbb22"));

    // The fixture channel becomes a competitor → no own videos remain.
    db.register_competitors(&[CHANNEL_ID.to_string()], "Fixture")
        .await
        .expect("competitor");

    // Check 2 (≥1s later — the snapshot PK is (keyword, checked_at)).
    tokio::time::sleep(Duration::from_secs(1)).await;
    let comp: HashSet<String> = db
        .list_competitors()
        .await
        .expect("competitors")
        .into_iter()
        .collect();
    akeywords::check(&db, &bm25(&cfg), &videos, &comp)
        .await
        .expect("check 2");

    // Trend: 2 snapshots; the newest is unranked (NULL position) → delta NULL.
    let report = akeywords::report(&db).await.expect("report");
    let rows = report["keywords"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    let t = &rows[0];
    assert_eq!(t["keyword"], "database");
    assert_eq!(t["snapshots"].as_array().unwrap().len(), 2);
    assert_eq!(t["previous_position"], 1);
    assert_eq!(t["latest_position"], serde_json::Value::Null);
    assert_eq!(t["delta"], serde_json::Value::Null, "unranked → no delta");
}

/// C2 keyword-rank dimension: a video row carrying `topic_categories`
/// (JSON array of category URLs) snapshots its derived labels into the
/// `keyword_rankings.topics` column and surfaces them in the report trend
/// JSON as a proper array.
#[tokio::test]
async fn p2_keyword_snapshot_carries_topics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let mut db = seed_channel(&cfg).await;
    db.add_keywords(&["database".to_string()], None)
        .await
        .expect("add");

    // Enrich the winning video with topic categories (C2) directly through
    // the repository write path.
    let videos = db.all_videos().await.expect("videos");
    let winning = videos
        .iter()
        .find(|v| v.video_id == "aaa111bbb22")
        .expect("winning video");
    let mut enriched = winning.clone();
    enriched.topic_categories = serde_json::to_string(&[
        "https://en.wikipedia.org/wiki/Artificial_intelligence".to_string(),
        "https://en.wikipedia.org/wiki/Deep_learning".to_string(),
    ])
    .expect("topics json");
    let mut batch = db.begin_batch().await.expect("batch");
    batch.upsert_video(&enriched).await.expect("upsert");
    batch.commit().await.expect("commit");

    let videos = db.all_videos().await.expect("videos");
    let own: HashSet<String> = HashSet::new();
    akeywords::check(&db, &bm25(&cfg), &videos, &own)
        .await
        .expect("check");

    // Snapshot row carries the derived labels (last URL segment, _ → space).
    let ranks = db.list_rankings().await.expect("rankings");
    assert_eq!(ranks.len(), 1);
    let topics: Vec<String> =
        serde_json::from_str(ranks[0].topics.as_deref().expect("topics")).expect("topics json");
    assert_eq!(topics, vec!["Artificial intelligence", "Deep learning"]);

    // Report trend JSON surfaces them as an array.
    let report = akeywords::report(&db).await.expect("report");
    assert_eq!(
        report["keywords"][0]["snapshots"][0]["topics"],
        json!(["Artificial intelligence", "Deep learning"])
    );
}

// ---------------------------------------------------------------------------
// §8.4 scorecard / health / alerts end-to-end
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_scorecard_health_alerts_end_to_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let db = seed_channel(&cfg).await;

    // Make the fixture channel a competitor (scorecard default set + the
    // "new competitor detected" alert rule).
    db.register_competitors(&[CHANNEL_ID.to_string()], "Fixture")
        .await
        .expect("competitor");
    // A tracked keyword that no competitor title contains → brand alert.
    db.add_keywords(&["nonexistentkeyword".to_string()], None)
        .await
        .expect("kw");

    // ---- scorecard ----
    let sc = scorecard::run(&cfg, &[]).await.expect("scorecard");
    let channels = sc["channels"].as_array().expect("channels");
    assert_eq!(channels.len(), 1);
    let c = &channels[0];
    assert_eq!(c["videos"], 3);
    assert!(c["total_views"].is_i64());
    assert!(c["centrality"].is_f64());
    assert_eq!(c["seo"]["scored"], 3, "scores computed at ingest");
    assert!(sc["median"]["seo_avg"].is_f64());
    assert_eq!(sc["compared"], 1);

    // ---- health ----
    let h = health::run(&cfg).await.expect("health");
    assert_eq!(h["counts"]["videos"], 3);
    assert_eq!(h["counts"]["channels"], 1);
    assert_eq!(h["integrity"], "ok");
    assert_eq!(h["quota"]["daily_limit"], 10_000);
    assert_eq!(h["stale_channels"].as_array().unwrap().len(), 0);
    assert_eq!(h["index"]["fresh"], true, "ingest stamped the index");
    assert!(h["last_ingest"].get("at").is_some());

    // ---- alerts: rules fire once, then stay idempotent ----
    let a1 = alerts::run(&cfg, None, false).await.expect("alerts run");
    let inserted = a1["inserted"].as_u64().expect("inserted");
    assert!(
        inserted >= 2,
        "brand + new-competitor alerts fired, got {inserted}"
    );
    let kinds: HashSet<String> = a1["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["kind"].as_str().map(String::from))
        .collect();
    assert!(kinds.contains("brand"), "brand rule fired: {kinds:?}");
    assert!(
        kinds.contains("gap"),
        "new-competitor rule fired: {kinds:?}"
    );

    // Re-running evaluates no NEW alerts (dedupe by kind+channel+message).
    let a2 = alerts::run(&cfg, None, false).await.expect("alerts rerun");
    assert_eq!(a2["inserted"], 0, "idempotent rule evaluation");

    // `alerts list` never evaluates.
    let l = alerts::run(&cfg, Some(AlertsAction::List), false)
        .await
        .expect("alerts list");
    assert_eq!(l["inserted"], 0);
    assert_eq!(
        l["alerts"].as_array().unwrap().len(),
        a1["alerts"].as_array().unwrap().len()
    );

    // `--mark-read` stamps read_at; `alerts clear` empties the table.
    let m = alerts::run(&cfg, None, true).await.expect("mark read");
    assert!(m["marked_read"].as_u64().unwrap() >= 2);
    let c2 = alerts::run(&cfg, Some(AlertsAction::Clear), false)
        .await
        .expect("clear");
    assert_eq!(
        c2["cleared"].as_u64().unwrap(),
        a1["alerts"].as_array().unwrap().len() as u64
    );
    let empty = alerts::run(&cfg, Some(AlertsAction::List), false)
        .await
        .expect("list empty");
    assert!(empty["alerts"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Envelope shape (LLD §4.2) — the machine contract agents rely on.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// §6.6 `register_competitors` (research discover): idempotent INSERT OR IGNORE
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_register_competitors_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let db = Db::open(&cfg.db_path).await.expect("open db");

    // First registration adds both.
    let added = db
        .register_competitors(
            &[
                CHANNEL_ID.to_string(),
                "UCx5XG1OV2P6uZZ5FSM9Ttw".to_string(),
            ],
            "discover:rust",
        )
        .await
        .expect("register");
    assert_eq!(added, 2);

    // Re-registering the same ids (as discover re-runs would) adds 0.
    let again = db
        .register_competitors(
            &[
                CHANNEL_ID.to_string(),
                "UCx5XG1OV2P6uZZ5FSM9Ttw".to_string(),
            ],
            "discover:rust",
        )
        .await
        .expect("register again");
    assert_eq!(again, 0);

    // list_competitors sees them exactly once each; blank ids are skipped.
    let _ = db
        .register_competitors(&["   ".to_string(), "".to_string()], "skip")
        .await
        .expect("blank skip");
    let comps = db.list_competitors().await.expect("list");
    assert_eq!(comps.len(), 2);
    assert!(comps.contains(&CHANNEL_ID.to_string()));
}

#[test]
fn p2_envelope_shape() {
    let ok = tubeforge::output::Envelope::ok(
        json!({"seo": {"total": 42.0}}),
        Some(tubeforge::output::meta(7, None)),
    );
    let v = ok.to_json();
    assert_eq!(v["ok"], true);
    assert!(v.get("data").is_some());
    assert_eq!(v["meta"]["duration_ms"], 7);
    assert!(v.get("error").is_none());

    let err = tubeforge::output::Envelope::error(&tubeforge::error::TubeforgeError::Quota {
        endpoint: tubeforge::error::Endpoint::VideosList,
        remaining: 0,
    });
    let v = err.to_json();
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "QUOTA_EXHAUSTED");
    assert!(v.get("data").is_none());
}
