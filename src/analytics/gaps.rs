//! Competitor Gap Mining (Phase 6.5) — pure-Rust implementation of the
//! 2026 research methods. No LLM calls: TubeForge stays AI-free and the
//! agentic tools (OpenCode/Claude Code/Codex) consume these signals via the
//! CLI envelope or the prompt bundles.
//!
//! Implemented methods:
//! - **Outliers (Method A)**: videos performing ≥3x their channel's mean
//!   views — proven-demand bank; the underlying questions are the gaps.
//! - **Coverage map (Method C)**: title-token topic clusters × channel
//!   matrix; sparse cells (high-demand topic, few covering channels) are
//!   priority gaps. Demand proxy = mean views of the topic's videos.
//! - **Freshness gaps**: topics whose covering videos are old (>1y) while
//!   the topic is still actively published by at least one channel.
//! - **Format gaps**: topics with no Short (≤60s) version in the corpus —
//!   a format-slot the research flags as an easier win than new topics.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::TubeforgeError;
use crate::storage::db::VideoRow;
use crate::util;

/// Outlier multiple threshold (Method A: 3x–10x+ above channel average).
pub const OUTLIER_MULTIPLE: f64 = 3.0;
/// Max videos used per channel for the mean (tail-robust: cap at 50).
const CHANNEL_MEAN_CAP: usize = 50;
/// Freshness gap age (days) beyond which coverage is "stale".
pub const FRESHNESS_DAYS: i64 = 365;
/// Shorts duration ceiling in seconds (YouTube Shorts ≤ 60s).
pub const SHORT_SECS: i64 = 60;

/// One outlier video (Method A).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlierVideo {
    pub video_id: String,
    pub title: String,
    pub channel_id: Option<String>,
    pub channel_name: String,
    pub views: i64,
    pub channel_mean: f64,
    pub multiple: f64,
}

/// One topic cell in the coverage map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageTopic {
    pub topic: String,
    /// Videos covering this topic (title-token cluster).
    pub videos: i64,
    /// Distinct channels covering it.
    pub channels: i64,
    /// Mean views across the topic's videos (demand proxy).
    pub mean_views: f64,
    /// Freshness: newest covering video (RFC3339), empty when none.
    pub newest_at: Option<String>,
    /// True when no covering video is ≤60s (format gap — Shorts slot).
    pub no_short: bool,
    /// True when the topic's videos form a series (episode/part/number
    /// patterns in titles — Phase 6.6 session-contribution signal).
    pub is_series: bool,
    /// Computed gap score: demand × weakness (see `gap_score`).
    pub score: f64,
    /// Covering channel ids.
    pub covering_channels: Vec<String>,
}

/// The full gap report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapReport {
    pub outliers: Vec<OutlierVideo>,
    pub topics: Vec<CoverageTopic>,
    pub freshness_gaps: Vec<String>,
    pub format_gaps: Vec<String>,
}

/// Per-channel mean view counts over the corpus (cap for tail-robustness).
fn channel_means(videos: &[VideoRow]) -> HashMap<String, f64> {
    let mut per_channel: HashMap<String, Vec<i64>> = HashMap::new();
    for v in videos {
        if let (Some(cid), Some(views)) = (&v.channel_id, v.view_count) {
            let bucket = per_channel.entry(cid.clone()).or_default();
            if bucket.len() < CHANNEL_MEAN_CAP {
                bucket.push(views);
            }
        }
    }
    per_channel
        .into_iter()
        .map(|(cid, views)| {
            let mean = if views.is_empty() {
                0.0
            } else {
                views.iter().map(|&v| v as f64).sum::<f64>() / views.len() as f64
            };
            (cid, mean)
        })
        .collect()
}

