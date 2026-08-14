//! `keywords` (LLD §4.1, §8.3): `add` tracked keywords, `check` corpus ranks
//! into `keyword_rankings` snapshots, `report` trends with Rust-computed
//! deltas (lag/lead unavailable in Turso 0.7.2).

use std::collections::HashSet;

use serde_json::{json, Value};

use crate::analytics::keywords;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::ytdlp::YtdlpClient;
use crate::fetch::FetchClients;
use crate::search::bm25::Bm25;
use crate::search::open_or_create;
use crate::storage::Db;

/// `keywords add <kw>...`: track keywords (INSERT OR IGNORE).
pub async fn run_add(db: &Db, keywords: &[String]) -> Result<Value, TubeforgeError> {
    let added = db.add_keywords(keywords, None).await?;
    let all: Vec<String> = db
        .list_keywords()
        .await?
        .into_iter()
        .map(|k| k.keyword)
        .collect();
    Ok(json!({ "added": added, "keywords": all }))
}

/// `keywords check`: snapshot the corpus rank of every tracked keyword.
pub async fn run_check(cfg: &Config) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let videos = db.all_videos().await?;
    if videos.is_empty() {
        return Ok(json!({
            "snapshots": 0,
            "note": "no videos in database — run `tubeforge ingest` first",
        }));
    }
    let competitors: HashSet<String> = db.list_competitors().await?.into_iter().collect();

    let index = open_or_create(&cfg.index_dir())?;
    let bm25 = Bm25::open(index)?;

    let snapshots = keywords::check(&db, &bm25, &videos, &competitors).await?;
    let report = keywords::report(&db).await?;

    Ok(json!({
        "snapshots": snapshots,
        "trends": report["keywords"],
    }))
}

/// `keywords research <topic>... [--serp N] [--dedupe]`: batch research
/// for many topics. Each topic runs the full keyless pipeline (ytsearch
/// SERP + tags + autocomplete) and persists: SERP videos + tags, competitor
/// tag aggregation, and a research-history snapshot. `--dedupe` collapses
/// duplicate videos (same channel + title) afterwards.
pub async fn run_research(
    cfg: &Config,
    topics: &[String],
    serp: u64,
    dedupe: bool,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;
    let ytdlp = YtdlpClient::new(
        cfg.ytdlp_path.clone(),
        cfg.ytdlp_enabled,
        cfg.ytdlp_client.clone(),
        cfg.ytdlp_js_runtime.clone(),
    )?;
    let clients = FetchClients::new()?;
    let bm25 = crate::search::open_or_create(&cfg.index_dir())
        .ok()
        .and_then(|index| Bm25::open(index).ok());

    let mut results: Vec<Value> = Vec::new();
    for topic in topics {
        let r =
            crate::analytics::research::inspect(&db, bm25.as_ref(), &ytdlp, &clients, topic, serp)
                .await?;
        // Persist SERP videos + tags (feeds the Tags Analyzer + gaps).
        crate::storage::db::persist_serp_db(&db, &r.serp).await?;
        crate::analytics::tags::analyze_competitors(&db).await?;
        // Research-history snapshot (migration 008).
        db.upsert_keyword_research(
            &r.keyword,
            &crate::util::now_rfc3339(),
            &r.volume_label,
            r.serp_total as i64,
            r.serp_mean_views,
            r.ranking_channels as i64,
            r.competition_score,
            r.opportunity_score,
            r.actively_published,
            &serde_json::to_string(&r.suggested_tags).unwrap_or_else(|_| "[]".to_string()),
            &serde_json::to_string(&r.related_keywords).unwrap_or_else(|_| "[]".to_string()),
        )
        .await?;
        results.push(json!({
            "topic": r.keyword,
            "serp": r.serp_total,
            "keyword_score": r.keyword_score,
            "opportunity": r.opportunity_score,
            "competition": r.competition_score,
            "volume": r.volume_label,
            "active": r.actively_published,
        }));
    }

    let mut out = json!({ "researched": results.len(), "topics": results });
    if dedupe {
        let (merged, deleted) = crate::storage::db::dedupe_videos(&db).await?;
        out["dedupe"] = json!({ "groups_merged": merged, "rows_deleted": deleted });
    }
    Ok(out)
}

/// `keywords report`: latest trend rows per keyword (deltas in Rust).
pub async fn run_report(db: &Db) -> Result<Value, TubeforgeError> {
    keywords::report(db).await
}

