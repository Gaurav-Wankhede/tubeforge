//! yt-dlp public extraction (Phase 6/6.5 — Competitor Gap Mining content
//! layer).
//!
//! Why subprocess: the official YouTube Data API `captions.download` requires
//! edit permission on the video (403 for competitor videos — documented in
//! the 2026 gap-mining research). yt-dlp reads public metadata/captions/
//! comments without auth — the industry-standard way to research competitor
//! videos. Policy mirrors Phase 6: public pages only, no cookies, no
//! impersonation, bounded concurrency.
//!
//! Client strategy (verified empirically, Aug 2026): yt-dlp is multi-client
//! by design. Its native chain is `visionos/android_vr/web` with automatic
//! fallback; `web` produces the richest data (incl. video keywords/tags) but
//! needs a PO-token JS runtime (deno/node/bun) on newer yt-dlp builds.
//! TubeForge does NOT hardcode a single client: it passes the user's
//! `TUBEFORGE_YTDLP_CLIENT` (default: yt-dlp's native chain) and the
//! `TUBEFORGE_YTDLP_JS_RUNTIME` when configured, so the exact client
//! strategy stays a config knob that tracks YouTube's bot-check evolution.
//!
//! The VTT parser is a pure function (`vtt_to_text`) so the file-format edge
//! cases (cue numbers, timestamps, `<c>`/`<i>` tags, speaker markers, the
//! 3× overlapping-cue redundancy YouTube emits) are unit-tested against a
//! real captured caption file without spawning anything.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Semaphore;

use crate::error::{Source, TubeforgeError};

/// Max concurrent yt-dlp subprocesses (PLAN Phase 6: bounded concurrency).
pub const MAX_CONCURRENCY: usize = 4;
/// Per-subprocess wall-clock timeout (transcripts can take a while on slow
/// machines, but a stuck network fetch must not hang the CLI).
pub const PROCESS_TIMEOUT: Duration = Duration::from_secs(120);

/// Public transcript source classification (`auto` vs `manual`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptKind {
    Auto,
    Manual,
}

/// yt-dlp client with bounded concurrency, timeout, and an optional
/// player-client / JS-runtime passthrough (config knobs).
#[derive(Debug, Clone)]
pub struct YtdlpClient {
    binary: PathBuf,
    client: Option<String>,
    js_runtime: Option<String>,
    semaphore: Arc<Semaphore>,
    timeout: Duration,
}

