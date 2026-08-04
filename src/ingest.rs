//! Ingest pipeline (LLD §6): channel/video link resolution, fetch, upsert,
//! backup guard, tantivy index sync, ingest_log.
//!
//! Ordering (LLD §6.3 — Turso single-writer constraint): fetch ALL (network
//! first), THEN backup guard, THEN one transaction, THEN the index writer.
//! Source precedence (LLD §6.2): api > oembed > rss — rich data wins, never
//! downgrade.

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::api::{ApiClient, ApiVideo, BATCH_MAX};
use crate::fetch::oembed;
use crate::fetch::quota;
use crate::fetch::rss::{self, FeedResult, RssVideo};
use crate::fetch::FetchClients;
use crate::search::{self, VideoDoc};
use crate::storage::backup;
use crate::storage::db::{ChannelRow, VideoRow};
use crate::storage::Db;
use crate::util;

/// Options controlling one ingest run (CLI: `--api`, `--no-backup`).
#[derive(Debug, Clone, Copy)]
pub struct IngestOptions {
    pub use_api: bool,
    pub no_backup: bool,
}

/// Per-run counters rendered by the commands.
#[derive(Debug, Default, Clone)]
pub struct IngestSummary {
    pub batch_id: String,
    pub channels_added: u64,
    pub channels_updated: u64,
    pub channels_skipped: u64,
    pub channels_failed: u64,
    pub videos_added: u64,
    pub videos_updated: u64,
    pub videos_skipped: u64,
    pub videos_failed: u64,
    /// Snapshot path when the backup guard ran, else None.
    pub snapshot: Option<PathBuf>,
    /// api enrichment state: off | ok | quota | error.
    pub api: String,
    pub alerts: Vec<String>,
    /// Per-item rejects (item label, detail): checksum-invalid bare ids and
    /// unsupported kinds (playlist/channel/handle in `ingest links` input).
    /// Recorded into `ingest_log` as `failed` by `record_invalid_items`
    /// (A1/A2 — labeled, never silently dropped).
    pub rejected: Vec<(String, String)>,
}

type LogRow = (String, String, Option<String>); // item, status, detail

// ---------------------------------------------------------------------------
// Channel reference resolution (LLD §6.1)
// ---------------------------------------------------------------------------

/// A resolved channel target: channel_id + optional handle (from @-refs).
#[derive(Debug, Clone)]
struct Target {
    channel_id: String,
    handle: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChannelRef {
    Direct(String),
    Handle(String),
}

/// Accepts `UC...` ids (and `SC...` legacy show ids, normalized to `UC`),
/// full URLs (youtube.com/@x, /channel/UC..., /user/NAME, /c/NAME), and
/// `@handle` forms (LLD §6.1, extended by A2).
pub fn parse_channel_ref(input: &str) -> Result<ChannelRef, TubeforgeError> {
    let t = input.trim();
    if t.is_empty() {
        return Err(TubeforgeError::Usage("empty channel reference".into()));
    }
    let normalized = normalize_channel_id(t);
    if is_channel_id(&normalized) {
        return Ok(ChannelRef::Direct(normalized));
    }
    if let Some(h) = t.strip_prefix('@') {
        if !h.is_empty() && !h.contains('/') {
            return Ok(ChannelRef::Handle(format!("@{h}")));
        }
    }
    if let Ok(u) = url::Url::parse(t) {
        let host = u.host_str().unwrap_or("").to_ascii_lowercase();
        if host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be" {
            if let Some(segs) = u.path_segments() {
                let segs: Vec<String> = segs.map(normalize_channel_id).collect();
                for (i, seg) in segs.iter().enumerate() {
                    if let Some(h) = seg.strip_prefix('@') {
                        if !h.is_empty() {
                            return Ok(ChannelRef::Handle(format!("@{h}")));
                        }
                    }
                    if is_channel_id(seg) {
                        return Ok(ChannelRef::Direct(seg.clone()));
                    }
                    // Legacy /user/NAME and /c/NAME slugs resolve through the
                    // @handle path (stored for later resolution, A2).
                    if matches!(seg.as_str(), "user" | "c") && i + 1 < segs.len() {
                        let name = segs[i + 1].as_str();
                        if !name.is_empty() {
                            return Ok(ChannelRef::Handle(format!("@{name}")));
                        }
                    }
                }
            }
        }
    }
    Err(TubeforgeError::Usage(format!(
        "cannot resolve channel reference: {input:?} (expected UC... id, URL, or @handle)"
    )))
}

