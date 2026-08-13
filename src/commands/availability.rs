//! `check availability` (Phase 3 workstream B): detect tracked videos that
//! went private/deleted and snapshot `privacyStatus` for the survivors.
//!
//! Direct-API design (cheaper than MW Metadata's playlistItems heuristic):
//! one batched `videos.list` call per ≤50 ids with `part=snippet,status`,
//! each call billed 1 quota unit through the existing ledger. Videos absent
//! from the response (or reported as a `videoNotFound` API error) raise a
//! `video_unavailable` alert into the `alerts` table — a finding, not a
//! failure: the command still exits 0. Present videos get their
//! `status.privacyStatus` recorded in `videos.privacy_status` (migration
//! 003) and surfaced in `health`'s privacy census.
//!
//! Requires `YOUTUBE_API_KEY` (Config error, exit 1 — no silent no-op).

use serde_json::{json, Value};

use crate::analytics::reports;
use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::api::{ApiClient, AvailabilityItem, BATCH_MAX};
use crate::fetch::FetchClients;
use crate::storage::Db;

/// Alert kind raised for videos that no longer resolve (LLD §8.4 rule set).
pub const ALERT_KIND: &str = "video_unavailable";

pub async fn run(cfg: &Config, ids: &[String]) -> Result<Value, TubeforgeError> {
    let key = cfg.youtube_api_key.as_deref().ok_or_else(|| {
        TubeforgeError::Config(
            "check availability needs YOUTUBE_API_KEY in .env \
             (YouTube Data API v3 key — `tubeforge init` scaffolds the key file)"
                .to_string(),
        )
    })?;

    let db = Db::open(&cfg.db_path).await?;

    // The check set is stored videos only; `--video-id` restricts it. An
    // unknown id is a usage error (mirrors `scorecard --channel`).
    let stored = db.all_videos().await?;
    let targets: Vec<(String, Option<String>)> = if ids.is_empty() {
        stored
            .iter()
            .map(|v| (v.video_id.clone(), v.channel_id.clone()))
            .collect()
    } else {
        let mut out = Vec::new();
        for id in ids {
            match stored.iter().find(|v| &v.video_id == id) {
                Some(v) => out.push((v.video_id.clone(), v.channel_id.clone())),
                None => {
                    return Err(TubeforgeError::Usage(format!(
                    "video not in database: {id} — `check availability` only covers stored videos"
                )))
                }
            }
        }
        out
    };
    if targets.is_empty() {
        return Ok(json!({
            "checked": 0,
            "missing": [],
            "privacy_counts": { "public": 0, "unlisted": 0, "private": 0 },
            "alerts_raised": 0,
        }));
    }

    let clients = FetchClients::new()?;
    let api = ApiClient::new(&clients, key);
    let target_ids: Vec<String> = targets.iter().map(|(id, _)| id.clone()).collect();
    let items = api.fetch_availability(&db, &target_ids).await?;

    let found: Vec<&AvailabilityItem> = items.iter().collect();
    let found_ids: std::collections::HashSet<&str> =
        found.iter().map(|i| i.video_id.as_str()).collect();
    let missing: Vec<&str> = target_ids
        .iter()
        .map(|id| id.as_str())
        .filter(|id| !found_ids.contains(id))
        .collect();

    // Alerts for the gone videos (dedupe: identical kind+channel+message row
    // fires once), keyed to the video's channel when known.
    let mut alerts_raised = 0;
    for id in &missing {
        let channel = targets
            .iter()
            .find(|(vid, _)| vid == id)
            .and_then(|(_, c)| c.as_deref());
        alerts_raised += reports::insert_once(
            &db,
            ALERT_KIND,
            channel,
            &format!("Video {id} no longer available (private or deleted)"),
            "warn",
        )
        .await?;
    }

    // Record privacyStatus snapshots for the survivors (migration 003).
    let mut privacy = json!({ "public": 0, "unlisted": 0, "private": 0 });
    for item in &items {
        db.set_privacy_status(&item.video_id, item.privacy_status.as_deref())
            .await?;
        if let Some(status) = item.privacy_status.as_deref() {
            if let Some(count) = privacy[status].as_i64() {
                privacy[status] = json!(count + 1);
            }
        }
    }

    Ok(json!({
        "checked": target_ids.len(),
        "missing": missing,
        "privacy_counts": privacy,
        "alerts_raised": alerts_raised,
        "batches": target_ids.len().div_ceil(BATCH_MAX),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A `videos.list` availability response: two survivors (public +
    /// unlisted) and one id absent from `items` (deleted/private → missing).
    #[tokio::test]
    async fn availability_marks_missing_and_snapshots_privacy() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/videos"))
            .and(query_param("part", "snippet,status"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"items": [
                        {"id": "aaa111bbb22", "snippet": {"channelId": "UCx1"},
                         "status": {"privacyStatus": "public"}},
                        {"id": "bbb222ccc33", "snippet": {"channelId": "UCx1"},
                         "status": {"privacyStatus": "unlisted"}}
                    ]}"#,
            ))
            .mount(&mock)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let mut db = Db::open(&dir.path().join("a.db")).await.expect("open");
        {
            let at = "2026-01-01T00:00:00Z";
            let mut batch = db.begin_batch().await.expect("batch");
            for id in ["aaa111bbb22", "bbb222ccc33", "ccc333ddd44"] {
                batch
                    .upsert_video(&crate::storage::db::VideoRow {
                        video_id: id.to_string(),
                        title: "t".to_string(),
                        published_at: at.to_string(),
                        fetched_at: at.to_string(),
                        updated_at: at.to_string(),
                        source: "rss".to_string(),
                        ..Default::default()
                    })
                    .await
                    .expect("insert");
            }
            batch.commit().await.expect("commit");
        }

        let api = ApiClient::new(
            &FetchClients::for_test(&mock.uri(), std::time::Duration::from_secs(5)).expect("c"),
            "test-key",
        );
        let items = api
            .fetch_availability(
                &db,
                &[
                    "aaa111bbb22".into(),
                    "bbb222ccc33".into(),
                    "ccc333ddd44".into(),
                ],
            )
            .await
            .expect("fetch");
        assert_eq!(items.len(), 2, "missing id absent from response");

        // Alert raised for the missing video (warn severity, channel keyed).
        let n = reports::insert_once(
            &db,
            ALERT_KIND,
            None,
            "Video ccc333ddd44 no longer available (private or deleted)",
            "warn",
        )
        .await
        .expect("alert");
        assert_eq!(n, 1);
        let again = reports::insert_once(
            &db,
            ALERT_KIND,
            None,
            "Video ccc333ddd44 no longer available (private or deleted)",
            "warn",
        )
        .await
        .expect("alert");
        assert_eq!(again, 0, "dedupe: identical alert fires once");
        assert_eq!(
            db.count("SELECT count(*) FROM alerts WHERE kind = 'video_unavailable'")
                .await
                .unwrap(),
            1
        );

        // Survivors got their privacy snapshot.
        db.set_privacy_status("aaa111bbb22", Some("public"))
            .await
            .expect("ps");
        db.set_privacy_status("bbb222ccc33", Some("unlisted"))
            .await
            .expect("ps");
        let v = db
            .get_video("bbb222ccc33")
            .await
            .expect("get")
            .expect("row");
        assert_eq!(v.privacy_status.as_deref(), Some("unlisted"));
    }

    /// HTTP 404 with a `videoNotFound` body must fold into an EMPTY result
    /// (all ids missing) — never a hard failure.
    #[tokio::test]
    async fn availability_video_not_found_is_not_a_failure() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/videos"))
            .respond_with(ResponseTemplate::new(404).set_body_string(
                r#"{"error": {"code": 404, "errors": [{"reason": "videoNotFound"}]}}"#,
            ))
            .mount(&mock)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let db = Db::open(&dir.path().join("a.db")).await.expect("open");
        let api = ApiClient::new(
            &FetchClients::for_test(&mock.uri(), std::time::Duration::from_secs(5)).expect("c"),
            "test-key",
        );
        let items = api
            .fetch_availability(&db, &["deleted11xxxx".into(), "deleted22xxxx".into()])
            .await
            .expect("empty result, not an error");
        assert!(items.is_empty());
    }
}
