//! A minimal, dependency-light HTTP framework built directly on `hyper`.
//!
//! This module is the **Axum replacement**. It provides exactly the surface
//! the TubeForge server needs — method routing with `{param}` path segments,
//! a `State`/`Query`/`Path`/`HeaderMap` extractor set, `Html`/`Json`/SSE
//! response helpers, static-file serving with SPA fallback, and a Hyper-based
//! `serve` loop with graceful shutdown — implemented on `hyper` + `hyper-util`
//! + `http` + `tokio` with no web-framework dependency.
//!
//! Design notes:
//! - Routes are plain `(Method, PathPattern)` -> handler functions. Path
//!   segments use `{name}` syntax (axum 0.8 compatible). A capture like
//!   `{id}/{status}` yields two params in order.
//! - Extractors are opt-in: a handler that needs state/query/path declares a
//!   `(&str path, Query, ...)` shaped signature via the `handler!` macro, or
//!   just a plain `fn` with no extractors. See `HandlerFn`.
//! - Responses are built with `into_response` on `(StatusCode, body)` tuples
//!   and `Html`/`Json`/`Sse` wrappers.

pub mod sse;
pub mod static_files;
pub mod ws;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use http::{HeaderMap, Method, Request, Response as HttpResponse, StatusCode, Uri};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;

use crate::error::TubeforgeError;

pub type Body = http_body_util::combinators::BoxBody<hyper::body::Bytes, std::convert::Infallible>;
pub type Response = HttpResponse<Body>;

/// Build a full (static) body from bytes.
pub fn full(bytes: impl Into<hyper::body::Bytes>) -> Body {
    http_body_util::Full::new(bytes.into())
        .map_err(|never: std::convert::Infallible| match never {})
        .boxed()
}

/// Erase any `http_body::Body` into the crate `Body` type.
pub fn full_erase<B>(b: B) -> Body
where
    B: http_body::Body<Data = hyper::body::Bytes, Error = std::convert::Infallible>
        + Send
        + Sync
        + 'static,
{
    http_body_util::combinators::BoxBody::new(b)
}

/// The shared server state, wrapped for cheap cloning into handlers.
pub struct AppState<S> {
    pub inner: Arc<S>,
}

impl<S> Clone for AppState<S> {
    fn clone(&self) -> Self {
        AppState {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// A boxed future returned by a handler.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A parsed path pattern with its captures.
#[derive(Debug, Clone)]
struct PathPattern {
    /// literal/capture segments, in order.
    segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq)]
enum Segment {
    Literal(String),
    /// named capture, positionally indexed.
    Capture(usize),
}

impl PathPattern {
    fn parse(pattern: &str) -> Self {
        let mut segments = Vec::new();
        let mut cap = 0usize;
        for part in pattern.trim_matches('/').split('/') {
            if part.is_empty() {
                continue;
            }
            if let Some(name) = part.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                let _ = name;
                segments.push(Segment::Capture(cap));
                cap += 1;
            } else {
                segments.push(Segment::Literal(part.to_string()));
            }
        }
        PathPattern { segments }
    }

    /// Match a request path against the pattern, returning captures.
    fn match_path(&self, path: &str) -> Option<Vec<String>> {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            if self.segments.is_empty() {
                return Some(Vec::new());
            }
            return None;
        }
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() != self.segments.len() {
            return None;
        }
        let mut captures = Vec::new();
        for (part, seg) in parts.iter().zip(self.segments.iter()) {
            match seg {
                Segment::Literal(lit) if lit == *part => {}
                Segment::Literal(_) => return None,
                Segment::Capture(_) => captures.push((*part).to_string()),
            }
        }
        Some(captures)
    }
}

/// A route: method + pattern + handler.
struct Route {
    method: Method,
    pattern: PathPattern,
    handler: Handler,
}

/// A boxed handler: takes the state, request path captures, query string,
/// request URI, and headers; returns a response. Inputs are owned, so the
/// future is `'static`.
type Handler = Box<
    dyn Fn(
            Arc<ServeState>,
            Vec<String>,
            HashMap<String, String>,
            Uri,
            HeaderMap,
        ) -> Pin<Box<dyn Future<Output = Response> + Send>>
        + Send
        + Sync,
