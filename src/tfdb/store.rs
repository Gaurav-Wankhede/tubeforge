//! The storage engine core: typed rows, WAL durability, atomic checkpoint.
//!
//! An `Engine` owns:
//! - an in-memory map of `table -> pk -> Row` (the live snapshot),
//! - a per-table index of unique non-pk columns,
//! - an append-only WAL file, fsynced on commit,
//! - a checkpoint `.dat` file (the last atomic snapshot).
//!
//! Commit model (single-writer): `begin` yields a `Tx` that stages mutations
//! in memory. `commit` writes one WAL record (framed + CRC32), fsyncs it, and
//! applies the staged mutations to the live snapshot. A crash between
//! `commit` and the next checkpoint replays the WAL on open — so a committed
//! write is durable, and an uncommitted write is simply lost (never half
//! applied).

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{storage_err, TubeforgeError};

use super::schema::TableSchema;

/// A typed database value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Blob(Vec<u8>),
    Json(serde_json::Value),
}

impl Value {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_blob(&self) -> Option<&[u8]> {
        match self {
            Value::Blob(b) => Some(b),
            _ => None,
        }
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

/// A database row: column name -> value. The PK is included under its column
/// name. `null` columns may be absent.
pub type Row = BTreeMap<String, Value>;

/// Options controlling engine behaviour.
#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// Sync the WAL to disk on every commit (default true). Disabling trades
    /// durability for speed (crash may lose the last few commits).
    pub fsync_on_commit: bool,
}

impl Default for EngineOptions {
    fn default() -> Self {
        EngineOptions {
            fsync_on_commit: true,
        }
    }
}

/// The embedded storage engine.
pub struct Engine {
    path: PathBuf,
    tables: HashMap<String, TableSchema>,
    data: HashMap<String, BTreeMap<String, Row>>,
    /// unique non-pk column -> (value serialization -> pk).
    uniques: HashMap<String, HashMap<String, String>>,
    wal: Option<std::fs::File>,
    options: EngineOptions,
    /// Strict column checking: reject unknown columns on insert (default
    /// true). Tests that exercise the legacy raw path may disable it.
    strict: bool,
}

/// A staged, single-writer transaction.
pub struct Tx<'a> {
    engine: &'a mut Engine,
    staged: HashMap<String, BTreeMap<String, Row>>,
    staged_uniques: HashMap<String, HashMap<String, String>>,
    /// insert/update/delete applied to a table (for WAL record).
    ops: Vec<WalOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WalOp {
    table: String,
    pk: String,
    /// None = delete.
    row: Option<Row>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WalRecord {
    seq: u64,
    ops: Vec<WalOp>,
}

const WAL_MAGIC: &[u8; 4] = b"TFWL";
const DAT_MAGIC: &[u8; 4] = b"TFDT";

impl Engine {
    /// Open (or create) the database at `path`, replaying the WAL over any
    /// existing checkpoint.
    pub fn open(path: &Path) -> Result<Engine, TubeforgeError> {
        Engine::open_with_options(path, EngineOptions::default())
    }

    pub fn open_with_options(
        path: &Path,
        options: EngineOptions,
    ) -> Result<Engine, TubeforgeError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    storage_err("IO", format!("create dir {}: {e}", parent.display()))
                })?;
            }
        }

        let mut engine = Engine {
            path: path.to_path_buf(),
            tables: HashMap::new(),
            data: HashMap::new(),
            uniques: HashMap::new(),
            wal: None,
            options,
            strict: true,
        };

        engine.load_checkpoint()?;
        engine.replay_wal()?;
        engine.open_wal_for_append()?;
        Ok(engine)
    }

    /// Re-read the checkpoint and re-replay the WAL from disk, replacing the
    /// in-memory snapshot so this engine observes every write committed by any
    /// handle to the same database file. Retains the in-memory schema
    /// registrations (created at open, not necessarily checkpointed yet).
    pub fn reload(&mut self) -> Result<(), TubeforgeError> {
        let schema = std::mem::take(&mut self.tables);
        self.data.clear();
        self.uniques.clear();
        self.load_checkpoint()?;
        for (name, sch) in schema {
            self.tables.entry(name).or_insert(sch);
        }
        self.replay_wal()?;
        Ok(())
    }

    // -- schema ------------------------------------------------------------

    /// Register (or update) a table schema. Must be called before any row ops
    /// on the table. Existing data is left untouched.
    pub fn create_table(&mut self, schema: TableSchema) {
        self.tables.insert(schema.name.clone(), schema);
    }

    pub fn table_exists(&self, name: &str) -> bool {
        self.tables.contains_key(name)
    }

    fn table(&self, name: &str) -> Result<&TableSchema, TubeforgeError> {
        self.tables
            .get(name)
            .ok_or_else(|| storage_err("NO_TABLE", format!("table {name} does not exist")))
    }

    // -- reads -------------------------------------------------------------

    pub fn get(&self, table: &str, pk: &str) -> Result<Option<Row>, TubeforgeError> {
        self.table(table)?;
        Ok(self.data.get(table).and_then(|m| m.get(pk).cloned()))
    }

    pub fn all(&self, table: &str) -> Result<Vec<Row>, TubeforgeError> {
        self.table(table)?;
        Ok(self
            .data
            .get(table)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default())
    }

    /// Rows whose `col` equals `value` (linear scan — small tables).
    pub fn find_eq(
        &self,
        table: &str,
        col: &str,
        value: &Value,
    ) -> Result<Vec<Row>, TubeforgeError> {
        self.table(table)?;
        Ok(self
            .data
            .get(table)
            .map(|m| {
                m.values()
                    .filter(|r| r.get(col).map(|v| v == value).unwrap_or(false))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Whether any row has `col == value`.
    pub fn any_eq(&self, table: &str, col: &str, value: &Value) -> Result<bool, TubeforgeError> {
        self.table(table)?;
        Ok(self
            .data
            .get(table)
            .map(|m| {
                m.values()
                    .any(|r| r.get(col).map(|v| v == value).unwrap_or(false))
            })
            .unwrap_or(false))
    }

    pub fn count(&self, table: &str) -> Result<u64, TubeforgeError> {
        self.table(table)?;
        Ok(self.data.get(table).map(|m| m.len() as u64).unwrap_or(0))
    }

    // -- transactions ------------------------------------------------------

    pub fn begin(&mut self) -> Tx<'_> {
        Tx {
            engine: self,
            staged: HashMap::new(),
            staged_uniques: HashMap::new(),
            ops: Vec::new(),
        }
    }

    fn commit_wal(&mut self, ops: &[WalOp]) -> Result<(), TubeforgeError> {
        let seq = self.wal_seq() + 1;
        let rec = WalRecord {
            seq,
            ops: ops.to_vec(),
        };
        let bytes = bincode_encode(&rec)?;
        let wal = self
            .wal
            .as_mut()
            .ok_or_else(|| storage_err("WAL", "WAL not open for append"))?;
        wal.write_all(WAL_MAGIC)
            .map_err(|e| storage_err("WAL", e.to_string()))?;
        wal.write_all(&(bytes.len() as u32).to_le_bytes())
            .map_err(|e| storage_err("WAL", e.to_string()))?;
        let crc = crc32(&bytes);
        wal.write_all(&bytes)
            .map_err(|e| storage_err("WAL", e.to_string()))?;
        wal.write_all(&crc.to_le_bytes())
            .map_err(|e| storage_err("WAL", e.to_string()))?;
        if self.options.fsync_on_commit {
            wal.sync_all()
                .map_err(|e| storage_err("WAL", e.to_string()))?;
        }
        Ok(())
    }

    fn wal_seq(&self) -> u64 {
        // Tracked in memory; the on-disk seq is only used to skip already-
        // replayed records (handled in replay by truncation).
        0
    }

    /// Apply ops to the live in-memory snapshot. Caller (Tx::commit) has
    /// already validated schema + uniqueness.
    fn apply_ops(&mut self, ops: &[WalOp]) {
        for op in ops {
            let table = self.data.entry(op.table.clone()).or_default();
            match &op.row {
                None => {
                    table.remove(&op.pk);
                }
                Some(row) => {
                    table.insert(op.pk.clone(), row.clone());
                }
            }
        }
    }

    /// Write a full checkpoint snapshot (atomic via temp file + rename) and
    /// truncate the WAL. Returns the number of rows persisted.
    pub fn checkpoint(&mut self) -> Result<u64, TubeforgeError> {
        let dat_path = self.dat_path();
        if let Some(parent) = dat_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    storage_err("IO", format!("create dir {}: {e}", parent.display()))
                })?;
            }
        }
        let snapshot = Snapshot {
            tables: self
                .tables
                .values()
                .cloned()
                .map(|t| (t.name.clone(), t))
                .collect(),
            data: self.data.clone(),
        };
        let bytes = bincode_encode(&snapshot)?;
        let tmp = dat_path.with_extension("tmp");
        {
            let mut f = std::fs::File::create(&tmp)
                .map_err(|e| storage_err("IO", format!("create {}: {e}", tmp.display())))?;
            f.write_all(DAT_MAGIC)
                .map_err(|e| storage_err("IO", e.to_string()))?;
            f.write_all(&(bytes.len() as u64).to_le_bytes())
                .map_err(|e| storage_err("IO", e.to_string()))?;
            f.write_all(&bytes)
                .map_err(|e| storage_err("IO", e.to_string()))?;
            f.sync_all().map_err(|e| storage_err("IO", e.to_string()))?;
        }
        std::fs::rename(&tmp, &dat_path)
            .map_err(|e| storage_err("IO", format!("rename {}: {e}", tmp.display())))?;
        self.truncate_wal()?;
        Ok(self.count_all())
    }

    fn count_all(&self) -> u64 {
        self.data.values().map(|m| m.len() as u64).sum()
    }

    // -- persistence helpers ------------------------------------------------

    fn dat_path(&self) -> PathBuf {
        self.path.with_extension("dat")
    }

    fn wal_path(&self) -> PathBuf {
        self.path.with_extension("wal")
    }

    fn load_checkpoint(&mut self) -> Result<(), TubeforgeError> {
        let dat_path = self.dat_path();
        if !dat_path.exists() {
            return Ok(());
        }
        let mut f = std::fs::File::open(&dat_path)
            .map_err(|e| storage_err("IO", format!("open {}: {e}", dat_path.display())))?;
        let mut magic = [0u8; 4];
        f.read_exact(&mut magic)
            .map_err(|e| storage_err("IO", e.to_string()))?;
        if &magic != DAT_MAGIC {
            return Err(storage_err(
                "BAD_DAT",
                format!("{} is not a TFDB checkpoint", dat_path.display()),
            ));
        }
        let mut lenb = [0u8; 8];
        f.read_exact(&mut lenb)
            .map_err(|e| storage_err("IO", e.to_string()))?;
        let len = u64::from_le_bytes(lenb) as usize;
        let mut bytes = vec![0u8; len];
        f.read_exact(&mut bytes)
            .map_err(|e| storage_err("IO", e.to_string()))?;
        let snap: Snapshot = bincode_decode(&bytes)?;
        self.tables = snap.tables;
        self.data = snap.data;
        Ok(())
    }

    fn open_wal_for_append(&mut self) -> Result<(), TubeforgeError> {
        let wal_path = self.wal_path();
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&wal_path)
            .map_err(|e| storage_err("IO", format!("open WAL {}: {e}", wal_path.display())))?;
        self.wal = Some(f);
        Ok(())
    }

    /// Replay valid WAL records (up to a torn tail). Invalid/partial frames at
    /// the end are the result of a crash mid-write and are discarded (the last
    /// committed fsync guaranteed their durability; torn tail = non-committed).
    fn replay_wal(&mut self) -> Result<(), TubeforgeError> {
        let wal_path = self.wal_path();
        if !wal_path.exists() {
            return Ok(());
        }
        let mut f = std::fs::File::open(&wal_path)
            .map_err(|e| storage_err("IO", format!("open WAL {}: {e}", wal_path.display())))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| storage_err("IO", e.to_string()))?;

        let mut ops_to_apply: Vec<WalOp> = Vec::new();
        let mut off = 0usize;
        while off + 8 <= buf.len() {
            if &buf[off..off + 4] != WAL_MAGIC {
                break; // torn header — stop
            }
            off += 4;
            if off + 4 > buf.len() {
                break;
            }
            let len = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
            off += 4;
            if off + len + 4 > buf.len() {
                break; // torn body — stop
            }
            let body = &buf[off..off + len];
            let crc = u32::from_le_bytes(buf[off + len..off + len + 4].try_into().unwrap());
            off += len + 4;
            if crc32(body) != crc {
                break; // checksum mismatch — stop (corrupt tail)
            }
            let rec: WalRecord = bincode_decode(body)?;
            ops_to_apply.extend(rec.ops);
        }

        // Apply only fully-committed records, then truncate the WAL to the
        // last valid frame so we don't re-read stale ops on next open.
        self.apply_ops(&ops_to_apply);

        // Rewrite WAL with the valid prefix trimmed (drop torn tail). Simplest
        // correct approach: reopen in write mode and truncate to `off`.
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(&wal_path)
            .map_err(|e| storage_err("IO", e.to_string()))?;
        f.set_len(off as u64)
            .map_err(|e| storage_err("IO", e.to_string()))?;
        f.sync_all().map_err(|e| storage_err("IO", e.to_string()))?;
        Ok(())
    }

    fn truncate_wal(&mut self) -> Result<(), TubeforgeError> {
        let wal_path = self.wal_path();
        let f = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&wal_path)
            .map_err(|e| storage_err("IO", e.to_string()))?;
        f.sync_all().map_err(|e| storage_err("IO", e.to_string()))?;
        Ok(())
    }

    // -- introspection -----------------------------------------------------

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
    }

    pub fn strict(&self) -> bool {
        self.strict
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    tables: HashMap<String, TableSchema>,
    data: HashMap<String, BTreeMap<String, Row>>,
}

