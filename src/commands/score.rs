//! `score` — full SEO/GEO mode (LLD §4.1, §7). Supersedes the Phase 1 BASIC
//! mode: the Phase 1 BM25 signals (`keyword_title` / `keyword_desc` /
//! `keyword_tags` / `title_length`) are retained inside the full weighted
//! component set, so `--draft-title` keeps working with the same envelope
//! shape plus `geo` and the composite `total`.
//!
//! Keyword queries resolve to explicit `--keywords` flags, else the tracked
//! `keywords` table (stored videos), else the title itself (draft flow).

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::scoring::{self, weights::Weights};
use crate::search::bm25::Bm25;
use crate::search::open_or_create;
use crate::storage::Db;

pub struct ScoreInput {
    pub video_id: Option<String>,
    pub draft_title: Option<String>,
    pub draft_desc: Option<String>,
    pub draft_tags: Option<String>,
}

/// `score --video-id <id>` or `score --draft-title "..." [--draft-desc]
/// [--draft-tags]` with no explicit keywords (Phase 1 call shape).
pub async fn run(cfg: &Config, input: &ScoreInput) -> Result<Value, TubeforgeError> {
    run_with_keywords(cfg, input, &[]).await
}

/// Same as `run`, with explicit target keywords (repeatable `--keywords`).
pub async fn run_with_keywords(
    cfg: &Config,
    input: &ScoreInput,
    keywords: &[String],
) -> Result<Value, TubeforgeError> {
    let (video_id, title, desc, tags, geo_meta) = resolve_input(cfg, input).await?;
    let weights = Weights::from_env()?;

    let index = open_or_create(&cfg.index_dir())?;
    let bm25 = Bm25::open(index)?;

    // Explicit flags win; stored videos fall back to tracked keywords; the
    // draft flow falls back to the title itself (LLD §7.1 "target keywords
    // (optional)").
    let eff: Vec<String> = if !keywords.is_empty() {
        keywords.to_vec()
    } else if input.video_id.is_some() {
        let db = Db::open(&cfg.db_path).await?;
        db.list_keywords()
            .await?
            .into_iter()
            .map(|k| k.keyword)
            .collect()
    } else {
        Vec::new()
    };

    let result = scoring::compute_with_meta(
        &title,
        &desc,
        &tags,
        &eff,
        &bm25,
        &weights,
        video_id.as_deref(),
        &geo_meta,
    );

    // Persist for stored videos so ingest/scores stay consistent (§6.4).
    if let Some(vid) = &video_id {
        let db = Db::open(&cfg.db_path).await?;
        scoring::persist(&db, vid, &result).await?;
    }

    Ok(json!({
        "video_id": video_id,
        "title": title,
        "seo": {
            "total": result.seo_total,
            "components": result.seo_components,
        },
        "geo": {
            "total": result.geo_total,
            "components": result.geo_components,
        },
        "total": result.total,
    }))
}

/// Load title/desc/tags from the DB (stored video) or the draft flags, plus
/// the stored video's free metadata (recording details + topic categories)
/// for the C1/C2 GEO signals — empty for drafts.
async fn resolve_input(
    cfg: &Config,
    input: &ScoreInput,
) -> Result<(Option<String>, String, String, Vec<String>, crate::scoring::geo::GeoMeta), TubeforgeError> {
    if let Some(vid) = &input.video_id {
        let db = Db::open(&cfg.db_path).await?;
        let row = db
            .get_video(vid)
            .await?
            .ok_or_else(|| TubeforgeError::Usage(format!("video not in database: {vid}")))?;
        let tags: Vec<String> = serde_json::from_str(&row.tags).unwrap_or_default();
        let meta = crate::scoring::geo::GeoMeta {
            published_at: row.published_at.clone(),
            recording_date: row.recording_date.clone(),
            recording_location_name: row.recording_location_name.clone(),
            recording_lat: row.recording_lat,
            recording_lng: row.recording_lng,
            topic_categories: serde_json::from_str(&row.topic_categories).unwrap_or_default(),
        };
        Ok((Some(vid.clone()), row.title, row.description, tags, meta))
    } else {
        let title = input.draft_title.clone().unwrap_or_default();
        let desc = input.draft_desc.clone().unwrap_or_default();
        let tags: Vec<String> = input
            .draft_tags
            .clone()
            .unwrap_or_default()
            .split([',', ' '])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if title.is_empty() && desc.is_empty() && tags.is_empty() {
            return Err(TubeforgeError::Usage(
                "score needs --video-id or at least one --draft-* flag".into(),
            ));
        }
        Ok((None, title, desc, tags, crate::scoring::geo::GeoMeta::default()))
    }
}