/// `keywords inspect <kw> [--serp N]`: VidIQ-style keyword research —
/// real SERP demand proxy + competition + opportunity + related keywords,
/// all keyless (yt-dlp ytsearch + Google YouTube autocomplete).
pub async fn run_inspect(cfg: &Config, keyword: &str, serp: u64) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;

    let ytdlp = YtdlpClient::new(
        cfg.ytdlp_path.clone(),
        cfg.ytdlp_enabled,
        cfg.ytdlp_client.clone(),
        cfg.ytdlp_js_runtime.clone(),
    )?;
    let clients = FetchClients::new()?;

    // Corpus resonance needs the tantivy index (best-effort).
    let bm25 = crate::search::open_or_create(&cfg.index_dir())
        .ok()
        .and_then(|index| Bm25::open(index).ok());

    let r =
        crate::analytics::research::inspect(&db, bm25.as_ref(), &ytdlp, &clients, keyword, serp)
            .await?;

    // Persist SERP videos + tags (research runs feed the Tags Analyzer).
    crate::storage::db::persist_serp_db(&db, &r.serp).await?;
    crate::analytics::tags::analyze_competitors(&db).await?;

    // Persist the snapshot (migration 008) so history accumulates across
    // CLI + API research runs.
    db.upsert_keyword_research(
        &r.keyword,
        &crate::util::now_rfc3339(),
        &r.volume_label,
        r.serp_total as i64,
        r.serp_mean_views,
        r.ranking_channels as i64,
        r.competition_score,
        r.opportunity_score,
        r.actively_published,
        &serde_json::to_string(&r.suggested_tags).unwrap_or_else(|_| "[]".to_string()),
        &serde_json::to_string(&r.related_keywords).unwrap_or_else(|_| "[]".to_string()),
    )
    .await?;

    serde_json::to_value(r).map_err(|e| TubeforgeError::Storage {
        code: "RESEARCH_JSON".to_string(),
        message: e.to_string(),
    })
}

/// `keywords discover "<topic>" [--serp N] [--enrich] [--transcripts]`:
/// dynamic search-driven discovery. Pulls the top-ranking channels & videos
/// for the SEARCHED TEXT, persists them into the corpus, registers the
/// ranking channels as competitors, and (with `--enrich`) fetches per-video
/// retention heatmaps + transcripts so trend signals (VPH, engagement,
/// retention) emerge from what ranks NOW for the topic. yt-dlp only.
pub async fn run_discover(
    cfg: &Config,
    topic: &str,
    serp: u64,
    enrich: bool,
    with_transcripts: bool,
) -> Result<Value, TubeforgeError> {
    let db = Db::open(&cfg.db_path).await?;

    let ytdlp = YtdlpClient::new(
        cfg.ytdlp_path.clone(),
        cfg.ytdlp_enabled,
        cfg.ytdlp_client.clone(),
        cfg.ytdlp_js_runtime.clone(),
    )?;
    let clients = FetchClients::new()?;

    let bm25 = crate::search::open_or_create(&cfg.index_dir())
        .ok()
        .and_then(|index| Bm25::open(index).ok());

    let d = crate::analytics::research::discover(
        &db,
        bm25.as_ref(),
        &ytdlp,
        &clients,
        topic,
        serp,
        enrich,
        with_transcripts,
    )
    .await?;

    // Rebuild the tantivy index so corpus BM25 (keywords check / ideas) sees
    // the freshly discovered videos immediately.
    let reindex = crate::commands::reindex::run(cfg).await?;
    let indexed = reindex["docs"].as_u64().unwrap_or(0);

    Ok(json!({
        "topic": d.research.keyword,
        "serp": d.research.serp_total,
        "keyword_score": d.research.keyword_score,
        "opportunity": d.research.opportunity_score,
        "competition": d.research.competition_score,
        "volume": d.research.volume_label,
        "active": d.research.actively_published,
        "competitors_registered": d.competitors_registered,
        "heatmaps_fetched": d.heatmaps_fetched,
        "transcripts_fetched": d.transcripts_fetched,
        "indexed_docs": indexed,
        "ranking_channels": d.research.ranking_channels,
        "suggested_tags": d.research.suggested_tags,
        "related_keywords": d.research.related_keywords,
        "verdict": d.research.verdict,
        "trends": d.trends,
        "note": "ranking channels now in `competitors` — run `tubeforge scorecard`, `gaps`, or `ideas` over the refreshed corpus",
    }))
}