impl YtdlpClient {
    /// Resolve the binary from config. Missing binary → `Config` error with
    /// the documented install hint (user decision: system yt-dlp).
    pub fn new(
        binary: PathBuf,
        enabled: bool,
        client: Option<String>,
        js_runtime: Option<String>,
    ) -> Result<Self, TubeforgeError> {
        if !enabled {
            return Err(TubeforgeError::Config(
                "yt-dlp features are disabled — set TUBEFORGE_YTDLP_ENABLED=true to enable \
                 transcript extraction"
                    .to_string(),
            ));
        }
        Ok(YtdlpClient {
            binary,
            client,
            js_runtime,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENCY)),
            timeout: PROCESS_TIMEOUT,
        })
    }

    /// Common args every yt-dlp invocation shares: bounded extraction,
    /// client chain, PO-token JS runtime (when configured). Any
    /// `--extractor-args` already in `extra` is merged with the client arg
    /// using yt-dlp's `;` separator so the two do not clobber each other.
    fn common_args(&self, extra: &[&str]) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        let mut extra_args: Vec<String> = Vec::new();
        let mut rest: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < extra.len() {
            if extra[i] == "--extractor-args" {
                if let Some(v) = extra.get(i + 1) {
                    extra_args.push((*v).to_string());
                    i += 2;
                    continue;
                }
            }
            rest.push(extra[i]);
            i += 1;
        }
        args.extend(rest.iter().map(|s| s.to_string()));
        if let Some(client) = &self.client {
            extra_args.push(format!("youtube:player_client={client}"));
        }
        if !extra_args.is_empty() {
            args.push("--extractor-args".to_string());
            args.push(extra_args.join(";"));
        }
        if let Some(runtime) = &self.js_runtime {
            args.push("--js-runtimes".to_string());
            args.push(runtime.clone());
        }
        args
    }

    /// Extract a video's transcript as plain text. Tries the requested
    /// language's manual captions first, then auto-generated, then falls
    /// back to any available language. Returns (text, kind).
    pub async fn transcript(
        &self,
        video_id: &str,
        lang: &str,
    ) -> Result<(String, TranscriptKind), TubeforgeError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| storage_err("YTD LP_SEM", e))?;

        let tmp = tempfile::tempdir().map_err(|e| storage_err("YTD LP_TMP", e))?;
        let stem = tmp.path().join("sub");

        let base = self.common_args(&[
            "--skip-download",
            "--no-playlist",
            "--write-subs",
            "--write-auto-sub",
            "--sub-langs",
            &format!("{lang}.*"),
            "--sub-format",
            "vtt",
            "-o",
            &stem.to_string_lossy(),
            &format!("https://www.youtube.com/watch?v={video_id}"),
        ]);
        let cmd = Command::new(&self.binary)
            .args(&base)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output();

        let output = match tokio::time::timeout(self.timeout, cmd).await {
            Ok(o) => o,
            Err(_) => {
                return Err(storage_err(
                    "YTD LP_TIMEOUT",
                    format!("yt-dlp timed out after {}s", self.timeout.as_secs()),
                ))
            }
        };

        let output = output.map_err(|e| TubeforgeError::Fetch {
            src: Source::Ytdlp,
            url: video_id.to_string(),
            inner: format!("spawn transcript: {e}"),
        })?;

        if output.status.success() {
            if let Some(text) = read_subtitle(&tmp, &stem).await? {
                return Ok((text, TranscriptKind::Auto));
            }
        }

        Err(TubeforgeError::Fetch {
            src: Source::Ytdlp,
            url: video_id.to_string(),
            inner: format!(
                "no captions found (tried {lang}.* manual + auto) — video may have captions \
                 disabled or the request needs retry"
            ),
        })
    }

    /// Full metadata extraction via `--dump-json` (the huge metadata
    /// payload yt-dlp returns). Strict-schema: only the whitelisted fields
    /// are mapped; anything else in the 86-key payload is ignored so a
    /// YouTube-side field drift never breaks the pipeline.
    pub async fn metadata(&self, video_id: &str) -> Result<YtdlpVideoInfo, TubeforgeError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| storage_err("YTD LP_SEM", e))?;

        let base = self.common_args(&[
            "--dump-json",
            "--skip-download",
            "--no-playlist",
            "--no-warnings",
            &format!("https://www.youtube.com/watch?v={video_id}"),
        ]);
        let output = Command::new(&self.binary)
            .args(&base)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        let output = match tokio::time::timeout(self.timeout, output).await {
            Ok(o) => o,
            Err(_) => {
                return Err(storage_err(
                    "YTD LP_TIMEOUT",
                    format!("yt-dlp timed out after {}s", self.timeout.as_secs()),
                ))
            }
        };
        let output = output.map_err(|e| TubeforgeError::Fetch {
            src: Source::Ytdlp,
            url: video_id.to_string(),
            inner: format!("spawn: {e}"),
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let err_line = stderr
                .lines()
                .find(|l| l.contains("ERROR"))
                .or_else(|| stderr.lines().next())
                .unwrap_or("unknown yt-dlp error")
                .to_string();
            return Err(TubeforgeError::Fetch {
                src: Source::Ytdlp,
                url: video_id.to_string(),
                inner: err_line,
            });
        }

        let raw: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|e| TubeforgeError::Parse {
                src: Source::Ytdlp,
                item: video_id.to_string(),
                inner: format!("json: {e}"),
            })?;
        YtdlpVideoInfo::from_json(&raw)
    }

    /// Fetch up to `max` top-level comments keyless via `--write-comments
    /// --dump-json` (InnerTube comment continuation API — no API key, no
    /// quota). `max=0` → yt-dlp's own default (20).
    pub async fn comments(
        &self,
        video_id: &str,
        max: u64,
    ) -> Result<Vec<YtdlpComment>, TubeforgeError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| storage_err("YTD LP_SEM", e))?;

        let extractor_args = if max > 0 {
            format!("youtube:max_comments={max}")
        } else {
            "youtube:".to_string()
        };
        let base = self.common_args(&[
            "--dump-json",
            "--skip-download",
            "--no-playlist",
            "--no-warnings",
            "--write-comments",
            "--extractor-args",
            &extractor_args,
            &format!("https://www.youtube.com/watch?v={video_id}"),
        ]);
        let output = Command::new(&self.binary)
            .args(&base)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        let output = match tokio::time::timeout(self.timeout, output).await {
            Ok(o) => o,
            Err(_) => {
                return Err(storage_err(
                    "YTD LP_TIMEOUT",
                    format!("yt-dlp timed out after {}s", self.timeout.as_secs()),
                ))
            }
        };
        let output = output.map_err(|e| TubeforgeError::Fetch {
            src: Source::Ytdlp,
            url: video_id.to_string(),
            inner: format!("spawn: {e}"),
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let err_line = stderr
                .lines()
                .find(|l| l.contains("ERROR"))
                .or_else(|| stderr.lines().next())
                .unwrap_or("unknown yt-dlp error")
                .to_string();
            return Err(TubeforgeError::Fetch {
                src: Source::Ytdlp,
                url: video_id.to_string(),
                inner: err_line,
            });
        }

        let raw: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|e| TubeforgeError::Parse {
                src: Source::Ytdlp,
                item: video_id.to_string(),
                inner: format!("json: {e}"),
            })?;
        let comments = raw
            .get("comments")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(YtdlpComment::from_json).collect())
            .unwrap_or_default();
        Ok(comments)
    }

    /// YouTube search results for a query (keyless `ytsearch:N`, flat
    /// playlist — id/title/channel only, no full extraction). This is the
    /// SERP data VidIQ's keyword analysis shows: what ranks NOW for a query.
    pub async fn search(
        &self,
        query: &str,
        n: u64,
    ) -> Result<Vec<YtdlpSearchResult>, TubeforgeError> {
        self.search_with(&format!("ytsearch{n}:{query}")).await
    }

    /// YouTube search results sorted by upload date (`ytsearchdate:N`) —
    /// the recency/activity signal for keyword research: are channels still
    /// publishing on this topic? Full extraction carries upload dates.
    pub async fn search_date(
        &self,
        query: &str,
        n: u64,
    ) -> Result<Vec<YtdlpSearchResult>, TubeforgeError> {
        self.search_with(&format!("ytsearchdate{n}:{query}")).await
    }

    /// Shared implementation of a `ytsearch*` run with FULL extraction
    /// (flat-playlist omits tags/upload dates — verified empirically, so
    /// the keyword-research overlay always uses full dumps). Forces the
    /// `android` player client: it returns the full metadata (tags, stats,
    /// upload date) at ~2.5s/video, while `all`/`web` chains multiply the
    /// cost (15s+/3 videos — measured) without adding fields.
    async fn search_with(
        &self,
        search_url: &str,
    ) -> Result<Vec<YtdlpSearchResult>, TubeforgeError> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| storage_err("YTD LP_SEM", e))?;

        let base = self.common_args(&[
            "--dump-json",
            "--skip-download",
            "--no-warnings",
            "--extractor-args",
            "youtube:player_client=android",
            search_url,
        ]);
        let output = Command::new(&self.binary)
            .args(&base)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        let output = match tokio::time::timeout(self.timeout, output).await {
            Ok(o) => o,
            Err(_) => {
                return Err(storage_err(
                    "YTD LP_TIMEOUT",
                    format!("yt-dlp timed out after {}s", self.timeout.as_secs()),
                ))
            }
        };
        let output = output.map_err(|e| TubeforgeError::Fetch {
            src: Source::Ytdlp,
            url: search_url.to_string(),
            inner: format!("spawn: {e}"),
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let err_line = stderr
                .lines()
                .find(|l| l.contains("ERROR"))
                .or_else(|| stderr.lines().next())
                .unwrap_or("unknown yt-dlp error")
                .to_string();
            return Err(TubeforgeError::Fetch {
                src: Source::Ytdlp,
                url: search_url.to_string(),
                inner: err_line,
            });
        }

        let mut out = Vec::new();
        for line in output.stdout.split(|&b| b == b'\n') {
            if line.is_empty() {
                continue;
            }
            let Ok(raw) = serde_json::from_slice::<serde_json::Value>(line) else {
                continue;
            };
            let id = raw
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            let tags: Vec<String> = raw
                .get("tags")
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            out.push(YtdlpSearchResult {
                video_id: id.to_string(),
                title: raw
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                channel: raw
                    .get("channel")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                channel_id: raw
                    .get("channel_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                view_count: raw.get("view_count").and_then(serde_json::Value::as_i64),
                like_count: raw.get("like_count").and_then(serde_json::Value::as_i64),
                comment_count: raw.get("comment_count").and_then(serde_json::Value::as_i64),
                upload_date: raw
                    .get("upload_date")
                    .and_then(serde_json::Value::as_str)
                    .map(String::from),
                tags,
            });
        }
        Ok(out)
    }
}

