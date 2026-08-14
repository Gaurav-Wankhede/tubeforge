//! `export` (Phase 3 workstream B): dump the local dataset as CSVs + JSON
//! arrays into a zip (default) or a plain directory. User-facing data —
//! deliberately separate from `backup` (the VACUUM INTO recovery snapshot).
//!
//! Column sets are adapted from the MW Metadata export reference
//! (mattwright324/youtube-metadata, MIT) to TubeForge's schema; missing
//! schema fields (e.g. `Language`) export as empty strings. Ordering is
//! deterministic (`ORDER BY video_id` etc.) so exports are reproducible.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::{storage_err, TubeforgeError};
use crate::export::csv;
use crate::storage::db::{
    AlertRow, ChannelRow, IdeaRow, KeywordRow, RankingRow, ScoreRow, VideoRow,
};
use crate::storage::Db;

/// `--format zip|dir`; zip is the default (single archive in `--out`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Zip,
    Dir,
}

/// JSON files written alongside the CSVs (same rows, machine-consumable).
pub const JSON_FILES: [&str; 5] = [
    "videos.json",
    "ideas.json",
    "alerts.json",
    "scores.json",
    "manifest.json",
];

pub async fn run(cfg: &Config, out: &Path, format: ExportFormat) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;

    let videos = db.all_videos().await?;
    let channels = db.all_channels().await?;
    let keywords = db.list_keywords().await?;
    let rankings = db.list_rankings().await?;
    let ideas = db.all_ideas().await?;
    let alerts = db.list_alerts(0).await?;
    let mut scores = db.all_scores().await?;
    // Deterministic order (all_scores sorts by total DESC; tie-break here).
    scores.sort_by(|a, b| a.video_id.cmp(&b.video_id));

    let channels_by_id: HashMap<&str, &ChannelRow> = channels
        .iter()
        .map(|c| (c.channel_id.as_str(), c))
        .collect();

    let csv_files: Vec<(&str, String)> = vec![
        ("videos.csv", videos_csv(&videos, &channels_by_id)),
        ("channels.csv", channels_csv(&channels)),
        ("tags.csv", tags_csv(&videos)),
        ("keywords.csv", keywords_csv(&keywords)),
        ("keyword_rankings.csv", keyword_rankings_csv(&rankings)),
    ];
    let json_files: Vec<(&str, Value)> = vec![
        ("videos.json", videos_json(&videos, &channels_by_id)),
        ("ideas.json", ideas_json(&ideas)),
        ("alerts.json", alerts_json(&alerts)),
        ("scores.json", scores_json(&scores)),
    ];
    let manifest = manifest_json(
        ExportCounts {
            videos: videos.len(),
            channels: channels.len(),
            tags: tags_csv_count(&videos),
            keywords: keywords.len(),
            keyword_rankings: rankings.len(),
            ideas: ideas.len(),
            alerts: alerts.len(),
            scores: scores.len(),
        },
        &csv_files,
    );

    std::fs::create_dir_all(out)
        .map_err(|e| storage_err("IO", format!("create dir {}: {e}", out.display())))?;

    let (archive, files) = match format {
        ExportFormat::Zip => {
            let name = format!("tubeforge-export-{}.zip", crate::util::batch_id());
            let path = out.join(&name);
            write_zip(&path, &csv_files, &json_files, &manifest)?;
            (Some(name.clone()), vec![name])
        }
        ExportFormat::Dir => {
            for (name, body) in &csv_files {
                std::fs::write(out.join(name), body).map_err(|e| {
                    storage_err("IO", format!("write {}: {e}", out.join(name).display()))
                })?;
            }
            for (name, v) in &json_files {
                std::fs::write(
                    out.join(name),
                    pretty(&json_merge(v.clone(), &manifest, name)),
                )
                .map_err(|e| {
                    storage_err("IO", format!("write {}: {e}", out.join(name).display()))
                })?;
            }
            std::fs::write(out.join("manifest.json"), pretty(&manifest))
                .map_err(|e| storage_err("IO", format!("write manifest.json: {e}")))?;
            let names: Vec<String> = csv_files
                .iter()
                .map(|(n, _)| n.to_string())
                .chain(JSON_FILES.iter().map(|n| n.to_string()))
                .collect();
            (None, names)
        }
    };

    Ok(json!({
        "format": match format { ExportFormat::Zip => "zip", ExportFormat::Dir => "dir" },
        "out": out.to_string_lossy(),
        "archive": archive,
        "files": files,
        "counts": manifest["counts"],
    }))
}

