//! Channel Audit (VidIQ's flagship free feature, Phase 6.6).
//!
//! VidIQ's Channel Audit scores a channel 0-100 with breakdowns. TubeForge
//! computes the same audit from its stored corpus (RSS/API/yt-dlp data):
//! - **Upload consistency**: regularity of publishing cadence (CV of
//!   inter-upload gaps — low variance = consistent).
//! - **Metadata quality**: mean SEO score of the channel's videos (our
//!   15-component metadata audit).
//! - **Tag usage**: mean tag count per video (15-30 is the target band) +
//!   tag diversity (unique tags / videos).
//! - **Series strength**: share of videos in detected series (session-
//!   contribution signal).
//! - **Engagement**: mean (comments×3 + likes)/views across videos.
//! - **Authority**: subscribers + mean views vs the competitor set.
//!
//! Composite = weighted blend, 0-100, with a per-component breakdown and a
//! plain-language verdict.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::analytics::gaps;
use crate::analytics::performance;
use crate::error::TubeforgeError;
use crate::storage::db::{Db, VideoRow};

/// One channel audit component (0-100).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditComponent {
    pub name: String,
    pub score: f64,
    pub weight: f64,
    pub detail: String,
}

/// The full channel audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAudit {
    pub channel_id: String,
    pub channel_name: String,
    pub total_score: f64,
    pub grade: String,
    pub verdict: String,
    pub components: Vec<AuditComponent>,
}

/// Composite weights (sum 1.0): metadata is the biggest lever a creator
/// controls, consistency and engagement are the algorithmic signals.
const W_METADATA: f64 = 0.30;
const W_CONSISTENCY: f64 = 0.15;
const W_ENGAGEMENT: f64 = 0.20;
const W_TAGS: f64 = 0.15;
const W_SERIES: f64 = 0.10;
const W_AUTHORITY: f64 = 0.10;

/// Tag-count target band (vidIQ 5 min, Alan Spicer 15-30 optimal).
const TAG_TARGET_MIN: usize = 5;
const TAG_TARGET_MAX: usize = 30;
/// Series share that earns full marks.
const SERIES_FULL: f64 = 0.40;
/// Engagement ratio (×100) that earns full marks (≈4%+).
const ENGAGEMENT_FULL: f64 = 4.0;

/// Audit one channel from the stored corpus.
pub async fn audit_channel(
    db: &Db,
    videos: &[VideoRow],
    channel_id: &str,
    channel_title: &str,
) -> Result<ChannelAudit, TubeforgeError> {
    let channel_videos: Vec<&VideoRow> = videos
        .iter()
        .filter(|v| v.channel_id.as_deref() == Some(channel_id))
        .collect();

    if channel_videos.is_empty() {
        return Ok(ChannelAudit {
            channel_id: channel_id.to_string(),
            channel_name: channel_title.to_string(),
            total_score: 0.0,
            grade: "N/A".to_string(),
            verdict: "No stored videos for this channel — run `tubeforge ingest` first."
                .to_string(),
            components: Vec::new(),
        });
    }

    // --- Metadata quality: mean SEO score of stored scores (recompute-free:
    // scores table already holds the 15-component total for ingested runs).
    let seo_total = mean_seo_total(db, &channel_videos).await;
    let metadata_score = seo_total.unwrap_or(0.0);

    // --- Upload consistency: CV of inter-upload gaps (lower = steadier).
    let consistency_score = upload_consistency(&channel_videos);

    // --- Tag usage: mean tag count vs the [5,30] band + diversity.
    let (tag_score, tag_detail) = tag_usage(&channel_videos);

    // --- Series strength: share of videos in series (title markers).
    let series_score = series_share(&channel_videos);

    // --- Engagement: mean (comments×3 + likes)/views.
    let (engagement_score, engagement_detail) = engagement(&channel_videos);

    // --- Authority: subscribers (if stored) + mean views vs all channels.
    let (authority_score, authority_detail) =
        authority(db, videos, &channel_videos, channel_id).await;

    let components = vec![
        AuditComponent {
            name: "metadata".to_string(),
            score: round2(metadata_score),
            weight: W_METADATA,
            detail: format!(
                "Mean SEO score of {} stored videos (15-component metadata audit)",
                channel_videos.len()
            ),
        },
        AuditComponent {
            name: "consistency".to_string(),
            score: round2(consistency_score),
            weight: W_CONSISTENCY,
            detail: "Upload cadence steadiness (inter-upload gap variance)".to_string(),
        },
        AuditComponent {
            name: "engagement".to_string(),
            score: round2(engagement_score),
            weight: W_ENGAGEMENT,
            detail: engagement_detail,
        },
        AuditComponent {
            name: "tags".to_string(),
            score: round2(tag_score),
            weight: W_TAGS,
            detail: tag_detail,
        },
        AuditComponent {
            name: "series".to_string(),
            score: round2(series_score),
            weight: W_SERIES,
            detail: "Share of videos in detected series (session contribution)".to_string(),
        },
        AuditComponent {
            name: "authority".to_string(),
            score: round2(authority_score),
            weight: W_AUTHORITY,
            detail: authority_detail,
        },
    ];

    let total = components
        .iter()
        .map(|c| c.score * c.weight)
        .sum::<f64>()
        .clamp(0.0, 100.0);
    let grade = grade_for(total);
    let verdict = verdict_for(&grade, &components, channel_title);

    Ok(ChannelAudit {
        channel_id: channel_id.to_string(),
        channel_name: channel_title.to_string(),
        total_score: round2(total),
        grade,
        verdict,
        components,
    })
}

