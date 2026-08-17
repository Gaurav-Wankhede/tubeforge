//! WebSocket support via `tokio-tungstenite` on a raw Hyper connection.
//!
//! Replaces axum's `WebSocketUpgrade` extractor. The server uses Hyper's
//! HTTP/1 upgrade mechanism (`hyper::upgrade::on`) to hand off the connection,
//! then runs a `tokio_tungstenite::WebSocketStream` over it. The public API
//! mirrors what the RPC handler needs: `Message`, `WebSocket`, and an
//! `accept(upgraded, callback)` helper.

pub use tokio_tungstenite::tungstenite::Message;

use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use http::{Response as HttpResponse, StatusCode};
use hyper::body::Incoming;
use tokio::sync::Mutex;

use crate::error::{storage_err, TubeforgeError};

/// A tungstenite WebSocket stream over an upgraded Hyper connection.
pub type WebSocket =
    tokio_tungstenite::WebSocketStream<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>;

/// Split a socket into its write and read halves.
pub fn split(
    socket: WebSocket,
) -> (
    futures::stream::SplitSink<WebSocket, Message>,
    futures::stream::SplitStream<WebSocket>,
) {
    socket.split()
}

/// Handle a WebSocket upgrade: on the upgraded connection, run `f`.
///
/// Returns an HTTP `101 Switching Protocols` response with the upgrade
/// headers. If the request is not a valid WebSocket upgrade, returns `None`
/// so the caller can fall back to a 400.
pub fn upgrade<F>(
    req: &mut http::Request<Incoming>,
    on_upgraded: F,
) -> Option<HttpResponse<super::Body>>
where
    F: FnOnce(WebSocket) + Send + 'static,
{
    // Validate the upgrade headers.
    let headers = req.headers();
    let connection = headers.get(http::header::CONNECTION)?.to_str().ok()?;
    if !connection
        .split(',')
        .any(|s| s.trim().eq_ignore_ascii_case("upgrade"))
    {
        return None;
    }
    let upgrade = headers.get(http::header::UPGRADE)?.to_str().ok()?;
    if !upgrade.eq_ignore_ascii_case("websocket") {
        return None;
    }
    let key = headers
        .get(http::header::SEC_WEBSOCKET_KEY)?
        .to_str()
        .ok()?
        .to_string();

    let resp = HttpResponse::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(http::header::UPGRADE, "websocket")
        .header(http::header::CONNECTION, "Upgrade")
        .header(http::header::SEC_WEBSOCKET_ACCEPT, ws_accept_key(&key))
        .header("Sec-WebSocket-Version", "13")
        .body(super::full(hyper::body::Bytes::new()))
        .expect("ws upgrade response");

    let fut = hyper::upgrade::on(req);
    tokio::spawn(async move {
        let upgraded = match fut.await {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!("ws upgrade failed: {e}");
                return;
            }
        };
        let io = hyper_util::rt::TokioIo::new(upgraded);
        let socket = tokio_tungstenite::WebSocketStream::from_raw_socket(
            io,
            tokio_tungstenite::tungstenite::protocol::Role::Server,
            None,
        )
        .await;
        on_upgraded(socket);
    });

    Some(resp)
}

/// Compute the `Sec-WebSocket-Accept` header value (RFC 6455 §1.3).
fn ws_accept_key(key: &str) -> String {
    use sha1::Sha1;
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let digest = hasher.digest().bytes();
    base64(digest)
}

fn base64(bytes: [u8; 20]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(n >> 18) as usize & 63] as char);
        out.push(CHARS[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(CHARS[(n >> 6) as usize & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[n as usize & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Send helper: serialize a JSON-RPC response to a WS text frame.
pub async fn send(
    sender: &Arc<Mutex<futures::stream::SplitSink<WebSocket, Message>>>,
    res: &serde_json::Value,
) -> Result<(), TubeforgeError> {
    let json = serde_json::to_string(res).map_err(|e| storage_err("ENCODE", e.to_string()))?;
    sender
        .lock()
        .await
        .send(Message::Text(json.into()))
        .await
        .map_err(|e| {
            tracing::warn!("ws send failed: {e}");
            storage_err("WS_SEND", e.to_string())
        })
}

// ---------------------------------------------------------------------------
// sha1: tiny SHA-1 for the WS handshake (no external dep).
// ---------------------------------------------------------------------------
mod sha1 {
    pub struct Sha1 {
        state: [u32; 5],
        buffer: Vec<u8>,
    }

    impl Sha1 {
        pub fn new() -> Self {
            Sha1 {
                state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
                buffer: Vec::new(),
            }
        }

        pub fn update(&mut self, data: &[u8]) {
            self.buffer.extend_from_slice(data);
        }

        pub fn digest(&self) -> Digest {
            let mut msg = self.buffer.clone();
            let bit_len = (msg.len() as u64) * 8;
            msg.push(0x80);
            while msg.len() % 64 != 56 {
                msg.push(0);
            }
            msg.extend_from_slice(&bit_len.to_be_bytes());

            let mut state = self.state;
            for chunk in msg.chunks(64) {
                let mut w = [0u32; 80];
                for (i, b) in chunk.chunks(4).enumerate() {
                    w[i] = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
                }
                for i in 16..80 {
                    w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
                }
                let (mut a, mut b, mut c, mut d, mut e) =
                    (state[0], state[1], state[2], state[3], state[4]);
                for (i, wi) in w.iter().enumerate() {
                    let (f, k) = match i {
                        0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                        20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                        40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                        _ => (b ^ c ^ d, 0xCA62C1D6),
                    };
                    let temp = a
                        .rotate_left(5)
                        .wrapping_add(f)
                        .wrapping_add(e)
                        .wrapping_add(k)
                        .wrapping_add(*wi);
                    e = d;
                    d = c;
                    c = b.rotate_left(30);
                    b = a;
                    a = temp;
                }
                state[0] = state[0].wrapping_add(a);
                state[1] = state[1].wrapping_add(b);
                state[2] = state[2].wrapping_add(c);
                state[3] = state[3].wrapping_add(d);
                state[4] = state[4].wrapping_add(e);
            }
            Digest::from(state)
        }
    }

    pub struct Digest {
        bytes: [u8; 20],
    }

    impl Digest {
        pub fn bytes(self) -> [u8; 20] {
            self.bytes
        }
    }

    impl From<[u32; 5]> for Digest {
        fn from(state: [u32; 5]) -> Self {
            let mut bytes = [0u8; 20];
            for (i, w) in state.iter().enumerate() {
                let be = w.to_be_bytes();
                bytes[i * 4..i * 4 + 4].copy_from_slice(&be);
            }
            Digest { bytes }
        }
    }
}
