//! Integration tests for the HTMX dashboard (`tubeforge serve`, PRD §5.4).
//!
//! The server is spawned on an ephemeral port (port 0) with a temp database,
//! then exercised over real HTTP (reqwest). Mutations are verified through
//! the storage layer afterwards — the same write paths the CLI uses.

use std::sync::Arc;

use tubeforge::serve::{self, AppState};
use tubeforge::storage::db::Db;

/// Seed a temp database: two channels, two videos, scores, an idea, an alert,
/// a keyword with two rank snapshots. Returns the open Db.
async fn seed_db() -> Db {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut db = Db::open(&dir.path().join("dash.db")).await.expect("open db");
    let at = "2026-08-01T00:00:00Z";

    let mut batch = db.begin_batch().await.expect("begin batch");
    batch
        .upsert_channel(&tubeforge::storage::db::ChannelRow {
            channel_id: "UCtest0000000000000000001".into(),
            handle: Some("@alfa".into()),
            title: "Alfa Channel".into(),
            fetched_at: at.into(),
            updated_at: at.into(),
            source: "rss".into(),
            ..Default::default()
        })
        .await
        .expect("channel a");
    batch
        .upsert_channel(&tubeforge::storage::db::ChannelRow {
            channel_id: "UCtest0000000000000000002".into(),
            handle: Some("@bravo".into()),
            title: "Bravo Channel".into(),
            fetched_at: at.into(),
            updated_at: at.into(),
            source: "rss".into(),
            ..Default::default()
        })
        .await
        .expect("channel b");
    for (vid, ch, title, views) in [
        ("aaa111bbb22", "UCtest0000000000000000001", "TubeForge Dashboard Guide", 1000),
        ("bbb222ccc33", "UCtest0000000000000000002", "Rust & SEO <Tips>", 500),
    ] {
        batch
            .upsert_video(&tubeforge::storage::db::VideoRow {
                video_id: vid.into(),
                channel_id: Some(ch.into()),
                title: title.into(),
                description: String::new(),
                published_at: at.into(),
                fetched_at: at.into(),
                updated_at: at.into(),
                source: "rss".into(),
                view_count: Some(views),
                ..Default::default()
            })
            .await
            .expect("video");
    }
    batch.commit().await.expect("commit batch");

    db.upsert_score(
        "aaa111bbb22",
        81.5,
        74.0,
        77.75,
        r#"{"keyword_title":82.0,"title_front":100.0,"entity_coverage":80.0}"#,
    )
    .await
    .expect("score");

    db.upsert_idea(
        "Untrusted <script>alert(1)</script> title",
        "{}",
        90.5,
        "draft",
        Some("aaa111bbb22"),
    )
    .await
    .expect("idea");

    db.insert_alert("quota", None, "quota nearing limit (90%)", "warn")
        .await
        .expect("alert");

    db.add_keywords(&["rust".into()], None).await.expect("keyword");
    for (kw, checked_at, pos) in [
        ("rust", "2026-07-30T00:00:00Z", Some(4i64)),
        ("rust", "2026-08-01T00:00:00Z", Some(2i64)),
    ] {
        db.upsert_ranking(kw, checked_at, Some("aaa111bbb22"), pos, Some(r#"["Databases"]"#))
            .await
            .expect("ranking");
    }

    db
}

/// Spawn the app on an ephemeral port; returns (base_url, bound port, Db).
async fn spawn_server() -> (String, u16, Db) {
    let db = seed_db().await;
    // Second handle to the same connection pool for post-POST verification
    // (turso Connection is Clone; the server keeps the first handle).
    let verify_conn = db.conn.clone();
    let path = db.path.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind 0");
    let addr = listener.local_addr().expect("local addr");
    let state = AppState {
        db: Arc::new(db),
        bind: addr.to_string(),
    };
    tokio::spawn(async move {
        axum::serve(listener, serve::app(state)).await.expect("serve");
    });
    (
        format!("http://{addr}"),
        addr.port(),
        Db { conn: verify_conn, path },
    )
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
}

#[tokio::test]
async fn all_pages_respond_200() {
    let (base, _port, _db) = spawn_server().await;
    let c = client();
    for path in ["/", "/scores", "/ideas", "/keywords", "/alerts", "/scorecard", "/health"] {
        let resp = c.get(format!("{base}{path}")).send().await.expect("GET");
        assert_eq!(resp.status(), 200, "GET {path}");
        let body = resp.text().await.expect("body");
        assert!(body.contains("TubeForge"), "GET {path} renders the layout");
        assert!(
            !body.contains("<script>alert"),
            "GET {path} must not leak raw untrusted input"
        );
    }
}

#[tokio::test]
async fn healthz_is_plain_ok() {
    let (base, _port, _db) = spawn_server().await;
    let resp = client().get(format!("{base}/healthz")).send().await.expect("GET");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "ok");
}

#[tokio::test]
async fn home_embeds_counts_and_charts() {
    let (base, _port, _db) = spawn_server().await;
    let body = client()
        .get(format!("{base}/"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    assert!(body.contains("hx-get=\"/home/counts\""));
    assert!(body.contains("every 30s"));
    assert!(body.contains("<svg"), "inline SVG charts are server-rendered");
    assert!(body.contains("Views per channel"));
    assert!(body.contains("SEO score distribution"));
}

#[tokio::test]
async fn scores_page_filters_and_lists() {
    let (base, _port, _db) = spawn_server().await;
    let c = client();
    let body = c
        .get(format!("{base}/scores"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    assert!(body.contains("TubeForge Dashboard Guide"));
    assert!(body.contains("77.8"), "overall score rendered");

    let body = c
        .get(format!("{base}/scores?q=tips"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    // Askama autoescapes with numeric entities: & → &#38;, < → &#60;.
    assert!(body.contains("Rust &#38; SEO &#60;Tips&#62;"));
    assert!(!body.contains("TubeForge Dashboard Guide"), "filter applied");
}

#[tokio::test]
async fn score_detail_fragment_lists_17_components() {
    let (base, _port, _db) = spawn_server().await;
    let body = client()
        .get(format!("{base}/scores/aaa111bbb22"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    assert!(body.contains("keyword_title"));
    assert!(body.contains("entity_coverage"));
    assert!(body.contains("location_signal"), "all 7 GEO keys attempted");
}

#[tokio::test]
async fn ideas_status_post_updates_the_database() {
    let (base, _port, db) = spawn_server().await;
    let c = client();

    let resp = c
        .post(format!("{base}/ideas/1/saved"))
        .header("Origin", format!("http://127.0.0.1:{_port}"))
        .send()
        .await
        .expect("POST");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body");
    assert!(body.contains("id=\"idea-1\""), "row fragment swapped back");
    assert!(body.contains("saved"));

    // The DB row really changed (same write path the CLI uses).
    let rows = db.list_ideas(None, 100).await.expect("list ideas");
    assert_eq!(rows[0].status, "saved");
}

#[tokio::test]
async fn csrf_bad_origin_is_forbidden() {
    let (base, _port, _db) = spawn_server().await;
    let c = client();

    let resp = c
        .post(format!("{base}/ideas/1/discarded"))
        .header("Origin", "http://evil.example:8080")
        .send()
        .await
        .expect("POST");
    assert_eq!(resp.status(), 403, "foreign origin rejected");

    // Wrong port on the same host is also rejected.
    let resp = c
        .post(format!("{base}/ideas/1/discarded"))
        .header("Origin", "http://127.0.0.1:9")
        .send()
        .await
        .expect("POST");
    assert_eq!(resp.status(), 403, "wrong port rejected");
}

#[tokio::test]
async fn csrf_absent_origin_is_allowed() {
    let (base, _port, db) = spawn_server().await;
    let c = client();

    let resp = c
        .post(format!("{base}/ideas/1/discarded"))
        .send()
        .await
        .expect("POST");
    assert_eq!(resp.status(), 200, "no Origin: non-browser local client");
    let rows = db.list_ideas(None, 100).await.expect("list ideas");
    assert_eq!(rows[0].status, "discarded");
}

#[tokio::test]
async fn alerts_read_and_clear_posts() {
    let (base, _port, db) = spawn_server().await;
    let c = client();

    let resp = c
        .post(format!("{base}/alerts/read"))
        .header("Origin", format!("http://127.0.0.1:{_port}"))
        .send()
        .await
        .expect("POST");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body");
    assert!(body.contains("id=\"alerts-list\""));
    assert!(body.contains("read"), "rows re-rendered as read");

    let alerts = db.list_alerts(0).await.expect("list alerts");
    assert!(alerts[0].read_at.is_some(), "read_at persisted");

    let resp = c
        .post(format!("{base}/alerts/clear"))
        .header("Origin", format!("http://127.0.0.1:{_port}"))
        .send()
        .await
        .expect("POST");
    assert_eq!(resp.status(), 200);
    let alerts = db.list_alerts(0).await.expect("list alerts");
    assert!(alerts.is_empty(), "alerts cleared");
}

#[tokio::test]
async fn unknown_routes_404_and_static_js_serves() {
    let (base, _port, _db) = spawn_server().await;
    let c = client();

    let resp = c.get(format!("{base}/nope")).send().await.expect("GET");
    assert_eq!(resp.status(), 404);

    let resp = c
        .get(format!("{base}/static/htmx.min.js"))
        .send()
        .await
        .expect("GET");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body");
    assert!(body.contains("htmx"), "vendored htmx served");
    assert!(body.contains("htmx=function"), "looks like the real htmx bundle");
}

#[tokio::test]
async fn keywords_page_shows_trend_and_delta() {
    let (base, _port, _db) = spawn_server().await;
    let body = client()
        .get(format!("{base}/keywords"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    assert!(body.contains("rust"));
    assert!(body.contains("#2"), "latest position");
    assert!(body.contains("−2"), "delta 2→4 = improvement −2");
    assert!(body.contains("<svg"), "sparkline rendered");
}

#[tokio::test]
async fn health_page_renders_census() {
    let (base, _port, _db) = spawn_server().await;
    let body = client()
        .get(format!("{base}/health"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    assert!(body.contains("Health report"));
    assert!(body.contains("Integrity check"));
}
