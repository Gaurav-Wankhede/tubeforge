//! Performance-half signals (Phase 6.6 — vidIQ's "50% performance").
//!
//! vidIQ's SEO score is 50% actionable metadata + 50% performance. This
//! module computes the performance half from data TubeForge already has or
//! captures keylessly via yt-dlp:
//! - **VPH** (views per hour): view_count / hours since publish — "trending
//!   now" when VPH exceeds 3× the channel's mean VPH (Method A outlier
//!   threshold).
//! - **Engagement ratio**: `(comments × 3 + likes × 1) / views` — reflects
//!   the 2024+ algorithm re-weighting (comments > likes, since comments
//!   take real effort).
//! - **Heatmap-derived retention**: from yt-dlp's 100-point audience
//!   retention curve — hook retention (first 10% of the curve) and the
//!   overall shape (mean value).
//!
//! All functions are deterministic over their inputs (pure); the DB layer
//! only supplies rows.

use chrono::{DateTime, Utc};

use crate::error::TubeforgeError;
use crate::storage::db::{Db, VideoRow};

/// Comments are weighted 3× over likes (2024+ algorithm weighting: comments
/// require real effort, likes are a cheap one-tap signal).
pub const COMMENT_WEIGHT: f64 = 3.0;

/// A video's performance signals (all 0-100 scaled where applicable).
#[derive(Debug, Clone, Default)]
pub struct PerformanceSignals {
    pub vph: Option<f64>,
    pub trending: bool,
    pub engagement_ratio: Option<f64>,
    /// 0-100 scaled engagement score (no data → None).
    pub engagement_score: Option<f64>,
    /// Hook retention: mean heatmap value over the first 10% of the curve
    /// (0-100). None when no heatmap is stored.
    pub hook_retention: Option<f64>,
    /// Mean retention across the whole curve (0-100).
    pub mean_retention: Option<f64>,
    /// 0-100 retention score from the heatmap shape (None without heatmap).
    pub retention_score: Option<f64>,
}

/// Views per hour for one video. `None` when view_count is absent or the
/// video is younger than a minute (division guard).
pub fn vph(view_count: Option<i64>, published_at: &str, now: DateTime<Utc>) -> Option<f64> {
    let views = view_count? as f64;
    let published = DateTime::parse_from_rfc3339(published_at)
        .ok()?
        .with_timezone(&Utc);
    let hours = now.signed_duration_since(published).num_minutes() as f64 / 60.0;
    if hours <= 0.016 {
        return None; // < ~1 minute old — no signal yet
    }
    Some(views / hours)
}

/// Engagement ratio `(comments × 3 + likes) / views` (None without data).
pub fn engagement_ratio(
    view_count: Option<i64>,
    like_count: Option<i64>,
    comment_count: Option<i64>,
) -> Option<f64> {
    let views = view_count?;
    if views <= 0 {
        return None;
    }
    let likes = like_count.unwrap_or(0) as f64;
    let comments = comment_count.unwrap_or(0) as f64;
    Some((comments * COMMENT_WEIGHT + likes) / views as f64)
}

/// 0-100 engagement score from the ratio: 1% engagement ≈ 50, 4%+ ≈ 100
/// (4% CTR is the benchmark; engagement scales similarly).
pub fn engagement_score(ratio: Option<f64>) -> Option<f64> {
    let r = ratio?;
    Some(((r * 100.0) / 4.0).min(1.0) * 100.0)
}

/// Retention metrics from a yt-dlp heatmap (array of {start_time, end_time,
/// value} where value ∈ [0,1]). Hook = first 10% of points.
pub fn retention_from_heatmap(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    if points.is_empty() {
        return None;
    }
    let mean = points.iter().map(|&(_, v)| v).sum::<f64>() / points.len() as f64;
    let hook_len = (points.len() as f64 * 0.10).ceil().max(1.0) as usize;
    let hook = points[..hook_len].iter().map(|&(_, v)| v).sum::<f64>() / hook_len as f64;
    Some((hook * 100.0, mean * 100.0))
}

/// 0-100 retention score: 50%+ retention is the benchmark (vidIQ blog), so
/// 50% retention → 50, 100% → 100; below 20% → near 0.
pub fn retention_score(mean_retention_pct: Option<f64>) -> Option<f64> {
    let m = mean_retention_pct?;
    Some((m / 100.0).clamp(0.0, 1.0) * 100.0)
}

/// Per-video performance signals.
pub fn video_signals(
    v: &VideoRow,
    heatmap: &[(f64, f64)],
    now: DateTime<Utc>,
) -> PerformanceSignals {
    let vph = vph(v.view_count, &v.published_at, now);
    let ratio = engagement_ratio(v.view_count, v.like_count, v.comment_count);
    let (hook, mean) = retention_from_heatmap(heatmap)
        .map(|(h, m)| (Some(h), Some(m)))
        .unwrap_or((None, None));
    PerformanceSignals {
        vph,
        trending: false, // set by `trending_videos` (needs channel context)
        engagement_ratio: ratio,
        engagement_score: engagement_score(ratio),
        hook_retention: hook,
        mean_retention: mean,
        retention_score: retention_score(mean),
    }
}