>;

/// The concrete shared state type (holds the app's `AppState`).
pub struct ServeState {
    pub data: Box<dyn std::any::Any + Send + Sync>,
}

impl ServeState {
    pub fn new<T: Send + Sync + 'static>(t: T) -> Arc<ServeState> {
        Arc::new(ServeState { data: Box::new(t) })
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.data.downcast_ref::<T>()
    }
}

/// The router. Owns routes + a fallback handler and serves requests.
pub struct Router {
    routes: Vec<Route>,
    fallback: Option<Handler>,
    /// static-file root with SPA fallback; None disables.
    spa: Option<static_files::Spa>,
    /// optional WebSocket upgrade handler.
    ws: Option<WsHandler>,
}

/// A WebSocket upgrade handler: takes the mutable request and shared state,
/// performs the upgrade, returns the 101 response.
pub type WsHandler = fn(&mut http::Request<Incoming>, Arc<ServeState>) -> Option<Response>;

impl Default for Router {
    fn default() -> Self {
        Router::new()
    }
}

impl Router {
    pub fn new() -> Self {
        Router {
            routes: Vec::new(),
            fallback: None,
            spa: None,
            ws: None,
        }
    }

    /// Register a WebSocket upgrade handler for the `/ws` path.
    pub fn ws(mut self, handler: WsHandler) -> Self {
        self.ws = Some(handler);
        self
    }

    /// Register a GET route. `handler` is a plain `fn` (see `IntoHandler`
    /// impls for the supported extractor signatures).
    pub fn get<F, Args>(mut self, path: &str, handler: F) -> Self
    where
        F: IntoHandler<Args> + Send + Sync + 'static,
    {
        self.routes.push(Route {
            method: Method::GET,
            pattern: PathPattern::parse(path),
            handler: handler.into_handler(),
        });
        self
    }

    /// Register a POST route.
    pub fn post<F, Args>(mut self, path: &str, handler: F) -> Self
    where
        F: IntoHandler<Args> + Send + Sync + 'static,
    {
        self.routes.push(Route {
            method: Method::POST,
            pattern: PathPattern::parse(path),
            handler: handler.into_handler(),
        });
        self
    }

    /// Set the fallback handler (called when no route matches).
    pub fn fallback<F, Args>(mut self, handler: F) -> Self
    where
        F: IntoHandler<Args> + Send + Sync + 'static,
    {
        self.fallback = Some(handler.into_handler());
        self
    }

    /// Register a route with an explicit method + handler.
    pub fn route_raw<F, Args>(mut self, method: Method, path: &str, handler: F) -> Self
    where
        F: IntoHandler<Args> + Send + Sync + 'static,
    {
        self.routes.push(Route {
            method,
            pattern: PathPattern::parse(path),
            handler: handler.into_handler(),
        });
        self
    }

    /// Register a route from a `get(handler)`/`post(handler)` method router.
    pub fn route<Args>(mut self, path: &str, m: MethodRouter<Args>) -> Self
where {
        self.routes.push(Route {
            method: m.method,
            pattern: PathPattern::parse(path),
            handler: m.handler,
        });
        self
    }

    /// Merge all routes from `other` into `self` (later routes checked first).
    pub fn merge(mut self, other: Router) -> Self {
        self.routes.extend(other.routes);
        if self.fallback.is_none() {
            self.fallback = other.fallback;
        }
        if self.ws.is_none() {
            self.ws = other.ws;
        }
        self
    }

    /// Serve static files from `root` with an SPA fallback to `index` for
    /// any unmatched non-file path.
    pub fn spa_fallback(mut self, root: std::path::PathBuf, index: std::path::PathBuf) -> Self {
        self.spa = Some(static_files::Spa::new(root, index));
        self
    }

    /// Handle one HTTP request.
    pub async fn serve(&self, req: Request<Body>, state: Arc<ServeState>) -> Response {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let headers = req.headers().clone();
        let path = uri.path().to_string();
        let query = parse_query(uri.query());

        // Exact route match.
        for route in &self.routes {
            if route.method == method {
                if let Some(captures) = route.pattern.match_path(&path) {
                    let handler = &route.handler;
                    return handler(
                        Arc::clone(&state),
                        captures,
                        query,
                        uri.clone(),
                        headers.clone(),
                    )
                    .await;
                }
            }
        }

        // API namespace owns its 404s (the SPA must not swallow /api typos).
        if path == "/api" || path.starts_with("/api/") {
            if let Some(fb) = &self.fallback {
                return fb(Arc::clone(&state), Vec::new(), query, uri, headers).await;
            }
            return json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found"}),
            );
        }

