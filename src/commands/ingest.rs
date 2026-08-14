//! `ingest` (LLD §4.1): `ingest channels <ref...>` and `ingest links`.
//!
//! Input parsing (LLD §6.1) is real and tested (Phase 0 kept green); the
//! actual pipeline lives in `crate::ingest`.

use std::io::Read;

use serde_json::{json, Value};

use crate::config::Config;
use crate::error::TubeforgeError;
use crate::fetch::FetchClients;
use crate::ingest::{self, IngestOptions, IngestSummary};
use crate::storage::Db;

pub use crate::ingest::{
    extract_video_ids, parse_channel_ref, parse_input_items, parse_links_input,
    valid_channel_id_checksum, valid_video_id_checksum, ChannelRef, InputItem,
};

/// `ingest channels <ref...> [--api] [--no-backup]`
pub async fn run_channels(
    cfg: &Config,
    refs: &[String],
    use_api: bool,
    no_backup: bool,
) -> Result<Value, TubeforgeError> {
    let clients = FetchClients::new()?;
    let mut db = Db::open(&cfg.db_path).await?;
    let opts = IngestOptions { use_api, no_backup };
    let summary = ingest::ingest_channels(cfg, &clients, &mut db, refs, &opts).await?;
    // Incremental KG update after ingest (non-fatal — logs warning on failure)
    if let Err(e) = crate::analytics::kg_builder::build(
        &db,
        crate::analytics::kg_builder::BuildMode::Incremental,
    )
    .await
    {
        tracing::warn!(err = %e, "KG incremental update after channel ingest failed");
    }
    Ok(summary_json(&summary))
}

/// `ingest links [--file FILE|-] [--api] [--no-backup]` — reads multi-line
/// video URLs, extracts IDs (LLD §6.1, extended A2), then fetches metadata.
/// Per-item rejects (checksum-invalid bare ids, playlist/channel/handle
/// items) are labeled in `rejected` and `ingest_log`, never silently dropped
/// (A1).
pub async fn run_links(
    cfg: &Config,
    file: Option<String>,
    use_api: bool,
    no_backup: bool,
) -> Result<Value, TubeforgeError> {
    let raw = read_input(file.as_deref())?;
    let lines = parse_links_input(&raw);
    let items = parse_input_items(&lines.join("\n"));
    if items.is_empty() {
        return Err(TubeforgeError::Usage(
            "no video IDs found in input (expected watch?v=..., youtu.be/..., shorts/... links)"
                .into(),
        ));
    }
    let (ids, rejected) = partition_items(items);
    if ids.is_empty() {
        let found = rejected
            .iter()
            .map(|(item, detail)| format!("{item} ({detail})"))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(TubeforgeError::Usage(format!(
            "no video IDs found in input (expected watch?v=..., youtu.be/..., shorts/... links) — found: {found}"
        )));
    }
    tracing::info!(count = ids.len(), "extracted video ids from input");

    let clients = FetchClients::new()?;
    let mut db = Db::open(&cfg.db_path).await?;
    let opts = IngestOptions { use_api, no_backup };
    let mut summary = ingest::ingest_links(cfg, &clients, &mut db, &ids, &opts).await?;
    let rejected_videos = rejected
        .iter()
        .filter(|(item, _)| item.starts_with("video "))
        .count() as u64;
    summary.rejected = rejected;
    summary.videos_failed += rejected_videos;
    ingest::record_invalid_items(&mut db, &summary.batch_id, &summary.rejected).await?;
    // Incremental KG update after ingest (non-fatal — logs warning on failure)
    if let Err(e) = crate::analytics::kg_builder::build(
        &db,
        crate::analytics::kg_builder::BuildMode::Incremental,
    )
    .await
    {
        tracing::warn!(err = %e, "KG incremental update after links ingest failed");
    }
    Ok(summary_json(&summary))
}