// ---------------------------------------------------------------------------
// CSV writers
// ---------------------------------------------------------------------------

/// `videos.csv` — MW Metadata reference column set, adapted to the schema.
/// Columns: Video ID, Title, Channel ID, Channel Title, Description,
/// Published, Views, Likes, Comments, Duration, Category ID, Category Name,
/// Language, Tags (joined), Source, Privacy Status, Recording Date,
/// Recording Location, Topic Categories.
fn videos_csv(videos: &[VideoRow], channels_by_id: &HashMap<&str, &ChannelRow>) -> String {
    let mut out = csv::record_strs(&[
        "Video ID",
        "Title",
        "Channel ID",
        "Channel Title",
        "Description",
        "Published",
        "Views",
        "Likes",
        "Comments",
        "Duration",
        "Category ID",
        "Category Name",
        "Language",
        "Tags",
        "Source",
        "Privacy Status",
        "Recording Date",
        "Recording Location",
        "Topic Categories",
    ]);
    for v in videos {
        let channel_title = v
            .channel_id
            .as_deref()
            .and_then(|cid| channels_by_id.get(cid))
            .map(|c| c.title.as_str())
            .unwrap_or("");
        let category_name = v
            .category_id
            .as_deref()
            .map(|cid| crate::categories::category_name(cid).unwrap_or(cid))
            .unwrap_or("");
        let tags = serde_json::from_str::<Vec<String>>(&v.tags)
            .unwrap_or_default()
            .join(", ");
        let topics = serde_json::from_str::<Vec<String>>(&v.topic_categories)
            .unwrap_or_default()
            .join(", ");
        out.push_str(&csv::record_strs(&[
            &v.video_id,
            &v.title,
            v.channel_id.as_deref().unwrap_or(""),
            channel_title,
            &v.description,
            &v.published_at,
            &opt_i64(v.view_count),
            &opt_i64(v.like_count),
            &opt_i64(v.comment_count),
            &opt_i64(v.duration_sec),
            v.category_id.as_deref().unwrap_or(""),
            category_name,
            "", // Language — no schema column (MW reference keeps it).
            &tags,
            &v.source,
            v.privacy_status.as_deref().unwrap_or(""),
            v.recording_date.as_deref().unwrap_or(""),
            v.recording_location_name.as_deref().unwrap_or(""),
            &topics,
        ]));
    }
    out
}

/// `channels.csv` — Channel ID, Title, Handle, Subscribers, Video Count,
/// Country.
fn channels_csv(channels: &[ChannelRow]) -> String {
    let mut out = csv::record_strs(&[
        "Channel ID",
        "Title",
        "Handle",
        "Subscribers",
        "Video Count",
        "Country",
    ]);
    for c in channels {
        out.push_str(&csv::record_strs(&[
            &c.channel_id,
            &c.title,
            c.handle.as_deref().unwrap_or(""),
            &opt_i64(c.subscriber_count),
            &opt_i64(c.video_count),
            c.country.as_deref().unwrap_or(""),
        ]));
    }
    out
}

/// `tags.csv` — Tag, Video Count, First Used, Last Used (MW reference
/// format; first/last = published_at bounds of videos carrying the tag).
fn tags_csv(videos: &[VideoRow]) -> String {
    let mut out = csv::record_strs(&["Tag", "Video Count", "First Used", "Last Used"]);
    for (tag, (count, first, last)) in tag_census(videos) {
        out.push_str(&csv::record_strs(&[
            &tag,
            &count.to_string(),
            &first,
            &last,
        ]));
    }
    out
}

