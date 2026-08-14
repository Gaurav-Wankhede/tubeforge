//! `rpc` (agent-harness bridge): serve JSON-RPC over **stdio** using the same
//! method surface as the WebSocket dashboard. Agent harnesses (OpenCode,
//! Claude Code, Codex, Hermes, Pi Agent, ...) spawn `tubeforge rpc` and speak
//! line-delimited JSON-RPC on stdin/stdout for analysis — the tfdb database is
//! the storage source, and the frontend dashboard provides visual analysis.
//!
//! Long-running command: it NEVER emits the LLD §4.2 JSON envelope (stdout is
//! reserved for RPC responses only); `--json` is ignored.

use crate::config::Config;
use crate::error::TubeforgeError;

/// `tubeforge rpc`: open the shared state and serve until stdin EOF.
pub async fn run(cfg: &Config) -> Result<(), TubeforgeError> {
    crate::serve::stdio::run(cfg).await
}
