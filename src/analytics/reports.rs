//! Reports (LLD §8.4): scorecard (channel vs competitor medians), health
//! (completeness/integrity/freshness), and alerts (rule evaluation over the
//! `alerts` table).

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::analytics::graph;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::quota::{self, DAILY_LIMIT};
use crate::search::bm25::Bm25;
use crate::search::FIELD_TITLE;
use crate::storage::db::{ChannelRow, Db, VideoRow};
use crate::util;

/// Days after which a channel counts as stale (TUBEFORGE_STALE_DAYS,
/// default 14 — baked, documented).
pub const DEFAULT_STALE_DAYS: u32 = 14;

/// Median of a sorted slice (average of the two middle elements when even).
pub fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// Per-channel category distribution rendered by display name (A3); unknown
/// category ids render raw. Deterministic: BTreeMap keys sort lexically.
pub fn category_breakdown<'a>(
    videos: impl IntoIterator<Item = &'a VideoRow>,
) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for v in videos {
        if let Some(cid) = &v.category_id {
            let name = crate::categories::category_name(cid).unwrap_or(cid);
            *m.entry(name.to_string()).or_insert(0) += 1;
        }
    }
    m
}

// ---------------------------------------------------------------------------
// scorecard (LLD §8.4)
// ---------------------------------------------------------------------------

