//! Phase 0 Gate (LLD §13) — must pass before feature work.
//!
//! 1. gate_crud_wal_transaction — WAL mode, CRUD, transaction commit/rollback, integrity_check
//! 2. gate_backup_roundtrip       — VACUUM INTO snapshot, reopen, row parity + integrity_check
//! 3. gate_rusqlite_escape        — same .db opens in rusqlite (bundled)
//! 4. gate_fts_probe              — INFORMATIONAL: Turso FTS probe (expected broken/wrong)
//! 5. gate_tantivy_compiles       — tantivy index + BM25 score > 0

use std::path::Path;

use tantivy::doc;
use tubeforge::search::new_index;
use tubeforge::storage::backup::backup;
use tubeforge::storage::Db;

// ---------------------------------------------------------------------------
// 1. WAL + CRUD + transactions + integrity_check
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gate_crud_wal_transaction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("gate.db");
    let mut db = Db::open(&db_path).await.expect("open db");

    assert_eq!(
        db.journal_mode().await.expect("journal mode").to_ascii_lowercase(),
        "wal",
        "journal mode must be WAL (ADR-5)"
    );

    db.conn
        .execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)", ())
        .await
        .expect("create table");
    db.conn
        .execute("INSERT INTO t (v) VALUES ('a')", ())
        .await
        .expect("insert");

    // Commit a transaction.
    {
        let tx = db
            .conn
            .transaction()
            .await
            .expect("begin tx");
        tx.execute("INSERT INTO t (v) VALUES ('b')", ())
            .await
            .expect("insert in tx");
        tx.commit().await.expect("commit tx");
    }

    // Roll a transaction back; rows must be absent afterwards.
    {
        let tx = db
            .conn
            .transaction()
            .await
            .expect("begin tx");
        tx.execute("INSERT INTO t (v) VALUES ('doomed')", ())
            .await
            .expect("insert doomed");
        tx.rollback().await.expect("rollback tx");
    }

    // Commit a third transaction.
    {
        let tx = db
            .conn
            .transaction()
            .await
            .expect("begin tx");
        tx.execute("INSERT INTO t (v) VALUES ('c')", ())
            .await
            .expect("insert c");
        tx.commit().await.expect("commit tx");
    }

    let count = db.count("SELECT count(*) FROM t").await.expect("count");
    assert_eq!(count, 3, "committed rows only (2 + 1), rolled back row absent");

    db.integrity_check().await.expect("integrity_check ok");
}

// ---------------------------------------------------------------------------
// 2. Backup round-trip (VACUUM INTO + integrity_check on the snapshot)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gate_backup_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("gate.db");
    let backup_dir = dir.path().join("backups");
    let db = Db::open(&db_path).await.expect("open db");

    const N: i64 = 25;
    db.conn
        .execute(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT NOT NULL)",
            (),
        )
        .await
        .expect("create table");
    for i in 0..N {
        db.conn
            .execute("INSERT INTO t (v) VALUES (?1)", [format!("row-{i}")])
            .await
            .expect("insert");
    }
    db.integrity_check().await.expect("main integrity ok");

    let snapshot = backup(&db, &backup_dir, 10).await.expect("backup");
    assert!(snapshot.exists(), "snapshot file exists");
    assert!(
        snapshot.file_name().unwrap().to_str().unwrap().starts_with("tubeforge-"),
        "snapshot naming: tubeforge-<ts>.db"
    );

    // Open the snapshot and verify parity + integrity.
    let snap = Db::open(&snapshot).await.expect("open snapshot");
    let count = snap.count("SELECT count(*) FROM t").await.expect("snapshot count");
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
// 3. rusqlite escape hatch (COMPAT guarantee #1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gate_rusqlite_escape() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("gate.db");
    let backup_dir = dir.path().join("backups");
    let db = Db::open(&db_path).await.expect("open db");

    db.conn
        .execute(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT NOT NULL)",
            (),
        )
        .await
        .expect("create table");
    for i in 0..10 {
        db.conn
            .execute("INSERT INTO t (v) VALUES (?1)", [format!("rusqlite-{i}")])
            .await
            .expect("insert");
    }
    db.conn.cacheflush().expect("flush dirty pages to WAL");

    // Attempt 1: open the LIVE main db with rusqlite (WAL-mode). libsql's WAL
    // is not guaranteed byte-compatible with SQLite's; if it fails we fall
    // back to the VACUUM INTO snapshot, which is a standalone single-file db.
    let main_attempt = rusqlite_open_rows(&db_path);
    match main_attempt {
        Ok(rows) => {
            assert_eq!(rows, 10, "rusqlite read back all rows from main db");
            eprintln!("gate_rusqlite_escape: main WAL-mode db opened directly in rusqlite (PASS, ideal path)");
        }
        Err(e) => {
            eprintln!("gate_rusqlite_escape: main db direct open failed: {e}");
            eprintln!("gate_rusqlite_escape: falling back to VACUUM INTO snapshot (documented)");
            let snapshot = backup(&db, &backup_dir, 10).await.expect("backup");
            let rows = rusqlite_open_rows(&snapshot).expect("snapshot opens in rusqlite");
            assert_eq!(rows, 10, "rusqlite read back all rows from snapshot");
            eprintln!("gate_rusqlite_escape: snapshot opened in rusqlite (PASS via snapshot path)");
        }
    }
}

