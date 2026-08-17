//! tfdb schema for the TubeForge domain model (LLD §3.1).
//!
//! Defines the same 22 tables as the legacy SQL schema, but as typed tfdb
//! `TableSchema` definitions. The tfdb engine has no SQL — queries (joins,
//! aggregates, ordering) are expressed in Rust over `Engine::all`/`get`/
//! `find_eq` scans, which is fine at the ~10k-video corpus scale (HLD §10).
//!
//! Column storage convention: every column is stored as its natural tfdb
//! `Value` (Text/Int/Float/Bool/Json/Blob). The `videos.embedding` column is
//! Blob; JSON columns (tags, topic_categories, rationale, top_entities,
//! suggested_tags, related_keywords) are stored as `Value::Json`.

use crate::tfdb::TableSchema;

/// Build the full set of domain table schemas, keyed by table name.
pub fn all() -> Vec<TableSchema> {
    vec![
        TableSchema::new("meta", "key").text("value"),
        TableSchema::new("channels", "channel_id")
            .text("handle")
            .text("title")
            .text("description")
            .text("avatar_url")
            .text("country")
            .int("subscriber_count")
            .int("video_count")
            .text("source")
            .text("etag")
            .text("fetched_at")
            .text("updated_at"),
        TableSchema::new("videos", "video_id")
            .text("channel_id")
            .text("title")
            .text("description")
            .json("tags")
            .text("category_id")
            .int("duration_sec")
            .text("published_at")
            .int("view_count")
            .int("like_count")
            .int("comment_count")
            .text("thumb_url")
            .blob("embedding")
            .text("source")
            .text("fetched_at")
            .text("updated_at")
            .text("recording_date")
            .text("recording_location_name")
            .float("recording_lat")
            .float("recording_lng")
            .json("topic_categories")
            .text("privacy_status"),
        TableSchema::new("competitors", "channel_id")
            .text("label")
            .text("added_at"),
        TableSchema::new("keywords", "keyword")
            .text("niche")
            .text("created_at"),
        TableSchema::new("keyword_rankings", "keyword_checked_at")
            .text("keyword")
            .text("checked_at")
            .text("video_id")
            .int("position")
            .json("topics"),
        TableSchema::new("scores", "video_id")
            .float("seo_score")
            .float("geo_score")
            .float("total_score")
            .json("components")
            .text("computed_at"),
        TableSchema::new("ideas", "idea_id")
            .text("title_suggestion")
            .json("rationale")
            .float("score")
            .text("status")
            .text("source_video")
            .text("created_at"),
        TableSchema::new("edges", "from_to")
            .text("from_channel")
            .text("to_channel")
            .float("weight")
            .text("source"),
        TableSchema::new("alerts", "alert_id")
            .text("kind")
            .text("channel_id")
            .text("message")
            .text("severity")
            .text("created_at")
            .text("read_at"),
        TableSchema::new("ingest_log", "log_id")
            .text("batch_id")
            .text("item")
            .text("status")
            .text("detail")
            .text("at"),
        TableSchema::new("tags", "tag_id").text("name"),
        TableSchema::new("video_tags", "video_tag_id")
            .text("video_id")
            .int("tag_id")
            .int("position")
            .text("source"),
        TableSchema::new("competitor_tags", "channel_tag")
            .text("channel_id")
            .text("tag_name")
            .int("video_count")
            .float("avg_views")
            .int("rank"),
        TableSchema::new("transcripts", "video_id")
            .text("lang")
            .text("source")
            .text("text")
            .int("word_count")
            .text("fetched_at"),
        TableSchema::new("comments", "comment_id")
            .text("video_id")
            .text("author")
            .text("text")
            .int("like_count")
            .text("published_at")
            .text("fetched_at"),
        TableSchema::new("video_heatmap", "video_id")
            .json("points")
            .text("fetched_at"),
        TableSchema::new("channel_snapshots", "channel_at")
            .text("channel_id")
            .text("at")
            .int("subscriber_count")
            .int("video_count")
            .int("total_views"),
        TableSchema::new("keyword_research", "research_id")
            .text("keyword")
            .text("at")
            .text("volume_label")
            .int("serp_total")
            .float("serp_mean_views")
            .int("ranking_channels")
            .float("competition_score")
            .float("opportunity_score")
            .int("actively_published")
            .json("suggested_tags")
            .json("related_keywords"),
        TableSchema::new("kg_entities", "entity_id")
            .text("entity_type")
            .text("canonical_name")
            .text("display_name")
            .json("properties")
            .blob("embedding")
            .float("centrality")
            .int("community_id")
            .text("source")
            .text("source_ref")
            .text("created_at")
            .text("updated_at"),
        TableSchema::new("kg_relations", "relation_id")
            .text("from_entity")
            .text("to_entity")
            .text("relation_type")
            .float("weight")
            .text("source")
            .text("created_at"),
        TableSchema::new("kg_communities", "community_id")
            .text("community_type")
            .text("summary")
            .int("member_count")
            .float("mean_views")
            .float("mean_seo_score")
            .json("top_entities")
            .text("created_at")
            .text("updated_at"),
        // -- greedy bot tables --
        TableSchema::new("greedy_seeds", "seed_id")
            .text("seed")
            .text("source")
            .text("added_at")
            .boolean("active"),
        TableSchema::new("greedy_research_history", "research_id")
            .text("topic")
            .text("researched_at")
            .text("video_ids_json")
            .int("video_count")
            .float("mean_views")
            .text("source")
            .text("duration_ms"),
        TableSchema::new("greedy_topic_log", "log_id")
            .text("topic")
            .text("status")
            .text("reason")
            .text("attempted_at"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_has_a_primary_key_and_unique_names() {
        let schemas = all();
        let mut names = std::collections::HashSet::new();
        for s in &schemas {
            assert!(!s.pk.is_empty(), "{} has a pk", s.name);
            assert!(names.insert(s.name.clone()), "duplicate table {}", s.name);
            // Every column name is unique within the table.
            let mut cols = std::collections::HashSet::new();
            for c in &s.cols {
                assert!(
                    cols.insert(c.name.clone()),
                    "dup col {} in {}",
                    c.name,
                    s.name
                );
            }
        }
    }

    #[test]
    fn core_tables_cover_the_domain() {
        let schemas = all();
        let names: Vec<String> = schemas.iter().map(|s| s.name.clone()).collect();
        for required in [
            "videos",
            "channels",
            "scores",
            "keywords",
            "keyword_rankings",
            "ideas",
            "alerts",
            "edges",
            "meta",
            "kg_entities",
            "kg_relations",
            "kg_communities",
            "tags",
            "video_tags",
            "competitor_tags",
            "transcripts",
            "comments",
            "video_heatmap",
            "channel_snapshots",
            "keyword_research",
            "competitors",
            "ingest_log",
        ] {
            assert!(
                names.iter().any(|n| n == required),
                "missing table {required}"
            );
        }
        assert_eq!(schemas.len(), 25);
    }
}