/// Tag → (video count, earliest published_at, latest published_at), sorted
/// by tag (deterministic).
fn tag_census(videos: &[VideoRow]) -> BTreeMap<String, (usize, String, String)> {
    let mut m: BTreeMap<String, (usize, String, String)> = BTreeMap::new();
    for v in videos {
        if let Ok(tags) = serde_json::from_str::<Vec<String>>(&v.tags) {
            for tag in tags {
                let e = m
                    .entry(tag)
                    .or_insert_with(|| (0, v.published_at.clone(), v.published_at.clone()));
                e.0 += 1;
                if v.published_at < e.1 {
                    e.1 = v.published_at.clone();
                }
                if v.published_at > e.2 {
                    e.2 = v.published_at.clone();
                }
            }
        }
    }
    m
}

fn tags_csv_count(videos: &[VideoRow]) -> usize {
    tag_census(videos).len()
}

/// `keywords.csv` — the tracked-keyword list: Keyword, Niche, Created At.
fn keywords_csv(keywords: &[KeywordRow]) -> String {
    let mut out = csv::record_strs(&["Keyword", "Niche", "Created At"]);
    for k in keywords {
        out.push_str(&csv::record_strs(&[
            &k.keyword,
            k.niche.as_deref().unwrap_or(""),
            &k.created_at,
        ]));
    }
    out
}

/// `keyword_rankings.csv` — ID, Keyword, Checked At, Video ID, Position,
/// Topics. ID is the deterministic 1-based row index (the table's PK is the
/// (keyword, checked_at) composite).
fn keyword_rankings_csv(rankings: &[RankingRow]) -> String {
    let mut out = csv::record_strs(&[
        "ID",
        "Keyword",
        "Checked At",
        "Video ID",
        "Position",
        "Topics",
    ]);
    for (i, r) in rankings.iter().enumerate() {
        out.push_str(&csv::record_strs(&[
            &(i + 1).to_string(),
            &r.keyword,
            &r.checked_at,
            r.video_id.as_deref().unwrap_or(""),
            &opt_i64(r.position),
            r.topics.as_deref().unwrap_or(""),
        ]));
    }
    out
}

// ---------------------------------------------------------------------------
// JSON writers (raw rows as arrays; same ordering as the CSVs)
// ---------------------------------------------------------------------------

fn videos_json(videos: &[VideoRow], channels_by_id: &HashMap<&str, &ChannelRow>) -> Value {
    let rows: Vec<Value> = videos
        .iter()
        .map(|v| {
            let channel_title = v
                .channel_id
                .as_deref()
                .and_then(|cid| channels_by_id.get(cid))
                .map(|c| c.title.as_str())
                .unwrap_or("");
            let tags = serde_json::from_str::<Vec<String>>(&v.tags).unwrap_or_default();
            let topics =
                serde_json::from_str::<Vec<String>>(&v.topic_categories).unwrap_or_default();
            json!({
                "video_id": v.video_id,
                "title": v.title,
                "channel_id": v.channel_id,
                "channel_title": channel_title,
                "description": v.description,
                "published_at": v.published_at,
                "view_count": v.view_count,
                "like_count": v.like_count,
                "comment_count": v.comment_count,
                "duration_sec": v.duration_sec,
                "category_id": v.category_id,
                "category_name": v.category_id.as_deref()
                    .map(|cid| crate::categories::category_name(cid).unwrap_or(cid)),
                "language": Value::Null,
                "tags": tags,
                "source": v.source,
                "privacy_status": v.privacy_status,
                "recording_date": v.recording_date,
                "recording_location_name": v.recording_location_name,
                "topic_categories": topics,
            })
        })
        .collect();
    Value::Array(rows)
}

fn ideas_json(ideas: &[IdeaRow]) -> Value {
    let rows: Vec<Value> = ideas
        .iter()
        .map(|i| {
            let rationale = serde_json::from_str::<Value>(&i.rationale).unwrap_or(Value::Null);
            json!({
                "idea_id": i.idea_id,
                "title_suggestion": i.title_suggestion,
                "rationale": rationale,
                "score": i.score,
                "status": i.status,
                "source_video": i.source_video,
                "created_at": i.created_at,
            })
        })
        .collect();
    Value::Array(rows)
}

