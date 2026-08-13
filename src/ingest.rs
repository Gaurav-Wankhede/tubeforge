//! Ingest pipeline (LLD §6): channel/video link resolution, fetch, upsert,
//! backup guard, tantivy index sync, ingest_log.
//!
//! Ordering (LLD §6.3 — Turso single-writer constraint): fetch ALL (network
//! first), THEN backup guard, THEN one transaction, THEN the index writer.
//! Source precedence (LLD §6.2): api > oembed > rss — rich data wins, never
//! downgrade.

use std::collections::{HashMap, HashSet};
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
    /// Per-item rejects (checksum-invalid ids, unsupported kinds).
    pub rejected: Vec<(String, String)>,
}

impl IngestSummary {
    pub fn ok(&self) -> bool {
        self.channels_failed == 0 && self.videos_failed == 0
    }
}

type LogRow = (String, String, Option<String>);

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
            ChannelRef::Direct(id) => targets.push(Target {
                channel_id: id,
                handle: None,
            }),
            ChannelRef::Handle(h) => {
                // @handle needs channels.list(forHandle) → API key (LLD §6.1).
                let key = cfg.youtube_api_key.as_deref().ok_or_else(|| {
                    TubeforgeError::Usage(format!(
                        "{h} requires YOUTUBE_API_KEY or a channel ID/URL"
                    ))
                })?;
                let id = ApiClient::new(clients, key).resolve_handle(&h).await?;
                targets.push(Target {
                    channel_id: id,
                    handle: Some(h),
                });
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
            targets.push(Target {
                channel_id: c.channel_id,
                handle: c.handle,
            });
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
        api: if opts.use_api {
            "ok".to_string()
        } else {
            "off".to_string()
        },
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
                alert_quota(
                    db,
                    &mut summary,
                    "videos.list quota exhausted — fell back to oEmbed",
                )
                .await;
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
                        title: ctitle.trim().to_string(),
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
                // If the handle already maps to a stored canonical channel,
                // attach the oEmbed video to that row instead of creating a
                // placeholder that would later split/duplicate the channel.
                let resolved = match handle.as_deref() {
                    Some(h) => db.get_channel_by_handle(h).await?.map(|c| c.channel_id),
                    None => None,
                };
                let channel_id = resolved.as_deref().or(handle.as_deref());
                let (mut row, chan) = video_from_oembed(&o, id, channel_id, &now);
                if let Some(mut c) = chan {
                    c.title = c.title.trim().to_string();
                    if placeholder_ids.insert(c.channel_id.clone()) {
                        channels.push(c);
                    }
                }
                row.title = row.title.trim().to_string();
                videos.push(row);
                logs.push((
                    format!("video {id}"),
                    "ok".to_string(),
                    Some("oembed".to_string()),
                ));
            }
            Err(e) => {
                summary.videos_failed += 1;
                logs.push((
                    format!("video {id}"),
                    "failed".to_string(),
                    Some(e.to_string()),
                ));
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
                    .unwrap_or_else(|| t.channel_id.clone())
                    .trim()
                    .to_string();
                let handle = t
                    .handle
                    .clone()
                    .or_else(|| existing.as_ref().and_then(|c| c.handle.clone()));
                let chan = ChannelRow {
                    channel_id: t.channel_id.clone(),
                    handle,
                    title,
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
                        let meta_changed = ex.title != chan.title || ex.handle != chan.handle;
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
                alert_quota(
                    db,
                    &mut summary,
                    "videos.list quota exhausted — keeping RSS data",
                )
                .await;
            }
            Err(e) => {
                summary.api = "error".to_string();
                tracing::warn!(err = %e, "videos.list failed — keeping RSS data");
            }
        }
    }

    // 3. Change detection + backup guard + single transaction + index.
    write_batch(cfg, db, opts, &mut summary, channels, videos, logs).await?;

    // Phase 6.6: channel growth snapshot (migration 007) — one row per
    // refreshed channel per run, with the DB-derived counts. RSS carries no
    // subscriber numbers, so backfill from the channels table (kept current by
    // the yt-dlp/API metadata enrichment) and dedupe to the latest snapshot
    // per channel per day.
    for t in targets {
        let total_views = db.channel_total_views(&t.channel_id).await?;
        let video_count = db.channel_video_count(&t.channel_id).await?;
        // Backfill subscriber count from the channels table when available.
        let subs = db
            .get_channel(&t.channel_id)
            .await?
            .and_then(|c| c.subscriber_count);
        db.upsert_channel_snapshot_daily(&t.channel_id, subs, Some(video_count), Some(total_views))
            .await?;
    }

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