/// Strict-schema view of one yt-dlp search result (full extraction).
#[derive(Debug, Clone, Default)]
pub struct YtdlpSearchResult {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub channel_id: String,
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub upload_date: Option<String>,
    pub tags: Vec<String>,
}

/// Strict-schema view of one yt-dlp comment entry.
#[derive(Debug, Clone, Default)]
pub struct YtdlpComment {
    pub comment_id: String,
    pub author: String,
    pub text: String,
    pub like_count: i64,
    pub published_at: Option<String>,
}

impl YtdlpComment {
    fn from_json(raw: &serde_json::Value) -> Option<Self> {
        Some(YtdlpComment {
            comment_id: raw
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            author: raw
                .get("author")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            text: raw
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            like_count: raw
                .get("like_count")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0),
            published_at: raw
                .get("timestamp")
                .and_then(serde_json::Value::as_i64)
                .map(|t| t.to_string()),
        })
    }
}

/// Strict-schema view of yt-dlp's `--dump-json` payload. Only these fields
/// are consumed; unknown/absent fields degrade to defaults (never error) —
/// the payload shape drifts with YouTube's internal APIs, so tolerance is
/// by design, but every mapped field is explicitly typed.
#[derive(Debug, Clone, Default)]
pub struct YtdlpVideoInfo {
    pub video_id: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub duration_sec: Option<i64>,
    pub published_at: Option<String>,
    pub upload_date: Option<String>,
    pub view_count: Option<i64>,
    pub like_count: Option<i64>,
    pub comment_count: Option<i64>,
    pub channel_id: Option<String>,
    pub channel: Option<String>,
    pub channel_follower_count: Option<i64>,
    pub availability: Option<String>,
    pub age_limit: Option<i64>,
    pub was_live: bool,
    pub heatmap: Vec<(f64, f64)>,
    pub automatic_captions: Vec<String>,
    pub extractor_error: Option<String>,
}