        // SPA / static fallback.
        if let Some(spa) = &self.spa {
            if let Some(resp) = static_files::try_serve(spa, &method, &path).await {
                return resp;
            }
        }

        // Generic fallback handler.
        if let Some(fb) = &self.fallback {
            return fb(Arc::clone(&state), Vec::new(), query, uri, headers).await;
        }

        // No fallback: 404.
        json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({"error": "not_found"}),
        )
    }
}

/// Free function: register a GET route (axum-compatible helper).
pub fn get<F, Args>(handler: F) -> MethodRouter<Args>
where
    F: IntoHandler<Args> + Send + Sync + 'static,
{
    MethodRouter {
        method: Method::GET,
        handler: handler.into_handler(),
        _marker: std::marker::PhantomData,
    }
}

/// Free function: register a POST route (axum-compatible helper).
pub fn post<F, Args>(handler: F) -> MethodRouter<Args>
where
    F: IntoHandler<Args> + Send + Sync + 'static,
{
    MethodRouter {
        method: Method::POST,
        handler: handler.into_handler(),
        _marker: std::marker::PhantomData,
    }
}

/// A method + handler pair, produced by `get`/`post` and consumed by
/// `Router::route`.
pub struct MethodRouter<Args> {
    method: Method,
    handler: Handler,
    _marker: std::marker::PhantomData<Args>,
}

/// Parse a query string into a map (first value wins).
pub fn parse_query(q: Option<&str>) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if let Some(q) = q {
        for pair in q.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut it = pair.splitn(2, '=');
            let k = percent_decode(it.next().unwrap_or("")).unwrap_or_default();
            let v = percent_decode(it.next().unwrap_or("")).unwrap_or_default();
            m.entry(k).or_insert(v);
        }
    }
    m
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                let v = u8::from_str_radix(hex, 16).ok()?;
                out.push(v);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

// ---------------------------------------------------------------------------
// Handlers & extractors
// ---------------------------------------------------------------------------

/// The raw request parts a handler can extract from.
pub struct RequestParts {
    pub state: Arc<ServeState>,
    pub captures: Vec<String>,
    pub query: HashMap<String, String>,
    pub uri: Uri,
    pub headers: HeaderMap,
}

/// An extractor: builds itself from the request parts (or an error response).
pub trait FromParts: Sized {
    fn from_parts(parts: &RequestParts) -> Result<Self, Box<Response>>;
}

/// Extractor: shared state clone.
pub struct State<S>(pub S);

impl<S: Clone + Send + Sync + 'static> FromParts for State<S> {
    fn from_parts(parts: &RequestParts) -> Result<Self, Box<Response>> {
        match parts.state.get::<S>() {
            Some(s) => Ok(State(s.clone())),
            None => Err(Box::new(json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "state missing"}),
            ))),
        }
    }
}

/// Extractor: parsed query parameters (a `HashMap<String, String>`).
pub struct Query<T>(pub T);

impl FromParts for Query<HashMap<String, String>> {
    fn from_parts(parts: &RequestParts) -> Result<Self, Box<Response>> {
        Ok(Query(parts.query.clone()))
    }
}

/// Extractor: decoded path captures.
///
/// Supports `Path<String>` (single capture) and `Path<(A, B)>` (two captures,
/// e.g. `{id}/{status}`). The tuple elements are decoded from the capture
/// strings.
pub struct Path<T>(pub T);

impl FromParts for Path<String> {
    fn from_parts(parts: &RequestParts) -> Result<Self, Box<Response>> {
        match parts.captures.first() {
            Some(s) => Ok(Path(s.clone())),
            None => Err(Box::new(json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "missing path param"}),
            ))),
        }
    }
}