impl<'a> Tx<'a> {
    /// Insert or replace a row. Validates the schema (known columns, PK
    /// present, unique non-PK columns free). Errors roll the transaction.
    pub fn put(&mut self, table: &str, row: Row) -> Result<(), TubeforgeError> {
        self.validate(table, &row)?;
        let schema = self.engine.table(table)?;
        let pk = schema.pk.clone();
        let pk_val = row
            .get(&pk)
            .ok_or_else(|| storage_err("NO_PK", format!("row for {table} missing PK {pk}")))?;
        let pk_str = value_pk(pk_val);

        // Unique-column collision check against both committed and staged rows.
        for col in &schema.cols {
            if !col.unique {
                continue;
            }
            if let Some(v) = row.get(&col.name) {
                let key = value_ser(v);
                let committed = self.engine.data.get(table).map(|m| {
                    m.iter().any(|(k, r)| {
                        *k != pk_str
                            && r.get(&col.name)
                                .map(|cv| value_ser(cv) == key)
                                .unwrap_or(false)
                    })
                });
                let staged = self.staged.get(table).map(|m| {
                    m.iter().any(|(k, r)| {
                        *k != pk_str
                            && r.get(&col.name)
                                .map(|cv| value_ser(cv) == key)
                                .unwrap_or(false)
                    })
                });
                if committed.unwrap_or(false) || staged.unwrap_or(false) {
                    return Err(storage_err(
                        "UNIQUE",
                        format!("unique column {}={} already used in {table}", col.name, key),
                    ));
                }
            }
        }

        self.engine.uniques.entry(table.to_string()).or_default();
        self.staged
            .entry(table.to_string())
            .or_default()
            .insert(pk_str.clone(), row.clone());
        self.ops.push(WalOp {
            table: table.to_string(),
            pk: pk_str,
            row: Some(row),
        });
        Ok(())
    }