/// Per-channel scorecard vs the median of the comparison set: views growth
/// proxy (rss views of the 3 newest vs the 3 previous), title patterns, tag
/// overlap, PageRank centrality, SEO score distribution.
pub async fn scorecard(db: &Db, only: &[String]) -> Result<Value, TubeforgeError> {
    let videos = db.all_videos().await?;
    let scores = db.all_scores().await?;
    let centrality = graph::build(db, &videos).await?;
    let all_channels = db.all_channels().await?;

    // Comparison set: explicit channel ids, else the competitors table.
    let set: Vec<ChannelRow> = if only.is_empty() {
        let ids: HashSet<String> = db.list_competitors().await?.into_iter().collect();
        all_channels
            .iter()
            .filter(|c| ids.contains(&c.channel_id))
            .cloned()
            .collect()
    } else {
        let by_id: HashMap<&str, &ChannelRow> = all_channels
            .iter()
            .map(|c| (c.channel_id.as_str(), c))
            .collect();
        let mut out = Vec::new();
        for id in only {
            match by_id.get(id.as_str()) {
                Some(c) => out.push((*c).clone()),
                None => {
                    return Err(TubeforgeError::Usage(format!(
                        "channel not in database: {id}"
                    )))
                }
            }
        }
        out
    };

    let mut tag_sets: HashMap<&str, HashSet<String>> = HashMap::new();
    for c in &set {
        let mut tokens = HashSet::new();
        for v in videos
            .iter()
            .filter(|v| v.channel_id.as_deref() == Some(&c.channel_id))
        {
            if let Ok(tags) = serde_json::from_str::<Vec<String>>(&v.tags) {
                for t in tags {
                    tokens.extend(util::tokens(&t));
                }
            }
        }
        tag_sets.insert(c.channel_id.as_str(), tokens);
    }

    let mut rows: Vec<Value> = Vec::new();
    for c in &set {
        let cid = c.channel_id.as_str();
        let mut chan_videos: Vec<&VideoRow> = videos
            .iter()
            .filter(|v| v.channel_id.as_deref() == Some(cid))
            .collect();
        chan_videos.sort_by(|a, b| b.published_at.cmp(&a.published_at));

        let total_views: i64 = chan_videos.iter().map(|v| v.view_count.unwrap_or(0)).sum();
        let recent: i64 = chan_videos
            .iter()
            .take(3)
            .map(|v| v.view_count.unwrap_or(0))
            .sum();
        let previous: i64 = chan_videos
            .iter()
            .skip(3)
            .take(3)
            .map(|v| v.view_count.unwrap_or(0))
            .sum();
        // Growth proxy: newest-3 vs next-3 view sums; 1.0 when undefined
        // (too few videos or zero baseline) — documented in the LLD report.
        let views_growth = if previous > 0 {
            recent as f64 / previous as f64
        } else {
            1.0
        };

        let n = chan_videos.len() as f64;
        let avg_title_len = if n > 0.0 {
            chan_videos
                .iter()
                .map(|v| v.title.chars().count())
                .sum::<usize>() as f64
                / n
        } else {
            0.0
        };
        let with_digits = chan_videos
            .iter()
            .filter(|v| v.title.chars().any(|c| c.is_ascii_digit()))
            .count() as f64;
        let with_howto = chan_videos
            .iter()
            .filter(|v| v.title.to_lowercase().contains("how to"))
            .count() as f64;
        let digit_ratio = if n > 0.0 { with_digits / n } else { 0.0 };
        let howto_ratio = if n > 0.0 { with_howto / n } else { 0.0 };

        // Tag overlap: mean Jaccard vs every other channel in the set.
        let mine = &tag_sets[cid];
        let mut overlaps = Vec::new();
        for other in &set {
            if other.channel_id == c.channel_id {
                continue;
            }
            let theirs = &tag_sets[other.channel_id.as_str()];
            let inter = mine.intersection(theirs).count();
            let union = mine.len() + theirs.len() - inter;
            if union > 0 {
                overlaps.push(inter as f64 / union as f64);
            }
        }
        let tag_overlap = median(&mut overlaps);

        let seo_scores: Vec<f64> = scores
            .iter()
            .filter(|s| {
                videos
                    .iter()
                    .any(|v| v.video_id == s.video_id && v.channel_id.as_deref() == Some(cid))
            })
            .map(|s| s.seo_score)
            .collect();
        let seo_avg = if seo_scores.is_empty() {
            0.0
        } else {
            seo_scores.iter().sum::<f64>() / seo_scores.len() as f64
        };
        let seo_median = median(&mut seo_scores.clone());
        let seo_min = seo_scores.iter().copied().fold(f64::INFINITY, f64::min);
        let seo_max = seo_scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        rows.push(json!({
            "channel_id": c.channel_id,
            "title": c.title,
            "videos": chan_videos.len(),
            "total_views": total_views,
            "views_growth": round2(views_growth),
            "title_patterns": {
                "avg_title_len": round2(avg_title_len),
                "digit_ratio": round2(digit_ratio),
                "howto_ratio": round2(howto_ratio),
            },
            "categories": category_breakdown(chan_videos.iter().copied()),
            "tag_overlap": round4(tag_overlap),
            "centrality": round4(centrality.get(cid).copied().unwrap_or(0.0)),
            "seo": {
                "avg": round2(seo_avg),
                "median": round2(seo_median),
                "min": round2(seo_min.min(0.0)),
                "max": round2(seo_max.max(0.0)),
                "scored": seo_scores.len(),
            },
        }));
    }

    // Median of each numeric metric across the comparison set.
    let m = |extract: &dyn Fn(&Value) -> Option<f64>| -> f64 {
        let mut vals: Vec<f64> = rows.iter().filter_map(extract).collect();
        median(&mut vals)
    };
    let mut title_lens: Vec<f64> = Vec::new();
    let mut seo_avgs: Vec<f64> = Vec::new();
    for r in &rows {
        if let Some(v) = r["title_patterns"]["avg_title_len"].as_f64() {
            title_lens.push(v);
        }
        if let Some(v) = r["seo"]["avg"].as_f64() {
            seo_avgs.push(v);
        }
    }
    let median_row = json!({
        "total_views": m(&|r| r["total_views"].as_f64()).round() as i64,
        "views_growth": round2(m(&|r| r["views_growth"].as_f64())),
        "avg_title_len": round2(median(&mut title_lens)),
        "tag_overlap": round4(m(&|r| r["tag_overlap"].as_f64())),
        "centrality": round4(m(&|r| r["centrality"].as_f64())),
        "seo_avg": round2(median(&mut seo_avgs)),
    });

    Ok(json!({
        "channels": rows,
        "median": median_row,
        "compared": set.len(),
    }))
}