/// Decide which channel rows to write and which placeholder/stale ids need to
/// be merged into a canonical channel. A real channel row always wins over an
/// `@handle` placeholder with the same handle; placeholders arriving after
/// the real row are dropped, and real rows arriving after a placeholder merge
/// on top of it.
fn plan_channel_merges(
    channels: &[ChannelRow],
    db_holders: &HashMap<String, String>,
) -> (Vec<ChannelRow>, Vec<(String, String)>) {
    let mut out: Vec<ChannelRow> = Vec::new();
    let mut repoint: Vec<(String, String)> = Vec::new();
    let mut claimed: HashMap<String, String> = HashMap::new(); // handle -> winning channel_id

    fn is_placeholder(id: &str) -> bool {
        id.starts_with('@')
    }

    for c in channels {
        let Some(h) = c.handle.as_deref() else {
            out.push(c.clone());
            continue;
        };
        let h = h.to_string();
        let holder = claimed
            .get(&h)
            .cloned()
            .or_else(|| db_holders.get(&h).cloned());

        match holder {
            None => {
                claimed.insert(h, c.channel_id.clone());
                out.push(c.clone());
            }
            Some(w) if w == c.channel_id => {
                // Channel already known (in db or batch) with same id — skip.
                claimed.insert(h, w);
            }
            Some(w) if is_placeholder(&w) => {
                // Real after placeholder: replace placeholder in output, merge data.
                repoint.push((w.clone(), c.channel_id.clone()));
                claimed.insert(h, c.channel_id.clone());
                if let Some(pos) = out.iter().position(|x| x.channel_id == w) {
                    out[pos] = c.clone();
                } else {
                    out.push(c.clone());
                }
            }
            Some(w) if is_placeholder(&c.channel_id) => {
                // Placeholder after real: point placeholder videos at the real id.
                repoint.push((c.channel_id.clone(), w));
            }
            Some(w) => {
                // Two real rows with the same handle (rare). Keep the existing
                // winner and point this batch's videos at it.
                repoint.push((c.channel_id.clone(), w));
            }
        }
    }
    (out, repoint)
}