/// Split parsed items into ingestable video ids and per-item rejects (A1):
/// bare ids must pass the checksum (URL captures are authoritative); other
/// kinds are labeled, not silently dropped.
fn partition_items(items: Vec<InputItem>) -> (Vec<String>, Vec<(String, String)>) {
    let mut ids = Vec::new();
    let mut rejected = Vec::new();
    for item in items {
        match item {
            InputItem::VideoUrl(id) => push_unique(&mut ids, id),
            InputItem::VideoBare(id) if valid_video_id_checksum(&id) => push_unique(&mut ids, id),
            InputItem::VideoBare(id) => {
                rejected.push((format!("video {id}"), "invalid id (checksum)".to_string()));
            }
            InputItem::Playlist(id) => rejected.push((
                format!("playlist {id}"),
                "playlist expansion not supported — ingest the channel instead".to_string(),
            )),
            InputItem::ChannelUrl(id) => {
                rejected.push((format!("channel {id}"), "use `ingest channels`".to_string()));
            }
            InputItem::ChannelBare(id) => {
                let detail = if valid_channel_id_checksum(&id) {
                    "use `ingest channels`"
                } else {
                    "invalid channel id (checksum)"
                };
                rejected.push((format!("channel {id}"), detail.to_string()));
            }
            InputItem::Handle(h) => {
                rejected.push((format!("handle {h}"), "use `ingest channels`".to_string()));
            }
            InputItem::Custom(c) => {
                rejected.push((format!("channel {c}"), "use `ingest channels`".to_string()));
            }
        }
    }
    (ids, rejected)
}

fn push_unique(ids: &mut Vec<String>, id: String) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

pub(crate) fn summary_json(s: &IngestSummary) -> Value {
    json!({
        "batch_id": s.batch_id,
        "channels": {
            "added": s.channels_added,
            "updated": s.channels_updated,
            "skipped": s.channels_skipped,
            "failed": s.channels_failed,
        },
        "videos": {
            "added": s.videos_added,
            "updated": s.videos_updated,
            "skipped": s.videos_skipped,
            "failed": s.videos_failed,
        },
        "api": s.api,
        "snapshot": s.snapshot.as_ref().map(|p| p.to_string_lossy().to_string()),
        "alerts": s.alerts,
        "rejected": s
            .rejected
            .iter()
            .map(|(item, detail)| json!({"item": item, "detail": detail}))
            .collect::<Vec<Value>>(),
    })
}

