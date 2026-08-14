//! Tags Analyzer — tag cloud, gap analysis, per-video tags, competitor comparison.
//!
//! Runs on top of the normalized tag tables created in migration 005:
//! `tags`, `video_tags`, `competitor_tags`.

use serde::{Deserialize, Serialize};

use crate::error::TubeforgeError;
use crate::storage::db::Db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagCloudItem {
    pub name: String,
    pub count: i64,
    pub trend: TrendDirection,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrendDirection {
    Rising,
    Stable,
    Declining,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagCloudResponse {
    pub tags: Vec<TagCloudItem>,
    pub total_unique: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagGap {
    pub tag: String,
    pub competitor_usage: i64,
    pub your_usage: i64,
    pub opportunity_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoTagsResponse {
    pub video_id: String,
    pub title: String,
    pub tags: Vec<VideoTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoTag {
    pub name: String,
    pub position: i64,
    pub source: TagSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TagSource {
    Youtube,
    Extracted,
    Suggested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitorTagsResponse {
    pub channel_id: String,
    pub channel_name: String,
    pub top_tags: Vec<CompetitorTagStat>,
    pub tag_diversity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitorTagStat {
    pub name: String,
    pub video_count: i64,
    pub avg_views: i64,
}

/// Get the full tag cloud with counts and trend (rising/stable/declining).
/// Trend is computed by comparing usage in last 30 days vs previous 30 days.
pub async fn tag_cloud(db: &Db) -> Result<TagCloudResponse, TubeforgeError> {
    let rows = db.tag_cloud().await?;
    let total_unique = rows.len() as i64;

    // For now, all trends are "stable" — can be enhanced with time-window analysis
    let tags = rows
        .into_iter()
        .map(|(name, count)| TagCloudItem {
            name,
            count,
            trend: TrendDirection::Stable,
        })
        .collect();

    Ok(TagCloudResponse { tags, total_unique })
}

/// Get competitor tag gaps — tags competitors use that we don't (or use less).
/// Returns tags with opportunity scores.
pub async fn tag_gaps(db: &Db, own_channel_id: &str) -> Result<Vec<TagGap>, TubeforgeError> {
    let gaps = db.tag_gaps(own_channel_id).await?;
    let result = gaps
        .into_iter()
        .map(|(tag, competitor_usage, your_usage)| {
            let opportunity_score = if your_usage == 0 {
                (competitor_usage as f64).min(100.0)
            } else {
                ((competitor_usage as f64 - your_usage as f64) / your_usage as f64 * 100.0)
                    .clamp(0.0, 100.0)
            };
            TagGap {
                tag,
                competitor_usage,
                your_usage,
                opportunity_score,
            }
        })
        .collect();

    Ok(result)
}

/// Get tags for a specific video, with position and source.
pub async fn video_tags(db: &Db, video_id: &str) -> Result<VideoTagsResponse, TubeforgeError> {
    // Get video title
    let video = db
        .get_video(video_id)
        .await?
        .ok_or_else(|| TubeforgeError::Storage {
            code: "VIDEO_NOT_FOUND".to_string(),
            message: format!("video {video_id} not found"),
        })?;

    // Get tags from normalized table
    let raw_tags = db.get_video_tags(video_id).await?;
    let tags: Vec<VideoTag> = raw_tags
        .into_iter()
        .map(|(name, position, source_str)| {
            let source = match source_str.as_str() {
                "youtube" => TagSource::Youtube,
                "extracted" => TagSource::Extracted,
                "suggested" => TagSource::Suggested,
                _ => TagSource::Youtube,
            };
            VideoTag {
                name,
                position,
                source,
            }
        })
        .collect();

    // If no normalized tags, fall back to JSON in videos.tags
    let tags = if tags.is_empty() {
        let json_tags: Vec<String> = serde_json::from_str(&video.tags).unwrap_or_default();
        json_tags
            .into_iter()
            .enumerate()
            .map(|(pos, name)| VideoTag {
                name,
                position: pos as i64,
                source: TagSource::Youtube,
            })
            .collect()
    } else {
        tags
    };

    Ok(VideoTagsResponse {
        video_id: video.video_id,
        title: video.title,
        tags,
    })
}

/// Get competitor tag stats for a channel.
pub async fn competitor_tags(
    db: &Db,
    channel_id: &str,
) -> Result<CompetitorTagsResponse, TubeforgeError> {
    let channel = db
        .get_channel(channel_id)
        .await?
        .ok_or_else(|| TubeforgeError::Storage {
            code: "CHANNEL_NOT_FOUND".to_string(),
            message: format!("channel {channel_id} not found"),
        })?;

    let raw_tags = db.get_competitor_tag_stats(channel_id).await?;
    let top_tags: Vec<CompetitorTagStat> = raw_tags
        .into_iter()
        .map(|(name, video_count, avg_views)| CompetitorTagStat {
            name,
            video_count,
            avg_views,
        })
        .collect();

    // Tag diversity: percentage of unique tags used out of total possible
    let diversity = if top_tags.is_empty() {
        0.0
    } else {
        let total_videos: i64 = top_tags.iter().map(|t| t.video_count).sum();
        if total_videos == 0 {
            0.0
        } else {
            (top_tags.len() as f64 / total_videos as f64 * 100.0).min(100.0)
        }
    };

    Ok(CompetitorTagsResponse {
        channel_id: channel.channel_id,
        channel_name: channel.title,
        top_tags,
        tag_diversity: diversity,
    })
}

/// Populate the normalized tag tables from existing video data.
/// Call this after migration 005 to backfill existing videos.
pub async fn backfill_tags(db: &mut Db) -> Result<usize, TubeforgeError> {
    let videos = db.all_videos().await?;
    let mut count = 0;

    for video in videos {
        let tags: Vec<String> = serde_json::from_str(&video.tags).unwrap_or_default();
        if !tags.is_empty() {
            db.upsert_tags(&video.video_id, &tags, "youtube").await?;
            count += 1;
        }
    }

    Ok(count)
}

/// Aggregate per-channel tag stats from `video_tags` into `competitor_tags`
/// (the table `/api/tags/gaps` reads). For every stored channel: each tag
/// they use gets video_count + avg_views, ranked by count. Call after
/// `backfill_tags` so the mapping tables are populated.
pub async fn analyze_competitors(db: &Db) -> Result<usize, TubeforgeError> {
    let videos = db.all_videos().await?;
    let channels = db.all_channels().await?;

    // channel_id → (tag → (count, views_sum))
    let mut per_channel: std::collections::HashMap<
        String,
        std::collections::HashMap<String, (i64, f64)>,
    > = std::collections::HashMap::new();

    for v in &videos {
        let Some(cid) = &v.channel_id else { continue };
        let tags: Vec<String> = serde_json::from_str(&v.tags).unwrap_or_default();
        let views = v.view_count.unwrap_or(0) as f64;
        let bucket = per_channel.entry(cid.clone()).or_default();
        for t in tags {
            let t = t.trim().to_lowercase();
            if t.is_empty() {
                continue;
            }
            let e = bucket.entry(t).or_insert((0, 0.0));
            e.0 += 1;
            e.1 += views;
        }
    }

    let channel_ids: std::collections::HashSet<String> =
        channels.iter().map(|c| c.channel_id.clone()).collect();
    let mut upserted = 0usize;
    for (cid, tags) in &per_channel {
        if !channel_ids.contains(cid) {
            continue;
        }
        let mut ranked: Vec<(&String, &(i64, f64))> = tags.iter().collect();
        ranked.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(b.1 .1.total_cmp(&a.1 .1)));
        for (rank, (tag, (count, views_sum))) in ranked.iter().enumerate() {
            let avg_views = if *count > 0 {
                views_sum / *count as f64
            } else {
                0.0
            };
            db.upsert_competitor_tags(cid, tag, *count, avg_views, Some(rank as i64 + 1))
                .await?;
            upserted += 1;
        }
    }

    Ok(upserted)
}
