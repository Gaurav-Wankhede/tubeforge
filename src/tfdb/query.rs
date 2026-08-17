//! SQL-free query helpers over the tfdb engine.
//!
//! The legacy `Db` used SQL (joins, GROUP BY, aggregates, ordering). tfdb has
//! no SQL, but at the ~10k-video corpus scale (HLD §10) the same results are
//! produced by in-memory scans. This module collects the reusable patterns so
//! the `Db` migration stays small and testable:
//!
//! - `sum`/`avg`/`min`/`max` over a numeric column (with an optional filter)
//! - `group_counts` — GROUP BY col → count (tag clouds, per-channel counts)
//! - `join` — inner join on a common key between two tables
//!
//! These are pure functions over `Engine` reads; no new storage semantics.

use std::collections::HashMap;

use super::store::{Engine, Row, Value};
use crate::error::{storage_err, TubeforgeError};

/// Sum a numeric column across rows matching a filter (empty filter = all).
pub fn sum(
    engine: &Engine,
    table: &str,
    col: &str,
    filter: Option<(&str, &Value)>,
) -> Result<f64, TubeforgeError> {
    Ok(rows(engine, table, filter)?
        .iter()
        .filter_map(|r| r.get(col).and_then(|v| v.as_f64()))
        .sum())
}

/// Average a numeric column across matching rows (0.0 when none match).
pub fn avg(
    engine: &Engine,
    table: &str,
    col: &str,
    filter: Option<(&str, &Value)>,
) -> Result<f64, TubeforgeError> {
    let vals: Vec<f64> = rows(engine, table, filter)?
        .iter()
        .filter_map(|r| r.get(col).and_then(|v| v.as_f64()))
        .collect();
    if vals.is_empty() {
        return Ok(0.0);
    }
    Ok(vals.iter().sum::<f64>() / vals.len() as f64)
}

/// Min of a numeric column across matching rows (None when none match).
pub fn min(
    engine: &Engine,
    table: &str,
    col: &str,
    filter: Option<(&str, &Value)>,
) -> Result<Option<f64>, TubeforgeError> {
    Ok(rows(engine, table, filter)?
        .iter()
        .filter_map(|r| r.get(col).and_then(|v| v.as_f64()))
        .min_by(|a, b| a.total_cmp(b)))
}

/// Max of a numeric column across matching rows (None when none match).
pub fn max(
    engine: &Engine,
    table: &str,
    col: &str,
    filter: Option<(&str, &Value)>,
) -> Result<Option<f64>, TubeforgeError> {
    Ok(rows(engine, table, filter)?
        .iter()
        .filter_map(|r| r.get(col).and_then(|v| v.as_f64()))
        .max_by(|a, b| a.total_cmp(b)))
}

/// Count of matching rows.
pub fn count(
    engine: &Engine,
    table: &str,
    filter: Option<(&str, &Value)>,
) -> Result<u64, TubeforgeError> {
    Ok(rows(engine, table, filter)?.len() as u64)
}

/// `GROUP BY col` → count of rows per distinct value.
pub fn group_counts(
    engine: &Engine,
    table: &str,
    col: &str,
) -> Result<Vec<(String, u64)>, TubeforgeError> {
    let mut m: HashMap<String, u64> = HashMap::new();
    for r in engine.all(table)? {
        if let Some(Value::Text(v)) = r.get(col) {
            *m.entry(v.clone()).or_insert(0) += 1;
        }
    }
    let mut out: Vec<(String, u64)> = m.into_iter().collect();
    out.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    Ok(out)
}

/// Inner join two tables on `left_col` == `right_col`, returning the
/// (left_row, right_row) pairs. Right rows with no left match are dropped.
pub fn join(
    engine: &Engine,
    left_table: &str,
    right_table: &str,
    left_col: &str,
    right_col: &str,
) -> Result<Vec<(Row, Row)>, TubeforgeError> {
    let right = engine.all(right_table)?;
    let mut index: HashMap<String, Vec<Row>> = HashMap::new();
    for r in &right {
        if let Some(Value::Text(k)) = r.get(right_col) {
            index.entry(k.clone()).or_default().push(r.clone());
        }
    }
    let mut out = Vec::new();
    for l in engine.all(left_table)? {
        let Some(Value::Text(k)) = l.get(left_col) else {
            continue;
        };
        if let Some(rs) = index.get(k) {
            for r in rs {
                out.push((l.clone(), r.clone()));
            }
        }
    }
    Ok(out)
}

/// Fetch rows (optionally filtered by one column equality).
fn rows(
    engine: &Engine,
    table: &str,
    filter: Option<(&str, &Value)>,
) -> Result<Vec<Row>, TubeforgeError> {
    match filter {
        Some((col, val)) => engine.find_eq(table, col, val),
        None => engine.all(table),
    }
}

