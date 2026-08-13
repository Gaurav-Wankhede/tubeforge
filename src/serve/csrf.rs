//! Localhost CSRF guard for the dashboard's POST endpoints (LLD §4.1 —
//! dashboard conventions; PRD §5.9 privacy).
//!
//! Threat model: the dashboard is a single-user loopback server with no
//! authentication. The remaining risk is **local CSRF**: a malicious webpage
//! in the user's browser POSTing to `http://127.0.0.1:<port>/...` and, say,
//! flipping an idea status or clearing alerts. Browsers attach an
//! `Origin`/`Referer` header to cross-site POSTs, so the guard compares the
//! presented origin's host:port against the bound loopback address.
//!
//! Policy (documented):
//! - `Origin` present → host:port must match the bound address
//!   (`localhost` ≡ `127.0.0.1`; scheme must be http/https). Mismatch → 403.
//! - No `Origin` → `Referer`'s origin is checked the same way.
//! - Neither present → allowed: non-browser local clients (curl, scripts,
//!   AI agents) send no Origin and cannot be tricked by browser CSRF.

use http::HeaderMap;

/// True when the request passes the origin guard against the bound
/// `host:port` string (e.g. `127.0.0.1:8080`).
pub fn origin_allowed(headers: &HeaderMap, bind: &str) -> bool {
    match origin_from(headers) {
        Some(Ok(origin)) => origins_match(&origin, bind),
        // Origin/Referer present but not a parseable http(s) origin
        // (e.g. `Origin: null` from a sandboxed iframe) — reject.
        Some(Err(())) => false,
        // Neither header present: non-browser local client (curl, scripts).
        None => true,
    }
}

/// `Ok(host[:port])` when a valid http(s) origin was presented, `Err(())`
/// when a header was present but unparseable, `None` when absent.
fn origin_from(headers: &HeaderMap) -> Option<Result<String, ()>> {
    if let Some(v) = headers.get(http::header::ORIGIN) {
        return Some(origin_of_url(v.to_str().ok()?));
    }
    headers
        .get(http::header::REFERER)
        .map(|v| match v.to_str() {
            Ok(s) => origin_of_url(s),
            Err(_) => Err(()),
        })
}

/// `scheme://host[:port]/path...` → `Ok(host[:port])`; `null` and garbage
/// (no `://`, or a non-http(s) scheme) → `Err(())`.
fn origin_of_url(url: &str) -> Result<String, ()> {
    let scheme_end = url.find("://").ok_or(())?;
    let scheme = &url[..scheme_end];
    if !matches!(scheme, "http" | "https") {
        return Err(());
    }
    let rest = &url[scheme_end + 3..];
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    Ok(host_port.to_string())
}

/// Compare `origin` (host[:port]) against the bound `host:port`. `localhost`
/// and `127.0.0.1` are the same endpoint; a missing origin port (default
/// http port) can only match a bind with no explicit port.
fn origins_match(origin: &str, bind: &str) -> bool {
    let (oh, op) = split_host_port(origin);
    let (bh, bp) = split_host_port(bind);
    normalize_host(oh) == normalize_host(bh) && op == bp
}

/// `host[:port]` → (host, Some(port)); a bare host has no port.
fn split_host_port(s: &str) -> (&str, Option<&str>) {
    match s.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p)),
        _ => (s, None),
    }
}

/// Lowercase, bracket-stripped host; `localhost` is normalized to the
/// loopback address it resolves to on every platform we ship.
fn normalize_host(host: &str) -> String {
    let h = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if h == "localhost" {
        "127.0.0.1".to_string()
    } else {
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            let key = http::HeaderName::from_bytes(k.as_bytes()).expect("header name");
            h.insert(key, v.parse().unwrap());
        }
        h
    }

    #[test]
    fn matching_origin_passes() {
        let h = headers(&[("origin", "http://127.0.0.1:8080")]);
        assert!(origin_allowed(&h, "127.0.0.1:8080"));
        // localhost is the same loopback endpoint.
        let h = headers(&[("origin", "http://localhost:8080")]);
        assert!(origin_allowed(&h, "127.0.0.1:8080"));
    }

    #[test]
    fn foreign_origin_is_rejected() {
        let h = headers(&[("origin", "http://evil.example:8080")]);
        assert!(!origin_allowed(&h, "127.0.0.1:8080"));
        // Right host, wrong port — also rejected.
        let h = headers(&[("origin", "http://127.0.0.1:9999")]);
        assert!(!origin_allowed(&h, "127.0.0.1:8080"));
        // Browser sandbox `Origin: null` cannot match.
        let h = headers(&[("origin", "null")]);
        assert!(!origin_allowed(&h, "127.0.0.1:8080"));
    }

    #[test]
    fn referer_is_checked_when_origin_absent() {
        let h = headers(&[("referer", "http://127.0.0.1:8080/scores")]);
        assert!(origin_allowed(&h, "127.0.0.1:8080"));
        let h = headers(&[("referer", "http://evil.example/scores")]);
        assert!(!origin_allowed(&h, "127.0.0.1:8080"));
    }

    #[test]
    fn missing_headers_are_allowed() {
        let h = HeaderMap::new();
        assert!(origin_allowed(&h, "127.0.0.1:8080"));
    }

    #[test]
    fn non_http_schemes_are_rejected() {
        let h = headers(&[("origin", "file://127.0.0.1:8080")]);
        assert!(!origin_allowed(&h, "127.0.0.1:8080"));
        let h = headers(&[("origin", "javascript:alert(1)")]);
        assert!(!origin_allowed(&h, "127.0.0.1:8080"));
    }

    #[test]
    fn ipv6_loopback_normalizes() {
        let h = headers(&[("origin", "http://[::1]:8080")]);
        assert!(origin_allowed(&h, "[::1]:8080"));
        let h = headers(&[("origin", "http://[::1]:8080")]);
        assert!(!origin_allowed(&h, "127.0.0.1:8080"));
    }
}