fn rusqlite_open_rows(path: &Path) -> rusqlite::Result<i64> {
    let conn = rusqlite::Connection::open(path)?;
    conn.query_row("SELECT count(*) FROM t", [], |row| row.get(0))
}

// ---------------------------------------------------------------------------
// 4. FTS probe (INFORMATIONAL — must pass regardless of outcome)
// ---------------------------------------------------------------------------

/// Probe Turso's FTS index method (`CREATE INDEX ... USING fts`,
/// `fts_match()`, `fts_score()`) per LLD §13 item 3.
///
/// Expected per HLD §7.2: broken/unavailable (issues #7523–7529: ASC order
/// wrong, LIMIT/OFFSET wrong, fallback divergence). This test records the
/// OBSERVED behavior verbatim and always passes — it exists to confirm the
/// design decision (BM25 via tantivy, not engine FTS).
#[tokio::test]
async fn gate_fts_probe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("fts_probe.db");

    // Builder enables the engine's `index_method` experimental feature; the
    // crate's `fts` cargo feature is on by default in turso 0.7.2.
    let db = turso::Builder::new_local(db_path.to_str().unwrap())
        .experimental_index_method(true)
        .build()
        .await
        .expect("open fts probe db");
    let conn = db.connect().expect("connect");

    let _ = conn
        .execute("PRAGMA journal_mode = WAL", ())
        .await
        .map_err(|e| eprintln!("fts probe: journal pragma row-returning quirk: {e}"));

    conn.execute(
        "CREATE TABLE IF NOT EXISTS docs (id INTEGER PRIMARY KEY, title TEXT, body TEXT)",
        (),
    )
    .await
    .expect("create docs table");

    // 1. Can the FTS index even be created?
    let create_fts = conn
        .execute(
            "CREATE INDEX IF NOT EXISTS docs_fts USING fts(title, body)",
            (),
        )
        .await;
    let create_outcome = match &create_fts {
        Ok(_) => "ok: index created",
        Err(e) => &format!("ERROR: {e}"),
    };
    eprintln!("fts probe [1/4] CREATE INDEX USING fts        -> {create_outcome}");

    if create_fts.is_err() {
        eprintln!("fts probe RESULT: FTS unavailable in turso 0.7.2 (probe passes: {create_outcome})");
        eprintln!("fts probe CONFIRMS: tantivy-direct BM25 decision stands (LLD §13 item 3)");
        return;
    }

    // 2. Insert three rows with different term frequencies.
    for (id, title) in [(1, "rust database guide"), (2, "rust rust rust database"), (3, "python web")] {
        conn.execute(
            "INSERT INTO docs (id, title, body) VALUES (?1, ?2, ?3)",
            vec![
                format!("{id}"),
                format!("{title} title {id}"),
                format!("body {id}"),
            ],
        )
        .await
        .expect("insert doc");
    }

    // 3. fts_match + ORDER BY fts_score (the #7524-class ranking probe).
    let ranked = conn
        .query(
            "SELECT id, fts_score() AS s FROM docs
             WHERE fts_match(docs_fts, 'rust')
             ORDER BY fts_score() DESC",
            (),
        )
        .await;
    match ranked {
        Ok(mut rows) => {
            let mut order = Vec::new();
            while let Some(row) = rows.next().await.expect("next") {
                let id = row.get_value(0).map(|v| format!("{v:?}")).unwrap_or_default();
                let s = row.get_value(1).map(|v| format!("{v:?}")).unwrap_or_default();
                order.push(format!("({id}, {s})"));
            }
            eprintln!("fts probe [2/4] fts_match+ORDER BY fts_score -> rows in order: {order:?}");
            eprintln!(
                "fts probe OBSERVED: expected rank (higher tf first) would be [2, 1]; got {:?}",
                order
            );
        }
        Err(e) => {
            eprintln!("fts probe [2/4] fts_match query            -> ERROR: {e}");
        }
    }

    // 4. LIMIT/OFFSET variant (#7523/#7526-class probe).
    let limited = conn
        .query(
            "SELECT id FROM docs WHERE fts_match(docs_fts, 'rust') ORDER BY fts_score() DESC LIMIT 1 OFFSET 1",
            (),
        )
        .await;
    match limited {
        Ok(mut rows) => {
            let mut got = Vec::new();
            while let Some(row) = rows.next().await.expect("next") {
                got.push(format!("{:?}", row.get_value(0)));
            }
            eprintln!("fts probe [3/4] LIMIT 1 OFFSET 1            -> {got:?}");
        }
        Err(e) => {
            eprintln!("fts probe [3/4] LIMIT/OFFSET variant        -> ERROR: {e}");
        }
    }

    // 5. Full scan sanity (is the index physically usable at all?).
    let scan = conn
        .query("SELECT count(*) FROM docs WHERE fts_match(docs_fts, 'rust')", ())
        .await;
    match scan {
        Ok(mut rows) => {
            let n = rows
                .next()
                .await
                .expect("next")
                .and_then(|r| r.get_value(0).ok())
                .map(|v| format!("{v:?}"))
                .unwrap_or_default();
            eprintln!("fts probe [4/4] count of 'rust' matches      -> {n}");
        }
        Err(e) => {
            eprintln!("fts probe [4/4] count query                 -> ERROR: {e}");
        }
    }

    eprintln!("fts probe RESULT: FTS available but ranking is engine-controlled and beta (see rows above); probe passes regardless — BM25 stays in tantivy (LLD §13 item 3)");
}