/// Mean SEO total from the scores table (0.0 when none stored).
async fn mean_seo_total(db: &Db, videos: &[&VideoRow]) -> Option<f64> {
    let scores = db.all_scores().await.ok()?;
    let by_id: HashMap<&str, f64> = scores
        .iter()
        .map(|s| (s.video_id.as_str(), s.seo_score))
        .collect();
    let vals: Vec<f64> = videos
        .iter()
        .filter_map(|v| by_id.get(v.video_id.as_str()).copied())
        .collect();
    if vals.is_empty() {
        None
    } else {
        Some(vals.iter().sum::<f64>() / vals.len() as f64)
    }
}

/// Consistency from inter-upload gaps: coefficient of variation, 0-100.
/// 5+ uploads needed for a stable estimate; fewer → partial credit.
fn upload_consistency(videos: &[&VideoRow]) -> f64 {
    let mut dates: Vec<i64> = videos
        .iter()
        .filter_map(|v| {
            chrono::DateTime::parse_from_rfc3339(&v.published_at)
                .ok()
                .map(|d| d.timestamp())
        })
        .collect();
    dates.sort_unstable();
    if dates.len() < 2 {
        return 50.0; // single video — neutral
    }
    let gaps: Vec<f64> = dates
        .windows(2)
        .map(|w| (w[1] - w[0]) as f64 / 86_400.0)
        .collect();
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    if mean <= 0.0 {
        return 0.0;
    }
    let variance = gaps.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / gaps.len() as f64;
    let cv = variance.sqrt() / mean;
    // CV 0 = perfectly steady → 100; CV 1+ (erratic) → ~20.
    let score = (100.0 - (cv * 80.0)).clamp(0.0, 100.0);
    // Cadence bonus: near the weekly ideal (4-10 day mean) → +10.
    let cadence_bonus = if (4.0..=10.0).contains(&mean) {
        10.0
    } else {
        0.0
    };
    (score + cadence_bonus).clamp(0.0, 100.0)
}

/// Mean tag count vs the [5,30] band + diversity across the channel.
fn tag_usage(videos: &[&VideoRow]) -> (f64, String) {
    let mut counts: Vec<usize> = Vec::new();
    let mut all_tags: std::collections::HashSet<String> = std::collections::HashSet::new();
    for v in videos {
        let tags: Vec<String> = serde_json::from_str(&v.tags).unwrap_or_default();
        counts.push(tags.len());
        all_tags.extend(tags.into_iter().map(|t| t.trim().to_lowercase()));
    }
    let mean = if counts.is_empty() {
        0.0
    } else {
        counts.iter().sum::<usize>() as f64 / counts.len() as f64
    };
    let mut score = if (TAG_TARGET_MIN..=TAG_TARGET_MAX).contains(&(mean as usize)) {
        100.0
    } else if mean >= 1.0 {
        60.0
    } else {
        0.0
    };
    let diversity = if videos.is_empty() {
        0.0
    } else {
        all_tags.len() as f64 / videos.len() as f64
    };
    if diversity >= 1.0 {
        score += 0.0; // diversity is informational
    }
    (
        score,
        format!("Avg {mean:.1} tags/video (target 5-30), {diversity:.1} unique tags per video"),
    )
}