/// A channel id is a `UC`-prefixed base64-ish id (canonical YouTube form:
/// `UC` + 22 chars; the 22..=28 window tolerates legacy/edge ids).
pub fn is_channel_id(s: &str) -> bool {
    (22..=28).contains(&s.len())
        && s.starts_with("UC")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

// ---------------------------------------------------------------------------
// ID validation (A1) — archiveteam-derived checksums, via MW Metadata
// (mattwright324/youtube-metadata, MIT — js/shared.js isValidVideoId /
// isValidChannelId).
// ---------------------------------------------------------------------------

/// Valid last chars of a canonical video id (archiveteam YouTube technical
/// details): `[AEIMQUYcgkosw048]`.
pub const VIDEO_ID_CHECKSUM_CHARS: [char; 16] = [
    'A', 'E', 'I', 'M', 'Q', 'U', 'Y', 'c', 'g', 'k', 'o', 's', 'w', '0', '4', '8',
];

/// Valid last chars of the 22-char base64 core of a channel id: `[AQgw]`.
pub const CHANNEL_ID_CHECKSUM_CHARS: [char; 4] = ['A', 'Q', 'g', 'w'];

/// `^[A-Za-z0-9_-]{10}[AEIMQUYcgkosw048]$` — a bare 11-char video id whose
/// last char passes the checksum. URL-extracted ids are authoritative and
/// must NOT be checksummed (A1).
pub fn valid_video_id_checksum(id: &str) -> bool {
    id.len() == 11
        && id.bytes().all(|b| is_id_char(&b))
        && ends_with_any(id, &VIDEO_ID_CHECKSUM_CHARS)
}

/// Checksum-valid bare channel id: the trailing 22-char base64 core must end
/// in `[AQgw]` — canonical `UC` + 22 (24 chars, the LLD §6.1 form) or a bare
/// 22-char legacy id (`^[A-Za-z0-9_-]{21}[AQgw]$`). URL-extracted channel
/// ids are authoritative and must NOT be checksummed (A1).
pub fn valid_channel_id_checksum(id: &str) -> bool {
    let core = match id.len() {
        22 => Some(id),
        24 => id.strip_prefix("UC"),
        _ => None,
    };
    core.is_some_and(|c| {
        c.bytes().all(|b| is_id_char(&b)) && ends_with_any(c, &CHANNEL_ID_CHECKSUM_CHARS)
    })
}

fn ends_with_any(s: &str, set: &[char]) -> bool {
    s.chars().last().is_some_and(|c| set.contains(&c))
}

// ---------------------------------------------------------------------------
// Input extraction (LLD §6.1, extended by A2)
// ---------------------------------------------------------------------------

/// A single parsed input item (MW Metadata `determineInput` table, adapted
/// to the 11-char capture contract of LLD §6.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputItem {
    /// URL-extracted video id — authoritative, no checksum applied.
    VideoUrl(String),
    /// Bare 11-char video id — caller checks `valid_video_id_checksum`.
    VideoBare(String),
    /// Playlist id (`playlist?list=` or bare `UU|UUSH|PL|FL|SP|OLAK` prefix).
    Playlist(String),
    /// URL-extracted channel id (`/channel/...`, `/show/SC...` — SC already
    /// normalized to UC) — authoritative, no checksum applied.
    ChannelUrl(String),
    /// Bare channel id (`UC`/`SC` + 22, SC normalized to UC) — caller checks
    /// `valid_channel_id_checksum`.
    ChannelBare(String),
    /// `@handle` (bare or `youtube.com/@x`) — resolve via `parse_channel_ref`.
    Handle(String),
    /// `/user/NAME` or `/c/NAME` slug — stored for later resolution.
    Custom(String),
}

/// Capture mode per marker: video markers take exactly 11 id-chars (LLD §6.1
/// contract — trailing junk is NOT captured); id markers take a run of id
/// chars; name markers (handles, /user/, /c/) take everything up to a
/// delimiter.
enum Capture {
    Video11,
    IdRun,
    UntilDelim,
}

/// Marker table (A2, from MW Metadata shared.js `patterns`). Deterministic
/// leftmost match; ties at the same byte position go to the EARLIER marker
/// in the list, so `channel/` must precede its prefix `c/`.
const MARKERS: [(&str, Capture); 14] = [
    ("v=", Capture::Video11),
    ("shorts/", Capture::Video11),
    ("youtu.be/", Capture::Video11),
    ("/v/", Capture::Video11),
    ("/embed/", Capture::Video11),
    ("/video/", Capture::Video11),
    ("/watch/", Capture::Video11),
    ("/live/", Capture::Video11),
    ("playlist?list=", Capture::IdRun),
    ("youtube.com/channel/", Capture::IdRun),
    ("youtube.com/show/", Capture::IdRun),
    ("youtube.com/user/", Capture::UntilDelim),
    ("youtube.com/c/", Capture::UntilDelim),
    ("youtube.com/@", Capture::UntilDelim),
];