fn alerts_json(alerts: &[AlertRow]) -> Value {
    let rows: Vec<Value> = alerts
        .iter()
        .map(|a| {
            json!({
                "alert_id": a.alert_id,
                "kind": a.kind,
                "channel_id": a.channel_id,
                "message": a.message,
                "severity": a.severity,
                "created_at": a.created_at,
                "read_at": a.read_at,
            })
        })
        .collect();
    Value::Array(rows)
}

fn scores_json(scores: &[ScoreRow]) -> Value {
    let rows: Vec<Value> = scores
        .iter()
        .map(|s| {
            let components = serde_json::from_str::<Value>(&s.components).unwrap_or(Value::Null);
            json!({
                "video_id": s.video_id,
                "seo_score": s.seo_score,
                "geo_score": s.geo_score,
                "total_score": s.total_score,
                "components": components,
                "computed_at": s.computed_at,
            })
        })
        .collect();
    Value::Array(rows)
}

// ---------------------------------------------------------------------------
// manifest + zip assembly
// ---------------------------------------------------------------------------

/// Per-file row counts carried into `manifest.json`.
#[derive(Debug, Clone, Copy)]
struct ExportCounts {
    videos: usize,
    channels: usize,
    tags: usize,
    keywords: usize,
    keyword_rankings: usize,
    ideas: usize,
    alerts: usize,
    scores: usize,
}

/// One top-level `manifest.json` with the export timestamp, per-file counts
/// and schema version (agents can validate an export without unzipping).
fn manifest_json(counts: ExportCounts, csv_files: &[(&str, String)]) -> Value {
    let mut files: Vec<String> = csv_files.iter().map(|(n, _)| n.to_string()).collect();
    files.extend(JSON_FILES.iter().map(|n| n.to_string()));
    json!({
        "format": "tubeforge-export",
        "exported_at": crate::util::now_rfc3339(),
        "schema_version": crate::storage::schema::SCHEMA_VERSION,
        "tool": "tubeforge",
        "tool_version": env!("CARGO_PKG_VERSION"),
        "counts": {
            "videos": counts.videos,
            "channels": counts.channels,
            "tags": counts.tags,
            "keywords": counts.keywords,
            "keyword_rankings": counts.keyword_rankings,
            "ideas": counts.ideas,
            "alerts": counts.alerts,
            "scores": counts.scores,
        },
        "files": files,
    })
}

/// One JSON data file: the array payload plus a manifest reference — each
/// JSON file keeps its own `_manifest` key so files are self-describing
/// when extracted standalone.
fn json_merge(payload: Value, manifest: &Value, name: &str) -> Value {
    json!({
        "_manifest": {
            "file": name,
            "exported_at": manifest["exported_at"],
            "schema_version": manifest["schema_version"],
        },
        "rows": payload,
    })
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("serializable") + "\n"
}

/// Write every export file into one zip archive (deflate; the `zip` crate
/// is the only new dependency — MIT, pure-Rust backends).
fn write_zip(
    path: &Path,
    csv_files: &[(&str, String)],
    json_files: &[(&str, Value)],
    manifest: &Value,
) -> Result<(), TubeforgeError> {
    let file = std::fs::File::create(path)
        .map_err(|e| storage_err("IO", format!("create {}: {e}", path.display())))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();

    for (name, body) in csv_files {
        zw.start_file(*name, opts)
            .map_err(|e| storage_err("ZIP", e))?;
        zw.write_all(body.as_bytes())
            .map_err(|e| storage_err("ZIP", e))?;
    }
    for (name, v) in json_files {
        let body = pretty(&json_merge(v.clone(), manifest, name));
        zw.start_file(*name, opts)
            .map_err(|e| storage_err("ZIP", e))?;
        zw.write_all(body.as_bytes())
            .map_err(|e| storage_err("ZIP", e))?;
    }
    let body = pretty(manifest);
    zw.start_file("manifest.json", opts)
        .map_err(|e| storage_err("ZIP", e))?;
    zw.write_all(body.as_bytes())
        .map_err(|e| storage_err("ZIP", e))?;

    zw.finish().map_err(|e| storage_err("ZIP", e))?;
    Ok(())
}