/// Method A: videos at ≥3x their channel's mean views.
pub fn outliers(videos: &[VideoRow], channel_names: &HashMap<String, String>) -> Vec<OutlierVideo> {
    let means = channel_means(videos);
    let mut out: Vec<OutlierVideo> = Vec::new();
    for v in videos {
        let (Some(cid), Some(views)) = (&v.channel_id, v.view_count) else {
            continue;
        };
        let Some(&mean) = means.get(cid) else {
            continue;
        };
        if mean <= 0.0 {
            continue;
        }
        let multiple = views as f64 / mean;
        if multiple >= OUTLIER_MULTIPLE {
            out.push(OutlierVideo {
                video_id: v.video_id.clone(),
                title: v.title.clone(),
                channel_id: Some(cid.clone()),
                channel_name: channel_names.get(cid).cloned().unwrap_or_default(),
                views,
                channel_mean: mean,
                multiple: round2(multiple),
            });
        }
    }
    out.sort_by(|a, b| b.multiple.total_cmp(&a.multiple));
    out
}

/// Build a title-token topic map: every significant title token becomes a
/// topic, with its covering videos (titles containing the token).
fn build_topics(videos: &[VideoRow]) -> Vec<(String, Vec<&VideoRow>)> {
    // Stopwords: common English + niche noise that would pollute clusters.
    let stop: &[&str] = &[
        "the", "a", "an", "and", "or", "but", "in", "on", "of", "for", "to", "with", "from",
        "your", "you", "how", "why", "what", "is", "are", "it", "this", "that", "my", "we", "i",
        "at", "by", "be", "as", "has", "have", "not", "can", "get", "use", "new", "vs", "vs",
        "part", "ep", "e01", "e02", "e03", "e04", "e05", "e06", "e07", "e08", "e09", "e10",
    ];
    let mut map: HashMap<String, Vec<&VideoRow>> = HashMap::new();
    for v in videos {
        for t in util::tokens(&v.title) {
            if t.len() < 3 || stop.contains(&t.as_str()) {
                continue;
            }
            map.entry(t).or_default().push(v);
        }
    }
    let mut topics: Vec<(String, Vec<&VideoRow>)> = map.into_iter().collect();
    // Drop ultra-rare tokens (≤1 video) — not enough signal to call a gap.
    topics.retain(|(_, vs)| vs.len() > 1);
    topics.sort_by_key(|(_, vs)| std::cmp::Reverse(vs.len()));
    topics
}

/// Demand proxy for a topic: mean views of its covering videos.
fn topic_mean_views(videos: &[&VideoRow]) -> f64 {
    let views: Vec<i64> = videos.iter().filter_map(|v| v.view_count).collect();
    if views.is_empty() {
        0.0
    } else {
        views.iter().map(|&v| v as f64).sum::<f64>() / views.len() as f64
    }
}

/// Gap score = demand × weakness:
/// - demand: mean views normalized to [0,1] (100k+ views ≈ saturated).
/// - weakness: fewer covering channels = weaker supply (1 channel = 1.0,
///   5+ channels ≈ 0).
pub fn gap_score(mean_views: f64, channels: i64) -> f64 {
    let demand = (mean_views / 100_000.0).min(1.0);
    let weakness = (1.0 - (channels as f64 / 5.0)).max(0.0);
    round2(demand * weakness * 100.0)
}