/// Share of videos in series (title episode markers), capped at SERIES_FULL.
fn series_share(videos: &[&VideoRow]) -> f64 {
    let owned: Vec<VideoRow> = videos.iter().map(|v| (*v).clone()).collect();
    let topics = gaps::coverage(&owned);
    let series_videos: usize = topics
        .iter()
        .filter(|t| t.is_series)
        .map(|t| t.videos as usize)
        .sum();
    let share = if videos.is_empty() {
        0.0
    } else {
        series_videos as f64 / videos.len() as f64
    };
    ((share / SERIES_FULL).min(1.0)) * 100.0
}

/// Mean engagement ratio (comments×3 + likes)/views scaled to 0-100.
fn engagement(videos: &[&VideoRow]) -> (f64, String) {
    let ratios: Vec<f64> = videos
        .iter()
        .filter_map(|v| performance::engagement_ratio(v.view_count, v.like_count, v.comment_count))
        .collect();
    if ratios.is_empty() {
        return (
            0.0,
            "No engagement data (views/likes/comments absent)".to_string(),
        );
    }
    let mean_ratio = ratios.iter().sum::<f64>() / ratios.len() as f64;
    let pct = mean_ratio * 100.0;
    let score = ((pct / ENGAGEMENT_FULL).min(1.0)) * 100.0;
    (
        score,
        format!("Mean engagement {pct:.2}% of views (comments×3 + likes)"),
    )
}

/// Authority: subscribers (stored or 0) + mean views vs the corpus max.
async fn authority(
    db: &Db,
    videos: &[VideoRow],
    channel_videos: &[&VideoRow],
    channel_id: &str,
) -> (f64, String) {
    let subs = db
        .get_channel(channel_id)
        .await
        .ok()
        .flatten()
        .and_then(|c| c.subscriber_count)
        .unwrap_or(0);
    let mean_views = if channel_videos.is_empty() {
        0.0
    } else {
        let views: Vec<f64> = channel_videos
            .iter()
            .filter_map(|v| v.view_count.map(|n| n as f64))
            .collect();
        if views.is_empty() {
            0.0
        } else {
            views.iter().sum::<f64>() / views.len() as f64
        }
    };
    let corpus_max_views = videos
        .iter()
        .filter_map(|v| v.view_count.map(|n| n as f64))
        .fold(0.0f64, f64::max);
    let views_n = if corpus_max_views > 0.0 {
        (mean_views / corpus_max_views).min(1.0)
    } else {
        0.0
    };
    let subs_n = ((subs as f64) / 100_000.0).min(1.0);
    let score = (0.5 * subs_n + 0.5 * views_n) * 100.0;
    let detail = format!(
        "{} subscribers · mean {:.0} views/video (subs 50% + views-vs-corpus 50%)",
        if subs > 0 {
            subs.to_string()
        } else {
            "no data".to_string()
        },
        mean_views
    );
    (score, detail)
}

fn grade_for(score: f64) -> String {
    match score as u8 {
        85.. => "A".to_string(),
        70..=84 => "B".to_string(),
        55..=69 => "C".to_string(),
        40..=54 => "D".to_string(),
        _ => "F".to_string(),
    }
}