// ---------------------------------------------------------------------------
// 5. tantivy compiles/pins + positive BM25 score
// ---------------------------------------------------------------------------

#[test]
fn gate_tantivy_compiles() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Build the index through the crate's own helper (compile/pin proof).
    let _index = new_index(&dir.path().join("index")).expect("new_index");

    // Re-open the same directory (persistence + reader path).
    let index = tantivy::Index::open_in_dir(dir.path().join("index")).expect("reopen index");

    let schema = index.schema();
    let title = schema.get_field("title").expect("title field");
    let video_id = schema.get_field("video_id").expect("video_id field");

    let mut writer = index.writer(50_000_000).expect("writer");
    writer
        .add_document(tantivy::doc![
            title => "tubeforge phase zero gate rust database",
            video_id => "7lCDEYXw3mM",
        ])
        .expect("add doc");
    writer.commit().expect("commit");

    let reader = index.reader().expect("reader");
    reader.reload().expect("reload");
    let searcher = reader.searcher();
    let parser = tantivy::query::QueryParser::for_index(&index, vec![title]);
    let query = parser.parse_query("database").expect("parse");
    let collector = tantivy::collector::TopDocs::with_limit(10).order_by_score();
    let hits = searcher.search(&query, &collector).expect("search");
    assert_eq!(hits.len(), 1, "exactly one matching doc");
    let (score, _) = hits[0];
    assert!(score > 0.0, "BM25 score > 0, got {score}");

    // Non-matching query yields nothing (sanity on the ranking path).
    let miss = parser.parse_query("nonexistentterm").expect("parse miss");
    let miss_hits = searcher.search(&miss, &collector).expect("search miss");
    assert_eq!(miss_hits.len(), 0, "no hits for non-matching term");
}
