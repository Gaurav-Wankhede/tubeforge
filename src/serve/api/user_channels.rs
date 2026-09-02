//! User Channels API — Dedicated End-User Channel Isolation & Competitive Benchmark.
//!
//! Provides isolated channel management and deep comparative diagnostics
//! between the user's personal channels and ingested competitor datasets.
//!
//! Strictly database-driven via the `user_channels` table (FK -> `channels.channel_id`).

use crate::serve::web::{get, post, Json, Path, Query, Router, State};
use http::StatusCode;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{api_err, AppState};
use crate::error::{Source, TubeforgeError};
use crate::fetch::innertube::next_ua_profile;
use crate::fetch::FetchClients;
use crate::storage::db_tf::{ChannelRow, Db, UserChannelRow, VideoRow};

/// Build user channels router mounted under `/api/user/channels`.
pub fn user_channels_routes() -> Router {
    Router::new()
        .route("/api/user/channels", get(list_user_channels))
        .route("/api/user/channels", post(add_user_channel))
        .route("/api/user/channels/delete", post(delete_user_channel))
        .route("/api/user/channels/{id}/analysis", get(analyze_user_channel))
        .route("/api/user/channels/{id}/refresh", post(refresh_user_channel_videos))
}

/// GET /api/user/channels — list all user channels strictly from the `user_channels` database table.
async fn list_user_channels(State(st): State<AppState>) -> Result<Json, (StatusCode, Json)> {
    let mut user_channel_rows = st.db.all_user_channels().await.unwrap_or_default();
    let all_channels = st.db.all_channels().await.unwrap_or_default();
    let all_videos = st.db.all_videos().await.unwrap_or_default();

    // If user_channels table is empty, auto-seed with configured own_channel or existing owner channel if present
    if user_channel_rows.is_empty() {
        if let Some(ref own) = st.own_channel {
            let now = crate::util::now_rfc3339();
            let row = UserChannelRow {
                channel_id: own.clone(),
                custom_name: None,
                is_primary: true,
                notes: None,
                created_at: now.clone(),
                updated_at: now,
            };
            let _ = st.db.put_user_channel(row).await;
            user_channel_rows = st.db.all_user_channels().await.unwrap_or_default();
        }
    }

    let mut user_channels = Vec::new();

    for uc in &user_channel_rows {
        let chan = all_channels.iter().find(|c| c.channel_id == uc.channel_id);
        let chan_videos: Vec<&VideoRow> = all_videos
            .iter()
            .filter(|v| v.channel_id.as_deref() == Some(&uc.channel_id))
            .collect();

        let video_count = chan_videos.len();
        let total_views: i64 = chan_videos
            .iter()
            .map(|v| v.view_count.unwrap_or(0))
            .sum();
        let avg_views = if video_count > 0 {
            total_views / video_count as i64
        } else {
            0
        };

        let title = chan
            .map(|c| c.title.clone())
            .unwrap_or_else(|| uc.custom_name.clone().unwrap_or_else(|| format!("Channel {}", uc.channel_id)));
        let handle = chan.and_then(|c| c.handle.clone());
        let subscriber_count = chan.and_then(|c| c.subscriber_count).unwrap_or(0);
        let avatar_url = chan.and_then(|c| c.avatar_url.clone());
        let description = chan.and_then(|c| c.description.clone()).unwrap_or_default();

        user_channels.push(json!({
            "channel_id": uc.channel_id,
            "title": title,
            "handle": handle,
            "description": description,
            "subscriber_count": subscriber_count,
            "video_count": video_count,
            "total_views": total_views,
            "avg_views": avg_views,
            "avatar_url": avatar_url,
            "is_primary": uc.is_primary,
            "created_at": uc.created_at,
        }));
    }

    Ok(Json(json!({
        "ok": true,
        "channels": user_channels,
    })))
}