fn verdict_for(grade: &str, components: &[AuditComponent], name: &str) -> String {
    let weakest = components
        .iter()
        .min_by(|a, b| a.score.total_cmp(&b.score))
        .map(|c| (c.name.clone(), c.score))
        .unwrap_or(("metadata".to_string(), 0.0));
    match grade {
        "A" => format!(
            "{name} is a well-optimized channel — keep the cadence and metadata discipline."
        ),
        "B" => format!(
            "{name} is strong. Biggest lever: {} ({:.0}/100).",
            weakest.0, weakest.1
        ),
        "C" => format!(
            "{name} is average. Focus on {} ({:.0}/100) to move the needle.",
            weakest.0, weakest.1
        ),
        _ => format!(
            "{name} needs work — start with {} ({:.0}/100), then consistency and tags.",
            weakest.0, weakest.1
        ),
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Audit every stored channel (the scorecard "audit" column).
pub async fn audit_all(db: &Db) -> Result<Vec<ChannelAudit>, TubeforgeError> {
    let videos = db.all_videos().await?;
    let channels = db.all_channels().await?;
    let mut out = Vec::new();
    for c in channels {
        out.push(audit_channel(db, &videos, &c.channel_id, &c.title).await?);
    }
    out.sort_by(|a, b| b.total_score.total_cmp(&a.total_score));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(
        id: &str,
        channel: &str,
        published: &str,
        views: Option<i64>,
        likes: Option<i64>,
        comments: Option<i64>,
        tags: &str,
    ) -> VideoRow {
        VideoRow {
            video_id: id.to_string(),
            channel_id: Some(channel.to_string()),
            title: format!("Video {id}"),
            published_at: published.to_string(),
            view_count: views,
            like_count: likes,
            comment_count: comments,
            tags: tags.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn consistency_steady_wins_over_erratic() {
        // Weekly cadence → low CV → high score.
        let steady = [
            video("a", "c", "2026-01-01T00:00:00Z", None, None, None, "[]"),
            video("b", "c", "2026-01-08T00:00:00Z", None, None, None, "[]"),
            video("c", "c", "2026-01-15T00:00:00Z", None, None, None, "[]"),
            video("d", "c", "2026-01-22T00:00:00Z", None, None, None, "[]"),
            video("e", "c", "2026-01-29T00:00:00Z", None, None, None, "[]"),
        ];
        let steady_refs: Vec<&VideoRow> = steady.iter().collect();
        let s = upload_consistency(&steady_refs);
        assert!(s >= 80.0, "weekly cadence should score high, got {s}");

        // Erratic: gaps 1d, 30d, 1d, 30d → high CV → low score.
        let erratic = [
            video("a", "c", "2026-01-01T00:00:00Z", None, None, None, "[]"),
            video("b", "c", "2026-01-02T00:00:00Z", None, None, None, "[]"),
            video("c", "c", "2026-02-01T00:00:00Z", None, None, None, "[]"),
            video("d", "c", "2026-02-02T00:00:00Z", None, None, None, "[]"),
            video("e", "c", "2026-03-04T00:00:00Z", None, None, None, "[]"),
        ];
        let erratic_refs: Vec<&VideoRow> = erratic.iter().collect();
        let e = upload_consistency(&erratic_refs);
        assert!(e < s, "erratic cadence must score lower ({e} < {s})");
    }

    #[test]
    fn tag_usage_bands() {
        let good = [
            video(
                "a",
                "c",
                "2026-01-01T00:00:00Z",
                None,
                None,
                None,
                r#"["rust","async","tokio","programming","tutorial","guide","beginner","advanced","systems","dev"]"#,
            ),
            video(
                "b",
                "c",
                "2026-01-02T00:00:00Z",
                None,
                None,
                None,
                r#"["rust","async","tokio","programming","tutorial","guide","beginner","advanced","systems","dev"]"#,
            ),
        ];
        let good_refs: Vec<&VideoRow> = good.iter().collect();
        let (score, _) = tag_usage(&good_refs);
        assert_eq!(score, 100.0, "10 tags each is in the 5-30 band");

        let none = [video(
            "a",
            "c",
            "2026-01-01T00:00:00Z",
            None,
            None,
            None,
            "[]",
        )];
        let none_refs: Vec<&VideoRow> = none.iter().collect();
        let (score, _) = tag_usage(&none_refs);
        assert_eq!(score, 0.0, "no tags → 0");
    }

    #[test]
    fn engagement_scales_to_4pct() {
        // 100 views, 1 comment + 1 like → (3+1)/100 = 4% → 100.
        let v = video(
            "a",
            "c",
            "2026-01-01T00:00:00Z",
            Some(100),
            Some(1),
            Some(1),
            "[]",
        );
        let (score, _) = engagement(&[&v]);
        assert_eq!(score, 100.0);
        // 1000 views, 1 like, 0 comments → 0.1% → 2.5.
        let v2 = video(
            "b",
            "c",
            "2026-01-01T00:00:00Z",
            Some(1000),
            Some(1),
            Some(0),
            "[]",
        );
        let (score, _) = engagement(&[&v2]);
        assert!((score - 2.5).abs() < 0.01);
    }
}