fn opt_i64(v: Option<i64>) -> String {
    v.map(|n| n.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::{Db, VideoRow};

    fn mk_video(id: &str, tags: &str, published: &str, privacy: Option<&str>) -> VideoRow {
        VideoRow {
            video_id: id.to_string(),
            title: format!("title {id}"),
            description: format!("desc with, comma for {id}"),
            tags: tags.to_string(),
            published_at: published.to_string(),
            privacy_status: privacy.map(String::from),
            topic_categories: "[\"https://en.wikipedia.org/wiki/Rust_(programming_language)\"]"
                .to_string(),
            ..Default::default()
        }
    }

    /// CSV escaping must survive quotes/commas/newlines in real video data.
    #[tokio::test]
    async fn videos_csv_escapes_and_orders() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut db = Db::open(&dir.path().join("e.db")).await.expect("open");
        for v in [
            mk_video(
                "bbb222ccc33",
                "[\"rust\",\"databases\"]",
                "2026-02-01T00:00:00Z",
                Some("unlisted"),
            ),
            mk_video(
                "aaa111bbb22",
                "[\"a,b\",\"say \\\"hi\\\"\"]",
                "2026-01-01T00:00:00Z",
                Some("public"),
            ),
        ] {
            let mut b = db.begin_batch().await.expect("batch");
            b.upsert_video(&v).await.expect("upsert");
            b.commit().await.expect("commit");
        }
        let videos = db.all_videos().await.expect("videos");
        assert_eq!(videos[0].video_id, "aaa111bbb22", "ordered by video_id");

        let out = videos_csv(&videos, &HashMap::new());
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 rows");
        assert!(lines[0].starts_with("Video ID,Title"));
        // Tags joined with ", " then re-quoted: `["a,b", "say "hi""]` →
        // `a,b, say "hi"` → quoted+escaped CSV field.
        assert!(
            lines[1].contains("\"a,b, say \"\"hi\"\"\""),
            "joined+quoted tag payload: {:?}",
            lines[1]
        );
        assert!(lines[2].contains("unlisted"), "privacy status column");
        assert!(
            lines[1].contains("Rust_(programming_language)"),
            "topic categories column (raw URLs)"
        );
    }

    #[test]
    fn tags_csv_census_and_bounds() {
        let videos = [
            mk_video("a", "[\"x\",\"y\"]", "2026-01-01T00:00:00Z", None),
            mk_video("b", "[\"x\"]", "2026-03-01T00:00:00Z", None),
            mk_video("c", "[]", "2026-02-01T00:00:00Z", None),
        ];
        let csv = tags_csv(&videos);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3, "header + x + y");
        assert_eq!(lines[1], "x,2,2026-01-01T00:00:00Z,2026-03-01T00:00:00Z");
        assert_eq!(lines[2], "y,1,2026-01-01T00:00:00Z,2026-01-01T00:00:00Z");
    }

    #[test]
    fn json_files_wrap_payload_with_manifest() {
        let manifest = manifest_json(
            ExportCounts {
                videos: 1,
                channels: 2,
                tags: 3,
                keywords: 4,
                keyword_rankings: 5,
                ideas: 6,
                alerts: 7,
                scores: 8,
            },
            &[],
        );
        let wrapped = json_merge(json!([1, 2]), &manifest, "videos.json");
        assert_eq!(wrapped["rows"], json!([1, 2]));
        assert_eq!(wrapped["_manifest"]["file"], "videos.json");
        assert_eq!(
            wrapped["_manifest"]["schema_version"],
            crate::storage::schema::SCHEMA_VERSION
        );
    }

    #[test]
    fn manifest_carries_counts_and_schema_version() {
        let manifest = manifest_json(
            ExportCounts {
                videos: 1,
                channels: 2,
                tags: 3,
                keywords: 4,
                keyword_rankings: 5,
                ideas: 6,
                alerts: 7,
                scores: 8,
            },
            &[],
        );
        assert_eq!(manifest["counts"]["videos"], 1);
        assert_eq!(manifest["counts"]["alerts"], 7);
        assert_eq!(
            manifest["schema_version"],
            crate::storage::schema::SCHEMA_VERSION
        );
        assert!(manifest["files"]
            .as_array()
            .unwrap()
            .contains(&json!("videos.json")));
    }
}
