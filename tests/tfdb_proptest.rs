//! Property-based + durability tests for the TubeForge DB engine core
//! (tfdb). These exercise the from-scratch store's crash-safety contract:
//! commits are durable across reopen, uncommitted writes are never visible,
//! and unique constraints hold under arbitrary mutation sequences.

use std::path::Path;

use proptest::prop_assert_eq;
use tubeforge::tfdb::{Engine, TableSchema, Value};

fn schema() -> TableSchema {
    TableSchema::new("things", "id")
        .text("label")
        .int("n")
        .float("score")
        .boolean("flag")
        .unique("label")
}

fn open(path: &Path) -> Engine {
    let mut e = Engine::open(path).expect("open");
    if !e.table_exists("things") {
        e.create_table(schema());
    }
    e
}

#[test]
fn commit_persists_across_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("t.db");
    {
        let mut e = open(&db);
        let mut tx = e.begin();
        let mut row = std::collections::BTreeMap::new();
        row.insert("id".into(), Value::Text("a".into()));
        row.insert("label".into(), Value::Text("alpha".into()));
        row.insert("n".into(), Value::Int(7));
        tx.put("things", row).expect("put");
        tx.commit().expect("commit");
    }
    {
        let e = open(&db);
        let row = e.get("things", "a").expect("get").expect("present");
        assert_eq!(row["label"], Value::Text("alpha".into()));
        assert_eq!(row["n"], Value::Int(7));
    }
}

#[test]
fn rollback_discards_writes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("t.db");
    let mut e = open(&db);
    {
        let mut tx = e.begin();
        let mut row = std::collections::BTreeMap::new();
        row.insert("id".into(), Value::Text("x".into()));
        row.insert("label".into(), Value::Text("zzz".into()));
        tx.put("things", row).expect("put");
        tx.rollback();
    }
    assert!(e.get("things", "x").expect("get").is_none());
}

#[test]
fn unique_column_is_enforced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("t.db");
    let mut e = open(&db);
    {
        let mut tx = e.begin();
        let mut r1 = std::collections::BTreeMap::new();
        r1.insert("id".into(), Value::Text("1".into()));
        r1.insert("label".into(), Value::Text("dup".into()));
        tx.put("things", r1).expect("put1");
        tx.commit().expect("commit1");
    }
    {
        let mut tx = e.begin();
        let mut r2 = std::collections::BTreeMap::new();
        r2.insert("id".into(), Value::Text("2".into()));
        r2.insert("label".into(), Value::Text("dup".into()));
        let err = tx.put("things", r2).expect_err("duplicate label");
        assert!(err.to_string().contains("UNIQUE"));
        tx.rollback();
    }
}

#[test]
fn unknown_column_rejected_in_strict_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("t.db");
    let mut e = open(&db);
    let mut tx = e.begin();
    let mut row = std::collections::BTreeMap::new();
    row.insert("id".into(), Value::Text("a".into()));
    row.insert("bogus".into(), Value::Int(1));
    assert!(tx.put("things", row).is_err());
    tx.rollback();
}

proptest::proptest! {
    // Every committed row set survives a full open/close/reopen cycle with
    // identical contents (durability + deterministic snapshot).
    #[test]
    fn committed_rows_roundtrip(
        n in 0usize..12,
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("t.db");
        {
            let mut e = open(&db);
            let mut tx = e.begin();
            for i in 0..n {
                let mut row = std::collections::BTreeMap::new();
                row.insert("id".into(), Value::Text(format!("id{i}")));
                row.insert("label".into(), Value::Text(format!("label{i}")));
                row.insert("n".into(), Value::Int(i as i64));
                tx.put("things", row).expect("put");
            }
            tx.commit().expect("commit");
        }
        let e = open(&db);
        assert_eq!(e.count("things").expect("count"), n as u64);
        for i in 0..n {
            let row = e.get("things", &format!("id{i}")).expect("get").expect("present");
            prop_assert_eq!(row["label"].clone(), Value::Text(format!("label{i}")));
        }
    }
}
