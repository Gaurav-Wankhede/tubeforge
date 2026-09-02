//! Pillar 1: Algorithmic Trust Score & Cold-Start Warming
//!
//! YouTube's Tier 1 classifier vets trust before distributing 1k–2k
//! impressions. This module quantifies that trust so the harness can
//! gate Tier 1 entry deterministically.
//!
//! Trust = weighted mean of four signals (0–100 each, 0–100 total):
//! - `metadata` (40%): channel description + avatar + handle + ≥3 tags on own videos
//! - `volume` (30%): 10–15 consistent assets trains the category neural vector
//! - `category_focus` (15%): ≥70% of videos share the dominant category → neural vector sharp
//! - `cadence` (15%): publish recency (≤14 days since last asset = fresh)

use serde::{Deserialize, Serialize};

use crate::storage::db::{ChannelRow, VideoRow};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    pub total: f64,
    pub metadata: f64,
    pub volume: f64,
    pub category_focus: f64,
    pub cadence: f64,
    /// Tier 1 ready when total ≥ 70 and volume ≥ 60
    pub tier1_ready: bool,
    pub reasons: Vec<String>,
}

pub fn compute(channel: Option<&ChannelRow>, videos: &[VideoRow]) -> TrustScore {
    let metadata = metadata_score(channel, videos);
    let volume = volume_score(videos.len());
    let category_focus = category_focus_score(videos);
    let cadence = cadence_score(videos);
    let total = (metadata * 0.40 + volume * 0.30 + category_focus * 0.15 + cadence * 0.15).round();
    let tier1_ready = total >= 70.0 && volume >= 60.0;
    let mut reasons = Vec::new();
    if metadata < 60.0 {
        reasons.push("Complete channel metadata (description, avatar, custom tags) to pass trust classifier".into());
    }
    if volume < 60.0 {
        reasons.push(format!(
            "Publish {} more consistent assets to reach 10–15 (trains category neural vector)",
            (10usize.saturating_sub(videos.len())).max(0)
        ));
    }
    if category_focus < 60.0 {
        reasons.push("Tighten category focus: 70%+ videos should share one category".into());
    }
    if cadence < 60.0 {
        reasons.push("Publish within 14 days — stale channels fail Tier 1".into());
    }
    if tier1_ready {
        reasons.push("Tier 1 ready — classifier will grant 1k–2k test impressions".into());
    }
    TrustScore {
        total,
        metadata,
        volume,
        category_focus,
        cadence,
        tier1_ready,
        reasons,
    }
}

fn metadata_score(channel: Option<&ChannelRow>, videos: &[VideoRow]) -> f64 {
    let mut pts = 0.0;
    if let Some(c) = channel {
        if c.description.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
            pts += 30.0;
        }
        if c.avatar_url.as_deref().map(|s| !s.is_empty()).unwrap_or(false) {
            pts += 20.0;
        }
        if c.handle.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false) {
            pts += 20.0;
        }
    }
    // Own videos with ≥3 tags → tag discipline
    if !videos.is_empty() {
        let tagged = videos
            .iter()
            .filter(|v| {
                serde_json::from_str::<Vec<String>>(&v.tags)
                    .map(|t| t.len() >= 3)
                    .unwrap_or(false)
            })
            .count();
        let ratio = tagged as f64 / videos.len() as f64;
        pts += (ratio * 30.0).min(30.0);
    }
    pts.min(100.0)
}

fn volume_score(n: usize) -> f64 {
    match n {
        0 => 0.0,
        1..=4 => 20.0 + n as f64 * 5.0,
        5..=9 => 50.0 + (n - 5) as f64 * 6.0,
        10..=15 => 80.0 + (n - 10) as f64 * 4.0,
        _ => 100.0,
    }
}

fn category_focus_score(videos: &[VideoRow]) -> f64 {
    if videos.is_empty() {
        return 0.0;
    }
    let mut counts = std::collections::HashMap::new();
    for v in videos {
        if let Some(cid) = &v.category_id {
            *counts.entry(cid.clone()).or_insert(0usize) += 1;
        }
    }
    let max = counts.values().copied().max().unwrap_or(0) as f64;
    let ratio = max / videos.len() as f64;
    if ratio >= 0.70 {
        100.0
    } else if ratio >= 0.50 {
        70.0
    } else if ratio >= 0.30 {
        40.0
    } else {
        20.0
    }
}

fn cadence_score(videos: &[VideoRow]) -> f64 {
    if videos.is_empty() {
        return 0.0;
    }
    let newest = videos.iter().map(|v| v.published_at.as_str()).max().unwrap_or("");
    let newest_ts = chrono::DateTime::parse_from_rfc3339(newest)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or(chrono::DateTime::<chrono::Utc>::UNIX_EPOCH);
    let age_days = (chrono::Utc::now() - newest_ts).num_days();
    if age_days <= 7 {
        100.0
    } else if age_days <= 14 {
        80.0
    } else if age_days <= 30 {
        50.0
    } else if age_days <= 60 {
        30.0
    } else {
        10.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::{ChannelRow, VideoRow};

    fn ch(desc: Option<&str>, avatar: bool, handle: Option<&str>) -> ChannelRow {
        ChannelRow {
            channel_id: "UC_TEST".into(),
            title: "Test".into(),
            description: desc.map(|s| s.to_string()),
            avatar_url: if avatar { Some("https://cdn/avatar.jpg".into()) } else { None },
            handle: handle.map(|s| s.to_string()),
            ..Default::default()
        }
    }
    fn vid(cat: Option<&str>, tags: &[&str], published: &str) -> VideoRow {
        VideoRow {
            video_id: format!("vid_{}", published),
            tags: serde_json::to_string(tags).unwrap(),
            category_id: cat.map(|s| s.to_string()),
            published_at: published.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn cold_start_zero_videos_scores_zero() {
        let s = compute(None, &[]);
        assert_eq!(s.volume, 0.0);
        assert!(!s.tier1_ready);
        assert!(s.total < 30.0);
    }

    #[test]
    fn ten_videos_well_tagged_is_tier1_ready() {
        let c = ch(Some("Helping devs"), true, Some("@techverse"));
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let vids: Vec<VideoRow> = (0..12).map(|_| vid(Some("28"), &["rust", "tokio", "security"], &now)).collect();
        let s = compute(Some(&c), &vids);
        assert!(s.metadata >= 80.0);
        assert!(s.volume >= 80.0);
        assert!(s.tier1_ready);
        assert!(s.total >= 70.0);
    }

    #[test]
    fn category_focus_sharp_vs_diffuse() {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let sharp: Vec<VideoRow> = (0..10).map(|_| vid(Some("28"), &["a", "b", "c"], &now)).collect();
        let diffuse: Vec<VideoRow> = (0..10)
            .map(|i| vid(Some(if i % 2 == 0 { "28" } else { "27" }), &["a", "b", "c"], &now))
            .collect();
        assert_eq!(category_focus_score(&sharp), 100.0);
        assert!(category_focus_score(&diffuse) < 100.0);
    }

    #[test]
    fn cadence_fresh_vs_stale() {
        let fresh = vec![vid(Some("28"), &["a", "b", "c"], &chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string())];
        let stale = vec![vid(Some("28"), &["a", "b", "c"], "2024-01-01T00:00:00Z")];
        assert_eq!(cadence_score(&fresh), 100.0);
        assert!(cadence_score(&stale) < 50.0);
    }
}
