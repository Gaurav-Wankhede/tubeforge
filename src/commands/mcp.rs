//! `mcp` (ADR-8, LLD §4.1): print the external MCP server config snippet for
//! Claude Code / Cursor, pointing at `tursodb <db> --mcp`. Detects `tursodb`
//! on PATH; the human output is the config JSON itself (drop into .mcp.json).

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::util;

/// Install hint for tursodb (official installer script).
pub const TURSODB_INSTALL_HINT: &str =
    "install tursodb: curl -sSfL https://get.turso.tech/install.sh | sh (or: brew install tursodatabase/tap/turso)";

/// The config snippet `{ "mcpServers": { "tubeforge": ... } }` — valid for
/// both Claude Code (.mcp.json) and Cursor.
pub fn mcp_servers_config(db_path: &str) -> Value {
    json!({
        "mcpServers": {
            "tubeforge": {
                "command": "tursodb",
                "args": [db_path, "--mcp"],
            }
        }
    })
}

pub async fn run(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db_path = std::fs::canonicalize(&cfg.db_path)
        .unwrap_or_else(|_| cfg.db_path.clone())
        .to_string_lossy()
        .to_string();

    if util::on_path("tursodb") {
        tracing::info!("tursodb found on PATH");
    } else {
        eprintln!("tursodb: NOT FOUND on PATH — {TURSODB_INSTALL_HINT}");
        tracing::warn!("tursodb absent from PATH; MCP servers will fail to launch until installed");
    }

    Ok(mcp_servers_config(&db_path))
}
