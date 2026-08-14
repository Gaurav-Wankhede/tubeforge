//! GEO components (LLD §7.3) — free public signals only: entity coverage,
//! Q&A phrasing, list phrasing, conversational tone with a keyword-density
//! ceiling penalty, metadata completeness, plus the two free API metadata
//! signals `location_signal` (recordingDetails, C1) and `topic_relevance`
//! (topicDetails, C2). All deterministic.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::util;

/// The five W-entities checked by `entity_coverage` (who/what/when/where/how).
const ENTITIES: [&str; 5] = ["who", "what", "when", "where", "how"];

/// Casual/conversational markers for the density-ceiling floor.
const TONE_MARKERS: [&str; 7] = [
    "you",
    "your",
    "we",
    "just",
    "really",
    "actually",
    "basically",
];

/// The seven GEO components as computed by `compute`.
#[derive(Debug, Clone)]
pub struct GeoComponents {
    pub entity_coverage: f64,
    pub qa_phrasing: f64,
    pub list_phrasing: f64,
    pub conversational: f64,
    pub metadata_complete: f64,
    /// C1: recorded filming location + "recorded near publish" bonus.
    pub location_signal: f64,
    /// C2: topic labels ∩ target-keyword tokens (Jaccard × 100).
    pub topic_relevance: f64,
}

impl GeoComponents {
    /// (key, value) pairs in canonical order — the weighted-sum input.
    pub fn values(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("entity_coverage", self.entity_coverage),
            ("qa_phrasing", self.qa_phrasing),
            ("list_phrasing", self.list_phrasing),
            ("conversational", self.conversational),
            ("metadata_complete", self.metadata_complete),
            ("location_signal", self.location_signal),
            ("topic_relevance", self.topic_relevance),
        ]
    }

    pub fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        for (k, v) in self.values() {
            m.insert(k.to_string(), json!(round4(v)));
        }
        Value::Object(m)
    }
}

/// Free metadata a stored video carries for the C1/C2 signals (recording
/// details + topic categories). Drafts pass `GeoMeta::default()` (both
/// signals score 0 — there is no metadata to signal with).
#[derive(Debug, Clone, Default)]
pub struct GeoMeta {
    pub published_at: String,
    pub recording_date: Option<String>,
    pub recording_location_name: Option<String>,
    pub recording_lat: Option<f64>,
    pub recording_lng: Option<f64>,
    /// `topicDetails.topicCategories` — Wikipedia category URLs.
    pub topic_categories: Vec<String>,
}

/// Full GEO pass over description + tags + target keywords + video metadata.
pub fn compute(desc: &str, tags: &[String], keywords: &[String], meta: &GeoMeta) -> GeoComponents {
    GeoComponents {
        entity_coverage: entity_coverage_score(desc),
        qa_phrasing: qa_phrasing_score(desc),
        list_phrasing: list_phrasing_score(desc),
        conversational: conversational_score(desc, keywords),
        metadata_complete: metadata_complete_score(desc, tags),
        location_signal: location_signal_score(meta),
        topic_relevance: topic_relevance_score(&meta.topic_categories, keywords),
    }
}

/// §7.3 `entity_coverage`: +20 per W-entity token present in the description.
pub fn entity_coverage_score(desc: &str) -> f64 {
    let words: HashSet<String> = util::tokens(desc).into_iter().collect();
    ENTITIES.iter().filter(|e| words.contains(**e)).count() as f64 * 20.0
}

/// §7.3 `qa_phrasing`: question-style lines — 0 → 0, 1 → 60, ≥2 → 100.
pub fn qa_phrasing_score(desc: &str) -> f64 {
    let questions = desc
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && l.ends_with('?'))
        .count();
    match questions {
        0 => 0.0,
        1 => 60.0,
        _ => 100.0,
    }
}