/// POST /api/user/channels?input=... — Add and ingest a user's channel into SQLite channels & user_channels tables.
async fn add_user_channel(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let raw_input = params
        .get("input")
        .or_else(|| params.get("channel"))
        .or_else(|| params.get("q"))
        .map(String::as_str)
        .unwrap_or_default()
        .trim();

    let custom_name = params.get("custom_name").cloned().filter(|s| !s.trim().is_empty());

    if raw_input.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Channel URL, handle or ID is required via 'input' parameter"})),
        ));
    }

    let clients = crate::fetch::FetchClients::new().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("HTTP client init failed: {e}")})),
        )
    })?;

    let channel_info = resolve_channel_details(&clients, raw_input).await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Could not resolve YouTube channel: {e}")})),
        )
    })?;

    let now = crate::util::now_rfc3339();
    let chan_row = ChannelRow {
        channel_id: channel_info.channel_id.clone(),
        handle: channel_info.handle.clone(),
        title: channel_info.title.clone(),
        description: channel_info.description.clone(),
        avatar_url: channel_info.avatar_url.clone(),
        country: None,
        subscriber_count: channel_info.subscriber_count,
        video_count: None,
        source: "user_added".to_string(),
        etag: None,
        fetched_at: now.clone(),
        updated_at: now.clone(),
    };

    // 1. Upsert into channels table
    let _ = st.db.put_channel(&chan_row).await;

    // 2. Insert into user_channels table (PK/FK relationship)
    let user_chan_row = UserChannelRow {
        channel_id: channel_info.channel_id.clone(),
        custom_name,
        is_primary: false,
        notes: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let _ = st.db.put_user_channel(user_chan_row).await;

    // 3. Ingest RSS videos for this channel into videos table
    let ingested_videos = ingest_channel_rss_feed(&clients, &st.db, &channel_info.channel_id).await.unwrap_or(0);

    // 4. Trigger background sync for the new channel's videos
    let db_clone = st.db.clone();
    let cid = channel_info.channel_id.clone();
    tokio::spawn(async move {
        if let Ok(clients) = crate::fetch::FetchClients::new() {
            let vids = db_clone.all_videos().await.unwrap_or_default();
            for v in vids.into_iter().filter(|v| v.channel_id.as_deref() == Some(&cid)) {
                if let Ok(meta) = crate::fetch::innertube::fetch_video_meta(&clients, &v.video_id).await {
                    let now_str = crate::util::now_rfc3339();
                    let tags_json = serde_json::to_string(&meta.tags).ok();
                    let _ = db_clone.upsert_tags(&v.video_id, &meta.tags, "youtube").await;
                    let _ = db_clone.update_video_full_metadata(
                        &v.video_id,
                        if meta.title.is_empty() { None } else { Some(&meta.title) },
                        if meta.description.is_empty() { None } else { Some(&meta.description) },
                        meta.duration_seconds,
                        meta.view_count,
                        meta.like_count,
                        meta.comment_count,
                        meta.published_at.as_deref(),
                        tags_json.as_deref(),
                        meta.thumb_url.as_deref(),
                        &now_str,
                    ).await;
                }
            }
        }
    });

    Ok(Json(json!({
        "ok": true,
        "message": format!("Successfully added {} and ingested {} videos into database", channel_info.title, ingested_videos),
        "channel": {
            "channel_id": channel_info.channel_id,
            "title": channel_info.title,
            "handle": channel_info.handle,
            "subscriber_count": channel_info.subscriber_count,
            "avatar_url": channel_info.avatar_url,
            "ingested_videos": ingested_videos,
        }
    })))
}

/// POST /api/user/channels/delete?channel_id=... — remove channel from user_channels table.
async fn delete_user_channel(
    State(st): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json, (StatusCode, Json)> {
    let id = params
        .get("channel_id")
        .or_else(|| params.get("id"))
        .map(String::as_str)
        .unwrap_or_default()
        .trim();

    if id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "channel_id parameter is required"})),
        ));
    }

    st.db.delete_user_channel(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to remove user channel: {e}")})),
        )
    })?;

    Ok(Json(json!({
        "ok": true,
        "message": format!("Removed channel {id} from your personal studio"),
    })))
}