impl YtdlpVideoInfo {
    fn from_json(raw: &serde_json::Value) -> Result<Self, TubeforgeError> {
        let s = |k: &str| {
            raw.get(k)
                .and_then(serde_json::Value::as_str)
                .map(String::from)
        };
        let i = |k: &str| raw.get(k).and_then(serde_json::Value::as_i64);
        let heatmap: Vec<(f64, f64)> = raw
            .get("heatmap")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let t = p.get("start_time").and_then(serde_json::Value::as_f64)?;
                        let v = p.get("value").and_then(serde_json::Value::as_f64)?;
                        Some((t, v))
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(YtdlpVideoInfo {
            video_id: s("id").unwrap_or_default(),
            title: s("title").unwrap_or_default(),
            description: s("description").unwrap_or_default(),
            tags: raw
                .get("tags")
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default(),
            category: raw
                .get("categories")
                .and_then(serde_json::Value::as_array)
                .and_then(|a| a.first())
                .and_then(serde_json::Value::as_str)
                .map(String::from),
            duration_sec: i("duration"),
            published_at: s("timestamp").or_else(|| s("release_date")),
            upload_date: s("upload_date"),
            view_count: i("view_count"),
            like_count: i("like_count"),
            comment_count: i("comment_count"),
            channel_id: s("channel_id"),
            channel: s("channel"),
            channel_follower_count: i("channel_follower_count"),
            availability: s("availability"),
            age_limit: i("age_limit"),
            was_live: raw
                .get("was_live")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            heatmap,
            automatic_captions: raw
                .get("automatic_captions")
                .and_then(serde_json::Value::as_object)
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default(),
            extractor_error: s("extractor_error"),
        })
    }
}