/// §7.3 `list_phrasing`: ≥2 bullet lines → +40, numbered lines → +30,
/// "step"/"steps" mention → +30.
pub fn list_phrasing_score(desc: &str) -> f64 {
    let lines: Vec<&str> = desc
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let bullets = lines
        .iter()
        .filter(|l| l.starts_with('-') || l.starts_with('*') || l.starts_with('•'))
        .count();
    let numbered = lines
        .iter()
        .filter(|l| l.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .count();
    let mut score = 0.0f64;
    if bullets >= 2 {
        score += 40.0;
    } else if bullets == 1 {
        score += 20.0;
    }
    if numbered >= 1 {
        score += 30.0;
    }
    let words: HashSet<String> = util::tokens(desc).into_iter().collect();
    if words.contains("step") || words.contains("steps") {
        score += 30.0;
    }
    score.min(100.0)
}

/// §7.3 `conversational`: natural tone vs keyword-stuffing. Density of
/// keyword tokens in the description drives the ceiling penalty; a genuine
/// conversational register (≥2 tone markers) floors the score at 80.
/// An empty description is neither natural nor stuffed: 20.
pub fn conversational_score(desc: &str, keywords: &[String]) -> f64 {
    let desc_tokens = util::tokens(desc);
    if desc_tokens.is_empty() {
        return 20.0;
    }
    let kw_tokens: Vec<String> = keywords.iter().flat_map(|k| util::tokens(k)).collect();
    let hits = if kw_tokens.is_empty() {
        0
    } else {
        desc_tokens.iter().filter(|t| kw_tokens.contains(t)).count()
    };
    let density = hits as f64 / desc_tokens.len() as f64;
    let penalty = if density <= 0.02 {
        0.0
    } else if density <= 0.05 {
        20.0
    } else if density <= 0.10 {
        50.0
    } else {
        80.0
    };
    let mut score = 100.0f64 - penalty;
    let markers = desc_tokens
        .iter()
        .filter(|t| TONE_MARKERS.contains(&t.as_str()))
        .count();
    if markers >= 2 {
        score = score.max(80.0);
    }
    score
}

/// §7.3 `metadata_complete`: non-empty description → +35, tags present → +35,
/// timestamps (mm:ss) in the description → +30.
pub fn metadata_complete_score(desc: &str, tags: &[String]) -> f64 {
    let mut score = 0.0f64;
    if !desc.trim().is_empty() {
        score += 35.0;
    }
    if !tags.is_empty() {
        score += 35.0;
    }
    let chars: Vec<char> = desc.chars().collect();
    if chars
        .windows(2)
        .any(|w| w[0].is_ascii_digit() && w[1] == ':')
    {
        score += 30.0;
    }
    score
}

/// Engagement-metric completeness (A4, consumed by the health report): a
/// missing `view_count`/`like_count`/`comment_count` on an API/oEmbed row
/// means the metric was DISABLED at fetch time (deliberate uploader choice)
/// and does not penalize completeness; a missing count on an RSS-only row is
/// genuinely unknown and does. Each of the three metrics contributes one
/// third; 0..100.
pub fn engagement_completeness(
    source: &str,
    view_count: Option<i64>,
    like_count: Option<i64>,
    comment_count: Option<i64>,
) -> f64 {
    let rich_source = source == "api" || source == "oembed";
    let known = |v: Option<i64>| v.is_some() || (rich_source && v.is_none());
    let known_count = [view_count, like_count, comment_count]
        .into_iter()
        .filter(|v| known(*v))
        .count();
    (known_count as f64 / 3.0) * 100.0
}

/// §7.3 `location_signal` (C1, `recordingDetails`): a recorded filming
/// location is the core spatial GEO signal; a recording date within ~7 days
/// of `publishedAt` ("recorded near publish", per MW Metadata) is a temporal
/// bonus.
///
/// Formula (deterministic):
/// ```text
/// has_location = (lat AND lng) OR non-empty location name
/// score = has_location ? 70 : 0
///       + (recording_date present AND |days(recording_date - published_at)| <= 7
///          AND has_location ? 30 : 0)
/// ```
/// Neither → 0; the date alone carries no score — the bonus only modifies
/// the location signal. An unparseable recording date adds no bonus.
pub fn location_signal_score(meta: &GeoMeta) -> f64 {
    let has_location = (meta.recording_lat.is_some() && meta.recording_lng.is_some())
        || meta
            .recording_location_name
            .as_deref()
            .is_some_and(|n| !n.trim().is_empty());
    if !has_location {
        return 0.0;
    }
    let near_publish = meta
        .recording_date
        .as_deref()
        .map(|d| days_between(&meta.published_at, d) <= 7.0)
        .unwrap_or(false);
    70.0 + if near_publish { 30.0 } else { 0.0 }
}

/// §7.3 `topic_relevance` (C2, `topicDetails`): YouTube topic labels vs the
/// target-keyword tokens, Jaccard overlap × 100.
///
/// Formula (deterministic):
/// ```text
/// T = token set over topic_labels(topicCategories)   (shared tokenizer)
/// K = token set over target keywords
/// score = (T or K empty) ? 0 : |T ∩ K| / |T ∪ K| × 100
/// ```
pub fn topic_relevance_score(topic_urls: &[String], keywords: &[String]) -> f64 {
    let topics: HashSet<String> = topic_labels(topic_urls)
        .iter()
        .flat_map(|l| util::tokens(l))
        .collect();
    let kw: HashSet<String> = keywords.iter().flat_map(|k| util::tokens(k)).collect();
    if topics.is_empty() || kw.is_empty() {
        return 0.0;
    }
    let union_len = topics.union(&kw).count();
    if union_len == 0 {
        return 0.0;
    }
    let overlap = topics.intersection(&kw).count();
    (overlap as f64 / union_len as f64) * 100.0
}

/// Derive readable topic labels from YouTube `topicCategories` URLs: the
/// last path segment with `_` → space (same derivation as MW Metadata's
/// topicDetails handler). Labels are derived at read time — never stored.
pub fn topic_labels(urls: &[String]) -> Vec<String> {
    urls.iter()
        .filter_map(|u| u.split('/').next_back())
        .map(|seg| seg.replace('_', " "))
        .filter(|s| !s.trim().is_empty())
        .collect()
}

/// Whole-day difference between two RFC3339 timestamps; `f64::MAX` when
/// either side is unparseable (never a bonus).
fn days_between(a: &str, b: &str) -> f64 {
    let (Ok(da), Ok(db)) = (
        DateTime::parse_from_rfc3339(a).map(|d| d.with_timezone(&Utc)),
        DateTime::parse_from_rfc3339(b).map(|d| d.with_timezone(&Utc)),
    ) else {
        return f64::MAX;
    };
    (db - da).num_days().abs() as f64
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_coverage_golden_vectors() {
        assert_eq!(entity_coverage_score(""), 0.0);
        let full = "who it is for, what it does, when it ships, where to get it, how to use it";
        assert_eq!(entity_coverage_score(full), 100.0);
        assert_eq!(entity_coverage_score("what and how only"), 40.0);
    }

    #[test]
    fn qa_phrasing_golden_vectors() {
        assert_eq!(qa_phrasing_score("plain description."), 0.0);
        assert_eq!(qa_phrasing_score("What is this?"), 60.0);
        assert_eq!(qa_phrasing_score("What is this?\nHow does it work?"), 100.0);
        // A question mark inside a line does not count as a heading.
        assert_eq!(qa_phrasing_score("really? no."), 0.0);
    }

    #[test]
    fn list_phrasing_golden_vectors() {
        assert_eq!(list_phrasing_score("no lists here"), 0.0);
        assert_eq!(list_phrasing_score("- a\n- b"), 40.0);
        assert_eq!(list_phrasing_score("1. first\n2. second"), 30.0);
        assert_eq!(
            list_phrasing_score("- a\n- b\n1. first\nFollow the steps"),
            100.0
        );
    }

    #[test]
    fn conversational_golden_vectors() {
        let kw = |s: &str| vec![s.to_string()];
        assert_eq!(conversational_score("", &kw("database")), 20.0);
        // Sparse keyword use → no penalty.
        assert_eq!(
            conversational_score(
                "a plain natural sentence without keywords at all",
                &kw("database")
            ),
            100.0
        );
        // Keyword stuffing → density ceiling penalty.
        let stuffed = "database database database database database database database database \
                       database database database database database database database database \
                       database database database database";
        assert_eq!(conversational_score(stuffed, &kw("database")), 20.0);
        // Natural register floors the ceiling penalty.
        let dense_but_tone = "you know we just use the database here, actually the database \
                              is fine, really, database works, just use the database";
        assert_eq!(conversational_score(dense_but_tone, &kw("database")), 80.0);
    }

    #[test]
    fn metadata_complete_golden_vectors() {
        assert_eq!(metadata_complete_score("", &[]), 0.0);
        assert_eq!(metadata_complete_score("desc only", &[]), 35.0);
        assert_eq!(
            metadata_complete_score("desc with 1:23 timestamps", &["t".to_string()]),
            100.0
        );
        assert_eq!(
            metadata_complete_score("desc https://example.com", &["t".to_string()]),
            70.0
        );
    }

    #[test]
    fn engagement_disabled_vs_unknown() {
        let approx = |got: f64, want: f64| (got - want).abs() < 1e-9;
        // API/oEmbed rows with a field absent = DISABLED → no penalty.
        assert_eq!(engagement_completeness("api", None, None, None), 100.0);
        assert_eq!(
            engagement_completeness("oembed", Some(1), None, Some(2)),
            100.0
        );
        // RSS rows with a field absent = genuinely unknown → penalized.
        assert!(approx(
            engagement_completeness("rss", Some(1), None, Some(2)),
            200.0 / 3.0
        ));
        assert_eq!(engagement_completeness("rss", None, None, None), 0.0);
        // Present counts are complete regardless of source.
        assert_eq!(
            engagement_completeness("rss", Some(1), Some(2), Some(3)),
            100.0
        );
        // Same row shape: api (like disabled) = 100, rss (like unknown) = 2/3.
        assert_eq!(
            engagement_completeness("api", Some(1), None, Some(3)),
            100.0
        );
        assert!(approx(
            engagement_completeness("rss", Some(1), None, Some(3)),
            200.0 / 3.0
        ));
    }

    fn meta(
        published_at: &str,
        rec_date: Option<&str>,
        name: Option<&str>,
        lat: Option<f64>,
        lng: Option<f64>,
    ) -> GeoMeta {
        GeoMeta {
            published_at: published_at.to_string(),
            recording_date: rec_date.map(|s| s.to_string()),
            recording_location_name: name.map(|s| s.to_string()),
            recording_lat: lat,
            recording_lng: lng,
            topic_categories: Vec::new(),
        }
    }

    /// C1 golden vectors: present+near → 100, present+far → 70, absent → 0;
    /// the date alone (no location) scores 0 — the bonus only modifies the
    /// location signal.
    #[test]
    fn location_signal_golden_vectors() {
        let near = meta(
            "2026-07-15T10:00:00Z",
            Some("2026-07-10T00:00:00Z"),
            Some("Googleplex"),
            Some(37.422),
            Some(-122.084),
        );
        assert_eq!(
            location_signal_score(&near),
            100.0,
            "present + near publish"
        );
        let far = meta(
            "2026-07-15T10:00:00Z",
            Some("2026-05-16T00:00:00Z"),
            None,
            Some(37.422),
            Some(-122.084),
        );
        assert_eq!(location_signal_score(&far), 70.0, "present + far publish");
        let name_only = meta("2026-07-15T10:00:00Z", None, Some("Berlin"), None, None);
        assert_eq!(
            location_signal_score(&name_only),
            70.0,
            "location name alone"
        );
        let unparseable = meta(
            "2026-07-15T10:00:00Z",
            Some("not-a-date"),
            Some("Berlin"),
            None,
            None,
        );
        assert_eq!(
            location_signal_score(&unparseable),
            70.0,
            "unparseable date → no bonus"
        );
        let absent = meta("2026-07-15T10:00:00Z", None, None, None, None);
        assert_eq!(location_signal_score(&absent), 0.0, "absent");
        let date_only = meta(
            "2026-07-15T10:00:00Z",
            Some("2026-07-10T00:00:00Z"),
            None,
            None,
            None,
        );
        assert_eq!(
            location_signal_score(&date_only),
            0.0,
            "date alone is not a location signal"
        );
    }

    /// C2 golden vector: full Jaccard overlap → 100, disjoint → 0, partial
    /// → 50, no topics → 0; underscore labels tokenize as separate words.
    #[test]
    fn topic_relevance_golden_vector() {
        let urls = |s: &[&str]| s.iter().map(|u| u.to_string()).collect::<Vec<String>>();
        let kw = |s: &str| vec![s.to_string()];
        let ai = "https://en.wikipedia.org/wiki/Artificial_intelligence";
        assert_eq!(
            topic_relevance_score(&urls(&[ai]), &kw("artificial intelligence")),
            100.0
        );
        assert_eq!(
            topic_relevance_score(&urls(&[ai]), &kw("rust database")),
            0.0
        );
        assert_eq!(topic_relevance_score(&urls(&[ai]), &kw("artificial")), 50.0);
        assert_eq!(topic_relevance_score(&[], &kw("ai")), 0.0, "no topics");
        assert_eq!(topic_relevance_score(&urls(&[ai]), &[]), 0.0, "no keywords");
        let dl = "https://en.wikipedia.org/wiki/Deep_learning";
        assert_eq!(
            topic_relevance_score(&urls(&[dl]), &kw("deep learning")),
            100.0
        );
    }

    #[test]
    fn topic_labels_derive_last_segment() {
        assert_eq!(
            topic_labels(&["https://en.wikipedia.org/wiki/Artificial_intelligence".to_string()]),
            vec!["Artificial intelligence".to_string()]
        );
        assert_eq!(
            topic_labels(&["https://en.wikipedia.org/wiki/Deep_learning".to_string()]),
            vec!["Deep learning".to_string()]
        );
        assert!(topic_labels(&[]).is_empty());
    }
}