/// GET /api/user/channels/{id}/analysis — Deep isolated analysis & competitor comparison.
async fn analyze_user_channel(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let all_channels = st.db.all_channels().await.unwrap_or_default();
    let all_videos = st.db.all_videos().await.unwrap_or_default();
    let user_channel_rows = st.db.all_user_channels().await.unwrap_or_default();

    let channel = all_channels.iter().find(|c| c.channel_id == id);
    let channel_title = channel
        .map(|c| c.title.clone())
        .unwrap_or_else(|| format!("Channel {id}"));
    let channel_handle = channel.and_then(|c| c.handle.clone());
    let subscriber_count = channel.and_then(|c| c.subscriber_count).unwrap_or(0);
    let avatar_url = channel.and_then(|c| c.avatar_url.clone());

    let user_channel_ids_set: HashSet<String> = user_channel_rows.iter().map(|uc| uc.channel_id.clone()).collect();

    // Separate user videos from competitor videos
    let mut user_videos = Vec::new();
    let mut competitor_videos = Vec::new();

    for v in &all_videos {
        if v.channel_id.as_deref() == Some(&id) {
            user_videos.push(v);
        } else if let Some(ref cid) = v.channel_id {
            if !user_channel_ids_set.contains(cid) {
                competitor_videos.push(v);
            }
        }
    }

    let user_video_count = user_videos.len();
    let user_total_views: i64 = user_videos
        .iter()
        .map(|v| v.view_count.unwrap_or(0))
        .sum();
    let user_avg_views = if user_video_count > 0 {
        user_total_views / user_video_count as i64
    } else {
        0
    };

    let user_durations: Vec<i64> = user_videos
        .iter()
        .filter_map(|v| v.duration_sec)
        .filter(|&d| d > 0)
        .collect();
    let user_avg_duration_sec = if !user_durations.is_empty() {
        user_durations.iter().sum::<i64>() / user_durations.len() as i64
    } else {
        0
    };

    // Competitor metrics
    let comp_video_count = competitor_videos.len();
    let comp_total_views: i64 = competitor_videos
        .iter()
        .map(|v| v.view_count.unwrap_or(0))
        .sum();
    let comp_avg_views = if comp_video_count > 0 {
        comp_total_views / comp_video_count as i64
    } else {
        0
    };

    let comp_durations: Vec<i64> = competitor_videos
        .iter()
        .filter_map(|v| v.duration_sec)
        .filter(|&d| d > 0)
        .collect();
    let comp_avg_duration_sec = if !comp_durations.is_empty() {
        comp_durations.iter().sum::<i64>() / comp_durations.len() as i64
    } else {
        0
    };

    // User tags collection
    let mut user_tags = HashSet::new();
    for v in &user_videos {
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&v.tags) {
            for t in parsed {
                user_tags.insert(t.to_lowercase());
            }
        }
    }

    // Competitor tag frequency map
    let mut comp_tag_counts: HashMap<String, usize> = HashMap::new();
    for v in &competitor_videos {
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&v.tags) {
            for t in parsed {
                let clean = t.trim().to_string();
                if !clean.is_empty() && clean.len() > 2 {
                    *comp_tag_counts.entry(clean).or_insert(0) += 1;
                }
            }
        }
    }

    // Identify top competitor tags missing from user channel
    let mut missing_tags: Vec<(String, usize)> = comp_tag_counts
        .into_iter()
        .filter(|(tag, _)| !user_tags.contains(&tag.to_lowercase()))
        .collect();
    missing_tags.sort_by(|a, b| b.1.cmp(&a.1));
    let top_missing_tags: Vec<Value> = missing_tags
        .into_iter()
        .take(15)
        .map(|(tag, count)| json!({ "tag": tag, "competitor_occurrences": count }))
        .collect();

    // Analyze Title Formatting & Colon Rule
    let mut titles_with_colons = 0;
    let mut total_title_len = 0;
    for v in &user_videos {
        total_title_len += v.title.len();
        if v.title.contains(':') {
            titles_with_colons += 1;
        }
    }
    let user_avg_title_len = if user_video_count > 0 {
        total_title_len / user_video_count
    } else {
        0
    };

    // Generate Prescriptive Actionable Improvements
    let mut improvements = Vec::new();

    // 1. Title formatting & Colon Law
    if titles_with_colons > 0 {
        improvements.push(json!({
            "id": "title_colons",
            "priority": "HIGH",
            "category": "Packaging & CTR",
            "title": format!("Remove Colons from {} Video Titles", titles_with_colons),
            "description": "Colons break YouTube mobile search cards and hurt initial 45-character curiosity hooks. Reformat with parenthetical hooks (e.g. 'How Linux Runs Code (Inside Syscalls & Memory Isolation)').",
            "action_type": "kanban",
            "action_label": "Format Titles with Parentheses",
        }));
    }

    // 2. Video Duration Benchmark
    if user_avg_duration_sec < 480 && comp_avg_duration_sec >= 480 {
        improvements.push(json!({
            "id": "duration_gap",
            "priority": "HIGH",
            "category": "Watch Time & Retention",
            "title": "Increase Core Video Duration to 8–14 Minutes",
            "description": format!("Your average duration is {}s vs top competitor average of {}s. YouTube algorithms heavily favor 8+ minute architectural deep-dives for long-session watch time retention.", user_avg_duration_sec, comp_avg_duration_sec),
            "action_type": "teleprompter",
            "action_label": "Draft Deep-Dive Script in Teleprompter",
        }));
    }

    // 3. Tag Coverage Gaps
    if !top_missing_tags.is_empty() {
        improvements.push(json!({
            "id": "tag_gaps",
            "priority": "MEDIUM",
            "category": "SEO & Search Radar",
            "title": "Ingest Missing High-Frequency Competitor Tags",
            "description": "Top competitors in your niche rank across high-volume keyword tags that your videos currently lack. Add these semantic tags to boost SERP discovery.",
            "action_type": "tags",
            "action_label": "Apply Top Missing Tags",
        }));
    }

    // 4. Mobile Hook Viewport (First 45 chars)
    if user_avg_title_len > 70 {
        improvements.push(json!({
            "id": "mobile_title_len",
            "priority": "QUICK_WIN",
            "category": "Mobile Viewport",
            "title": "Front-Load Curiosity in First 45 Characters",
            "description": "Over 70% of YouTube impressions occur on mobile feeds where titles truncate after 45–50 characters. Place the primary question or curiosity gap at the beginning.",
            "action_type": "research",
            "action_label": "Inspect Topic SERPs",
        }));
    }

    // Calculate channel median views for statistical outlier detection
    let mut view_counts: Vec<i64> = user_videos.iter().map(|v| v.view_count.unwrap_or(0)).collect();
    view_counts.sort_unstable();
    let median_views = if view_counts.is_empty() {
        0.0
    } else if view_counts.len() % 2 == 1 {
        view_counts[view_counts.len() / 2] as f64
    } else {
        let mid = view_counts.len() / 2;
        (view_counts[mid - 1] + view_counts[mid]) as f64 / 2.0
    };
    let safe_median = median_views.max(1.0);

    // Format user videos payload with full micro-metadata, HD thumbnails, and Mathematical EDA metrics
    let user_videos_payload: Vec<Value> = user_videos
        .iter()
        .map(|v| {
            let tags_arr: Vec<String> = serde_json::from_str(&v.tags).unwrap_or_default();
            let thumb = v.thumb_url.clone().unwrap_or_else(|| {
                format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", v.video_id)
            });
            let views = v.view_count.unwrap_or(0);
            let likes = v.like_count.unwrap_or(0);
            let comments = v.comment_count.unwrap_or(0);
            let duration_sec = v.duration_sec.unwrap_or(0);

            // Mathematical EDA Metrics:
            // 1. Outlier Multiplier R_outlier = Views / Median_Views
            let outlier_multiplier = ((views as f64) / safe_median * 100.0).round() / 100.0;
            let outlier_label = if outlier_multiplier >= 3.0 {
                "Breakout Outlier"
            } else if outlier_multiplier >= 1.5 {
                "High Resonance"
            } else if outlier_multiplier >= 0.7 {
                "Baseline"
            } else {
                "Underperforming"
            };

            // 2. Log-Normalized Engagement Density E_norm = (Likes + 2.5 * Comments) / log10(max(Views, 10))
            let log_views = ((views.max(10) as f64).log10()).max(1.0);
            let engagement_density = (((likes as f64 + 2.5 * comments as f64) / log_views) * 10.0).round() / 10.0;

            // 3. Expected Watch Time E[T] in hours = (Views * Duration * 0.45 retention) / 3600
            let expected_watch_hours = (((views as f64 * (duration_sec as f64 * 0.45)) / 3600.0) * 10.0).round() / 10.0;

            // 4. Mobile Viewport 45-character check
            let char_count = v.title.chars().count();
            let is_mobile_safe = char_count <= 55 && !v.title.contains(':') && !v.title.contains('|');
            let mobile_preview = if char_count <= 45 {
                v.title.clone()
            } else {
                format!("{}...", v.title.chars().take(42).collect::<String>().trim_end())
            };

            json!({
                "video_id": v.video_id,
                "title": v.title,
                "description": v.description,
                "thumb_url": thumb,
                "view_count": views,
                "duration_sec": duration_sec,
                "like_count": likes,
                "comment_count": comments,
                "published_at": v.published_at,
                "tags": tags_arr,
                "updated_at": v.updated_at,
                "eda": {
                    "outlier_multiplier": outlier_multiplier,
                    "outlier_label": outlier_label,
                    "engagement_density": engagement_density,
                    "expected_watch_hours": expected_watch_hours,
                    "is_mobile_safe": is_mobile_safe,
                    "mobile_preview": mobile_preview,
                    "char_count": char_count,
                    "has_colon": v.title.contains(':'),
                }
            })
        })
        .collect();

    let user_chan_row = user_channel_rows.iter().find(|uc| uc.channel_id == id);
    let channel_desc = channel.and_then(|c| c.description.clone()).unwrap_or_default();

    Ok(Json(json!({
        "ok": true,
        "channel": {
            "channel_id": id,
            "title": channel_title,
            "handle": channel_handle,
            "description": channel_desc,
            "custom_name": user_chan_row.and_then(|uc| uc.custom_name.clone()),
            "is_primary": user_chan_row.map(|uc| uc.is_primary).unwrap_or(false),
            "subscriber_count": subscriber_count,
            "avatar_url": avatar_url,
            "video_count": user_video_count,
            "total_views": user_total_views,
            "avg_views": user_avg_views,
            "median_views": (median_views * 10.0).round() / 10.0,
            "avg_duration_sec": user_avg_duration_sec,
            "avg_title_length": user_avg_title_len,
            "titles_with_colons": titles_with_colons,
        },
        "competitor_benchmark": {
            "channel_count": all_channels.len().saturating_sub(user_channel_rows.len()),
            "video_count": comp_video_count,
            "avg_views": comp_avg_views,
            "avg_duration_sec": comp_avg_duration_sec,
        },
        "missing_tags": top_missing_tags,
        "improvements": improvements,
        "videos": user_videos_payload,
    })))
}

