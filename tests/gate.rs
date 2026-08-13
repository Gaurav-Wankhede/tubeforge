//! Phase 0 Gate (LLD §13) — must pass before feature work.
//!
//! 1. gate_crud_wal_transaction — WAL parity, CRUD, transaction commit/rollback,
//!    persistence across reopen, integrity_check
//! 2. gate_backup_roundtrip — checkpoint snapshot, reopen, row parity + integrity_check
//! 3. gate_tantivy_compiles — custom BM25 index + score > 0
//!
//! The legacy turso `gate_rusqlite_escape` and `gate_fts_probe` tests were
//! removed with the turso dependency: tfdb is the single engine and produces
//! no SQLite file, so neither a rusqlite escape hatch nor an engine FTS probe
//! applies.

use tubeforge::search::new_index;
use tubeforge::search::{Bm25, VideoDoc, FIELD_TITLE};
use tubeforge::storage::backup::backup;
use tubeforge::storage::Db;
use tubeforge::tfdb::store::{Row, Value};

// ---------------------------------------------------------------------------
// 1. WAL parity + CRUD + transactions + persistence + integrity_check
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gate_crud_wal_transaction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("gate.db");
    let db = Db::open(&db_path).await.expect("open db");

    assert_eq!(
        db.journal_mode()
            .await
            .expect("journal mode")
            .to_ascii_lowercase(),
        "wal",
        "journal mode must be WAL (ADR-5)"
    );

    // Commit a write.
    db.meta_set("commit", "yes").await.expect("meta_set");
    assert_eq!(db.meta_get("commit").await.unwrap(), Some("yes".to_string()));

    // Roll a transaction back; its write must be absent afterwards.
    {
        let mut eng = db.engine.lock().unwrap();
        let mut tx = eng.begin();
        let mut r = Row::new();
        r.insert("key".to_string(), Value::Text("doomed".into()));
        r.insert("value".to_string(), Value::Text("x".into()));
        tx.put("meta", r).unwrap();
        tx.rollback();
    }
    assert_eq!(
        db.meta_get("doomed").await.unwrap(),
        None,
        "rolled-back write absent"
    );

    // Commit a third write.
    db.meta_set("commit2", "y").await.expect("meta_set");
    assert_eq!(db.meta_get("commit2").await.unwrap(), Some("y".to_string()));

    // Durability: committed writes survive a reopen; the rolled-back write
    // stays absent.
    drop(db);
    let db2 = Db::open(&db_path).await.expect("reopen");
    assert_eq!(db2.meta_get("commit").await.unwrap(), Some("yes".to_string()));
    assert_eq!(db2.meta_get("commit2").await.unwrap(), Some("y".to_string()));
    assert_eq!(
        db2.meta_get("doomed").await.unwrap(),
        None,
        "rolled-back write stays absent across reopen"
    );
    db2.integrity_check().await.expect("integrity_check ok");
}

// ---------------------------------------------------------------------------
// 2. Backup round-trip (checkpoint snapshot + reopen + integrity_check)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gate_backup_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("gate.db");
    let backup_dir = dir.path().join("backups");
    let db = Db::open(&db_path).await.expect("open db");

    const N: i64 = 25;
    for i in 0..N {
        db.add_keywords(&[format!("kw-{i}")], None)
            .await
            .expect("add keyword");
    }
    db.integrity_check().await.expect("main integrity ok");

    let snapshot = backup(&db, &backup_dir, 10).await.expect("backup");
    assert!(snapshot.exists(), "snapshot file exists");
    assert!(
        snapshot
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("tubeforge-"),
        "snapshot naming: tubeforge-<ts>.db"
    );

    // Open the snapshot and verify parity + integrity.
    let snap = Db::open(&snapshot).await.expect("open snapshot");
    let count = snap
        .count("SELECT count(*) FROM keywords")
        .await
        .expect("snapshot count");
    assert_eq!(count, N, "snapshot has all rows");
    snap.integrity_check().await.expect("snapshot integrity ok");

    // Retention: second backup should keep both; prune keeps last N.
    let snapshot2 = backup(&db, &backup_dir, 10).await.expect("backup 2");
    assert_ne!(snapshot, snapshot2, "snapshots differ per timestamp");
    let files: Vec<_> = std::fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().unwrap().ends_with(".db"))
        .collect();
    assert_eq!(files.len(), 2, "keep=10 => no prune after 2 backups");
}

// ---------------------------------------------------------------------------
// 3. custom index compiles/pins + positive BM25 score
// ---------------------------------------------------------------------------

#[test]
fn gate_tantivy_compiles() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Build the index through the crate's own helper (compile/pin proof).
    let _index = new_index(&dir.path().join("index")).expect("new_index");

    // Re-open the same directory (persistence + reader path).
    let index = tubeforge::search::open_or_create(&dir.path().join("index")).expect("reopen index");

    let mut writer = index.writer(50_000_000);
    writer
        .add_document(VideoDoc {
            video_id: "7lCDEYXw3mM".to_string(),
            title: "tubeforge phase zero gate rust database".to_string(),
            ..Default::default()
        })
        .expect("add doc");
    writer.commit().expect("commit");

    let bm25 = Bm25::open(index).expect("bm25");
    let score = bm25.corpus_resonance(FIELD_TITLE, "database", None);
    assert!(score > 0.0, "BM25 score > 0, got {score}");

    // Non-matching query yields nothing (sanity on the ranking path).
    let miss = bm25.corpus_resonance(FIELD_TITLE, "nonexistentterm", None);
    assert_eq!(miss, 0.0, "no hits for non-matching term");
}
