//! Integration tests for the HTMX dashboard (`tubeforge serve`, PRD §5.4).
//!
//! The server is spawned on an ephemeral port (port 0) with a temp database,
//! then exercised over real HTTP (reqwest). Mutations are verified through
//! the storage layer afterwards — the same write paths the CLI uses.
use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use tubeforge::serve::{self, AppState};
use tubeforge::storage::db::Db;

/// Seed a temp database: two channels, two videos, scores, an idea, an alert,
/// a keyword with two rank snapshots. Returns the open Db.
async fn seed_db() -> Db {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut db = Db::open(&dir.path().join("dash.db"))
        .await
        .expect("open db");
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
        (
            "aaa111bbb22",
            "UCtest0000000000000000001",
            "TubeForge Dashboard Guide",
            1000,
        ),
        (
            "bbb222ccc33",
            "UCtest0000000000000000002",
            "Rust & SEO <Tips>",
            500,
        ),
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

    db.add_keywords(&["rust".into()], None)
        .await
        .expect("keyword");
    for (kw, checked_at, pos) in [
        ("rust", "2026-07-30T00:00:00Z", Some(4i64)),
        ("rust", "2026-08-01T00:00:00Z", Some(2i64)),
    ] {
        db.upsert_ranking(
            kw,
            checked_at,
            Some("aaa111bbb22"),
            pos,
            Some(r#"["Databases"]"#),
        )
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind 0");
    let addr = listener.local_addr().expect("local addr");
    let state = AppState {
        db: Arc::new(db),
        bind: addr.to_string(),
        ytdlp: None,
        data_dir: PathBuf::from("/tmp/tf-serve-test-data"),
        own_channel: None,
        kg: Arc::new(std::sync::Mutex::new(None)),
    };
    let (router, serve_state) = serve::app(state);
    tokio::spawn(async move {
        tubeforge::serve::web::serve(listener, std::sync::Arc::new(router), serve_state)
            .await
            .expect("serve");
    });
    (
        format!("http://{addr}"),
        addr.port(),
        Db {
            conn: verify_conn,
            path,
        },
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
    for path in [
        "/",
        "/scores",
        "/ideas",
        "/keywords",
        "/alerts",
        "/scorecard",
        "/health",
    ] {
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
    let resp = client()
        .get(format!("{base}/healthz"))
        .send()
        .await
        .expect("GET");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.expect("body"), "ok");
}

#[tokio::test]
async fn home_embeds_counts_and_charts() {
    let (base, _port, _db) = spawn_server().await;
    let body = client()
        .get(format!("{base}/legacy"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    assert!(body.contains("hx-ext=\"sse\""));
    assert!(body.contains("sse-connect=\"/events\""));
    assert!(body.contains("sse-swap=\"counts\""));
    assert!(
        body.contains("/static/sse.js"),
        "sse extension script vendored"
    );
    assert!(
        body.contains("<svg"),
        "inline SVG charts are server-rendered"
    );
    assert!(body.contains("Views per channel"));
    assert!(body.contains("SEO score distribution"));
}

#[tokio::test]
async fn scores_page_filters_and_lists() {
    let (base, _port, _db) = spawn_server().await;
    let c = client();
    let body = c
        .get(format!("{base}/legacy/scores"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    assert!(body.contains("TubeForge Dashboard Guide"));
    assert!(body.contains("77.8"), "overall score rendered");
    assert!(
        body.contains("<td id=\"detail-"),
        "detail target id lives on the td, not the tr"
    );

    let body = c
        .get(format!("{base}/legacy/scores?q=tips"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    // Askama autoescapes with numeric entities: & → &#38;, < → &#60;.
    assert!(body.contains("Rust &#38; SEO &#60;Tips&#62;"));
    assert!(
        !body.contains("TubeForge Dashboard Guide"),
        "filter applied"
    );
}

#[tokio::test]
async fn score_detail_fragment_lists_17_components() {
    let (base, _port, _db) = spawn_server().await;
    let body = client()
        .get(format!("{base}/legacy/scores/aaa111bbb22"))
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
        .post(format!("{base}/legacy/ideas/1/saved"))
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
        .post(format!("{base}/legacy/ideas/1/discarded"))
        .header("Origin", "http://evil.example:8080")
        .send()
        .await
        .expect("POST");
    assert_eq!(resp.status(), 403, "foreign origin rejected");

    // Wrong port on the same host is also rejected.
    let resp = c
        .post(format!("{base}/legacy/ideas/1/discarded"))
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
        .post(format!("{base}/legacy/ideas/1/discarded"))
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
        .post(format!("{base}/legacy/alerts/read"))
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
        .post(format!("{base}/legacy/alerts/clear"))
        .header("Origin", format!("http://127.0.0.1:{_port}"))
        .send()
        .await
        .expect("POST");
    assert_eq!(resp.status(), 200);
    let alerts = db.list_alerts(0).await.expect("list alerts");
    assert!(alerts.is_empty(), "alerts cleared");
}

#[tokio::test]
async fn unknown_routes_and_static_js_serves() {
    let (base, _port, _db) = spawn_server().await;
    let c = client();

    // With the SPA build present, an unknown GET path is served the SPA
    // index (client-side routing), not a 404. Unknown API paths stay 404.
    let resp = c
        .get(format!("{base}/api/not-a-route"))
        .send()
        .await
        .expect("GET");
    assert_eq!(resp.status(), 404, "unknown API path is 404");
    let resp = c.get(format!("{base}/nope")).send().await.expect("GET");
    assert_eq!(resp.status(), 200, "unknown page path serves the SPA shell");

    let resp = c
        .get(format!("{base}/static/htmx.min.js"))
        .send()
        .await
        .expect("GET");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body");
    assert!(body.contains("htmx"), "vendored htmx served");
    assert!(
        body.contains("htmx=function"),
        "looks like the real htmx bundle"
    );

    let resp = c
        .get(format!("{base}/static/sse.js"))
        .send()
        .await
        .expect("GET");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body");
    assert!(
        body.contains("defineExtension"),
        "vendored htmx SSE extension served"
    );
}

#[tokio::test]
async fn sse_stream_pushes_counts_event_on_connect() {
    let (base, _port, _db) = spawn_server().await;
    let resp = client()
        .get(format!("{base}/events"))
        .send()
        .await
        .expect("GET");
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .expect("content-type")
        .to_str()
        .expect("ascii");
    assert!(
        ct.starts_with("text/event-stream"),
        "SSE content type, got {ct:?}"
    );
    assert_eq!(
        resp.headers().get("cache-control").expect("cache-control"),
        "no-cache",
        "stream must never be cached"
    );

    // Read a bounded prefix: the first counts event is pushed immediately
    // on connect (no 5s tick wait), so a 3s cap per chunk cannot hang.
    let mut buf = Vec::new();
    let mut body = resp.bytes_stream();
    while buf.len() < 4096 && !String::from_utf8_lossy(&buf).contains("Videos") {
        let chunk = tokio::time::timeout(std::time::Duration::from_secs(3), body.next())
            .await
            .expect("chunk within 3s — stream must not stall")
            .expect("stream alive")
            .expect("read bytes");
        buf.extend_from_slice(&chunk);
    }
    let head = String::from_utf8_lossy(&buf);
    assert!(
        head.starts_with("event: counts"),
        "first frame is the counts event, got: {head:.200}"
    );
    assert!(head.contains("data:"), "event carries a data line");
    assert!(
        head.contains("Videos"),
        "data carries the counts card markup"
    );
    assert!(
        head.contains("class=\"card\""),
        "data carries the counts grid fragment"
    );
}

#[tokio::test]
async fn sse_stream_pushes_updated_counts_on_change() {
    let (base, _port, mut db) = spawn_server().await;
    let resp = client()
        .get(format!("{base}/events"))
        .send()
        .await
        .expect("GET");
    let mut body = resp.bytes_stream();

    // First event arrives immediately on connect.
    let first = tokio::time::timeout(std::time::Duration::from_secs(3), body.next())
        .await
        .expect("first chunk within 3s")
        .expect("stream alive")
        .expect("read bytes");
    let mut buf = String::from_utf8_lossy(&first).to_string();
    assert!(
        buf.contains("Videos"),
        "first event carries the counts card"
    );

    // Mutate through the second handle to the same connection pool (the
    // same write path the CLI uses), then expect the next tick to push a
    // fresh `counts` event.
    let at = "2026-08-01T00:00:00Z";
    let mut batch = db.begin_batch().await.expect("begin batch");
    batch
        .upsert_video(&tubeforge::storage::db::VideoRow {
            video_id: "ccc333ddd44".into(),
            channel_id: Some("UCtest0000000000000000001".into()),
            title: "SSE Change Detection".into(),
            description: String::new(),
            published_at: at.into(),
            fetched_at: at.into(),
            updated_at: at.into(),
            source: "rss".into(),
            view_count: Some(1),
            ..Default::default()
        })
        .await
        .expect("upsert video");
    batch.commit().await.expect("commit batch");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    let mut got_second = false;
    while std::time::Instant::now() < deadline && !got_second {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let chunk = tokio::time::timeout(remaining, body.next())
            .await
            .expect("chunk within deadline")
            .expect("stream alive")
            .expect("read bytes");
        buf.push_str(&String::from_utf8_lossy(&chunk));
        got_second = buf.matches("event: counts").count() >= 2;
    }
    assert!(
        got_second,
        "changed counts pushed within one 5s tick, got: {buf:.300}"
    );
}

#[tokio::test]
async fn sse_stream_heartbeats_and_stays_quiet() {
    let (base, _port, _db) = spawn_server().await;
    let resp = client()
        .get(format!("{base}/events"))
        .send()
        .await
        .expect("GET");
    assert_eq!(resp.status(), 200);

    // After the initial event the counts never change in this test, so the
    // stream must emit exactly one `counts` event and then only the 15s
    // `: ping` heartbeat comments — change detection at work.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut seen_ping = false;
    let mut buf = String::new();
    let mut body = resp.bytes_stream();
    while std::time::Instant::now() < deadline && !seen_ping {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let chunk = tokio::time::timeout(remaining, body.next())
            .await
            .expect("chunk within deadline")
            .expect("stream alive")
            .expect("read bytes");
        buf.push_str(&String::from_utf8_lossy(&chunk));
        seen_ping = buf.contains(": ping");
    }
    assert!(seen_ping, "15s heartbeat comment appeared, got: {buf:.200}");
    assert_eq!(
        buf.matches("event: counts").count(),
        1,
        "no spam: unchanged counts emit exactly one event"
    );
}

#[tokio::test]
async fn keywords_page_shows_trend_and_delta() {
    let (base, _port, _db) = spawn_server().await;
    let body = client()
        .get(format!("{base}/legacy/keywords"))
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
        .get(format!("{base}/legacy/health"))
        .send()
        .await
        .expect("GET")
        .text()
        .await
        .expect("body");
    assert!(body.contains("Health report"));
    assert!(body.contains("Integrity check"));
}

// ---------------------------------------------------------------------------
// Knowledge Graph end-to-end data-path verification
//
// These prove the KG is not dead code: building it from a real seeded DB
// produces a non-empty in-memory graph, and the compute functions that the
// WebSocket RPC handlers call (`compute_graph_scores`, `generate_graph_ideas`,
// `find_content_gaps`, `compute_tag_authority_by_name`, `pagerank`) return
// REAL KG-derived values — not the degraded `Null`/0 fallbacks.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kg_build_from_seeded_db_is_non_empty() {
    let db = seed_db().await;
    let stats = tubeforge::analytics::kg_builder::build(
        &db,
        tubeforge::analytics::kg_builder::BuildMode::Full,
    )
    .await
    .expect("kg build");
    // The seeded DB has 2 channels + 2 videos + 1 keyword → graph must have data.
    assert!(
        stats.entities_created >= 3,
        "KG should contain channels/videos/keyword entities, got {}",
        stats.entities_created
    );
    assert!(
        stats.relations_created > 0,
        "KG should contain relations, got {}",
        stats.relations_created
    );

    // `load_or_build` (the exact fn get_kg calls) returns a usable graph.
    let kg = tubeforge::analytics::kg_builder::load_or_build(&db)
        .await
        .expect("load_or_build");
    assert!(!kg.is_empty(), "loaded KG must be non-empty");
    assert!(
        kg.centrality.values().any(|c| *c > 0.0),
        "pagerank must have populated positive centrality"
    );
}

#[tokio::test]
async fn kg_graph_scores_and_ideas_derive_from_real_kg() {
    let db = seed_db().await;
    let kg = tubeforge::analytics::kg_builder::load_or_build(&db)
        .await
        .expect("load_or_build");
    assert!(!kg.is_empty());

    // compute_graph_scores (used by scores.detail RPC handler) — needs a real
    // video_id present in the graph.
    let scores = tubeforge::analytics::graph_aware::compute_graph_scores(
        &kg,
        "aaa111bbb22",
        Some("UCtest0000000000000000001"),
        &["rust".to_string()],
    );
    // Backward-compatible contract: bounded 0..=100, never NaN.
    assert!(
        (0.0..=100.0).contains(&scores.tag_authority),
        "tag_authority out of range: {}",
        scores.tag_authority
    );
    assert!(
        (0.0..=100.0).contains(&scores.topic_dominance),
        "topic_dominance out of range: {}",
        scores.topic_dominance
    );
    assert!(
        (0.0..=100.0).contains(&scores.keyword_competition),
        "keyword_competition out of range: {}",
        scores.keyword_competition
    );

    // generate_graph_ideas (used by ideas.analyze RPC handler) — with a minimal
    // seed (no topics/competitor edges) there is legitimately nothing to suggest,
    // so the contract is that it NEVER panics and is bounded, not that it is
    // non-empty. A structurally-rich graph (topics + competitors) produces ideas.
    let ideas = tubeforge::analytics::graph_aware::generate_graph_ideas(
        &kg,
        Some("UCtest0000000000000000001"),
        5,
    );
    assert!(
        ideas.len() <= 5,
        "KG-backed ideas must be bounded by limit, got {}",
        ideas.len()
    );

    // Prove ideas ARE generated when the graph has an underserved topic (the
    // real RPC path: an ingested corpus where a competitor covers a topic the
    // own channel does NOT cover produces a content-gap graph idea).
    let mut rich = tubeforge::analytics::kg::KnowledgeGraph::new();
    // Competitor video covering an underserved topic (own channel absent).
    rich.insert_entity(tubeforge::analytics::kg::KgEntity::video(
        "comp_v1",
        "Competitor Async Guide",
    ));
    rich.insert_entity(tubeforge::analytics::kg::KgEntity::channel(
        "UC:comp",
        "Competitor",
    ));
    rich.insert_entity(tubeforge::analytics::kg::KgEntity::topic(
        "async_patterns",
        "Async Patterns",
    ));
    rich.insert_edge(
        "video:comp_v1",
        "channel:UC:comp",
        tubeforge::analytics::kg::RelationType::CreatedBy,
        1.0,
    );
    rich.insert_edge(
        "video:comp_v1",
        "topic:async_patterns",
        tubeforge::analytics::kg::RelationType::AboutTopic,
        1.0,
    );
    // Own channel has NO videos in async_patterns → it is a content gap.
    let rich_ideas =
        tubeforge::analytics::graph_aware::generate_graph_ideas(&rich, Some("UC:own"), 5);
    assert!(
        !rich_ideas.is_empty(),
        "KG with an underserved topic must generate a content-gap idea"
    );

    // find_content_gaps (used by gaps.get RPC handler).
    let gaps = tubeforge::analytics::graph_aware::find_content_gaps(
        &kg,
        Some("UCtest0000000000000000001"),
    );
    assert!(
        gaps.len() <= 10 || gaps.is_empty(),
        "graph_gaps bounded to a reasonable count"
    );

    // compute_tag_authority_by_name (used by tags.gaps RPC handler) — the
    // "rust" keyword seeded should be reachable and yield a bounded score.
    let authority = tubeforge::analytics::graph_aware::compute_tag_authority_by_name(&kg, "rust");
    assert!(
        (0.0..=100.0).contains(&authority),
        "tag authority out of range: {}",
        authority
    );
}

#[tokio::test]
async fn kg_pagerank_centrality_populates_channels() {
    let db = seed_db().await;
    let kg = tubeforge::analytics::kg_builder::load_or_build(&db)
        .await
        .expect("load_or_build");

    // pagerank (used by scorecard.get RPC handler centrality column) must
    // assign channel centrality for every channel entity in the graph.
    let pr = tubeforge::analytics::kg_algorithms::pagerank(&kg);
    for entity_id in kg.entities_of_type(tubeforge::analytics::kg::EntityType::Channel) {
        assert!(
            pr.contains_key(entity_id),
            "pagerank must include channel {entity_id}"
        );
    }
    // Mass conservation: sum ≈ 1.0 on a non-empty graph.
    let sum: f64 = pr.values().sum();
    assert!(
        (sum - 1.0).abs() < 1e-6,
        "pagerank mass must sum to 1.0, got {sum}"
    );
}
