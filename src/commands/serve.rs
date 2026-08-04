//! `serve` (PRD §5.4 deferred item — HTMX dashboard): bind the local
//! dashboard server and run until Ctrl-C.
//!
//! Long-running command: it NEVER emits the JSON envelope (LLD §4.2 is the
//! CLI contract for one-shot commands) — stdout stays empty, the listening
//! line goes to stderr. The `--json` global flag is ignored for `serve`
//! (documented in `cli.rs` help text).

use crate::config::Config;
use crate::error::TubeforgeError;

/// `tubeforge serve --port <PORT>`: port resolution is flag > env
/// (`TUBEFORGE_SERVE_PORT`) > 8080; the server itself lives in
/// `crate::serve`.
pub async fn run(cfg: &Config, host: &str, port: Option<u16>) -> Result<(), TubeforgeError> {
    let port = match port {
        Some(p) => p,
        None => match std::env::var("TUBEFORGE_SERVE_PORT") {
            Ok(v) => v.parse().map_err(|_| {
                TubeforgeError::Config(format!("TUBEFORGE_SERVE_PORT not a number: {v}"))
            })?,
            Err(_) => 8080,
        },
    };
    crate::serve::run(cfg, host, port).await
}