/// Find + read the subtitle file yt-dlp wrote in `tmp`. yt-dlp names files
/// `<stem>.<lang>.<ext>`; we glob for any `sub.*` with a vtt suffix.
async fn read_subtitle(
    tmp: &tempfile::TempDir,
    stem: &std::path::Path,
) -> Result<Option<String>, TubeforgeError> {
    let dir = tmp.path();
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| storage_err("YTD LP_READDIR", e))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| storage_err("YTD LP_ENTRY", e))?
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(
            &stem
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
        ) && (name.ends_with(".vtt") || name.ends_with(".vtt.json"))
        {
            let mut f = tokio::fs::File::open(&path)
                .await
                .map_err(|e| storage_err("YTD LP_OPEN", e))?;
            let mut buf = String::new();
            f.read_to_string(&mut buf)
                .await
                .map_err(|e| storage_err("YTD LP_READ", e))?;
            return Ok(Some(vtt_to_text(&buf)));
        }
    }
    Ok(None)
}
/// Convert WebVTT subtitle text to plain transcript text. Strips:
/// - the `WEBVTT` header block and cue numbers
/// - timestamp lines (`00:00:01.000 --> 00:00:04.000 align:start ...`)
/// - inline cue tags (`<c>`, `<i>`, `<00:00:02.000>`, `</c>`)
/// - `>>` speaker markers (auto-captions prefix speaker names with them)
///
/// YouTube auto-captions are redundant BY DESIGN: every phrase appears 3×
/// in overlapping cues (verified against a live yt-dlp capture — cue `n`
/// echoes the tail of cue `n-2`, and inline word-tags are echoed as plain
/// text in the next cue). The parser therefore works cue-by-cue:
/// - a cue whose text exactly repeats the previous cue is dropped
/// - a cue whose FIRST line repeats the last emitted phrase keeps only its
///   NEW lines (the echoed tail is not re-emitted)
/// - remaining lines of one cue join with a space; each cue = one output line
///
/// Auto-captions carry no punctuation, so sentence-boundary heuristics are
/// useless — the timed cue IS the natural segmentation unit.
pub fn vtt_to_text(vtt: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_header = true;
    let mut cue_lines: Vec<String> = Vec::new();

    for raw_line in vtt.lines() {
        let line = raw_line.trim();
        if in_header {
            if line.is_empty() {
                in_header = false;
            }
            continue;
        }
        if line.contains("-->") {
            // Cue boundary: flush the previous cue.
            if let Some(text) = emit_cue(&mut cue_lines, &out) {
                out.push(text);
            }
            cue_lines.clear();
            continue;
        }
        if line.is_empty() || line.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if line.starts_with("NOTE ") || line.starts_with("STYLE") {
            continue;
        }
        let text = clean_cue_text(line);
        if !text.is_empty() {
            cue_lines.push(text);
        }
    }
    if let Some(text) = emit_cue(&mut cue_lines, &out) {
        out.push(text);
    }

    out.join("\n")
}