/// Method C + freshness + format: the topic coverage map.
pub fn coverage(videos: &[VideoRow]) -> Vec<CoverageTopic> {
    let topics = build_topics(videos);
    let mut out: Vec<CoverageTopic> = Vec::new();
    for (topic, covering) in topics {
        let mut channel_set: std::collections::HashSet<String> = std::collections::HashSet::new();
        for v in &covering {
            if let Some(cid) = &v.channel_id {
                channel_set.insert(cid.clone());
            }
        }
        let mut channel_ids: Vec<String> = channel_set.into_iter().collect();
        channel_ids.sort();
        let mean = topic_mean_views(&covering);
        let newest = covering
            .iter()
            .map(|v| v.published_at.as_str())
            .max()
            .map(String::from);
        let has_short = covering
            .iter()
            .any(|v| v.duration_sec.map(|d| d <= SHORT_SECS).unwrap_or(false));
        let channels = channel_ids.len() as i64;
        out.push(CoverageTopic {
            topic: topic.clone(),
            videos: covering.len() as i64,
            channels,
            mean_views: round2(mean),
            newest_at: newest,
            no_short: !has_short,
            is_series: is_series_titles(&covering),
            score: gap_score(mean, channels),
            covering_channels: channel_ids,
        });
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// Topics whose newest covering video is older than `FRESHNESS_DAYS` while
/// at least one covering video is newer (topic still active somewhere).
pub fn freshness_gaps(topics: &[CoverageTopic], now: &str) -> Vec<String> {
    use chrono::{DateTime, Utc};
    let Ok(now_ts) = DateTime::parse_from_rfc3339(now).map(|d| d.with_timezone(&Utc)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for t in topics {
        let Some(newest) = t.newest_at.as_deref().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|d| d.with_timezone(&Utc))
        }) else {
            continue;
        };
        let age = now_ts.signed_duration_since(newest).num_days();
        if age > FRESHNESS_DAYS {
            out.push(t.topic.clone());
        }
    }
    out.sort();
    out
}

/// Topics with no Short (≤60s) covering video — the format-gap list.
pub fn format_gaps(topics: &[CoverageTopic]) -> Vec<String> {
    let mut out: Vec<String> = topics
        .iter()
        .filter(|t| t.no_short && t.mean_views > 0.0)
        .map(|t| t.topic.clone())
        .collect();
    out.sort();
    out
}

/// Series detection (Phase 6.6 — session-contribution signal): a topic is a
/// series when at least two covering titles carry episode/part/number
/// markers (e.g. "Part 3", "E02", "#2", "Episode 5", "Day 2").
fn is_series_titles(covering: &[&VideoRow]) -> bool {
    let markers = [
        "part", "episode", "ep ", "episode ", "e0", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8",
        "e9", "day ", "week ", "#1", "#2", "#3", "#4", "#5", "#6", "#7", "#8", "#9", "1/", "2/",
        "3/", "4/", "5/", "6/", "7/", "8/", "9/",
    ];
    let hits = covering
        .iter()
        .filter(|v| {
            let t = v.title.to_lowercase();
            markers.iter().any(|m| t.contains(m))
        })
        .count();
    hits >= 2
}