/// Mark `trending` on videos whose VPH > 3× the channel's mean VPH.
pub fn mark_trending(videos: &mut [VideoRow], signals: &mut [PerformanceSignals]) {
    // Channel mean VPH across all videos of that channel.
    let mut sums: std::collections::HashMap<String, (f64, usize)> =
        std::collections::HashMap::new();
    for (v, s) in videos.iter().zip(signals.iter()) {
        if let (Some(cid), Some(v)) = (&v.channel_id, s.vph) {
            let e = sums.entry(cid.clone()).or_default();
            e.0 += v;
            e.1 += 1;
        }
    }
    for (v, s) in videos.iter_mut().zip(signals.iter_mut()) {
        let Some(cid) = &v.channel_id else { continue };
        let Some(&(sum, n)) = sums.get(cid) else {
            continue;
        };
        if n == 0 {
            continue;
        }
        let mean = sum / n as f64;
        if let Some(vph) = s.vph {
            s.trending = mean > 0.0 && vph > mean * 3.0;
        }
    }
}

/// Persist the heatmap for one video (no-op when the points list is empty).
pub async fn persist_heatmap(
    db: &Db,
    video_id: &str,
    points: &[(f64, f64)],
    now: &str,
) -> Result<(), TubeforgeError> {
    if points.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_string(
        &points
            .iter()
            .map(|&(t, v)| serde_json::json!({ "start_time": t, "value": v }))
            .collect::<Vec<_>>(),
    )
    .map_err(|e| TubeforgeError::Storage {
        code: "HEATMAP_JSON".to_string(),
        message: e.to_string(),
    })?;
    db.upsert_heatmap(video_id, &json, now).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(
        id: &str,
        views: Option<i64>,
        likes: Option<i64>,
        comments: Option<i64>,
        published: &str,
    ) -> VideoRow {
        VideoRow {
            video_id: id.to_string(),
            channel_id: Some("chA".to_string()),
            view_count: views,
            like_count: likes,
            comment_count: comments,
            published_at: published.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn vph_computes_hourly_rate() {
        // 100 views over 10 hours → 10 vph.
        let now = DateTime::parse_from_rfc3339("2026-08-05T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let v = vph(Some(100), "2026-08-05T02:00:00Z", now);
        assert!((v.unwrap() - 10.0).abs() < 1e-9);
        // No view count → None.
        assert_eq!(vph(None, "2026-08-05T02:00:00Z", now), None);
        // Future-dated / broken timestamps → None.
        assert_eq!(vph(Some(100), "not-a-date", now), None);
    }

    #[test]
    fn engagement_weights_comments_over_likes() {
        // 100 views, 1 comment, 1 like → (3 + 1)/100 = 0.04 → 4% → 100.
        let r = engagement_ratio(Some(100), Some(1), Some(1)).unwrap();
        assert!((r - 0.04).abs() < 1e-9);
        assert_eq!(engagement_score(Some(r)), Some(100.0));
        // 0 views → None.
        assert_eq!(engagement_ratio(Some(0), Some(1), Some(1)), None);
        assert_eq!(engagement_ratio(None, None, None), None);
    }

    #[test]
    fn retention_from_real_shaped_heatmap() {
        // Flat 50% curve: hook = mean = 0.5.
        let pts: Vec<(f64, f64)> = (0..100).map(|i| (i as f64, 0.5)).collect();
        let (hook, mean) = retention_from_heatmap(&pts).unwrap();
        assert!((hook - 50.0).abs() < 1e-9);
        assert!((mean - 50.0).abs() < 1e-9);
        assert_eq!(retention_score(Some(mean)), Some(50.0));
        // Empty → None.
        assert_eq!(retention_from_heatmap(&[]), None);
    }

    #[test]
    fn trending_flags_3x_vph() {
        let now = DateTime::parse_from_rfc3339("2026-08-05T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // 5 baseline videos at 1 vph + 1 outlier at 10 vph → mean = 2.5,
        // 10 > 7.5 (3x) → trending. The outlier alone must not skew the mean.
        let mut videos: Vec<VideoRow> = (0..5)
            .map(|i| {
                video(
                    &format!("b{i}"),
                    Some(10),
                    None,
                    None,
                    "2026-08-05T02:00:00Z",
                )
            })
            .collect();
        videos.push(video("a3", Some(100), None, None, "2026-08-05T02:00:00Z"));
        let mut signals: Vec<PerformanceSignals> =
            videos.iter().map(|v| video_signals(v, &[], now)).collect();
        mark_trending(&mut videos, &mut signals);
        for s in &signals[..5] {
            assert!(!s.trending, "baseline must not flag");
        }
        assert!(signals[5].trending, "10x mean VPH must flag trending");
    }
}
