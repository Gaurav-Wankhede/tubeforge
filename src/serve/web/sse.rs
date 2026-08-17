//! Server-Sent Events (SSE) streaming for the dashboard.
//!
//! Implements the `text/event-stream` wire format with `event:`/`data:` frames
//! and `: ping` comment heartbeats. Mirrors the axum `Sse`/`Event`/`KeepAlive`
//! surface the old dashboard used, so the `/events` handler ports directly.
//!
//! The response body is a dedicated `http_body::Body` implementation fed by a
//! spawned producer task over a bounded `mpsc` channel: a clean, `Send + Sync`
//! design that avoids boxing/`Sync` gymnastics with arbitrary streams.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use http::{Response as HttpResponse, StatusCode};
use http_body::{Body as HttpBody, Frame, SizeHint};

/// A single SSE event.
#[derive(Default)]
pub struct Event {
    name: Option<String>,
    data: String,
}

impl Event {
    pub fn event(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn data(mut self, data: impl Into<String>) -> Self {
        self.data = data.into();
        self
    }
}

/// Keep-alive configuration (comment heartbeats).
pub struct KeepAlive {
    interval: Duration,
    text: String,
}

impl Default for KeepAlive {
    fn default() -> Self {
        KeepAlive {
            interval: Duration::from_secs(15),
            text: "ping".to_string(),
        }
    }
}

impl KeepAlive {
    pub fn new() -> Self {
        KeepAlive::default()
    }

    pub fn interval(mut self, d: Duration) -> Self {
        self.interval = d;
        self
    }

    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = t.into();
        self
    }
}

/// An SSE response builder that turns an event stream into an HTTP response.
pub struct Sse<S> {
    stream: S,
    keep_alive: KeepAlive,
}

impl<S> Sse<S>
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    pub fn new(stream: S) -> Self {
        Sse {
            stream,
            keep_alive: KeepAlive::default(),
        }
    }

    pub fn keep_alive(mut self, ka: KeepAlive) -> Self {
        self.keep_alive = ka;
        self
    }

    /// Build the HTTP response, spawning a producer that frames events and
    /// injects heartbeats into a channel-backed body.
    pub fn into_response(self) -> super::Response {
        let body = sse_body(self.stream, self.keep_alive);
        HttpResponse::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("connection", "keep-alive")
            .body(body)
            .expect("sse response")
    }
}

impl<S> super::IntoResponse for Sse<S>
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    fn into_response(self) -> super::Response {
        self.into_response()
    }
}

/// Frame one event into the wire format.
fn frame_event(e: &Event) -> String {
    let mut out = String::new();
    if let Some(name) = &e.name {
        out.push_str("event: ");
        out.push_str(name);
        out.push('\n');
    }
    for line in e.data.split('\n') {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out
}

/// The channel-backed SSE body.
struct SseBody {
    rx: tokio::sync::mpsc::Receiver<Frame<hyper::body::Bytes>>,
}

impl HttpBody for SseBody {
    type Data = hyper::body::Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.rx).poll_recv(cx).map(|opt| opt.map(Ok))
    }

    fn size_hint(&self) -> SizeHint {
        // Unknown length → chunked transfer, not a fixed content-length.
        SizeHint::default()
    }
}

/// Spawn a producer task that reads events, emits frames + heartbeats, and
/// returns a channel-backed body. The stream is consumed by the task; when the
/// connection drops the receiver is dropped, cancelling the task.
fn sse_body<S>(stream: S, ka: KeepAlive) -> crate::serve::web::Body
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel::<Frame<hyper::body::Bytes>>(8);
    let heartbeat = ka.text.clone();
    let interval = ka.interval;

    tokio::spawn(async move {
        use futures::StreamExt;
        let mut stream = Box::pin(stream);
        loop {
            let next = tokio::select! {
                n = stream.next() => n,
                _ = tokio::time::sleep(interval) => None,
            };
            match next {
                Some(Ok(e)) => {
                    let f = Frame::data(hyper::body::Bytes::from(frame_event(&e)));
                    if tx.send(f).await.is_err() {
                        break; // client gone
                    }
                }
                Some(Err(_)) => break,
                None => {
                    // No event ready within the tick → heartbeat comment.
                    let hb = Frame::data(hyper::body::Bytes::from(format!(": {heartbeat}\n\n")));
                    if tx.send(hb).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    crate::serve::web::full_erase(SseBody { rx })
}
