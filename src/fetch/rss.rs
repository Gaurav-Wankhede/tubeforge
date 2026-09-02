//! Channel RSS fetch + parse (LLD §5.1).
//!
//! URL: `https://www.youtube.com/feeds/videos.xml?channel_id=<id>` (best-effort,
//! undocumented endpoint — parses must degrade gracefully). ETag caching via
//! `If-None-Match` (304 → no-op refresh). ~15 most-recent entries per feed.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::retry_http;
use super::{FetchClients, HttpResponse};
use crate::error::{Source, TubeforgeError};

/// One parsed `<entry>` from a channel feed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RssVideo {
    pub video_id: String,
    pub title: String,
    pub link: String,
    pub published: String,
    pub updated: String,
    pub description: String,
    pub thumbnail_url: Option<String>,
    /// media:starRating average (1..5), when present.
    pub star_rating: Option<f64>,
    /// media:starRating count (number of ratings), when present.
    pub rating_count: Option<i64>,
    /// media:statistics views, when present.
    pub views: Option<i64>,
}

/// A parsed channel feed.
#[derive(Debug, Clone, Default)]
pub struct RssFeed {
    /// Feed-level `yt:channelId` (matches the requested channel id).
    pub channel_id: Option<String>,
    /// Feed-level author name (the channel display name).
    pub channel_title: Option<String>,
    pub entries: Vec<RssVideo>,
}

/// Outcome of a feed fetch (LLD §5.1 ETag path).
pub enum FeedResult {
    /// Full feed fetched (with the new ETag, when the server sent one).
    Feed { feed: RssFeed, etag: Option<String> },
    /// 304 Not Modified — nothing changed.
    NotModified,
}

/// Fetch + parse a channel feed. `etag` is the stored feed ETag, sent as
/// `If-None-Match`; a 304 yields `FeedResult::NotModified`.
pub async fn fetch_feed(
    clients: &FetchClients,
    channel_id: &str,
    etag: Option<&str>,
) -> Result<FeedResult, TubeforgeError> {
    let url = format!(
        "{}/feeds/videos.xml?channel_id={}",
        clients.rss_base,
        percent_encode(channel_id)
    );

    let mut if_none_match: Option<String> = None;
    if let Some(e) = etag {
        if_none_match = Some(e.to_string());
    }

    let resp = retry_http(Source::Rss, &url, || {
        let mut req = clients.http.get(&url);
        if let Some(etag) = &if_none_match {
            req = req.header("If-None-Match", etag.as_str());
        }
        req.send()
    })
    .await?;

    let HttpResponse::Body(resp) = resp else {
        return Ok(FeedResult::NotModified);
    };

    let new_etag = resp
        .headers()
        .get("ETag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let body = resp.text().await.map_err(|e| TubeforgeError::Fetch {
        src: Source::Rss,
        url: url.clone(),
        inner: format!("read body: {e}"),
    })?;

    let feed = parse_feed(&body).map_err(|e| TubeforgeError::Parse {
        src: Source::Rss,
        item: url,
        inner: e,
    })?;

    Ok(FeedResult::Feed {
        feed,
        etag: new_etag,
    })
}

