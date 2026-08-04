//! `keywords` (LLD §4.1, §8.3): `add` tracked keywords, `check` corpus ranks
//! into `keyword_rankings` snapshots, `report` trends with Rust-computed
//! deltas (lag/lead unavailable in Turso 0.7.2).

use std::collections::HashSet;

use serde_json::{json, Value};

use crate::analytics::keywords;
use crate::config::Config;
use crate::error::TubeforgeError;
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

/// `keywords report`: latest trend rows per keyword (deltas in Rust).
pub async fn run_report(db: &Db) -> Result<Value, TubeforgeError> {
    keywords::report(db).await
}
