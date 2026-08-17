//! Round-trip proof for the tfdb-backed domain model (migration foundation).
//!
//! The full TubeForge `Db` migration replaces turso SQL with in-memory scans
//! over the tfdb engine. These tests prove the engine persists/reloads the
//! real domain entities (channels, videos, scores, tags, KG relations) through
//! the tfdb schema, so the storage layer swap is grounded in verified data.

use std::collections::BTreeMap;

use proptest::prop_assert_eq;
use tubeforge::tfdb::tfdb_schema;
use tubeforge::tfdb::{Engine, Value};

fn open(path: &std::path::Path) -> Engine {
    let mut e = Engine::open(path).expect("open");
    for s in tfdb_schema::all() {
        e.create_table(s);
    }
    e
}

fn row(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[test]
fn videos_and_channels_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("d.db");
    {
        let mut e = open(&db);
        let mut tx = e.begin();
        tx.put(
            "channels",
            row(&[
                ("channel_id", Value::Text("UC1".into())),
                ("title", Value::Text("Tech Verse".into())),
                ("source", Value::Text("rss".into())),
                ("fetched_at", Value::Text("2026-08-01T00:00:00Z".into())),
                ("updated_at", Value::Text("2026-08-01T00:00:00Z".into())),
            ]),
        )
        .expect("channel");
        tx.put(
            "videos",
            row(&[
                ("video_id", Value::Text("vid1".into())),
                ("channel_id", Value::Text("UC1".into())),
                ("title", Value::Text("Rust DB Guide".into())),
                ("description", Value::Text("How to build a DB.".into())),
                ("tags", Value::Json(serde_json::json!(["rust", "database"]))),
                ("published_at", Value::Text("2026-08-01T10:00:00Z".into())),
                ("source", Value::Text("rss".into())),
                ("fetched_at", Value::Text("2026-08-01T00:00:00Z".into())),
                ("updated_at", Value::Text("2026-08-01T00:00:00Z".into())),
            ]),
        )
        .expect("video");
        tx.commit().expect("commit");
    }
    let e = open(&db);
    let v = e.get("videos", "vid1").expect("get").expect("present");
    assert_eq!(v["channel_id"], Value::Text("UC1".into()));
    assert_eq!(v["title"], Value::Text("Rust DB Guide".into()));
    assert_eq!(
        v["tags"],
        Value::Json(serde_json::json!(["rust", "database"]))
    );
    let c = e.get("channels", "UC1").expect("get").expect("present");
    assert_eq!(c["title"], Value::Text("Tech Verse".into()));
    assert_eq!(e.count("videos").expect("count"), 1);
    assert_eq!(e.count("channels").expect("count"), 1);
}

#[test]
fn scores_and_ideas_roundtrip_with_json_columns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("d.db");
    {
        let mut e = open(&db);
        let mut tx = e.begin();
        tx.put(
            "scores",
            row(&[
                ("video_id", Value::Text("vid1".into())),
                ("seo_score", Value::Float(77.8)),
                ("geo_score", Value::Float(50.0)),
                ("total_score", Value::Float(63.9)),
                (
                    "components",
                    Value::Json(serde_json::json!({"keyword_title": 100.0})),
                ),
                ("computed_at", Value::Text("2026-08-01T00:00:00Z".into())),
            ]),
        )
        .expect("score");
        tx.put(
            "ideas",
            row(&[
                ("idea_id", Value::Int(1)),
                ("title_suggestion", Value::Text("Next video topic".into())),
                ("rationale", Value::Json(serde_json::json!({"gap": 0.9}))),
                ("score", Value::Float(88.0)),
                ("status", Value::Text("draft".into())),
                ("created_at", Value::Text("2026-08-01T00:00:00Z".into())),
            ]),
        )
        .expect("idea");
        tx.commit().expect("commit");
    }
    let e = open(&db);
    let s = e.get("scores", "vid1").expect("get").expect("present");
    assert_eq!(s["seo_score"], Value::Float(77.8));
    let i = e.get("ideas", "1").expect("get").expect("present");
    assert_eq!(
        i["title_suggestion"],
        Value::Text("Next video topic".into())
    );
    assert_eq!(i["rationale"], Value::Json(serde_json::json!({"gap": 0.9})));
}

#[test]
fn kg_relations_persist_and_query() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("d.db");
    {
        let mut e = open(&db);
        let mut tx = e.begin();
        tx.put(
            "kg_entities",
            row(&[
                ("entity_id", Value::Text("e1".into())),
                ("entity_type", Value::Text("tag".into())),
                ("canonical_name", Value::Text("rust".into())),
                ("display_name", Value::Text("Rust".into())),
                ("properties", Value::Json(serde_json::json!({}))),
                ("created_at", Value::Text("2026-08-01T00:00:00Z".into())),
                ("updated_at", Value::Text("2026-08-01T00:00:00Z".into())),
            ]),
        )
        .expect("entity");
        tx.commit().expect("commit");
    }
    let e = open(&db);
    let rows = e
        .find_eq("kg_entities", "entity_type", &Value::Text("tag".into()))
        .expect("find");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["canonical_name"], Value::Text("rust".into()));
    // Update centrality in place.
    let mut e = open(&db);
    let mut ent = e.get("kg_entities", "e1").expect("get").expect("present");
    ent.insert("centrality".into(), Value::Float(0.42));
    {
        let mut tx = e.begin();
        tx.put("kg_entities", ent).expect("put");
        tx.commit().expect("commit");
    }
    let e = open(&db);
    assert_eq!(
        e.get("kg_entities", "e1").expect("get").expect("p")["centrality"],
        Value::Float(0.42)
    );
}

#[test]
fn meta_key_value_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("d.db");
    {
        let mut e = open(&db);
        let mut tx = e.begin();
        tx.put(
            "meta",
            row(&[
                ("key", Value::Text("schema_version".into())),
                ("value", Value::Text("9".into())),
            ]),
        )
        .expect("meta");
        tx.commit().expect("commit");
    }
    let e = open(&db);
    let m = e
        .get("meta", "schema_version")
        .expect("get")
        .expect("present");
    assert_eq!(m["value"], Value::Text("9".into()));
}

proptest::proptest! {
    // Any number of video rows round-trip losslessly (durability + shape).
    #[test]
    fn many_videos_roundtrip(
        n in 0usize..30,
        titles in proptest::collection::vec("[a-zA-Z ]{1,40}", 30),
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("d.db");
        {
            let mut e = open(&db);
            let mut tx = e.begin();
            for i in 0..n {
                tx.put(
                    "videos",
                    row(&[
                        ("video_id", Value::Text(format!("vid{i}"))),
                        ("title", Value::Text(titles.get(i).cloned().unwrap_or_default())),
                        ("description", Value::Text("".into())),
                        ("tags", Value::Json(serde_json::json!([]))),
                        ("published_at", Value::Text("2026-08-01T00:00:00Z".into())),
                        ("source", Value::Text("rss".into())),
                        ("fetched_at", Value::Text("2026-08-01T00:00:00Z".into())),
                        ("updated_at", Value::Text("2026-08-01T00:00:00Z".into())),
                    ]),
                )
                .expect("video");
            }
            tx.commit().expect("commit");
        }
        let e = open(&db);
        assert_eq!(e.count("videos").expect("count"), n as u64);
        for i in 0..n {
            let v = e.get("videos", &format!("vid{i}")).expect("get").expect("present");
            prop_assert_eq!(
                v["title"].clone(),
                Value::Text(titles.get(i).cloned().unwrap_or_default())
            );
        }
    }
}