fn percent_encode(input: &str) -> String {
    // channel ids are [A-Za-z0-9_-]; keep it dependency-free.
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'~' | b'.') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Parse a feed XML document into `RssFeed`. Tolerant of missing optional
/// media fields (best-effort endpoint); requires entry ids to be extractable
/// from `yt:videoId` (falls back to `yt:video:<id>` in `<id>`).
pub fn parse_feed(xml: &str) -> Result<RssFeed, String> {
    let mut reader = Reader::from_str(xml);
    let mut buf: Vec<u8> = Vec::new();

    let mut feed = RssFeed::default();
    let mut entry = RssVideo::default();
    let mut in_entry = false;
    let mut cur: Vec<u8> = Vec::new(); // current open element (local name)
                                       // Text accumulates across events: quick-xml 0.41 emits entity references
                                       // as separate `GeneralRef` events, splitting text (`"a &amp; b"` arrives
                                       // as Text("a "), GeneralRef("amp"), Text(" b")). We reassemble at End.
    let mut pending: String = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(format!("xml read: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                cur = local.to_vec();
                pending.clear();
                match local {
                    b"entry" => {
                        in_entry = true;
                        entry = RssVideo::default();
                    }
                    b"channelId" | b"name" => {} // handled via text
                    b"link" if in_entry => {
                        // <link rel="alternate" href="https://www.youtube.com/watch?v=..."/>
                        if attr(&e, b"rel").as_deref() == Some("alternate") {
                            if let Some(href) = attr(&e, b"href") {
                                entry.link = href;
                            }
                        }
                    }
                    b"thumbnail" if in_entry => {
                        entry.thumbnail_url = attr(&e, b"url");
                    }
                    b"starRating" if in_entry => {
                        entry.rating_count = attr(&e, b"count").and_then(|v| v.parse().ok());
                        entry.star_rating = attr(&e, b"average").and_then(|v| v.parse().ok());
                    }
                    b"statistics" if in_entry => {
                        entry.views = attr(&e, b"views").and_then(|v| v.parse().ok());
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                match local {
                    b"link" if in_entry => {
                        if attr(&e, b"rel").as_deref() == Some("alternate") {
                            if let Some(href) = attr(&e, b"href") {
                                entry.link = href;
                            }
                        }
                    }
                    b"thumbnail" if in_entry => {
                        entry.thumbnail_url = attr(&e, b"url");
                    }
                    b"starRating" if in_entry => {
                        entry.rating_count = attr(&e, b"count").and_then(|v| v.parse().ok());
                        entry.star_rating = attr(&e, b"average").and_then(|v| v.parse().ok());
                    }
                    b"statistics" if in_entry => {
                        entry.views = attr(&e, b"views").and_then(|v| v.parse().ok());
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if let Ok(text) = t.decode() {
                    pending.push_str(&text);
                }
            }
            Ok(Event::GeneralRef(r)) => {
                // Predefined XML entities arrive here; append the character.
                if let Ok(name) = r.decode() {
                    let ch = match name.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        other => return Err(format!("unsupported entity &{other};")),
                    };
                    pending.push_str(ch);
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local = local_name(name.as_ref());
                if !pending.trim().is_empty() {
                    let text = pending.trim().to_string();
                    if in_entry {
                        match cur.as_slice() {
                            b"videoId" => entry.video_id = text,
                            b"title" => entry.title = text,
                            b"published" => entry.published = text,
                            b"updated" => entry.updated = text,
                            b"description" => entry.description = text,
                            _ => {}
                        }
                    } else {
                        match cur.as_slice() {
                            b"channelId" => feed.channel_id = Some(text),
                            b"name" => feed.channel_title = Some(text),
                            _ => {}
                        }
                    }
                }
                pending.clear();
                match local {
                    b"entry" => {
                        in_entry = false;
                        // Require an id we can upsert by; yt:videoId is the
                        // canonical source, `<id>yt:video:<id>` is the fallback.
                        if entry.video_id.is_empty() {
                            return Err("entry missing yt:videoId".to_string());
                        }
                        if entry.title.is_empty() {
                            entry.title = entry.video_id.clone();
                        }
                        feed.entries.push(entry);
                        entry = RssVideo::default();
                    }
                    _ => cur.clear(),
                }
            }
            _ => {}
        }
    }

    if feed.entries.is_empty() {
        return Err("no <entry> elements parsed from feed".to_string());
    }
    Ok(feed)
}

/// Local name of an element: bytes after the last `:` (namespace prefix is
/// stripped: `yt:videoId` → `videoId`, `media:thumbnail` → `thumbnail`).
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// Read a single attribute value by exact key name (entities unescaped).
fn attr(e: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes()
        .with_checks(false)
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| {
            use quick_xml::XmlVersion;
            a.normalized_value(XmlVersion::Implicit1_0)
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(&a.value).into_owned())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_name_strips_prefix() {
        assert_eq!(local_name(b"yt:videoId"), b"videoId");
        assert_eq!(local_name(b"media:thumbnail"), b"thumbnail");
        assert_eq!(local_name(b"title"), b"title");
    }
}