fn read_input(file: Option<&str>) -> Result<String, TubeforgeError> {
    match file {
        None | Some("-") => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| TubeforgeError::Config(format!("read stdin: {e}")))?;
            Ok(buf)
        }
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| TubeforgeError::Config(format!("read input file {path}: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use crate::ingest::{
        extract_video_ids, parse_channel_ref, parse_input_items, parse_links_input,
        valid_channel_id_checksum, valid_video_id_checksum, ChannelRef, InputItem,
    };

    fn ids(input: &str) -> Vec<String> {
        extract_video_ids(input)
    }

    // -- A1 checksum tables ---------------------------------------------------

    #[test]
    fn checksum_accepts_valid_last_chars() {
        // Real id: rickroll ends 'Q' (in [AEIMQUYcgkosw048]).
        assert!(valid_video_id_checksum("dQw4w9WgXcQ"));
        for c in [
            'A', 'E', 'I', 'M', 'Q', 'U', 'Y', 'c', 'g', 'k', 'o', 's', 'w', '0', '4', '8',
        ] {
            assert!(
                valid_video_id_checksum(&format!("aaaaaaaaaa{c}")),
                "last char {c} valid"
            );
        }
        assert!(
            valid_channel_id_checksum("UCa1b2c3d4e5f6g7h8i9j0kQ"),
            "canonical UC+22"
        );
        assert!(
            valid_channel_id_checksum("a1b2c3d4e5f6g7h8i9j0kQ"),
            "bare 22-char legacy"
        );
    }

    #[test]
    fn checksum_rejects_bad_last_chars_and_lengths() {
        for c in ['B', 'D', 'b', '9', '1', 'X'] {
            assert!(
                !valid_video_id_checksum(&format!("aaaaaaaaaa{c}")),
                "last char {c} invalid"
            );
        }
        // Fixture-style synthetic ids (tests use them; not real) fail too.
        assert!(!valid_video_id_checksum("aaaaaaaaaaa"));
        assert!(!valid_video_id_checksum("aaaaaaaaaa")); // 10 chars
        assert!(!valid_video_id_checksum("aaaaaaaaaaaa")); // 12 chars
        assert!(!valid_video_id_checksum("aaaaaaaaaa!")); // non-id char
        assert!(
            !valid_channel_id_checksum("UCa1b2c3d4e5f6g7h8i9j0kM"),
            "24-char id ending 'M'"
        );
        assert!(
            !valid_channel_id_checksum("UCa1b2c3d4e5f6g7h8i9j0kL"),
            "24-char id ending 'L'"
        );
        assert!(
            !valid_channel_id_checksum("a1b2c3d4e5f6g7h8i9j0kM"),
            "bare 22-char ending 'M'"
        );
    }

    // -- A2 URL forms ---------------------------------------------------------

    #[test]
    fn new_url_forms_parse() {
        for (url, want) in [
            ("https://www.youtube.com/v/7lCDEYXw3mM", "7lCDEYXw3mM"),
            ("https://www.youtube.com/embed/8Ab_2vG4TkM", "8Ab_2vG4TkM"),
            ("https://www.youtube.com/video/8Ab_2vG4TkM", "8Ab_2vG4TkM"),
            ("https://www.youtube.com/watch/8Ab_2vG4TkM", "8Ab_2vG4TkM"),
            ("https://www.youtube.com/live/8Ab_2vG4TkM", "8Ab_2vG4TkM"),
            ("https://youtu.be/8Ab_2vG4TkM?t=30", "8Ab_2vG4TkM"),
            (
                "https://www.youtube.com/watch?v=7lCDEYXw3mM&list=PLabc",
                "7lCDEYXw3mM",
            ),
        ] {
            assert_eq!(ids(url), vec![want.to_string()], "{url}");
        }
    }

    #[test]
    fn bare_11_char_ids_are_checksum_filtered() {
        assert_eq!(ids("dQw4w9WgXcQ"), vec!["dQw4w9WgXcQ"]);
        assert!(
            ids("dQw4w9WgXcB").is_empty(),
            "bad-checksum bare id rejected"
        );
        // Bare + URL mixed in one input, deduped in order.
        assert_eq!(
            ids("dQw4w9WgXcQ\nyoutu.be/dQw4w9WgXcQ"),
            vec!["dQw4w9WgXcQ"]
        );
    }

    #[test]
    fn typed_items_playlist_channel_handle_custom() {
        use InputItem as I;
        assert_eq!(
            parse_input_items("https://youtube.com/playlist?list=PLabc123"),
            vec![I::Playlist("PLabc123".into())]
        );
        assert_eq!(
            parse_input_items("PLabc123xyz123"),
            vec![I::Playlist("PLabc123xyz123".into())]
        );
        assert_eq!(
            parse_input_items("https://youtube.com/channel/UCa1b2c3d4e5f6g7h8i9j0kQ"),
            vec![I::ChannelUrl("UCa1b2c3d4e5f6g7h8i9j0kQ".into())]
        );
        assert_eq!(
            parse_input_items("SCa1b2c3d4e5f6g7h8i9j0kM"),
            vec![I::ChannelBare("UCa1b2c3d4e5f6g7h8i9j0kM".into())]
        );
        assert_eq!(
            parse_input_items("https://youtube.com/@weird_name"),
            vec![I::Handle("@weird_name".into())]
        );
        assert_eq!(
            parse_input_items("@handle"),
            vec![I::Handle("@handle".into())]
        );
        assert_eq!(
            parse_input_items("https://youtube.com/c/MyCustomName"),
            vec![I::Custom("MyCustomName".into())]
        );
        assert_eq!(
            parse_input_items("https://youtube.com/user/LegacyUser"),
            vec![I::Custom("LegacyUser".into())]
        );
    }

    // -- A2 SC→UC transform ---------------------------------------------------

    #[test]
    fn sc_channel_ids_transform_to_uc() {
        let bare = parse_channel_ref("SCa1b2c3d4e5f6g7h8i9j0kM").expect("bare SC id");
        assert_eq!(
            bare,
            ChannelRef::Direct("UCa1b2c3d4e5f6g7h8i9j0kM".to_string())
        );
        let show = parse_channel_ref("https://youtube.com/show/SCa1b2c3d4e5f6g7h8i9j0kM")
            .expect("show URL");
        assert_eq!(
            show,
            ChannelRef::Direct("UCa1b2c3d4e5f6g7h8i9j0kM".to_string())
        );
        // UC ids pass through untouched; user/c forms resolve as handles.
        assert_eq!(
            parse_channel_ref("UCa1b2c3d4e5f6g7h8i9j0kQ").expect("uc"),
            ChannelRef::Direct("UCa1b2c3d4e5f6g7h8i9j0kQ".to_string())
        );
        assert_eq!(
            parse_channel_ref("https://youtube.com/user/LegacyUser").expect("user"),
            ChannelRef::Handle("@LegacyUser".to_string())
        );
        assert_eq!(
            parse_channel_ref("https://youtube.com/c/MyCustomName").expect("custom"),
            ChannelRef::Handle("@MyCustomName".to_string())
        );
    }

    // -- existing contract ----------------------------------------------------

    #[test]
    fn watch_url() {
        assert_eq!(
            ids("https://www.youtube.com/watch?v=7lCDEYXw3mM"),
            vec!["7lCDEYXw3mM"]
        );
    }

    #[test]
    fn shorts_url() {
        assert_eq!(
            ids("https://youtube.com/shorts/8Ab_2vG4TkM?si=abc"),
            vec!["8Ab_2vG4TkM"]
        );
    }

    #[test]
    fn youtu_be_short_url() {
        assert_eq!(ids("https://youtu.be/dQw4w9WgXcQ"), vec!["dQw4w9WgXcQ"]);
    }

    #[test]
    fn captures_are_11_chars_in_class() {
        // 11 chars of [A-Za-z0-9_-], then trailing junk is NOT captured.
        assert_eq!(ids("youtu.be/ABCDEFGHIJK_extra"), vec!["ABCDEFGHIJK"]);
    }

    #[test]
    fn non_id_chars_fail_and_scan_continues() {
        // Marker followed by a non-class char must not match; a later valid
        // marker still matches (leftmost-match semantics).
        let input = "youtu.be/ABC!!v=7lCDEYXw3mM";
        assert_eq!(ids(input), vec!["7lCDEYXw3mM"]);
    }

    #[test]
    fn multiple_ids_deduped_in_order() {
        let input = "https://youtu.be/aaaaaaaaaaa\nhttps://youtu.be/aaaaaaaaaaa\nhttps://youtu.be/bbbbbbbbbbb";
        assert_eq!(ids(input), vec!["aaaaaaaaaaa", "bbbbbbbbbbb"]);
    }

    #[test]
    fn no_match() {
        assert!(ids("https://example.com/not-a-youtube-link").is_empty());
        // Too short / wrong class at capture.
        assert!(ids("youtu.be/ab").is_empty());
        assert!(ids("v=???????????").is_empty());
    }

    #[test]
    fn parse_input_blank_lines_and_comments() {
        let input = "# a comment line\n\nhttps://youtu.be/aaaaaaaaaaa # trailing comment\n\nhttps://youtu.be/bbbbbbbbbbb\n";
        assert_eq!(
            parse_links_input(input),
            vec![
                "https://youtu.be/aaaaaaaaaaa",
                "https://youtu.be/bbbbbbbbbbb"
            ]
        );
    }

    #[test]
    fn partition_items_labels_rejects() {
        let (ids, rejected) = super::partition_items(vec![
            InputItem::VideoUrl("aaa111bbb22".into()),
            InputItem::VideoBare("dQw4w9WgXcB".into()), // bad checksum
            InputItem::VideoBare("dQw4w9WgXcQ".into()), // good checksum
            InputItem::Playlist("PLabc".into()),
            InputItem::ChannelUrl("UCa1b2c3d4e5f6g7h8i9j0kLM".into()),
            InputItem::Handle("@handle".into()),
        ]);
        assert_eq!(ids, vec!["aaa111bbb22", "dQw4w9WgXcQ"]);
        assert_eq!(rejected.len(), 4, "every non-video/invalid item labeled");
        assert_eq!(rejected[0].0, "video dQw4w9WgXcB");
        assert!(rejected[0].1.contains("checksum"));
        assert_eq!(rejected[1].0, "playlist PLabc");
        assert_eq!(rejected[2].0, "channel UCa1b2c3d4e5f6g7h8i9j0kLM");
        assert_eq!(rejected[3].0, "handle @handle");
    }
}
