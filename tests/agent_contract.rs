//! Agent interface contract (Phase 3 workstream B): EVERY subcommand's
//! `--json` output must be the stable LLD §4.2 envelope — `{ok, data, meta,
//! error}` on stdout, nothing else (no ANSI, no logs), correct exit codes,
//! `error` null/absent on success. Runs the real binary via
//! `CARGO_BIN_EXE_tubeforge`.
//!
//! Representative set: init / health / score / ideas / keywords (add +
//! report) / scorecard / alerts / export / check availability (no-key error
//! path) / thumbnail list-templates / mcp.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

/// Run the tubeforge binary with `--json` and a temp DB, assert the full
/// success contract, and return `data`.
fn run_ok(args: &[&str], envs: &[(&str, &str)]) -> Value {
    let out = run_cli(args, envs);
    assert_eq!(out.status.code(), Some(0), "exit 0 for {args:?}");
    let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
    assert!(!stdout.contains('\u{1b}'), "no ANSI escapes in --json stdout");
    let env: Value = serde_json::from_str(stdout.trim()).expect("stdout is one JSON envelope");
    assert_eq!(env["ok"], true, "ok==true for {args:?}");
    assert!(env.get("data").is_some(), "data present for {args:?}");
    assert!(
        env["meta"]["duration_ms"].is_u64(),
        "meta.duration_ms present for {args:?}"
    );
    assert!(env.get("error").is_none(), "error absent on success for {args:?}");
    env["data"].clone()
}

fn run_cli(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tubeforge"));
    cmd.arg("--json");
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("binary runs")
}

/// Temp env context: a dedicated DB + data root per test so no real user
/// data is touched, and the API key forced empty (no accidental network).
struct Ctx {
    _dir: tempfile::TempDir,
    db: PathBuf,
    data: PathBuf,
}

impl Ctx {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("data");
        Ctx {
            db: dir.path().join("tf.db"),
            data,
            _dir: dir,
        }
    }

    fn envs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("TUBEFORGE_DB_PATH", self.db.to_string_lossy().to_string()),
            ("TUBEFORGE_DATA_DIR", self.data.to_string_lossy().to_string()),
            ("YOUTUBE_API_KEY", String::new()),
        ]
    }
}

#[test]
fn agent_contract_init() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let data = run_ok(&["init"], &env_refs);
    assert!(data["db_path"].is_string());
    assert_eq!(data["integrity"], "ok");
}

#[test]
fn agent_contract_health() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);
    let data = run_ok(&["health"], &env_refs);
    assert!(data["counts"]["videos"].is_u64());
    assert!(data["privacy"]["unlisted"].is_u64(), "privacy census present");
    assert!(data["integrity"].is_string());
}

#[test]
fn agent_contract_score_draft() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);
    let data = run_ok(
        &["score", "--draft-title", "Rust Database Engineering Guide"],
        &env_refs,
    );
    assert!(data["seo"]["total"].is_f64());
    assert!(data["geo"]["total"].is_f64());
    assert!(data["total"].is_f64());
}

#[test]
fn agent_contract_ideas() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);
    let data = run_ok(&["ideas", "--limit", "3"], &env_refs);
    assert!(data["ideas"].is_array(), "empty corpus → empty pool, still ok");
}

#[test]
fn agent_contract_keywords() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);
    let data = run_ok(&["keywords", "add", "rust", "database"], &env_refs);
    assert_eq!(data["added"], 2);
    let data = run_ok(&["keywords", "report"], &env_refs);
    assert!(data["keywords"].is_array());
}

#[test]
fn agent_contract_scorecard() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);
    let data = run_ok(&["scorecard"], &env_refs);
    assert!(data["channels"].is_array());
    assert_eq!(data["compared"], 0, "empty set still succeeds");
}

#[test]
fn agent_contract_alerts() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);
    let data = run_ok(&["alerts"], &env_refs);
    assert!(data["alerts"].is_array());
    assert!(data["inserted"].is_u64());
    let data = run_ok(&["alerts", "list"], &env_refs);
    assert!(data["alerts"].is_array());
}

#[test]
fn agent_contract_export() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);
    let out_dir = ctx._dir.path().join("export");
    let data = run_ok(
        &[
            "export",
            "--out",
            out_dir.to_str().expect("path"),
            "--format",
            "dir",
        ],
        &env_refs,
    );
    assert_eq!(data["format"], "dir");
    assert!(out_dir.join("manifest.json").is_file());
    assert!(out_dir.join("videos.csv").is_file());
}

/// No API key → clear Config error (exit 1), full envelope on stdout, never
/// a silent no-op.
#[test]
fn agent_contract_check_availability_no_key() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);

    let out = run_cli(&["check", "availability"], &env_refs);
    assert_eq!(out.status.code(), Some(1), "config error exit code");
    let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
    assert!(!stdout.contains('\u{1b}'), "no ANSI in error envelope");
    let env: Value = serde_json::from_str(stdout.trim()).expect("error envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "CONFIG");
    assert!(env["error"]["message"].is_string());
    assert!(env.get("data").is_none());
}

#[test]
fn agent_contract_thumbnail_list_templates() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);
    let data = run_ok(&["thumbnail", "list-templates"], &env_refs);
    let templates = data["templates"].as_array().expect("templates array");
    assert!(!templates.is_empty());
}

/// `mcp --json` must still yield a valid `.mcp.json` snippet under `data`
/// (LLD §4.2 / ADR-8) — the machine path for agent MCP setup.
#[test]
fn agent_contract_mcp_snippet() {
    let ctx = Ctx::new();
    let envs = ctx.envs();
    let env_refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_ok(&["init"], &env_refs);
    let data = run_ok(&["mcp"], &env_refs);
    let server = &data["mcpServers"]["tubeforge"];
    assert_eq!(server["command"], "tursodb");
    assert_eq!(server["args"][1], "--mcp");
}