// ---------------------------------------------------------------------------
// health (LLD §8.4)
// ---------------------------------------------------------------------------

/// Data completeness + quota + integrity + freshness snapshot.
pub async fn health(db: &Db, stale_days: u32) -> Result<Value, TubeforgeError> {
    let counts = json!({
        "channels": db.count("SELECT count(*) FROM channels").await?,
        "videos": db.count("SELECT count(*) FROM videos").await?,
        "scores": db.count("SELECT count(*) FROM scores").await?,
        "keywords": db.count("SELECT count(*) FROM keywords").await?,
        "keyword_rankings": db.count("SELECT count(*) FROM keyword_rankings").await?,
        "ideas": db.count("SELECT count(*) FROM ideas").await?,
        "alerts": db.count("SELECT count(*) FROM alerts").await?,
        "edges": db.count("SELECT count(*) FROM edges").await?,
        "ingest_log": db.count("SELECT count(*) FROM ingest_log").await?,
    });

    let last = db.last_ingest().await?;
    let (quota_used, quota_date) = quota::used(db).await?;

    let integrity = match db.integrity_check().await {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("FAILED: {e}"),
    };

    let stale_cutoff = Utc::now() - chrono::Duration::days(stale_days as i64);
    let mut stale: Vec<Value> = Vec::new();
    for c in db.all_channels().await? {
        // Unparsable fetched_at means freshness is unknown: fall back to epoch
        // so the channel is reported stale (needs refresh) instead of being
        // hidden by an optimistic "now". Closure keeps the clock read lazy.
        let fetched = DateTime::parse_from_rfc3339(&c.fetched_at)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| DateTime::<Utc>::UNIX_EPOCH);
        if fetched < stale_cutoff {
            stale.push(json!({
                "channel_id": c.channel_id,
                "title": c.title,
                "fetched_at": c.fetched_at,
            }));
        }
    }

    // Index freshness: last reindex must be at/after the last ingest.
    let videos = counts["videos"].as_i64().unwrap_or(0);
    let reindex_at = db.meta_get("last_reindex_at").await?;
    let index_fresh = if videos == 0 {
        true
    } else {
        match (&reindex_at, &last) {
            (Some(r), Some(l)) => r >= &l.at,
            _ => false,
        }
    };

    // A4: engagement completeness + disabled-metric census. A NULL count on
    // an API/oEmbed row = DISABLED at fetch time (deliberate uploader
    // choice) — counted, not penalized; RSS rows with NULL counts are
    // genuinely unknown (current behavior kept). Computed in Rust (no
    // recursive CTEs / window functions — Turso constraint, LLD §12).
    // The `privacy` census (migration 003 snapshots from `check
    // availability`) rides the same single pass over `videos`.
    let mut engagement = Vec::new();
    let mut disabled_videos = 0i64;
    let mut disabled_view = 0i64;
    let mut disabled_like = 0i64;
    let mut disabled_comment = 0i64;
    let mut privacy_unlisted = 0i64;
    let mut privacy_private = 0i64;
    for v in db.all_videos().await? {
        engagement.push(crate::scoring::geo::engagement_completeness(
            &v.source,
            v.view_count,
            v.like_count,
            v.comment_count,
        ));
        if v.source == "api" || v.source == "oembed" {
            if v.view_count.is_none() {
                disabled_view += 1;
            }
            if v.like_count.is_none() {
                disabled_like += 1;
            }
            if v.comment_count.is_none() {
                disabled_comment += 1;
            }
            if v.view_count.is_none() || v.like_count.is_none() || v.comment_count.is_none() {
                disabled_videos += 1;
            }
        }
        match v.privacy_status.as_deref() {
            Some("unlisted") => privacy_unlisted += 1,
            Some("private") => privacy_private += 1,
            _ => {}
        }
    }
    let engagement_complete = if engagement.is_empty() {
        0.0
    } else {
        engagement.iter().sum::<f64>() / engagement.len() as f64
    };

    Ok(json!({
        "counts": counts,
        "privacy": {
            "unlisted": privacy_unlisted,
            "private": privacy_private,
        },
        "last_ingest": last.map(|l| json!({
            "at": l.at, "batch_id": l.batch_id, "item": l.item, "status": l.status,
        })),
        "quota": {
            "videos_list_used": quota_used,
            "daily_limit": DAILY_LIMIT,
            "date": quota_date,
        },
        "integrity": integrity,
        "stale_channels": stale,
        "stale_days": stale_days,
        "index": {
            "last_reindex_at": reindex_at,
            "fresh": index_fresh,
        },
        "metadata_completeness": {
            "engagement_complete": round2(engagement_complete),
            "disabled_metrics": {
                "videos": disabled_videos,
                "view_count": disabled_view,
                "like_count": disabled_like,
                "comment_count": disabled_comment,
            },
        },
    }))
}