/// Parse one capture segment into a typed value (i64, String, ...).
fn parse_capture<T: std::str::FromStr>(s: &str) -> Option<T> {
    s.parse().ok()
}

impl FromParts for Path<(i64, String)> {
    fn from_parts(parts: &RequestParts) -> Result<Self, Box<Response>> {
        let mut it = parts.captures.iter();
        let a = it.next().and_then(|s| parse_capture::<i64>(s));
        let b = it.next().cloned();
        match (a, b) {
            (Some(a), Some(b)) => Ok(Path((a, b))),
            _ => Err(Box::new(json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "missing path params"}),
            ))),
        }
    }
}

impl FromParts for Path<(String, String)> {
    fn from_parts(parts: &RequestParts) -> Result<Self, Box<Response>> {
        let mut it = parts.captures.iter();
        let a = it.next().cloned();
        let b = it.next().cloned();
        match (a, b) {
            (Some(a), Some(b)) => Ok(Path((a, b))),
            _ => Err(Box::new(json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "missing path params"}),
            ))),
        }
    }
}

/// Extractor: the raw request headers.
pub struct Headers(pub http::HeaderMap);

impl FromParts for Headers {
    fn from_parts(parts: &RequestParts) -> Result<Self, Box<Response>> {
        Ok(Headers(parts.headers.clone()))
    }
}

/// Extractor: the full request URI.
pub struct ReqUri(pub Uri);

impl FromParts for ReqUri {
    fn from_parts(parts: &RequestParts) -> Result<Self, Box<Response>> {
        Ok(ReqUri(parts.uri.clone()))
    }
}

/// Convert a handler into a boxed handler. `Args` is a marker tuple of the
/// extractor types the handler takes (e.g. `(State<AppState>,)`).
pub trait IntoHandler<Args> {
    fn into_handler(self) -> Handler;
}

/// Build a boxed handler from a closure that produces a `RequestParts`.
fn box_parts<F>(f: F) -> Handler
where
    F: Fn(RequestParts) -> Pin<Box<dyn Future<Output = Response> + Send>> + Send + Sync + 'static,
{
    Box::new(move |state, captures, query, uri, headers| {
        let parts = RequestParts {
            state: Arc::clone(&state),
            captures,
            query,
            uri,
            headers,
        };
        f(parts)
    })
}

macro_rules! impl_handler {
    ($(($($T:ident),*)),* $(,)?) => {
        $(
            #[allow(unreachable_patterns, unused_variables, non_snake_case)]
            impl<F, Fut, R, $($T),*> IntoHandler<($($T,)*)> for F
            where
                F: Fn($($T),*) -> Fut + Copy + Send + Sync + 'static,
                Fut: Future<Output = R> + Send + 'static,
                R: IntoResponse + 'static,
                $($T: FromParts + Send,)*
            {
                fn into_handler(self) -> Handler {
                    box_parts(move |parts: RequestParts| {
                        let f = self; // F: Copy — clone for each request
                        Box::pin(async move {
                            match ($($T::from_parts(&parts),)*) {
                                ($(Ok($T),)*) => {
                                    let fut = f($($T),*);
                                    fut.await.into_response()
                                }
                                _ => json_response(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    serde_json::json!({"error": "extractor failed"}),
                                ),                            }
                        })
                    })
                }
            }
        )*
    };
}

impl_handler!((), (T0), (T0, T1), (T0, T1, T2),);

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// Anything that can be turned into an HTTP response.
pub trait IntoResponse {
    fn into_response(self) -> Response;
}

impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
    }
}

impl IntoResponse for StatusCode {
    fn into_response(self) -> Response {
        HttpResponse::builder()
            .status(self)
            .body(full(hyper::body::Bytes::new()))
            .expect("status-only response")
    }
}

impl IntoResponse for &'static str {
    fn into_response(self) -> Response {
        HttpResponse::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain; charset=utf-8")
            .body(full(hyper::body::Bytes::from(self)))
            .expect("str response")
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Response {
        HttpResponse::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain; charset=utf-8")
            .body(full(hyper::body::Bytes::from(self)))
            .expect("string response")
    }
}

impl IntoResponse for Html {
    fn into_response(self) -> Response {
        HttpResponse::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html; charset=utf-8")
            .body(full(hyper::body::Bytes::from(self.0)))
            .expect("html response")
    }
}