/// The extraction contract (LLD §6.1) extended (A2): all video URL forms
/// (`v=`, `/v/`, `/embed/`, `/shorts/`, `/video/`, `/watch/`, `/live/`,
/// `youtu.be/`) plus bare 11-char ids. Bare ids must pass the archiveteam
/// checksum (A1); URL captures are authoritative. Deterministic
/// leftmost-match scanning with per-item continue semantics and order-
/// preserving dedupe.
pub fn extract_video_ids(input: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for item in parse_input_items(input) {
        let id = match item {
            InputItem::VideoUrl(id) => Some(id),
            InputItem::VideoBare(id) if valid_video_id_checksum(&id) => Some(id),
            _ => None,
        };
        if let Some(id) = id {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Parse multi-line input into typed items (A2): per line — bare ids first
/// (whole-line anchored, MW style), then the marker scan. Order-preserving
/// dedupe of identical (kind, value) pairs.
pub fn parse_input_items(input: &str) -> Vec<InputItem> {
    let mut out = Vec::new();
    for line in input.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(item) = bare_item(t) {
            push_item(&mut out, item);
        } else {
            scan_line(t, &mut out);
        }
    }
    out
}

/// Whole-line bare inputs (MW anchored patterns): `@handle`, 11-char video
/// id, `UC`/`SC` + 22 channel id (SC→UC), playlist-prefixed id.
fn bare_item(t: &str) -> Option<InputItem> {
    if let Some(h) = t.strip_prefix('@') {
        if !h.is_empty() && !h.contains('/') && !h.contains('?') {
            return Some(InputItem::Handle(format!("@{h}")));
        }
    }
    if t.len() == 11 && t.bytes().all(|b| is_id_char(&b)) {
        return Some(InputItem::VideoBare(t.to_string()));
    }
    if t.len() == 24 && (t.starts_with("UC") || t.starts_with("SC"))
        && t.bytes().all(|b| is_id_char(&b))
    {
        return Some(InputItem::ChannelBare(normalize_channel_id(t)));
    }
    for prefix in ["UU", "UUSH", "PL", "FL", "SP", "OLAK"] {
        if t.len() > prefix.len() && t.starts_with(prefix) && t.bytes().all(|b| is_id_char(&b)) {
            return Some(InputItem::Playlist(t.to_string()));
        }
    }
    None
}

/// Leftmost-marker scan of one line, per-item continue semantics (the
/// Phase 0 contract: a failed capture advances one byte and keeps scanning).
fn scan_line(line: &str, out: &mut Vec<InputItem>) {
    let bytes = line.as_bytes();
    let n = bytes.len();
    let mut pos = 0;
    while pos < n {
        let mut found_at = usize::MAX;
        let mut found = 0usize;
        for (mi, (marker, _)) in MARKERS.iter().enumerate() {
            if let Some(rel) = find_sub(bytes, marker.as_bytes(), pos) {
                if rel < found_at {
                    found_at = rel;
                    found = mi;
                }
            }
        }
        if found_at == usize::MAX {
            break;
        }
        let (marker, capture) = &MARKERS[found];
        let cap_start = found_at + marker.len();
        match capture {
            Capture::Video11 => {
                if cap_start + 11 <= n && bytes[cap_start..cap_start + 11].iter().all(is_id_char) {
                    let id = String::from_utf8_lossy(&bytes[cap_start..cap_start + 11]).to_string();
                    push_item(out, InputItem::VideoUrl(id));
                    pos = cap_start + 11;
                } else {
                    pos = found_at + 1;
                }
            }
            Capture::IdRun => {
                let run = take_id_run(bytes, cap_start);
                if !run.is_empty() {
                    let value = String::from_utf8_lossy(run).to_string();
                    push_item(out, id_run_item(marker, value));
                    pos = cap_start + run.len();
                } else {
                    pos = found_at + 1;
                }
            }
            Capture::UntilDelim => {
                let run = take_until_delim(bytes, cap_start);
                if !run.is_empty() {
                    let value = String::from_utf8_lossy(run).to_string();
                    push_item(out, delim_item(marker, value));
                    pos = cap_start + run.len();
                } else {
                    pos = found_at + 1;
                }
            }
        }
    }
}

fn id_run_item(marker: &str, value: String) -> InputItem {
    match marker {
        "playlist?list=" => InputItem::Playlist(value),
        "youtube.com/channel/" | "youtube.com/show/" => {
            InputItem::ChannelUrl(normalize_channel_id(&value))
        }
        _ => unreachable!("id-run markers covered"),
    }
}

fn delim_item(marker: &str, value: String) -> InputItem {
    match marker {
        "youtube.com/user/" | "youtube.com/c/" => InputItem::Custom(value),
        "youtube.com/@" => InputItem::Handle(format!("@{value}")),
        _ => unreachable!("delim markers covered"),
    }
}

fn take_id_run(bytes: &[u8], from: usize) -> &[u8] {
    let mut end = from;
    while end < bytes.len() && is_id_char(&bytes[end]) {
        end += 1;
    }
    &bytes[from..end]
}

fn take_until_delim(bytes: &[u8], from: usize) -> &[u8] {
    let mut end = from;
    while end < bytes.len() && !matches!(bytes[end], b'/' | b'?' | b' ' | b'\t') {
        end += 1;
    }
    &bytes[from..end]
}

fn push_item(out: &mut Vec<InputItem>, item: InputItem) {
    if !out.contains(&item) {
        out.push(item);
    }
}

/// `SC`-prefixed channel ids (legacy shows) query as their `UC` twin — the
/// MW Metadata quirk (`/show/SC...` → transform prefix before querying).
fn normalize_channel_id(s: &str) -> String {
    if s.len() == 24 && s.starts_with("SC") && s.bytes().all(|b| is_id_char(&b)) {
        format!("UC{}", &s[2..])
    } else {
        s.to_string()
    }
}

fn find_sub(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() || needle.len() > hay.len() - from {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

fn is_id_char(b: &u8) -> bool {
    b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-'
}

/// Parse multi-line input: blank-line separated groups, `#` comments (LLD §6.1).
pub fn parse_links_input(input: &str) -> Vec<String> {
    input
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// `ingest channels <ref...>`: resolve refs → fetch RSS (ETag-aware) →
/// optional API enrichment → upsert.
pub async fn ingest_channels(
    cfg: &Config,
    clients: &FetchClients,
    db: &mut Db,
    refs: &[String],
    opts: &IngestOptions,
) -> Result<IngestSummary, TubeforgeError> {
    check_api_requirement(cfg, opts)?;

    let mut targets = Vec::new();
    for r in refs {
        match parse_channel_ref(r)? {
            ChannelRef::Direct(id) => targets.push(Target { channel_id: id, handle: None }),
            ChannelRef::Handle(h) => {
                // @handle needs channels.list(forHandle) → API key (LLD §6.1).
                let key = cfg.youtube_api_key.as_deref().ok_or_else(|| {
                    TubeforgeError::Usage(format!(
                        "{h} requires YOUTUBE_API_KEY or a channel ID/URL"
                    ))
                })?;
                let id = ApiClient::new(clients, key).resolve_handle(&h).await?;
                targets.push(Target { channel_id: id, handle: Some(h) });
            }
        }
    }

    run_channel_batch(cfg, clients, db, &targets, opts).await
}

/// `refresh [--channel <id>...]`: re-fetch known channels (ETag-aware); a
/// 304 skips the channel. Unknown refs are a Usage error (refresh is only
/// for channels already in the database).
pub async fn refresh_channels(
    cfg: &Config,
    clients: &FetchClients,
    db: &mut Db,
    only: &[String],
    opts: &IngestOptions,
) -> Result<IngestSummary, TubeforgeError> {
    let mut targets = Vec::new();
    if only.is_empty() {
        for c in db.all_channels().await? {
            targets.push(Target { channel_id: c.channel_id, handle: c.handle });
        }
    } else {
        for r in only {
            let (id, handle) = match parse_channel_ref(r)? {
                ChannelRef::Direct(id) => (id, None),
                ChannelRef::Handle(h) => (String::new(), Some(h)),
            };
            // Handle refs resolve against stored channels; ids must exist.
            let row = if handle.is_none() {
                db.get_channel(&id).await?
            } else {
                db.all_channels()
                    .await?
                    .into_iter()
                    .find(|c| c.handle == handle)
            };
            let Some(row) = row else {
                return Err(TubeforgeError::Usage(format!(
                    "channel not in database: {r} (refresh only re-fetches known channels)"
                )));
            };
            targets.push(Target {
                channel_id: row.channel_id,
                handle: row.handle,
            });
        }
    }
    run_channel_batch(cfg, clients, db, &targets, opts).await
}

/// `ingest links`: video ids → oEmbed (no key) or batched videos.list (key +
/// `--api`), with oEmbed fallback when the API degrades (LLD §5.3).
pub async fn ingest_links(
    cfg: &Config,
    clients: &FetchClients,
    db: &mut Db,
    ids: &[String],
    opts: &IngestOptions,
) -> Result<IngestSummary, TubeforgeError> {
    check_api_requirement(cfg, opts)?;

    let mut summary = IngestSummary {
        batch_id: util::batch_id(),
        api: if opts.use_api { "ok".to_string() } else { "off".to_string() },
        ..Default::default()
    };
    let now = util::now_rfc3339();
    let mut channels: Vec<ChannelRow> = Vec::new();
    let mut videos: Vec<VideoRow> = Vec::new();
    let mut logs: Vec<LogRow> = Vec::new();
    let mut placeholder_ids: HashSet<String> = HashSet::new();

    if opts.use_api {
        let key = cfg.youtube_api_key.as_deref().expect("checked above");
        let client = ApiClient::new(clients, key);
        let items = match fetch_api_items(cfg, db, &client, ids, &mut summary).await {
            Ok(items) => items,
            Err(TubeforgeError::Quota { .. }) => {
                summary.api = "quota".to_string();
                alert_quota(db, &mut summary, "videos.list quota exhausted — fell back to oEmbed").await;
                Vec::new()
            }
            Err(e) => {
                summary.api = "error".to_string();
                tracing::warn!(err = %e, "videos.list failed — falling back to oEmbed");
                Vec::new()
            }
        };
        for item in items {
            videos.push(video_from_api(&item, &now));
            if let (Some(cid), Some(ctitle)) = (&item.channel_id, &item.channel_title) {
                if placeholder_ids.insert(cid.clone()) {
                    channels.push(ChannelRow {
                        channel_id: cid.clone(),
                        title: ctitle.clone(),
                        source: "api".to_string(),
                        fetched_at: now.clone(),
                        updated_at: now.clone(),
                        ..Default::default()
                    });
                }
            }
        }
    }

    let covered: HashSet<String> = videos.iter().map(|v| v.video_id.clone()).collect();
    for id in ids {
        if covered.contains(id) {
            continue; // already enriched via API
        }
        match oembed::fetch(clients, id).await {
            Ok(o) => {
                let handle = o.handle();
                let (row, chan) = video_from_oembed(&o, id, handle.as_deref(), &now);
                if let Some(c) = chan {
                    if placeholder_ids.insert(c.channel_id.clone()) {
                        channels.push(c);
                    }
                }
                videos.push(row);
                logs.push((
                    format!("video {id}"),
                    "ok".to_string(),
                    Some("oembed".to_string()),
                ));
            }
            Err(e) => {
                summary.videos_failed += 1;
                logs.push((format!("video {id}"), "failed".to_string(), Some(e.to_string())));
            }
        }
    }

    write_batch(cfg, db, opts, &mut summary, channels, videos, logs).await?;
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Channel batch core
// ---------------------------------------------------------------------------

async fn run_channel_batch(
    cfg: &Config,
    clients: &FetchClients,
    db: &mut Db,
    targets: &[Target],
    opts: &IngestOptions,
) -> Result<IngestSummary, TubeforgeError> {
    let mut summary = IngestSummary {
        batch_id: util::batch_id(),
        api: "off".to_string(),
        ..Default::default()
    };
    let now = util::now_rfc3339();
    let mut channels: Vec<ChannelRow> = Vec::new();
    let mut videos: Vec<VideoRow> = Vec::new();
    let mut logs: Vec<LogRow> = Vec::new();

    // 1. Fetch all (network first; single-connection constraint is irrelevant
    //    here — no DB writes during fetch).
    for t in targets {
        let existing = db.get_channel(&t.channel_id).await?;
        let etag = existing.as_ref().and_then(|c| c.etag.clone());
        match rss::fetch_feed(clients, &t.channel_id, etag.as_deref()).await {
            Ok(FeedResult::NotModified) => {
                summary.channels_skipped += 1;
                logs.push((
                    format!("channel {}", t.channel_id),
                    "skipped".to_string(),
                    Some("304 not modified".to_string()),
                ));
            }
            Ok(FeedResult::Feed { feed, etag }) => {
                let title = feed
                    .channel_title
                    .clone()
                    .or_else(|| existing.as_ref().map(|c| c.title.clone()))
                    .unwrap_or_else(|| t.channel_id.clone());
                let handle = t
                    .handle
                    .clone()
                    .or_else(|| existing.as_ref().and_then(|c| c.handle.clone()));
                let chan = ChannelRow {
                    channel_id: t.channel_id.clone(),
                    handle,
                    title: title.clone(),
                    description: existing.as_ref().and_then(|c| c.description.clone()),
                    source: "rss".to_string(),
                    etag,
                    fetched_at: now.clone(),
                    updated_at: now.clone(),
                    ..Default::default()
                };
                match &existing {
                    None => {
                        summary.channels_added += 1;
                        channels.push(chan);
                    }
                    Some(ex) => {
                        let meta_changed = ex.title != title || ex.handle != chan.handle;
                        if meta_changed {
                            summary.channels_updated += 1;
                            channels.push(chan);
                        } else {
                            summary.channels_skipped += 1;
                            // ETag-only change: persist it (keeps future
                            // refreshes 304-efficient) but not content.
                            if ex.etag != chan.etag {
                                channels.push(chan);
                            }
                        }
                    }
                }
                logs.push((
                    format!("channel {}", t.channel_id),
                    "ok".to_string(),
                    Some(format!("feed fetched ({} entries)", feed.entries.len())),
                ));
                for v in feed.entries {
                    videos.push(video_from_rss(&v, &t.channel_id, &now));
                }
            }
            Err(e) => {
                summary.channels_failed += 1;
                logs.push((
                    format!("channel {}", t.channel_id),
                    "failed".to_string(),
                    Some(e.to_string()),
                ));
            }
        }
    }

    // 2. Optional API enrichment (videos.list, batched, quota-ledgered).
    if opts.use_api {
        let key = cfg.youtube_api_key.as_deref().expect("checked above");
        let client = ApiClient::new(clients, key);
        match fetch_api_items(cfg, db, &client, &video_ids(&videos), &mut summary).await {
            Ok(items) => {
                merge_api_rows(&mut videos, items);
                summary.api = "ok".to_string();
            }
            Err(TubeforgeError::Quota { .. }) => {
                summary.api = "quota".to_string();
                alert_quota(db, &mut summary, "videos.list quota exhausted — keeping RSS data").await;
            }
            Err(e) => {
                summary.api = "error".to_string();
                tracing::warn!(err = %e, "videos.list failed — keeping RSS data");
            }
        }
    }

    // 3. Change detection + backup guard + single transaction + index.
    write_batch(cfg, db, opts, &mut summary, channels, videos, logs).await?;
    Ok(summary)
}

/// Fetch rich metadata for `ids` (batched ≤50/call, quota ledgered) after a
/// pre-flight quota check.
async fn fetch_api_items(
    cfg: &Config,
    db: &Db,
    client: &ApiClient,
    ids: &[String],
    _summary: &mut IngestSummary,
) -> Result<Vec<ApiVideo>, TubeforgeError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let projected = ids.len().div_ceil(BATCH_MAX) as u64;
    let pre = quota::preflight(db, projected, cfg.quota_warn_at).await?;
    if pre.warn {
        tracing::warn!(
            used = pre.used,
            projected = pre.projected,
            remaining = pre.remaining,
            "YouTube API quota nearing daily limit"
        );
    }
    if pre.remaining == 0 {
        return Err(TubeforgeError::Quota {
            endpoint: crate::error::Endpoint::VideosList,
            remaining: 0,
        });
    }
    client.fetch_videos(db, ids).await
}

// ---------------------------------------------------------------------------
// Write phase (LLD §6.3): backup guard → one transaction → index → logs
// ---------------------------------------------------------------------------

async fn write_batch(
    cfg: &Config,
    db: &mut Db,
    opts: &IngestOptions,
    summary: &mut IngestSummary,
    channels: Vec<ChannelRow>,
    videos: Vec<VideoRow>,
    mut logs: Vec<LogRow>,
) -> Result<(), TubeforgeError> {
    // Channel rows written here were counted by the caller; videos need
    // change detection (source precedence + field comparison).
    let mut to_write: Vec<VideoRow> = Vec::new();
    for row in &videos {
        match db.get_video(&row.video_id).await? {
            None => {
                summary.videos_added += 1;
                to_write.push(row.clone());
                logs.push((format!("video {}", row.video_id), "ok".to_string(), Some("added".to_string())));
            }
            Some(existing) => {
                let in_rank = source_rank(&row.source);
                let ex_rank = source_rank(&existing.source);
                if in_rank < ex_rank {
                    summary.videos_skipped += 1;
                    logs.push((
                        format!("video {}", row.video_id),
                        "skipped".to_string(),
                        Some(format!("lower source precedence ({} < {})", row.source, existing.source)),
                    ));
                } else if videos_equal(row, &existing) {
                    summary.videos_skipped += 1;
                    logs.push((format!("video {}", row.video_id), "skipped".to_string(), Some("unchanged".to_string())));
                } else {
                    summary.videos_updated += 1;
                    to_write.push(row.clone());
                    logs.push((format!("video {}", row.video_id), "ok".to_string(), Some("updated".to_string())));
                }
            }
        }
    }

    let has_changes = summary.channels_added + summary.channels_updated
        + summary.videos_added + summary.videos_updated
        > 0;

    // Backup guard: before EVERY batch that will write, unless --no-backup
    // (LLD §6.3, §9.1). 304-only refreshes never reach here with changes.
    if has_changes && !opts.no_backup {
        summary.snapshot = Some(backup::backup(db, &cfg.backup_dir, cfg.backup_keep).await?);
    }

    if has_changes || !channels.is_empty() || !logs.is_empty() {
        let mut batch = db.begin_batch().await?;
        for c in &channels {
            batch.upsert_channel(c).await?;
        }
        for v in &to_write {
            batch.upsert_video(v).await?;
        }
        for (item, status, detail) in &logs {
            batch
                .log_ingest(&summary.batch_id, item, status, detail.as_deref())
                .await?;
        }
        batch.commit().await?;
    }

    // tantivy: one writer, delete stale + add new, single commit (LLD §6.4).
    if !to_write.is_empty() {
        let docs: Vec<VideoDoc> = to_write.iter().map(video_to_doc).collect();
        let index = search::open_or_create(&cfg.index_dir())?;
        let fields = index.schema();
        let mut writer = index.writer(50_000_000).map_err(|e| TubeforgeError::Index {
            detail: e.to_string(),
        })?;
        for d in &docs {
            search::index::upsert(&mut writer, &fields, d)?;
        }
        writer.commit().map_err(|e| TubeforgeError::Index {
            detail: format!("commit: {e}"),
        })?;
        tracing::info!(docs = docs.len(), "index updated");
        // Index freshness stamp (health checks last_reindex_at vs last ingest).
        db.meta_set("last_reindex_at", &util::now_rfc3339()).await?;
    }

    // Scoring: recompute only for changed/inserted videos (LLD §6.4). The
    // batch and index are already committed — scoring is best-effort: a
    // failure must not roll back ingested data.
    if !to_write.is_empty() {
        match post_ingest_scoring(cfg, db, &to_write).await {
            Ok(scored) => tracing::info!(scored, "scores recomputed"),
            Err(e) => tracing::warn!(err = %e, "score recompute failed (data committed)"),
        }
    }

    Ok(())
}

/// Recompute + persist SEO/GEO scores for the given videos (LLD §6.4, §7.5).
async fn post_ingest_scoring(
    cfg: &Config,
    db: &Db,
    rows: &[VideoRow],
) -> Result<usize, TubeforgeError> {
    let index = search::open_or_create(&cfg.index_dir())?;
    let mut bm25 = crate::search::bm25::Bm25::open(index)?;
    bm25.reload()?; // pick up the just-committed segment
    let weights = crate::scoring::weights::Weights::from_env()?;
    let mut scored = 0;
    for v in rows {
        crate::scoring::score_video(db, &bm25, v, &weights).await?;
        scored += 1;
    }
    Ok(scored)
}

// ---------------------------------------------------------------------------
// Row construction
// ---------------------------------------------------------------------------

/// Source precedence rank (LLD §6.2): api > oembed > rss.
pub fn source_rank(source: &str) -> u8 {
    match source {
        "api" => 3,
        "oembed" => 2,
        _ => 1,
    }
}

fn video_from_rss(v: &RssVideo, channel_id: &str, now: &str) -> VideoRow {
    VideoRow {
        video_id: v.video_id.clone(),
        channel_id: Some(channel_id.to_string()),
        title: v.title.clone(),
        description: v.description.clone(),
        tags: "[]".to_string(),
        published_at: normalize_ts(&v.published).unwrap_or_else(|| now.to_string()),
        view_count: v.views,
        thumb_url: v.thumbnail_url.clone(),
        source: "rss".to_string(),
        fetched_at: now.to_string(),
        updated_at: now.to_string(),
        ..Default::default()
    }
}

fn video_from_oembed(o: &oembed::OEmbed, id: &str, handle: Option<&str>, now: &str) -> (VideoRow, Option<ChannelRow>) {
    let row = VideoRow {
        video_id: id.to_string(),
        // oEmbed links have no channel_id; @handle-keyed placeholder channel
        // when the author URL carries a handle, else NULL (LLD §3.1 note).
        channel_id: handle.map(|h| h.to_string()),
        title: o.title.clone().unwrap_or_else(|| id.to_string()),
        description: String::new(),
        tags: "[]".to_string(),
        published_at: now.to_string(), // oEmbed carries no publish date (LLD §5.2)
        thumb_url: o.thumbnail_url.clone(),
        source: "oembed".to_string(),
        fetched_at: now.to_string(),
        updated_at: now.to_string(),
        ..Default::default()
    };
    let chan = handle.map(|h| ChannelRow {
        channel_id: h.to_string(),
        handle: Some(h.to_string()),
        title: o.author_name.clone().unwrap_or_else(|| h.to_string()),
        source: "oembed".to_string(),
        fetched_at: now.to_string(),
        updated_at: now.to_string(),
        ..Default::default()
    });
    (row, chan)
}

fn video_from_api(a: &ApiVideo, now: &str) -> VideoRow {
    VideoRow {
        video_id: a.video_id.clone(),
        channel_id: a.channel_id.clone(),
        title: a.title.clone().unwrap_or_default(),
        description: a.description.clone().unwrap_or_default(),
        tags: serde_json::to_string(&a.tags).unwrap_or_else(|_| "[]".to_string()),
        category_id: a.category_id.clone(),
        duration_sec: a.duration_sec,
        published_at: normalize_ts(a.published_at.as_deref().unwrap_or("")).unwrap_or_else(|| now.to_string()),
        view_count: a.view_count,
        like_count: a.like_count,
        comment_count: a.comment_count,
        thumb_url: a.thumb_url.clone(),
        source: "api".to_string(),
        fetched_at: now.to_string(),
        updated_at: now.to_string(),
        recording_date: a.recording_date.clone(),
        recording_location_name: a.recording_location_name.clone(),
        recording_lat: a.recording_lat,
        recording_lng: a.recording_lng,
        topic_categories: serde_json::to_string(&a.topic_categories).unwrap_or_else(|_| "[]".to_string()),
    }
}

/// Replace RSS/oEmbed rows with richer API rows for the same video ids
/// (api rank 3 > rss rank 1 — a later write_batch pass keeps api data).
fn merge_api_rows(videos: &mut [VideoRow], items: Vec<ApiVideo>) {
    let by_id: std::collections::HashMap<String, ApiVideo> =
        items.into_iter().map(|i| (i.video_id.clone(), i)).collect();
    let now = util::now_rfc3339();
    for row in videos.iter_mut() {
        let Some(a) = by_id.get(&row.video_id) else { continue };
        row.source = "api".to_string();
        row.updated_at = now.clone();
        if let Some(t) = &a.title {
            if !t.is_empty() {
                row.title = t.clone();
            }
        }
        if let Some(d) = &a.description {
            if !d.is_empty() {
                row.description = d.clone();
            }
        }
        if !a.tags.is_empty() {
            row.tags = serde_json::to_string(&a.tags).unwrap_or_else(|_| "[]".to_string());
        }
        if let Some(c) = &a.category_id {
            row.category_id = Some(c.clone());
        }
        if let Some(d) = a.duration_sec {
            row.duration_sec = Some(d);
        }
        if let Some(p) = &a.published_at {
            if let Some(ts) = normalize_ts(p) {
                row.published_at = ts;
            }
        }
        if let Some(v) = a.view_count {
            row.view_count = Some(v);
        }
        if let Some(l) = a.like_count {
            row.like_count = Some(l);
        }
        if let Some(c) = a.comment_count {
            row.comment_count = Some(c);
        }
        if let Some(t) = &a.thumb_url {
            row.thumb_url = Some(t.clone());
        }
        if let Some(c) = &a.channel_id {
            row.channel_id = Some(c.clone());
        }
        if let Some(d) = &a.recording_date {
            row.recording_date = Some(d.clone());
        }
        if let Some(n) = &a.recording_location_name {
            row.recording_location_name = Some(n.clone());
        }
        if let Some(lat) = a.recording_lat {
            row.recording_lat = Some(lat);
        }
        if let Some(lng) = a.recording_lng {
            row.recording_lng = Some(lng);
        }
        if !a.topic_categories.is_empty() {
            row.topic_categories =
                serde_json::to_string(&a.topic_categories).unwrap_or_else(|_| "[]".to_string());
        }
    }
}

fn video_to_doc(v: &VideoRow) -> VideoDoc {
    let tags: Vec<String> = serde_json::from_str(&v.tags).unwrap_or_default();
    let published_at = DateTime::parse_from_rfc3339(&v.published_at)
        .ok()
        .map(|d| d.with_timezone(&Utc).timestamp());
    VideoDoc {
        video_id: v.video_id.clone(),
        channel_id: v.channel_id.clone(),
        title: v.title.clone(),
        description: v.description.clone(),
        tags,
        published_at,
    }
}

/// Normalize an RFC3339 timestamp to UTC seconds precision.
fn normalize_ts(ts: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|d| d.with_timezone(&Utc).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn video_ids(videos: &[VideoRow]) -> Vec<String> {
    videos.iter().map(|v| v.video_id.clone()).collect()
}

fn videos_equal(a: &VideoRow, b: &VideoRow) -> bool {
    a.title == b.title
        && a.description == b.description
        && a.tags == b.tags
        && a.published_at == b.published_at
        && a.view_count == b.view_count
        && a.like_count == b.like_count
        && a.comment_count == b.comment_count
        && a.thumb_url == b.thumb_url
        && a.duration_sec == b.duration_sec
        && a.category_id == b.category_id
        && a.recording_date == b.recording_date
        && a.recording_location_name == b.recording_location_name
        && a.recording_lat == b.recording_lat
        && a.recording_lng == b.recording_lng
        && a.topic_categories == b.topic_categories
}

fn check_api_requirement(cfg: &Config, opts: &IngestOptions) -> Result<(), TubeforgeError> {
    if opts.use_api && cfg.youtube_api_key.is_none() {
        return Err(TubeforgeError::Usage(
            "--api requires YOUTUBE_API_KEY in .env (RSS/oEmbed work without it)".to_string(),
        ));
    }
    Ok(())
}

async fn alert_quota(db: &Db, summary: &mut IngestSummary, message: &str) {
    if let Err(e) = db.insert_alert("quota", None, message, "warn").await {
        tracing::warn!(err = %e, "failed to write quota alert");
    } else {
        summary.alerts.push(message.to_string());
    }
}

/// Record per-item rejects (checksum-invalid ids, unsupported kinds) into
/// `ingest_log` as `failed` rows so nothing is silently dropped (A1/A2
/// labeling contract, LLD §6.4 "ingest_log rows per item"). Logs-only mini
/// batch: no data writes, so no backup guard (LLD §6.3).
pub async fn record_invalid_items(
    db: &mut Db,
    batch_id: &str,
    items: &[(String, String)],
) -> Result<(), TubeforgeError> {
    if items.is_empty() {
        return Ok(());
    }
    let mut batch = db.begin_batch().await?;
    for (item, detail) in items {
        batch
            .log_ingest(batch_id, item, "failed", Some(detail))
            .await?;
    }
    batch.commit().await?;
    Ok(())
}