// ---------------------------------------------------------------------------
// alerts (LLD §8.4)
// ---------------------------------------------------------------------------

/// Evaluate the alert rules and insert any new alerts (idempotent: a rule
/// fires at most once per distinct (kind, channel_id, message)):
/// - quota exhausted / approaching (meta ledger vs daily limit + warn %)
/// - integrity failure
/// - brand keyword absent from competitor top titles
/// - stale channel
/// - new competitor detected
///
/// Returns the number of alerts inserted. `bm25` is None when the index is
/// unavailable — the brand rule is skipped in that case.
pub async fn evaluate_alerts(
    db: &Db,
    cfg: &Config,
    stale_days: u32,
    bm25: Option<&Bm25>,
) -> Result<usize, TubeforgeError> {
    let mut inserted = 0;

    // 1. Quota rule.
    let (used, _) = quota::used(db).await?;
    if used >= DAILY_LIMIT {
        inserted += insert_once(
            db,
            "quota",
            None,
            &format!("YouTube API daily quota exhausted ({used}/{DAILY_LIMIT})"),
            "critical",
        )
        .await?;
    } else {
        let pct = (used * 100) / DAILY_LIMIT;
        if pct >= cfg.quota_warn_at.min(100) {
            inserted += insert_once(
                db,
                "quota",
                None,
                &format!("YouTube API daily quota nearing limit ({pct}%)"),
                "warn",
            )
            .await?;
        }
    }

    // 2. Integrity rule.
    if let Err(e) = db.integrity_check().await {
        inserted += insert_once(
            db,
            "integrity",
            None,
            &format!("integrity_check failed: {e} — run `tubeforge backup`"),
            "critical",
        )
        .await?;
    }

    // 3. Brand rule: tracked keyword absent from competitor top titles.
    if let Some(bm25) = bm25 {
        if bm25.num_docs() > 0 {
            for kw in db.list_keywords().await? {
                if bm25.matches(FIELD_TITLE, &kw.keyword).is_empty() {
                    inserted += insert_once(
                        db,
                        "brand",
                        None,
                        &format!(
                            "brand keyword '{}' absent from competitor top titles",
                            kw.keyword
                        ),
                        "warn",
                    )
                    .await?;
                }
            }
        }
    }

    // 4. Stale channel rule.
    let stale_cutoff = Utc::now() - chrono::Duration::days(stale_days as i64);
    for c in db.all_channels().await? {
        // Same fail-safe as the health report above: corrupt timestamps are
        // unknown freshness, so they must surface as stale, not as fresh.
        let fetched = DateTime::parse_from_rfc3339(&c.fetched_at)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| DateTime::<Utc>::UNIX_EPOCH);
        if fetched < stale_cutoff {
            inserted += insert_once(
                db,
                "gap",
                Some(&c.channel_id),
                &format!("channel '{}' stale (fetched {})", c.title, c.fetched_at),
                "warn",
            )
            .await?;
        }
    }

    // 5. New competitor rule (dedupe by exact kind+channel+message).
    let channels: HashMap<String, String> = db
        .all_channels()
        .await?
        .into_iter()
        .map(|c| (c.channel_id, c.title))
        .collect();
    for id in db.list_competitors().await? {
        let label = channels.get(&id).cloned().unwrap_or_else(|| id.clone());
        inserted += insert_once(
            db,
            "gap",
            Some(&id),
            &format!("new competitor detected: {label}"),
            "info",
        )
        .await?;
    }

    Ok(inserted)
}

