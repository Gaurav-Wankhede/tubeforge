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

    // PRD v4.2 supporting layer: high-CTR packaging psychology from the
    // researched creators (Martell/Hormozi/etc.). Computed separately and
    // surfaced alongside — never blended into the SEO total, which stays the
    // honest, primary rank-or-not signal.
    let psych = scoring::psych::score(&title);
    let psych_detected: Vec<Value> = psych
        .detected
        .iter()
        .map(|f| {
            json!({
                "formula": format!("{:?}", f).to_lowercase(),
                "label": f.label(),
            })
        })
        .collect();

    // Phase 6.6: actionable checklist from the SEO components.
    let recommendations: Vec<Value> =
        scoring::recommend::recommendations(&result.seo_struct, &title, &desc, &tags, &eff)
            .iter()
            .map(|r| {
                json!({
                    "component": r.component,
                    "message": r.message,
                    "current": r.current,
                })
            })
            .collect();

    // Persist for stored videos so ingest/scores stay consistent (§6.4).
    if let Some(vid) = &video_id {
        let db = Db::open(&cfg.db_path).await?;
        scoring::persist(&db, vid, &result).await?;
    }

    // 5-Pillar wiring (all computed alongside SEO/GEO, never blended)
    let db_for_pillars = Db::open(&cfg.db_path).await.ok();
    let all_videos = if let Some(db) = &db_for_pillars { db.all_videos().await.unwrap_or_default() } else { Vec::new() };
    let beachhead = crate::analytics::beachhead::score_topic(&title, &bm25, &all_videos);
    // Monopoly: synthesize a VideoRow for draft scoring
    let draft_video = crate::storage::db::VideoRow {
        video_id: video_id.clone().unwrap_or_else(|| "draft".into()),
        title: title.clone(),
        description: desc.clone(),
        tags: serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()),
        thumb_url: None,
        duration_sec: Some(900),
        published_at: crate::util::now_rfc3339(),
        ..Default::default()
    };
    let monopoly = crate::analytics::monopoly::score_video(&draft_video, &desc, &tags);
    let trust = if let Some(db) = &db_for_pillars {
        let channels = db.all_channels().await.unwrap_or_default();
        let own_id = std::env::var("TUBEFORGE_OWN_CHANNEL").ok()
            .and_then(|id| channels.iter().find(|c| c.channel_id == id).map(|c| c.channel_id.clone()))
            .or_else(|| channels.first().map(|c| c.channel_id.clone()));
        if let Some(oid) = own_id {
            let own_ch = channels.iter().find(|c| c.channel_id == oid);
            let own_vids: Vec<crate::storage::db::VideoRow> = all_videos.iter().filter(|v| v.channel_id.as_deref() == Some(oid.as_str())).cloned().collect();
            let t = crate::analytics::trust::compute(own_ch, &own_vids);
            json!({ "channel_id": oid, "total": t.total, "tier1_ready": t.tier1_ready, "reasons": t.reasons })
        } else { json!({ "total": 0, "tier1_ready": false }) }
    } else { json!({ "total": 0, "tier1_ready": false }) };

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
        "psychology": {
            "total": psych.total,
            "detected": psych_detected,
            "variants": scoring::psych::variants(&title, None),
        },
        "pillars": {
            "trust": trust,
            "beachhead": { "total": beachhead.total, "token_specificity": beachhead.token_specificity, "competition_weakness": beachhead.competition_weakness, "intent_sharpness": beachhead.intent_sharpness, "verdict": beachhead.verdict, "is_beachhead": beachhead.is_beachhead },
            "monopoly": { "total": monopoly.total, "completeness": monopoly.completeness, "visual_tangibility": monopoly.visual_tangibility, "packaging": monopoly.packaging, "is_monopoly": monopoly.is_monopoly, "verdict": monopoly.verdict },
            "session_hint": "see health.session_chains for pillar loops — chain feeders (done tickets) to pillar masterworks"
        },
        "recommendations": recommendations,
    }))
}

/// Load title/desc/tags from the DB (stored video) or the draft flags, plus
/// the stored video's free metadata (recording details + topic categories)
/// for the C1/C2 GEO signals — empty for drafts.
async fn resolve_input(
    cfg: &Config,
    input: &ScoreInput,
) -> Result<
    (
        Option<String>,
        String,
        String,
        Vec<String>,
        crate::scoring::geo::GeoMeta,
    ),
    TubeforgeError,
> {
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
        Ok((
            None,
            title,
            desc,
            tags,
            crate::scoring::geo::GeoMeta::default(),
        ))
    }
}