/// Read a required text column value (errors if missing/wrong type).
pub fn text<'a>(row: &'a Row, col: &str) -> Result<&'a str, TubeforgeError> {
    row.get(col)
        .and_then(|v| v.as_text())
        .ok_or_else(|| storage_err("MISSING_COL", format!("missing text col {col}")))
}

/// Read an optional text column value.
pub fn opt_text(row: &Row, col: &str) -> Option<String> {
    row.get(col).and_then(|v| v.as_text()).map(str::to_string)
}

/// Read an optional int column value.
pub fn opt_int(row: &Row, col: &str) -> Option<i64> {
    row.get(col).and_then(|v| v.as_i64())
}

/// Read an optional float column value.
pub fn opt_float(row: &Row, col: &str) -> Option<f64> {
    row.get(col).and_then(|v| v.as_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prop_assert_eq;
    use std::collections::BTreeMap;

    fn open() -> (tempfile::TempDir, Engine) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut e = Engine::open(&dir.path().join("t.db")).expect("open");
        e.create_table(
            crate::tfdb::TableSchema::new("videos", "video_id")
                .text("channel_id")
                .int("view_count"),
        );
        e.create_table(crate::tfdb::TableSchema::new("channels", "channel_id").text("title"));
        (dir, e)
    }

    fn r(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn seed(e: &mut Engine) {
        let mut tx = e.begin();
        tx.put(
            "videos",
            r(&[
                ("video_id", "v1".into()),
                ("channel_id", "c1".into()),
                ("view_count", Value::Int(100)),
            ]),
        )
        .unwrap();
        tx.put(
            "videos",
            r(&[
                ("video_id", "v2".into()),
                ("channel_id", "c1".into()),
                ("view_count", Value::Int(300)),
            ]),
        )
        .unwrap();
        tx.put(
            "videos",
            r(&[
                ("video_id", "v3".into()),
                ("channel_id", "c2".into()),
                ("view_count", Value::Int(50)),
            ]),
        )
        .unwrap();
        tx.put(
            "channels",
            r(&[("channel_id", "c1".into()), ("title", "Alpha".into())]),
        )
        .unwrap();
        tx.put(
            "channels",
            r(&[("channel_id", "c2".into()), ("title", "Beta".into())]),
        )
        .unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn aggregates_and_group_counts() {
        let (_d, e) = open();
        let mut e = e;
        seed(&mut e);
        assert_eq!(sum(&e, "videos", "view_count", None).unwrap(), 450.0);
        assert_eq!(avg(&e, "videos", "view_count", None).unwrap(), 150.0);
        assert_eq!(min(&e, "videos", "view_count", None).unwrap(), Some(50.0));
        assert_eq!(max(&e, "videos", "view_count", None).unwrap(), Some(300.0));
        assert_eq!(count(&e, "videos", None).unwrap(), 3);
        let gc = group_counts(&e, "videos", "channel_id").unwrap();
        assert_eq!(gc[0], ("c1".to_string(), 2));
        assert_eq!(gc[1], ("c2".to_string(), 1));
    }

    #[test]
    fn filtered_aggregate() {
        let (_d, e) = open();
        let mut e = e;
        seed(&mut e);
        let f = Some(("channel_id", &Value::Text("c1".into())));
        assert_eq!(sum(&e, "videos", "view_count", f).unwrap(), 400.0);
        assert_eq!(count(&e, "videos", f).unwrap(), 2);
    }

    #[test]
    fn join_on_key() {
        let (_d, e) = open();
        let mut e = e;
        seed(&mut e);
        let joined = join(&e, "videos", "channels", "channel_id", "channel_id").unwrap();
        assert_eq!(joined.len(), 3);
        // Every joined pair carries the channel title.
        assert!(joined.iter().all(|(_, c)| c.get("title").is_some()));
    }

    proptest::proptest! {
        // sum of a non-negative column >= 0 and >= max individual value.
        #[test]
        fn sum_geq_each_value(
            vals in proptest::collection::vec(0u64..10_000, 0..50),
        ) {
            let (_d, e) = open();
            let mut e = e;
            let mut tx = e.begin();
            for (i, v) in vals.iter().enumerate() {
                tx.put("videos", r(&[
                    ("video_id", format!("v{i}").into()),
                    ("channel_id", "c1".into()),
                    ("view_count", Value::Int(*v as i64)),
                ])).unwrap();
            }
            tx.commit().unwrap();
            let s = sum(&e, "videos", "view_count", None).unwrap();
            let expected: u64 = vals.iter().sum();
            prop_assert_eq!(s, expected as f64);
        }
    }
}