impl IntoResponse for Json {
    fn into_response(self) -> Response {
        let body = serde_json::to_vec(&self.0).unwrap_or_else(|_| b"{}".to_vec());
        HttpResponse::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(full(hyper::body::Bytes::from(body)))
            .expect("json response")
    }
}

/// `(StatusCode, Html)` tuple.
impl IntoResponse for (StatusCode, Html) {
    fn into_response(self) -> Response {
        HttpResponse::builder()
            .status(self.0)
            .header("content-type", "text/html; charset=utf-8")
            .body(full(hyper::body::Bytes::from(self.1 .0)))
            .expect("status+html response")
    }
}

/// `(StatusCode, Json)` tuple.
impl IntoResponse for (StatusCode, Json) {
    fn into_response(self) -> Response {
        let body = serde_json::to_vec(&self.1 .0).unwrap_or_else(|_| b"{}".to_vec());
        HttpResponse::builder()
            .status(self.0)
            .header("content-type", "application/json")
            .body(full(hyper::body::Bytes::from(body)))
            .expect("status+json response")
    }
}

/// `(StatusCode, &'static str)` tuple.
impl IntoResponse for (StatusCode, &'static str) {
    fn into_response(self) -> Response {
        HttpResponse::builder()
            .status(self.0)
            .header("content-type", "text/plain; charset=utf-8")
            .body(full(hyper::body::Bytes::from(self.1)))
            .expect("status+str response")
    }
}

/// `(StatusCode, [(HeaderName, &'static str); 1], &'static str)` — e.g. the
/// vendored JS with an explicit content-type.
impl IntoResponse for (StatusCode, [(&'static str, &'static str); 1], &'static str) {
    fn into_response(self) -> Response {
        let (status, headers, body) = self;
        HttpResponse::builder()
            .status(status)
            .header(headers[0].0, headers[0].1)
            .body(full(hyper::body::Bytes::from(body)))
            .expect("status+header+str response")
    }
}

/// `(StatusCode, [(HeaderName, &'static str); 1], String)`.
impl IntoResponse for (StatusCode, [(&'static str, &'static str); 1], String) {
    fn into_response(self) -> Response {
        let (status, headers, body) = self;
        HttpResponse::builder()
            .status(status)
            .header(headers[0].0, headers[0].1)
            .body(full(hyper::body::Bytes::from(body)))
            .expect("status+header+string response")
    }
}

/// `Result<R, E>` where both sides are responses.
impl<R, E> IntoResponse for Result<R, E>
where
    R: IntoResponse,
    E: IntoResponse,
{
    fn into_response(self) -> Response {
        match self {
            Ok(r) => r.into_response(),
            Err(e) => e.into_response(),
        }
    }
}

/// A JSON response body wrapper.
pub struct Json(pub serde_json::Value);

/// An HTML response body wrapper.
pub struct Html(pub String);

/// Build a JSON response with a status code.
pub fn json_response(status: StatusCode, value: serde_json::Value) -> Response {
    let body = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
    HttpResponse::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(full(hyper::body::Bytes::from(body)))
        .expect("json response")
}

// ---------------------------------------------------------------------------
// Serve loop
// ---------------------------------------------------------------------------