/// Insert an alert unless an identical (kind, channel_id, message) row
/// already exists. Public: `check availability` raises `video_unavailable`
/// alerts through the same dedupe rule (Phase 3 workstream B).
pub async fn insert_once(
    db: &Db,
    kind: &str,
    channel_id: Option<&str>,
    message: &str,
    severity: &str,
) -> Result<usize, TubeforgeError> {
    db.insert_alert(kind, channel_id, message, severity).await
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_odd_and_even() {
        assert_eq!(median(&mut [5.0, 1.0, 3.0]), 3.0);
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(median(&mut []), 0.0);
        assert_eq!(median(&mut [7.0]), 7.0);
    }

    #[test]
    fn category_breakdown_renders_names() {
        let mk = |category: Option<&str>| VideoRow {
            category_id: category.map(String::from),
            ..Default::default()
        };
        // 42 → "Shorts", 28 → "Science & Technology", unknown → raw id.
        let videos = [
            mk(Some("42")),
            mk(Some("28")),
            mk(Some("42")),
            mk(Some("999")),
            mk(None),
        ];
        let m = category_breakdown(videos.iter());
        assert_eq!(m.get("Shorts"), Some(&2));
        assert_eq!(m.get("Science & Technology"), Some(&1));
        assert_eq!(m.get("999"), Some(&1), "unknown id renders raw");
        assert_eq!(m.len(), 3, "videos without category_id are skipped");
        // Deterministic key order.
        let keys: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, vec!["999", "Science & Technology", "Shorts"]);
    }

    #[tokio::test]
    async fn health_reports_disabled_metrics() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut db = Db::open(&dir.path().join("h.db")).await.expect("open db");
        let at = "2026-08-01T00:00:00Z";
        {
            let mut batch = db.begin_batch().await.expect("batch");
            for (id, source, view, like, comment) in [
                ("aaa111bbb22", "api", Some(100), None, None),
                ("bbb222ccc33", "rss", Some(100), None, None),
                ("ccc333ddd44", "oembed", Some(5), Some(2), Some(1)),
            ] {
                batch
                    .upsert_video(&crate::storage::db::VideoRow {
                        video_id: id.to_string(),
                        title: "t".to_string(),
                        published_at: at.to_string(),
                        fetched_at: at.to_string(),
                        updated_at: at.to_string(),
                        source: source.to_string(),
                        view_count: view,
                        like_count: like,
                        comment_count: comment,
                        ..Default::default()
                    })
                    .await
                    .expect("insert video");
            }
            batch.commit().await.expect("commit");
        }

        let h = health(&db, 14).await.expect("health");
        let m = &h["metadata_completeness"];
        assert_eq!(
            m["disabled_metrics"]["videos"], 1,
            "api row with NULL like/comment"
        );
        assert_eq!(m["disabled_metrics"]["view_count"], 0);
        assert_eq!(m["disabled_metrics"]["like_count"], 1);
        assert_eq!(m["disabled_metrics"]["comment_count"], 1);
        // api (disabled like/comment) → 100; rss (unknown like/comment) → 1/3;
        // oembed full → 100. Mean = (100 + 33.33 + 100) / 3.
        let want = (100.0 + 100.0 / 3.0 + 100.0) / 3.0;
        assert!(
            (m["engagement_complete"].as_f64().unwrap() - want).abs() < 0.01,
            "got {} want {}",
            m["engagement_complete"],
            want
        );
    }
}