async fn write_batch(
    cfg: &Config,
    db: &mut Db,
    opts: &IngestOptions,
    summary: &mut IngestSummary,
    channels: Vec<ChannelRow>,
    mut videos: Vec<VideoRow>,
    mut logs: Vec<LogRow>,
) -> Result<(), TubeforgeError> {
    // Resolve @handle placeholder collisions before change detection: a real
    // channel (UC… id with handle set) wins over an oEmbed placeholder, so
    // the same logical channel is never split across two rows.
    let mut db_holders: HashMap<String, String> = HashMap::new();
    for c in &channels {
        if let Some(h) = &c.handle {
            if !db_holders.contains_key(h) {
                if let Some(row) = db.get_channel_by_handle(h).await? {
                    db_holders.insert(h.clone(), row.channel_id);
                }
            }
        }
    }
    let (channels, repoints) = plan_channel_merges(&channels, &db_holders);
    for v in &mut videos {
        if let Some(cid) = &v.channel_id {
            if let Some((_, new)) = repoints.iter().find(|(old, _)| old == cid) {
                v.channel_id = Some(new.clone());
            }
        }
    }

    // Channel rows written here were counted by the caller; videos need
    // change detection (source precedence + field comparison).
    let mut to_write: Vec<VideoRow> = Vec::new();
    for row in &videos {
        match db.get_video(&row.video_id).await? {
            None => {
                summary.videos_added += 1;
                to_write.push(row.clone());
                logs.push((
                    format!("video {}", row.video_id),
                    "ok".to_string(),
                    Some("added".to_string()),
                ));
            }
            Some(existing) => {
                let in_rank = source_rank(&row.source);
                let ex_rank = source_rank(&existing.source);
                if in_rank < ex_rank {
                    summary.videos_skipped += 1;
                    logs.push((
                        format!("video {}", row.video_id),
                        "skipped".to_string(),
                        Some(format!(
                            "lower source precedence ({} < {})",
                            row.source, existing.source
                        )),
                    ));
                } else if videos_equal(row, &existing) {
                    summary.videos_skipped += 1;
                    logs.push((
                        format!("video {}", row.video_id),
                        "skipped".to_string(),
                        Some("unchanged".to_string()),
                    ));
                } else {
                    summary.videos_updated += 1;
                    to_write.push(row.clone());
                    logs.push((
                        format!("video {}", row.video_id),
                        "ok".to_string(),
                        Some("updated".to_string()),
                    ));
                }
            }
        }
    }

    let has_changes = summary.channels_added
        + summary.channels_updated
        + summary.videos_added
        + summary.videos_updated
        > 0;

    // Backup guard: before EVERY batch that will write, unless --no-backup
    // (LLD §6.3, §9.1). 304-only refreshes never reach here with changes.
    if has_changes && !opts.no_backup {
        summary.snapshot = Some(backup::backup(db, &cfg.backup_dir, cfg.backup_keep).await?);
    }

    if has_changes || !channels.is_empty() || !logs.is_empty() {
        let mut batch = db.begin_batch().await?;
        for (old, new) in &repoints {
            batch.merge_channel(old, new).await?;
        }
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
        let mut writer = index.writer(50_000_000);
        for d in &docs {
            search::index::upsert(&mut writer, &search::Schema, d)?;
        }
        writer.commit().map_err(|e| TubeforgeError::Index {
            detail: e.to_string(),
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
        title: v.title.trim().to_string(),
        description: v.description.trim().to_string(),
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

fn video_from_oembed(
    o: &oembed::OEmbed,
    id: &str,
    handle: Option<&str>,
    now: &str,
) -> (VideoRow, Option<ChannelRow>) {
    let row = VideoRow {
        video_id: id.to_string(),
        // oEmbed links have no channel_id; @handle-keyed placeholder channel
        // when the author URL carries a handle, else NULL (LLD §3.1 note).
        channel_id: handle.map(|h| h.to_string()),
        title: o
            .title
            .clone()
            .unwrap_or_else(|| id.to_string())
            .trim()
            .to_string(),
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
        title: o
            .author_name
            .clone()
            .unwrap_or_else(|| h.to_string())
            .trim()
            .to_string(),
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
        title: a.title.clone().unwrap_or_default().trim().to_string(),
        description: a.description.clone().unwrap_or_default().trim().to_string(),
        tags: serde_json::to_string(&a.tags).unwrap_or_else(|_| "[]".to_string()),
        category_id: a.category_id.clone(),
        duration_sec: a.duration_sec,
        published_at: normalize_ts(a.published_at.as_deref().unwrap_or(""))
            .unwrap_or_else(|| now.to_string()),
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
        topic_categories: serde_json::to_string(&a.topic_categories)
            .unwrap_or_else(|_| "[]".to_string()),
        // Privacy is snapshotted separately by `check availability` — the
        // API path never sets it here.
        privacy_status: None,
    }
}

/// Replace RSS/oEmbed rows with richer API rows for the same video ids
/// (api rank 3 > rss rank 1 — a later write_batch pass keeps api data).
fn merge_api_rows(videos: &mut [VideoRow], items: Vec<ApiVideo>) {
    let by_id: std::collections::HashMap<String, ApiVideo> =
        items.into_iter().map(|i| (i.video_id.clone(), i)).collect();
    let now = util::now_rfc3339();
    for row in videos.iter_mut() {
        let Some(a) = by_id.get(&row.video_id) else {
            continue;
        };
        row.source = "api".to_string();
        row.updated_at = now.clone();
        if let Some(t) = &a.title {
            if !t.is_empty() {
                row.title = t.trim().to_string();
            }
        }
        if let Some(d) = &a.description {
            if !d.is_empty() {
                row.description = d.trim().to_string();
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
    DateTime::parse_from_rfc3339(ts).ok().map(|d| {
        d.with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    })
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
    match db.insert_alert("quota", None, message, "warn").await {
        Ok(0) => {}
        Ok(_) => summary.alerts.push(message.to_string()),
        Err(e) => tracing::warn!(err = %e, "failed to write quota alert"),
    }
}

/// Record per-item rejects (checksum-invalid ids, unsupported kinds) into
/// `ingest_log` as `failed` rows so nothing is silently dropped (A1/A2
/// labeling contract, LLD §6.4). Logs-only mini batch: no data writes, so no
/// backup guard (LLD §6.3).
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

// ---------------------------------------------------------------------------
// Channel reference parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelRef {
    Direct(String),
    Handle(String),
}

#[derive(Debug, Clone)]
struct Target {
    channel_id: String,
    handle: Option<String>,
}

pub fn parse_channel_ref(s: &str) -> Result<ChannelRef, TubeforgeError> {
    // Trim @ prefix only if the whole string is a handle; URLs are parsed below.
    if let Some(rest) = s.strip_prefix('@') {
        if !rest.is_empty() && rest.chars().all(valid_handle_char) {
            return Ok(ChannelRef::Handle(format!("@{rest}")));
        }
    }
    if let Ok(url) = url::Url::parse(s) {
        let path = url.path().trim_matches('/');
        // /channel/UC... or /c/... or /@handle
        if let Some(rest) = path.strip_prefix("channel/") {
            let id = rest.split('/').next().unwrap_or(rest);
            if !id.is_empty() {
                return Ok(ChannelRef::Direct(id.to_string()));
            }
        }
        if let Some(rest) = path.strip_prefix("c/") {
            let id = rest.split('/').next().unwrap_or(rest);
            if !id.is_empty() {
                return Ok(ChannelRef::Handle(format!("@{id}")));
            }
        }
        if let Some(rest) = path.strip_prefix("show/") {
            let id = rest.split('/').next().unwrap_or(rest);
            if !id.is_empty() {
                return Ok(ChannelRef::Direct(transform_sc_to_uc(id)));
            }
        }
        if let Some(rest) = path.strip_prefix("user/") {
            let id = rest.split('/').next().unwrap_or(rest);
            if !id.is_empty() {
                return Ok(ChannelRef::Handle(format!("@{id}")));
            }
        }
        if let Some(rest) = path.strip_prefix('@') {
            if !rest.is_empty() && rest.chars().all(valid_handle_char) {
                return Ok(ChannelRef::Handle(format!("@{rest}")));
            }
        }
    }
    // Bare UC/SC/CL... channel id or @handle. SC is legacy → canonical UC.
    let transformed = transform_sc_to_uc(s);
    if transformed.starts_with("UC")
        || transformed.starts_with("CL")
        || transformed.starts_with("UU")
    {
        return Ok(ChannelRef::Direct(transformed));
    }
    Err(TubeforgeError::Usage(format!(
        "unrecognized channel reference: {s}"
    )))
}

fn valid_handle_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '.'
}

// ---------------------------------------------------------------------------
// Input parsing (used by `commands/ingest`)
// ---------------------------------------------------------------------------

/// Typed parsed item from link input (A2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputItem {
    VideoUrl(String),
    VideoBare(String),
    Playlist(String),
    ChannelUrl(String),
    ChannelBare(String),
    Handle(String),
    Custom(String),
}

/// Parse multi-line link input: strip blank lines and `#` comments, trim
/// trailing `# …` comments from each line.
pub fn parse_links_input(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|l| {
            let line = l.trim();
            // strip inline `# ...` comments
            let line = line.find('#').map(|i| &line[..i]).unwrap_or(line);
            line.trim().to_string()
        })
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Classify each line into a typed `InputItem` for the reject/ingest
/// partition (A1/A2 contract).
pub fn parse_input_items(raw: &str) -> Vec<InputItem> {
    let mut items = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Ok(url) = url::Url::parse(line) {
            let host = url.host_str().unwrap_or("");
            let is_yt = host.ends_with("youtube.com") || host == "youtu.be";
            if !is_yt {
                continue;
            }
            let path = url.path().trim_matches('/');
            if path == "playlist" || url.query_pairs().any(|(k, _)| k == "list") {
                let id = url
                    .query_pairs()
                    .find(|(k, _)| k == "list")
                    .map(|(_, v)| v.into_owned())
                    .unwrap_or_default();
                if !id.is_empty() {
                    items.push(InputItem::Playlist(id));
                }
                continue;
            }
            if let Some(rest) = path.strip_prefix("channel/") {
                let id = rest.split('/').next().unwrap_or(rest);
                if !id.is_empty() {
                    items.push(InputItem::ChannelUrl(id.to_string()));
                }
                continue;
            }
            if path.starts_with("show/") {
                let id = path.strip_prefix("show/").unwrap_or(path);
                let id = id.split('/').next().unwrap_or(id);
                let id = transform_sc_to_uc(id);
                items.push(InputItem::ChannelBare(id));
                continue;
            }
            if let Some(rest) = path.strip_prefix("c/") {
                let id = rest.split('/').next().unwrap_or(rest);
                items.push(InputItem::Custom(id.to_string()));
                continue;
            }
            if let Some(rest) = path.strip_prefix("user/") {
                let id = rest.split('/').next().unwrap_or(rest);
                items.push(InputItem::Custom(id.to_string()));
                continue;
            }
            if let Some(rest) = path.strip_prefix('@') {
                items.push(InputItem::Handle(format!("@{rest}")));
                continue;
            }
            // watch, embed, v/, video, live, shorts → extract video id
            if let Some(rest) = path.strip_prefix("shorts/") {
                let id = rest.split('/').next().unwrap_or(rest);
                items.push(InputItem::VideoUrl(id.to_string()));
                continue;
            }
            if let Some(rest) = path.strip_prefix("embed/") {
                let id = rest.split('/').next().unwrap_or(rest);
                items.push(InputItem::VideoUrl(id.to_string()));
                continue;
            }
            if let Some(rest) = path.strip_prefix("v/") {
                let id = rest.split('/').next().unwrap_or(rest);
                items.push(InputItem::VideoUrl(id.to_string()));
                continue;
            }
            if let Some(rest) = path.strip_prefix("video/") {
                let id = rest.split('/').next().unwrap_or(rest);
                items.push(InputItem::VideoUrl(id.to_string()));
                continue;
            }
            if let Some(rest) = path.strip_prefix("watch/") {
                let id = rest.split('/').next().unwrap_or(rest);
                items.push(InputItem::VideoUrl(id.to_string()));
                continue;
            }
            if let Some(rest) = path.strip_prefix("live/") {
                let id = rest.split('/').next().unwrap_or(rest);
                items.push(InputItem::VideoUrl(id.to_string()));
                continue;
            }
            if let Some((_, v)) = url.query_pairs().find(|(k, _)| k == "v") {
                items.push(InputItem::VideoUrl(v.into_owned()));
                continue;
            }
            if host == "youtu.be" {
                let id = path.split('/').next().unwrap_or(path);
                items.push(InputItem::VideoUrl(id.to_string()));
                continue;
            }
        }
        // Bare @handle
        if let Some(rest) = line.strip_prefix('@') {
            if !rest.is_empty() {
                items.push(InputItem::Handle(line.to_string()));
                continue;
            }
        }
        // Bare PL... → playlist
        if (line.starts_with("PL") || line.starts_with("UU")) && line.len() > 2 {
            items.push(InputItem::Playlist(line.to_string()));
            continue;
        }
        // Bare UC/CL channel id
        let transformed = transform_sc_to_uc(line);
        if (transformed.starts_with("UC") || transformed.starts_with("CL"))
            && transformed.len() == 24
        {
            items.push(InputItem::ChannelBare(transformed));
            continue;
        }
        // Bare 11-char video id (checksum validated later in partition_items)
        if line.len() == 11
            && line
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            items.push(InputItem::VideoBare(line.to_string()));
            continue;
        }
        // Treat unrecognized as channel ref (will be rejected downstream)
        items.push(InputItem::ChannelBare(line.to_string()));
    }
    items
}

/// Extract YouTube video ids from multi-line link/URL input. Bare 11-char
/// ids are checksum-validated; URL captures are authoritative.
pub fn extract_video_ids(input: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Strip inline `# ...` comments
        let line = line.find('#').map(|i| &line[..i]).unwrap_or(line);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(id) = extract_id_from_url(line) {
            if seen.insert(id.clone()) {
                ids.push(id);
            }
            continue;
        }
        // Regex fallback for malformed URLs / bare patterns (authoritative
        // captures — no checksum needed, like URL-parsed captures).
        if let Some(id) = extract_id_from_text(line) {
            if seen.insert(id.clone()) {
                ids.push(id);
            }
            continue;
        }
        // Bare 11-char video id
        if line.len() == 11
            && line
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && valid_video_id_checksum(line)
            && seen.insert(line.to_string())
        {
            ids.push(line.to_string());
        }
    }
    ids
}

fn extract_id_from_url(line: &str) -> Option<String> {
    let url = url::Url::parse(line).ok()?;
    let host = url.host_str()?;
    let is_yt = host.ends_with("youtube.com") || host == "youtu.be";
    if !is_yt {
        return None;
    }
    let path = url.path().trim_matches('/');
    // shorts, embed, v, video, live, watch paths
    for prefix in &["shorts", "embed", "v", "video", "watch", "live"] {
        if let Some(rest) = path.strip_prefix(*prefix) {
            let rest = rest.trim_start_matches('/');
            // Extract only the first 11 valid id chars (stop at non-id char)
            let id: String = rest
                .chars()
                .take(11)
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if id.len() == 11 {
                return Some(id);
            }
        }
    }
    // watch?v=...
    if let Some((_, v)) = url.query_pairs().find(|(k, _)| k == "v") {
        let id = v.into_owned();
        if !id.is_empty() {
            return Some(id);
        }
    }
    // youtu.be/<id> — extract first 11 valid id chars from path
    if host == "youtu.be" {
        let id: String = path
            .chars()
            .take(11)
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if id.len() == 11 {
            return Some(id);
        }
    }
    None
}

/// Regex-free fallback: scan raw text for video id patterns (handles
/// malformed URLs where url::Url::parse fails).
fn extract_id_from_text(text: &str) -> Option<String> {
    // Check v= first (more specific — youtu.be paths may contain junk).
    // Then other path-based patterns, and finally youtu.be bare paths.
    for needle in &["v=", "/v/", "/embed/", "/video/", "/shorts/", "youtu.be/"] {
        if let Some(pos) = text.find(needle) {
            let start = pos + needle.len();
            let rest = &text[start..];
            let id: String = rest
                .chars()
                .take(11)
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if id.len() == 11 {
                return Some(id);
            }
        }
    }
    None
}

/// Validate YouTube video id checksum (last char in the valid set).
pub fn valid_video_id_checksum(id: &str) -> bool {
    if id.len() != 11 {
        return false;
    }
    matches!(
        id.as_bytes()[10],
        b'A' | b'E'
            | b'I'
            | b'M'
            | b'Q'
            | b'U'
            | b'Y'
            | b'c'
            | b'g'
            | b'k'
            | b'o'
            | b's'
            | b'w'
            | b'0'
            | b'4'
            | b'8'
    )
}

/// Validate YouTube channel id checksum. Accepts both 24-char `UC...` ids
/// and 22-char bare legacy ids. The last character must be in the upper half
/// of the base64 alphabet (position >= 16).
pub fn valid_channel_id_checksum(id: &str) -> bool {
    let (id, bare) = if id.starts_with("UC") && id.len() == 24 {
        (&id[2..], true)
    } else if id.len() == 22 {
        (id, true)
    } else {
        return false;
    };
    if !bare {
        return false;
    }
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let last = id.as_bytes()[id.len() - 1];
    alphabet
        .iter()
        .position(|&b| b == last)
        .is_some_and(|pos| pos >= 16)
}

/// Transform `SC...` legacy channel id prefix to canonical `UC...`.
fn transform_sc_to_uc(id: &str) -> String {
    if id.starts_with("SC") && id.len() == 24 {
        format!("UC{}", &id[2..])
    } else {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_ref_parsing() {
        assert!(matches!(
            parse_channel_ref("UCabc").unwrap(),
            ChannelRef::Direct(_)
        ));
        assert!(
            matches!(parse_channel_ref("@rust").unwrap(), ChannelRef::Handle(h) if h == "@rust")
        );
    }

    #[test]
    fn plan_keeps_real_channel_over_placeholder() {
        let real = ChannelRow {
            channel_id: "UC123".to_string(),
            handle: Some("@rust".to_string()),
            title: "Real".to_string(),
            source: "api".to_string(),
            fetched_at: "t".to_string(),
            updated_at: "t".to_string(),
            ..Default::default()
        };
        let placeholder = ChannelRow {
            channel_id: "@rust".to_string(),
            handle: Some("@rust".to_string()),
            title: "Placeholder".to_string(),
            source: "oembed".to_string(),
            fetched_at: "t".to_string(),
            updated_at: "t".to_string(),
            ..Default::default()
        };
        // Real arriving after placeholder: merge placeholder → real.
        let (out, repoint) =
            plan_channel_merges(&[real.clone(), placeholder.clone()], &HashMap::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].channel_id, "UC123");
        assert_eq!(repoint, vec![("@rust".to_string(), "UC123".to_string())]);

        // Placeholder arriving after real: dropped, videos repointed to real.
        let db: HashMap<String, String> = [("@rust".to_string(), "UC123".to_string())].into();
        let (out, repoint) = plan_channel_merges(&[placeholder, real.clone()], &db);
        assert!(out.is_empty());
        assert_eq!(repoint, vec![("@rust".to_string(), "UC123".to_string())]);
    }

    #[test]
    fn plan_no_merge_for_distinct_handles() {
        let a = ChannelRow {
            channel_id: "UCa".to_string(),
            handle: Some("@a".to_string()),
            title: "A".to_string(),
            source: "rss".to_string(),
            fetched_at: "t".to_string(),
            updated_at: "t".to_string(),
            ..Default::default()
        };
        let b = ChannelRow {
            channel_id: "UCb".to_string(),
            handle: Some("@b".to_string()),
            title: "B".to_string(),
            source: "rss".to_string(),
            fetched_at: "t".to_string(),
            updated_at: "t".to_string(),
            ..Default::default()
        };
        let (out, repoint) = plan_channel_merges(&[a, b], &HashMap::new());
        assert_eq!(out.len(), 2);
        assert!(repoint.is_empty());
    }
}
