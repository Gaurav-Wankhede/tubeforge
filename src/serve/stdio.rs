//! JSON-RPC over stdio — the agent-harness bridge.
//!
//! Agent harnesses (OpenCode, Claude Code, Codex, Hermes, Pi Agent, ...)
//! spawn `tubeforge rpc` and speak the **same JSON-RPC method surface** as
//! the WebSocket dashboard (`serve::rpc`), but over **line-delimited
//! stdin/stdout** instead of a separate network server.
//!
//! Contract (identical to the WebSocket protocol — one protocol, two
//! transports):
//!   Client → stdout-of-process input (stdin): {"id":"r1","method":"ideas.analyze","params":{...}}
//!   Process → stdout:                        {"id":"r1","type":"progress","progress":0.3,"message":"..."}
//!   Process → stdout:                        {"id":"r1","type":"result","data":{...}}
//!   Parse error → stdout:                    {"id":null,"type":"error","error":{"code":-32700,"message":"..."}}
//!
//! stdout carries **only** responses (one JSON object per line); every
//! diagnostic/log goes to stderr. The process runs until stdin EOF.

use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::ytdlp::YtdlpClient;
use crate::serve::rpc::{self, RpcError, RpcRequest, RpcResponse};
use crate::serve::AppState;
use crate::storage::db::Db;

/// `tubeforge rpc`: build the shared state (open the DB, opt-in yt-dlp) and
/// serve JSON-RPC over stdio until stdin closes. Long-running — like `serve`,
/// it never emits the LLD §4.2 JSON envelope; stdout is reserved for responses.
pub async fn run(cfg: &Config) -> Result<(), TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let ytdlp = YtdlpClient::new(
        cfg.ytdlp_path.clone(),
        cfg.ytdlp_enabled,
        cfg.ytdlp_client.clone(),
        cfg.ytdlp_js_runtime.clone(),
    )
    .ok();
    let state = AppState {
        db: Arc::new(db),
        bind: "stdio".to_string(),
        ytdlp,
        data_dir: cfg.data_dir.clone(),
        own_channel: cfg.own_channel.clone(),
        kg: Arc::new(std::sync::Mutex::new(None)),
        sync_status: Arc::new(std::sync::Mutex::new(Default::default())),
    };
    serve_stdio(state).await
}

/// Serve JSON-RPC over stdio: read one request per stdin line, dispatch to
/// the shared `serve::rpc` handlers, and stream responses to stdout.
pub(crate) async fn serve_stdio(state: AppState) -> Result<(), TubeforgeError> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // stdout forwarder: one JSON response per line, flushed per message so the
    // agent can consume results incrementally (progress → result streaming).
    let forwarder = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(line) = rx.recv().await {
            let mut bytes = line.into_bytes();
            bytes.push(b'\n');
            if out.write_all(&bytes).await.is_err() {
                break; // stdout closed (agent exited)
            }
            if out.flush().await.is_err() {
                break;
            }
        }
    });

    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    loop {
        let line = match stdin.next_line().await {
            Ok(Some(l)) => l,
            Ok(None) => break, // stdin EOF → clean exit
            Err(e) => {
                return Err(TubeforgeError::Storage {
                    code: "STDIN".into(),
                    message: e.to_string(),
                });
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: RpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let err = RpcResponse::Error {
                    id: Value::Null,
                    error: RpcError {
                        code: -32700,
                        message: format!("parse error: {e}"),
                    },
                };
                let _ = rpc::send(&tx, &err).await;
                continue;
            }
        };

        // Owned clones so no borrow of the loop-local is held across await.
        let owned_state = state.clone();
        let owned_sender = tx.clone();
        rpc::dispatch(owned_state, owned_sender, req).await;
    }

    drop(tx); // EOF: signal the forwarder to finish
    let _ = forwarder.await;
    Ok(())
}
