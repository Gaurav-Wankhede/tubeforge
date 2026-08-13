//! Static file serving with SPA fallback for the React frontend.
//!
//! Serves files from `frontend/dist` (the Vite production build). For any
//! path that is not an existing file, serves `index.html` — the SPA shell —
//! so client-side routing works. Replaces `tower-http`'s `ServeDir`/`ServeFile`.

use std::path::{Path, PathBuf};

use http::{Method, Response as HttpResponse, StatusCode};

use super::Response;

/// Static-file root + SPA shell.
pub struct Spa {
    root: PathBuf,
    index: PathBuf,
}

impl Spa {
    pub fn new(root: PathBuf, index: PathBuf) -> Self {
        Spa { root, index }
    }
}

/// Serve a static file or the SPA shell. Returns `None` when the path is not
/// a file and should fall through to the generic fallback handler (e.g. an
/// HTMX 404 when no SPA is present).
pub async fn try_serve(spa: &Spa, method: &Method, path: &str) -> Option<Response> {
    // Only GET/HEAD for static assets.
    if method != Method::GET && method != Method::HEAD {
        return None;
    }
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        // Directory root → serve the shell.
        return Some(shell_response(spa).await);
    }
    // Reject path traversal outright.
    if path.contains("..") {
        return Some(not_found());
    }
    let file = spa.root.join(path);
    // Only serve actual files under root; directories → SPA shell.
    if file.is_file() {
        match tokio::fs::read(&file).await {
            Ok(bytes) => {
                let ct = mime_guess::from_path(&file).first_or_octet_stream();
                return Some(
                    HttpResponse::builder()
                        .status(StatusCode::OK)
                        .header("content-type", ct.to_string())
                        .body(super::full(hyper::body::Bytes::from(bytes)))
                        .expect("static response"),
                );
            }
            Err(_) => return Some(not_found()),
        }
    }
    // Not a file → SPA shell (client-side routing).
    Some(shell_response(spa).await)
}

async fn shell_response(spa: &Spa) -> Response {
    match tokio::fs::read(&spa.index).await {
        Ok(bytes) => HttpResponse::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html; charset=utf-8")
            .body(super::full(hyper::body::Bytes::from(bytes)))
            .expect("spa shell response"),
        Err(_) => not_found(),
    }
}

fn not_found() -> Response {
    HttpResponse::builder()
        .status(StatusCode::NOT_FOUND)
        .body(super::full(hyper::body::Bytes::from("not found")))
        .expect("404 response")
}

/// Verify the file is within the root (defense-in-depth for the `..` check).
#[allow(dead_code)]
fn is_within(root: &Path, file: &Path) -> bool {
    file.starts_with(root)
}