/// POST /api/user/channels/{id}/refresh — Trigger instant live sync for this channel's videos.
async fn refresh_user_channel_videos(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json, (StatusCode, Json)> {
    let clients = Arc::new(FetchClients::new().map_err(api_err)?);
    let all_videos = st.db.all_videos().await.unwrap_or_default();
    let target_videos: Vec<VideoRow> = all_videos
        .into_iter()
        .filter(|v| v.channel_id.as_deref() == Some(&id))
        .collect();

    let count = target_videos.len();
    let now = crate::util::now_rfc3339();
    let db = st.db.clone();
    let bin = if std::path::Path::new("/opt/homebrew/bin/yt-dlp").exists() {
        std::path::PathBuf::from("/opt/homebrew/bin/yt-dlp")
    } else if std::path::Path::new("/usr/local/bin/yt-dlp").exists() {
        std::path::PathBuf::from("/usr/local/bin/yt-dlp")
    } else {
        std::path::PathBuf::from("yt-dlp")
    };
    let ytdlp = st.ytdlp.clone().or_else(|| {
        crate::fetch::ytdlp::YtdlpClient::new(bin, true, None, None).ok()
    });

    let chan_id_clone = id.clone();
    tokio::spawn(async move {
        // Refresh channel-level metadata (subscribers, avatar, description)
        if let Ok(res) = resolve_channel_details(&clients, &chan_id_clone).await {
            if let Some(subs) = res.subscriber_count {
                let _ = db.update_channel_subscribers(&chan_id_clone, subs, &now).await;
            }
        }

        for v in target_videos {
            let vid = v.video_id;
            let mut got_full_meta = false;

            match crate::fetch::innertube::fetch_video_meta(&clients, &vid).await {
                Ok(info) => {
                    tracing::info!("Synced meta for {vid}: views={:?}, dur={:?}, tags={}", info.view_count, info.duration_seconds, info.tags.len());
                    let clean_title = info.title.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'");
                    let patch = crate::storage::db_tf::VideoPatch {
                        title: if clean_title.is_empty() { None } else { Some(clean_title) },
                        description: if info.description.is_empty() { None } else { Some(info.description) },
                        duration_sec: info.duration_seconds,
                        view_count: info.view_count,
                        like_count: info.like_count,
                        comment_count: info.comment_count,
                        published_at: info.published_at,
                        tags: if !info.tags.is_empty() {
                            let _ = db.upsert_tags(&vid, &info.tags, "youtube").await;
                            Some(info.tags)
                        } else {
                            None
                        },
                        thumb_url: info.thumb_url,
                        updated_at: now.clone(),
                    };
                    let patched = db.patch_video_coalesced(&vid, &patch).await;
                    tracing::info!("Patch result for {vid}: {:?}", patched);
                    got_full_meta = true;
                }
                Err(e) => {
                    tracing::warn!("InnerTube fetch failed for {vid}: {e}");
                }
            }

            if !got_full_meta {
                if let Some(ref y) = ytdlp {
                    if let Ok(info) = y.metadata(&vid).await {
                        let clean_title = info.title.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'");
                        let patch = crate::storage::db_tf::VideoPatch {
                            title: if clean_title.is_empty() { None } else { Some(clean_title) },
                            description: if info.description.is_empty() { None } else { Some(info.description) },
                            duration_sec: info.duration_sec,
                            view_count: info.view_count,
                            like_count: info.like_count,
                            comment_count: info.comment_count,
                            published_at: info.published_at,
                            tags: if !info.tags.is_empty() {
                                let _ = db.upsert_tags(&vid, &info.tags, "youtube").await;
                                Some(info.tags)
                            } else {
                                None
                            },
                            thumb_url: info.thumbnail,
                            updated_at: now.clone(),
                        };
                        let _ = db.patch_video_coalesced(&vid, &patch).await;
                    }
                }
            }
        }
        let _ = crate::analytics::tags::analyze_competitors(&db).await;
        let _ = db.checkpoint().await;
    });

    Ok(Json(json!({
        "ok": true,
        "message": format!("Triggered live sync for {count} videos of channel {id}"),
        "videos_queued": count,
    })))
}

#[derive(Debug, Default)]
struct ResolvedChannel {
    channel_id: String,
    title: String,
    handle: Option<String>,
    description: Option<String>,
    avatar_url: Option<String>,
    subscriber_count: Option<i64>,
}

/// Helper to resolve channel details keylessly.
async fn resolve_channel_details(
    clients: &FetchClients,
    input: &str,
) -> Result<ResolvedChannel, TubeforgeError> {
    let clean = input
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.youtube.com/")
        .trim_start_matches("youtube.com/");

    let (url, handle_opt) = if clean.starts_with('@') {
        (format!("https://www.youtube.com/{clean}"), Some(clean.to_string()))
    } else if clean.starts_with("channel/") {
        let cid = clean.trim_start_matches("channel/");
        (format!("https://www.youtube.com/channel/{cid}"), None)
    } else if clean.starts_with("UC") {
        (format!("https://www.youtube.com/channel/{clean}"), None)
    } else {
        (format!("https://www.youtube.com/@{clean}"), Some(format!("@{clean}")))
    };

    let profile = next_ua_profile();
    let resp = clients
        .http
        .get(&url)
        .header("User-Agent", profile.ua)
        .header("sec-ch-ua", profile.sec_ch_ua)
        .header("sec-ch-ua-platform", profile.platform)
        .header("sec-ch-ua-mobile", profile.mobile)
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|e| TubeforgeError::Fetch {
            src: Source::Api,
            url: url.clone(),
            inner: format!("Channel page request failed: {e}"),
        })?;

    let html = resp.text().await.map_err(|e| TubeforgeError::Parse {
        src: Source::Api,
        item: url.clone(),
        inner: format!("Channel HTML decode failed: {e}"),
    })?;

    // Extract channel ID
    let channel_id = if let Some(pos) = html.find("channel_id=") {
        let sub = &html[pos + 11..];
        let end = sub.find(|c| c == '"' || c == '&' || c == '\'').unwrap_or(sub.len().min(24));
        sub[..end].to_string()
    } else if let Some(pos) = html.find("\"channelId\":\"") {
        let sub = &html[pos + 13..];
        let end = sub.find('"').unwrap_or(sub.len().min(24));
        sub[..end].to_string()
    } else if let Some(pos) = html.find("\"browseId\":\"") {
        let sub = &html[pos + 12..];
        let end = sub.find('"').unwrap_or(sub.len().min(24));
        sub[..end].to_string()
    } else {
        return Err(TubeforgeError::Parse {
            src: Source::Api,
            item: url,
            inner: "Could not locate channelId in YouTube page HTML".to_string(),
        });
    };

    // Extract Title
    let title = if let Some(pos) = html.find("<meta property=\"og:title\" content=\"") {
        let sub = &html[pos + 35..];
        let end = sub.find('"').unwrap_or(sub.len().min(60));
        sub[..end].to_string()
    } else {
        handle_opt.clone().unwrap_or_else(|| channel_id.clone())
    };

    // Extract Avatar
    let avatar_url = if let Some(pos) = html.find("<meta property=\"og:image\" content=\"") {
        let sub = &html[pos + 35..];
        let end = sub.find('"').unwrap_or(sub.len().min(200));
        Some(sub[..end].to_string())
    } else {
        None
    };

    // Extract Description
    let description = if let Some(pos) = html.find("<meta property=\"og:description\" content=\"") {
        let sub = &html[pos + 41..];
        let end = sub.find('"').unwrap_or(sub.len().min(300));
        Some(sub[..end].to_string())
    } else {
        None
    };

    // Extract Subscriber Count from HTML metadata
    let subscriber_count = if let Some(pos) = html.find("\"subscriberCountText\":{\"accessibility\":{\"accessibilityData\":{\"label\":\"") {
        let sub = &html[pos + 70..];
        let end = sub.find('"').unwrap_or(sub.len().min(40));
        parse_subscriber_text(&sub[..end])
    } else if let Some(pos) = html.find("\"subscriberCountText\":{\"simpleText\":\"") {
        let sub = &html[pos + 37..];
        let end = sub.find('"').unwrap_or(sub.len().min(40));
        parse_subscriber_text(&sub[..end])
    } else {
        None
    };

    Ok(ResolvedChannel {
        channel_id,
        title,
        handle: handle_opt,
        description,
        avatar_url,
        subscriber_count,
    })
}