/// Run the server: accept connections on `listener`, serve HTTP/1, and stop
/// accepting when Ctrl-C is received.
pub async fn serve(
    listener: tokio::net::TcpListener,
    router: Arc<Router>,
    state: Arc<ServeState>,
) -> Result<(), TubeforgeError> {
    loop {
        let accept = listener.accept();
        tokio::pin!(accept);
        tokio::select! {
            res = &mut accept => {
                let (stream, _peer) = match res {
                    Ok(x) => x,
                    Err(e) => {
                        tracing::warn!("accept error: {e}");
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                };
                let io = TokioIo::new(stream);
                let router = Arc::clone(&router);
                let state = Arc::clone(&state);
                let ws = router.ws;
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req| {
                        let router = Arc::clone(&router);
                        let state = Arc::clone(&state);
                        async move {
                            // WebSocket upgrade path: needs the Incoming body.
                            if req.method() == Method::GET && req.uri().path() == "/ws" {
                                if let Some(ws) = ws {
                                    let mut req = req;
                                    if let Some(resp) = ws(&mut req, Arc::clone(&state)) {
                                        return Ok::<_, std::convert::Infallible>(resp);
                                    }
                                    // Upgrade rejected: fall through normally.
                                    return Ok::<_, std::convert::Infallible>(router.serve(
                                        Request::from_parts(req.into_parts().0, full(hyper::body::Bytes::new())),
                                        state,
                                    ).await);
                                }
                            }
                            let (parts, _body) = req.into_parts();
                            let body = full(hyper::body::Bytes::new());
                            let req = Request::from_parts(parts, body);
                            Ok::<_, std::convert::Infallible>(router.serve(req, state).await)
                        }
                    });
                    let conn = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .with_upgrades();
                    if let Err(e) = conn.await {
                        tracing::warn!("connection error: {e}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("serve: shutdown signal received");
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestState {
        n: u32,
    }

    async fn home(State(s): State<TestState>) -> Response {
        format!("n={}", s.n).into_response()
    }

    async fn hello(
        State(_s): State<TestState>,
        Path(who): Path<String>,
        Query(q): Query<HashMap<String, String>>,
    ) -> Response {
        let extra = q.get("x").cloned().unwrap_or_default();
        format!("hello {who} x={extra}").into_response()
    }
    async fn header_test(Headers(h): Headers) -> Response {
        let ct = h
            .get("x-custom")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("none");
        format!("header={ct}").into_response()
    }

    fn make_router() -> Router {
        Router::new()
            .get("/", home)
            .get("/hello/{name}", hello)
            .post("/head", header_test)
    }

    async fn call(router: &Router, method: Method, path: &str) -> String {
        let state = ServeState::new(TestState { n: 42 });
        let uri: Uri = path.parse().expect("uri");
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-custom", "abc")
            .body(full(hyper::body::Bytes::new()))
            .expect("req");
        let resp = router.serve(req, state).await;
        let body = http_body_util::BodyExt::collect(resp.into_body()).await;
        let bytes = body.unwrap().to_bytes();
        String::from_utf8_lossy(&bytes).to_string()
    }

    #[tokio::test]
    async fn routes_state_and_path_and_query() {
        let router = make_router();
        assert_eq!(call(&router, Method::GET, "/").await, "n=42");
        assert_eq!(
            call(&router, Method::GET, "/hello/world?x=7").await,
            "hello world x=7"
        );
    }

    #[tokio::test]
    async fn header_extractor_and_method_routing() {
        let router = make_router();
        assert_eq!(call(&router, Method::POST, "/head").await, "header=abc");
        // GET on a POST-only route → 404 (no match).
        assert!(call(&router, Method::GET, "/head")
            .await
            .contains("not_found"));
    }

    #[tokio::test]
    async fn path_pattern_requires_full_match() {
        let router = make_router();
        assert!(call(&router, Method::GET, "/hello/world/extra")
            .await
            .contains("not_found"));
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    async fn legacy_page() -> Response {
        "legacy-page".into_response()
    }

    #[tokio::test]
    async fn multi_segment_route_matches() {
        let router = Router::new()
            .route("/legacy/keywords", get(legacy_page))
            .route("/legacy/ideas/{id}/{status}", post(legacy_page));
        let state = ServeState::new(());
        let req = Request::builder()
            .method(Method::GET)
            .uri("http://x/legacy/keywords")
            .body(full(hyper::body::Bytes::new()))
            .expect("req");
        let resp = router.serve(req, state).await;
        let body = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(
            String::from_utf8_lossy(&body),
            "legacy-page",
            "route matched"
        );
    }

    #[tokio::test]
    async fn multi_segment_post_matches() {
        let router = Router::new().route("/legacy/ideas/{id}/{status}", post(legacy_page));
        let state = ServeState::new(());
        let req = Request::builder()
            .method(Method::POST)
            .uri("http://x/legacy/ideas/1/saved")
            .body(full(hyper::body::Bytes::new()))
            .expect("req");
        let resp = router.serve(req, state).await;
        let body = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(
            String::from_utf8_lossy(&body),
            "legacy-page",
            "post matched"
        );
    }
}