/// Assemble the full report. `channel_names`: channel_id → title map
/// (caller builds from `db.list_channels()` or the videos themselves).
pub async fn report(
    videos: &[VideoRow],
    channel_names: &HashMap<String, String>,
) -> Result<GapReport, TubeforgeError> {
    let outliers = outliers(videos, channel_names);
    let topics = coverage(videos);
    let now = util::now_rfc3339();
    let freshness_gaps = freshness_gaps(&topics, &now);
    let format_gaps = format_gaps(&topics);
    Ok(GapReport {
        outliers,
        topics,
        freshness_gaps,
        format_gaps,
    })
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(
        id: &str,
        channel: &str,
        title: &str,
        views: i64,
        published: &str,
        dur: Option<i64>,
    ) -> VideoRow {
        VideoRow {
            video_id: id.to_string(),
            channel_id: Some(channel.to_string()),
            title: title.to_string(),
            published_at: published.to_string(),
            view_count: Some(views),
            duration_sec: dur,
            ..Default::default()
        }
    }

    fn names() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("A".to_string(), "Channel A".to_string());
        m.insert("B".to_string(), "Channel B".to_string());
        m
    }

    #[test]
    fn outliers_3x_exact() {
        let vids = vec![
            video("a1", "A", "one", 1, "2026-01-01T00:00:00Z", Some(600)),
            video("a2", "A", "two", 1, "2026-01-02T00:00:00Z", Some(700)),
            video("a3", "A", "three", 1, "2026-01-03T00:00:00Z", Some(900)),
            video("a4", "A", "four", 100, "2026-01-04T00:00:00Z", Some(300)),
        ];
        // Mean = 25.75 → 100/25.75 = 3.88 ≥ 3 ✓
        let out = outliers(&vids, &names());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].video_id, "a4");
        assert_eq!(out[0].channel_name, "Channel A");
    }

    #[test]
    fn coverage_builds_topics_and_scores() {
        let vids = vec![
            video(
                "a1",
                "A",
                "Rust ownership explained simply",
                1000,
                "2026-01-01T00:00:00Z",
                Some(600),
            ),
            video(
                "a2",
                "A",
                "Rust ownership for beginners",
                2000,
                "2026-01-02T00:00:00Z",
                Some(60),
            ),
            video(
                "b1",
                "B",
                "Rust ownership in production",
                5000,
                "2026-02-01T00:00:00Z",
                Some(900),
            ),
        ];
        let topics = coverage(&vids);
        // "ownership" appears in 3 videos, 2 channels, mean 2666 views,
        // newest 2026-02-01, has a short (a2 = 60s).
        let own = topics.iter().find(|t| t.topic == "ownership").unwrap();
        assert_eq!(own.videos, 3);
        assert_eq!(own.channels, 2);
        assert!((own.mean_views - 2666.67).abs() < 1.0);
        assert!(!own.no_short);
        assert_eq!(
            own.covering_channels,
            vec!["A".to_string(), "B".to_string()]
        );
    }

    #[test]
    fn coverage_marks_no_short_topics() {
        let vids = vec![
            video(
                "a1",
                "A",
                "Zero-copy parsing deep dive",
                5000,
                "2026-01-01T00:00:00Z",
                Some(1200),
            ),
            video(
                "b1",
                "B",
                "Zero-copy parsing in tokio",
                3000,
                "2026-01-02T00:00:00Z",
                Some(800),
            ),
        ];
        let topics = coverage(&vids);
        let parsing = topics.iter().find(|t| t.topic == "parsing").unwrap();
        assert!(parsing.no_short);
        let gaps = format_gaps(&topics);
        assert!(gaps.contains(&"parsing".to_string()));
        assert_eq!(gaps.len(), 3, "copy/zero/parsing all lack a Short version");
    }

    #[test]
    fn freshness_flags_old_topics() {
        let vids = vec![
            video(
                "a1",
                "A",
                "Rust wasm compilation",
                1000,
                "2024-06-01T00:00:00Z",
                Some(900),
            ),
            video(
                "b1",
                "B",
                "Rust wasm performance",
                2000,
                "2024-07-01T00:00:00Z",
                Some(800),
            ),
        ];
        let topics = coverage(&vids);
        // newest = 2024-07-01; now = 2026-08-05 → ~765 days > 365 ✓
        let gaps = freshness_gaps(&topics, "2026-08-05T00:00:00Z");
        assert!(gaps.contains(&"wasm".to_string()));
    }

    #[test]
    fn gap_score_penalizes_saturation() {
        // 5+ channels → weakness 0 → score 0.
        assert_eq!(gap_score(50_000.0, 5), 0.0);
        // 1 channel + 50k views → demand .5 × weakness .8 = 40.
        assert_eq!(gap_score(50_000.0, 1), 40.0);
        // 200k views caps demand at 1.0 → 1.0 × .8 = 80.
        assert_eq!(gap_score(200_000.0, 1), 80.0);
    }

    #[test]
    fn series_detection_flags_episodic_titles() {
        let mk = |title: &str| VideoRow {
            video_id: title.to_string(),
            title: title.to_string(),
            ..Default::default()
        };
        let one = [&mk("Building zcoder Part 1"), &mk("Building zcoder Part 2")];
        assert!(is_series_titles(&one), "part markers → series");
        let two = [&mk("Bevy Jam Day 1"), &mk("Bevy Jam Day 2 Progress")];
        assert!(is_series_titles(&two), "day markers → series");
        let three = [&mk("Rust async explained"), &mk("Rust lifetimes deep dive")];
        assert!(!is_series_titles(&three), "no markers → not a series");
    }
}