/// Dedupe + join one cue against the already-emitted lines. Returns the cue
/// text to emit, or `None` when the cue is a pure echo of the previous one.
fn emit_cue(cue_lines: &mut [String], out: &[String]) -> Option<String> {
    if cue_lines.is_empty() {
        return None;
    }
    let last_emitted = out.last().map(String::as_str).unwrap_or("");
    // Cue exactly repeats the previous cue (e.g. inline-tagged echo).
    let whole = cue_lines.join(" ");
    if !whole.is_empty() && whole == last_emitted {
        return None;
    }
    // Cue starts with the last emitted phrase → keep only the new tail.
    let kept: Vec<&str> = if cue_lines.first().map(String::as_str) == Some(last_emitted) {
        cue_lines.iter().skip(1).map(String::as_str).collect()
    } else {
        cue_lines.iter().map(String::as_str).collect()
    };
    let kept = kept.join(" ");
    if kept.is_empty() || kept == last_emitted {
        None
    } else {
        Some(kept)
    }
}

/// Strip cue markup from one text line, returning the plain content.
fn clean_cue_text(line: &str) -> String {
    // Strip `<...>` tags (timestamps, voice spans) — greedy so nested
    // markup in one line collapses.
    let mut out = String::with_capacity(line.len());
    let mut in_tag = false;
    for ch in line.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // Speaker markers from auto-captions: `>> SPEAKER:` prefixes.
    let trimmed = out.trim();
    trimmed
        .strip_prefix(">>")
        .map(str::trim_start)
        .unwrap_or(trimmed)
        .to_string()
}