fn parse_subscriber_text(s: &str) -> Option<i64> {
    let clean = s.to_lowercase();
    let words: Vec<&str> = clean.split_whitespace().collect();
    let num_part = words.first()?;
    if let Some(stripped) = num_part.strip_suffix('k') {
        let val: f64 = stripped.parse().ok()?;
        Some((val * 1000.0).round() as i64)
    } else if let Some(stripped) = num_part.strip_suffix('m') {
        let val: f64 = stripped.parse().ok()?;
        Some((val * 1_000_000.0).round() as i64)
    } else if let Some(stripped) = num_part.strip_suffix('b') {
        let val: f64 = stripped.parse().ok()?;
        Some((val * 1_000_000_000.0).round() as i64)
    } else {
        let digits: String = num_part.chars().filter(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    }
}

/// Helper to ingest channel RSS feed into SQLite videos table.
async fn ingest_channel_rss_feed(
    clients: &FetchClients,
    db: &Db,
    channel_id: &str,
) -> Result<usize, TubeforgeError> {
    let feed_url = format!("https://www.youtube.com/feeds/videos.xml?channel_id={channel_id}");
    let profile = next_ua_profile();

    let resp = clients
        .http
        .get(&feed_url)
        .header("User-Agent", profile.ua)
        .send()
        .await
        .map_err(|e| TubeforgeError::Fetch {
            src: Source::Rss,
            url: feed_url.clone(),
            inner: format!("RSS request failed: {e}"),
        })?;

    let xml = resp.text().await.map_err(|e| TubeforgeError::Parse {
        src: Source::Rss,
        item: feed_url.clone(),
        inner: format!("RSS decode failed: {e}"),
    })?;

    let now = crate::util::now_rfc3339();
    let mut count = 0;

    // Split entries
    for entry in xml.split("<entry>") {
        if !entry.contains("<yt:videoId>") {
            continue;
        }

        let video_id = if let Some(pos) = entry.find("<yt:videoId>") {
            let sub = &entry[pos + 12..];
            let end = sub.find("</yt:videoId>").unwrap_or(11);
            sub[..end].trim().to_string()
        } else {
            continue;
        };

        let title = if let Some(pos) = entry.find("<title>") {
            let sub = &entry[pos + 7..];
            let end = sub.find("</title>").unwrap_or(sub.len().min(100));
            sub[..end].trim().to_string()
        } else {
            "Untitled".to_string()
        };

        let published_at = if let Some(pos) = entry.find("<published>") {
            let sub = &entry[pos + 11..];
            let end = sub.find("</published>").unwrap_or(sub.len().min(30));
            sub[..end].trim().to_string()
        } else {
            now.clone()
        };

        let row = VideoRow {
            video_id,
            channel_id: Some(channel_id.to_string()),
            title,
            description: String::new(),
            tags: "[]".to_string(),
            category_id: None,
            duration_sec: None,
            published_at,
            view_count: None,
            like_count: None,
            comment_count: None,
            thumb_url: None,
            source: "rss".to_string(),
            fetched_at: now.clone(),
            updated_at: now.clone(),
            recording_date: None,
            recording_location_name: None,
            recording_lat: None,
            recording_lng: None,
            topic_categories: "[]".to_string(),
            privacy_status: None,
        };

        if db.put_video(&row).await.is_ok() {
            count += 1;
        }
    }

    Ok(count)
}