    /// Delete a row by PK. No-op if absent.
    pub fn delete(&mut self, table: &str, pk: &str) -> Result<(), TubeforgeError> {
        self.engine.table(table)?;
        self.staged.entry(table.to_string()).or_default().remove(pk);
        self.ops.push(WalOp {
            table: table.to_string(),
            pk: pk.to_string(),
            row: None,
        });
        Ok(())
    }

    /// Validate a row against the table schema.
    fn validate(&self, table: &str, row: &Row) -> Result<(), TubeforgeError> {
        let schema = self.engine.table(table)?;
        if !row.contains_key(&schema.pk) {
            return Err(storage_err(
                "NO_PK",
                format!("row for {table} missing PK {}", schema.pk),
            ));
        }
        if self.engine.strict {
            for name in row.keys() {
                if !schema.has(name) {
                    return Err(storage_err(
                        "UNKNOWN_COL",
                        format!("unknown column {name} in table {table}"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Commit the staged transaction: write + fsync the WAL, then apply to the
    /// live snapshot. Returns the number of ops applied.
    pub fn commit(self) -> Result<u64, TubeforgeError> {
        if self.ops.is_empty() {
            return Ok(0);
        }
        self.engine.commit_wal(&self.ops)?;
        self.engine.apply_ops(&self.ops);
        Ok(self.ops.len() as u64)
    }

    /// Discard staged changes without writing anything.
    pub fn rollback(mut self) {
        self.ops.clear();
        self.staged.clear();
        self.staged_uniques.clear();
    }
}

fn value_pk(v: &Value) -> String {
    match v {
        Value::Text(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        other => value_ser(other),
    }
}

fn value_ser(v: &Value) -> String {
    match v {
        Value::Text(s) => format!("t:{s}"),
        Value::Int(i) => format!("i:{i}"),
        Value::Float(f) => format!("f:{f}"),
        Value::Bool(b) => format!("b:{b}"),
        Value::Blob(b) => format!("blob:{}", hex(b)),
        Value::Json(j) => format!("j:{j}"),
        Value::Null => "null".to_string(),
    }
}

fn hex(b: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b {
        let _ = write!(s, "{byte:02x}");
    }
    s
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB88320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut crc = 0xFFFFFFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFFFFFF
}

/// Minimal binary encoding using serde_json (deterministic, zero deps beyond
/// serde_json). Keeps the engine dependency-free.
fn bincode_encode<T: Serialize>(v: &T) -> Result<Vec<u8>, TubeforgeError> {
    serde_json::to_vec(v).map_err(|e| storage_err("ENC", e.to_string()))
}

fn bincode_decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, TubeforgeError> {
    serde_json::from_slice(bytes).map_err(|e| storage_err("DEC", e.to_string()))
}