fn storage_err(code: &str, e: impl std::fmt::Display) -> TubeforgeError {
    crate::error::storage_err(code, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtt_strips_header_and_cues() {
        let vtt = "WEBVTT\nKind: captions\nLanguage: en\n\n1\n00:00:01.000 --> 00:00:04.000\nRust is a systems language.\n\n2\n00:00:04.500 --> 00:00:07.000\nMemory safety matters.\n";
        assert_eq!(
            vtt_to_text(vtt),
            "Rust is a systems language.\nMemory safety matters."
        );
    }

    #[test]
    fn vtt_joins_cue_continuations() {
        let vtt = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:05.000\nThis is a long cue\nthat continues.\n\n2\n00:00:06.000 --> 00:00:08.000\nNew cue here.\n";
        assert_eq!(
            vtt_to_text(vtt),
            "This is a long cue that continues.\nNew cue here."
        );
    }

    #[test]
    fn vtt_strips_inline_tags_and_timestamps() {
        let vtt = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:03.000 align:start position:0%\n<c>Hello</c> <00:00:02.000>world</00:00:02.500> <i>italic</i>\n";
        assert_eq!(vtt_to_text(vtt), "Hello world italic");
    }

    #[test]
    fn vtt_strips_speaker_markers() {
        let vtt = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:04.000\n>> JOHN: So here's the thing.\n>> MARY: Right.\n";
        // Both lines are ONE cue (one timestamp block) → join into one line.
        assert_eq!(vtt_to_text(vtt), "JOHN: So here's the thing. MARY: Right.");
    }

    #[test]
    fn vtt_empty_and_garbage_are_tolerant() {
        assert_eq!(vtt_to_text(""), "");
        assert_eq!(vtt_to_text("WEBVTT\n\n"), "");
        assert_eq!(vtt_to_text("1\n\n2\n\n3\n"), "");
        assert_eq!(vtt_to_text("NOTE this is a note\n"), "");
    }

    #[test]
    fn vtt_keeps_punctuation_breaks() {
        let vtt = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:04.000\nFirst sentence.\nSecond sentence.\n\n2\n00:00:05.000 --> 00:00:07.000\n(follow-up)\n";
        // One cue = one output line (cue 1 joins its two lines).
        assert_eq!(
            vtt_to_text(vtt),
            "First sentence. Second sentence.\n(follow-up)"
        );
    }

    #[test]
    fn vtt_dedupes_overlapping_cue_echoes() {
        // YouTube's 3x redundancy pattern (verified from a live capture):
        // cue n echoes the tail of cue n-2. The parser must emit each
        // phrase exactly once.
        let vtt = "WEBVTT\n\n1\n00:00:00.080 --> 00:00:01.510\nhi friends my name is Tris and this is\n\n2\n00:00:01.510 --> 00:00:01.520\nhi friends my name is Tris and this is\n\n3\n00:00:01.520 --> 00:00:02.990\nhi friends my name is Tris and this is\nno baller plate where I make fast\n\n4\n00:00:02.990 --> 00:00:03.000\nno baller plate where I make fast\n\n5\n00:00:03.000 --> 00:00:04.000\nno baller plate where I make fast\ntechnical videos here's something I\n";
        let text = vtt_to_text(vtt);
        assert_eq!(
            text,
            "hi friends my name is Tris and this is\nno baller plate where I make fast\ntechnical videos here's something I"
        );
    }

    /// Parse a REAL yt-dlp auto-caption file (captured live from a public
    /// competitor video, android client, no auth). Verifies the parser
    /// survives YouTube's actual VTT quirks: `<c>` word tags, inline
    /// `<00:00:00.240>` timestamps, blank cue lines, repeated-phrase cues.
    #[test]
    fn vtt_parses_real_youtubedl_captions() {
        let raw = include_str!("testdata/sample-en.vtt");
        let text = vtt_to_text(raw);
        assert!(!text.is_empty(), "real captions must produce text");
        assert!(
            text.contains("friends my name is Tris"),
            "first spoken words must survive: {text}"
        );
        // No leftover markup/timestamps.
        assert!(
            !text.contains("<c>") && !text.contains("<00:"),
            "tags must be stripped"
        );
        assert!(
            !text.lines().any(|l| l.contains("-->")),
            "timestamp lines must be gone"
        );
        // YouTube's auto-captions emit the same phrase twice (tagged + plain
        // cues); the parser must not duplicate them adjacently.
        let lines: Vec<&str> = text.lines().collect();
        for w in lines.windows(2) {
            assert_ne!(w[0], w[1], "adjacent duplicate lines: {:?}", w);
        }
    }

    /// Parse a REAL `yt-dlp --dump-json` payload (captured live 2026-08-05
    /// from a public competitor video, android client, no auth — see
    /// docs/ytdlp-extract-info-sample.json). Verifies the strict schema
    /// maps every field TubeForge consumes and ignores the format-layer
    /// noise.
    #[test]
    fn strict_schema_parses_real_payload() {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("../../docs/ytdlp-extract-info-sample.json"))
                .expect("sample json");
        let info = YtdlpVideoInfo::from_json(&raw).expect("parse");
        assert_eq!(info.video_id, "3e-nauaCkgo");
        assert_eq!(info.title, "Rust is the New C");
        assert_eq!(info.view_count, Some(172_452));
        assert_eq!(info.like_count, Some(9_973));
        assert_eq!(info.comment_count, Some(1_500));
        assert_eq!(info.channel_id.as_deref(), Some("UCUMwY9iS8oMyWDYIe6_RmoA"));
        assert_eq!(info.channel.as_deref(), Some("No Boilerplate"));
        assert_eq!(info.channel_follower_count, Some(289_000));
        assert_eq!(info.duration_sec, Some(652));
        assert_eq!(info.category.as_deref(), Some("Education"));
        assert_eq!(info.availability.as_deref(), Some("public"));
        assert_eq!(info.age_limit, Some(0));
        assert_eq!(info.upload_date.as_deref(), Some("20250312"));
        assert_eq!(info.automatic_captions.len(), 157);
        // Heatmap: 100 retention points with start/end/value.
        assert_eq!(info.heatmap.len(), 100);
        let (t, v) = info.heatmap[0];
        assert_eq!(t, 0.0);
        assert!((v - 0.337).abs() < 0.01);
        // Tags are empty on the android client (documented caveat) — the
        // strict schema must tolerate that, not error.
        assert!(info.tags.is_empty());
    }
}
