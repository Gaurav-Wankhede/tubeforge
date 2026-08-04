//! Scoring engine (LLD §7): signals → components → weighted totals.
//!
//! Composite per §7.4:
//! ```text
//! total = (seo_weight * seo_total + geo_weight * geo_total) / (seo_weight + geo_weight)
//! seo_total = Σ(w_i * comp_i) / Σ w_i ; geo_total likewise
//! ```
//! Components JSON persists into `scores.components` (LLD §7.5, §6.4:
//! recompute only for changed/inserted videos). Dependency rule: scoring goes
//! through the `storage`/`search` APIs only.

use serde_json::Value;

use crate::error::TubeforgeError;
use crate::search::bm25::Bm25;
use crate::storage::db::{VideoRow, Db};

pub mod geo;
pub mod seo;
pub mod weights;

use weights::Weights;

/// The full score of one video/draft: weighted totals + component JSON.
#[derive(Debug, Clone)]
pub struct ScoreResult {
    pub seo_total: f64,
    pub geo_total: f64,
    pub total: f64,
    /// `seo: {keyword_title: …, …}` — the envelope's `seo.components`.
    pub seo_components: Value,
    /// `geo: {entity_coverage: …, …}` — the envelope's `geo.components`.
    pub geo_components: Value,
    /// Flat merged components (LLD §7.5) — what persists to `scores.components`.
    pub components_flat: Value,
}

/// Keyword query resolution: explicit target keywords when given, else the
/// title itself (Phase 1 basic-mode corpus-resonance semantics — keeps the
/// `--draft-title` flow working without a keyword list).
pub fn effective_keywords(keywords: &[String], title: &str) -> Vec<String> {
    if keywords.is_empty() {
        vec![title.to_string()]
    } else {
        keywords.to_vec()
    }
}

/// Compute the full SEO+GEO score. `exclude_video_id` self-excludes the
/// stored video from its own BM25 corpus resonance (LLD §7.1). Stored-video
/// callers with recording/topic metadata should use `compute_with_meta` —
/// the C1/C2 signals are 0 for a metadata-less draft.
pub fn compute(
    title: &str,
    desc: &str,
    tags: &[String],
    keywords: &[String],
    bm25: &Bm25,
    weights: &Weights,
    exclude_video_id: Option<&str>,
) -> ScoreResult {
    compute_with_meta(
        title,
        desc,
        tags,
        keywords,
        bm25,
        weights,
        exclude_video_id,
        &geo::GeoMeta::default(),
    )
}

/// `compute` plus the stored-video free metadata (recording details + topic
/// categories) feeding the C1/C2 GEO components. Mirrors `compute`'s seven
/// parameters plus the metadata input — clippy's 7-arg ceiling is waived
/// deliberately so both entry points stay symmetric.
#[allow(clippy::too_many_arguments)]
pub fn compute_with_meta(
    title: &str,
    desc: &str,
    tags: &[String],
    keywords: &[String],
    bm25: &Bm25,
    weights: &Weights,
    exclude_video_id: Option<&str>,
    geo_meta: &geo::GeoMeta,
) -> ScoreResult {
    let eff = effective_keywords(keywords, title);
    let seo_c = seo::compute(title, desc, tags, &eff, bm25, exclude_video_id);
    let geo_c = geo::compute(desc, tags, &eff, geo_meta);

    let seo_total = weighted(&seo_c.values(), weights);
    let geo_total = weighted(&geo_c.values(), weights);
    let denom = weights.seo_group + weights.geo_group;
    let total = if denom <= 0.0 {
        0.0
    } else {
        (weights.seo_group * seo_total + weights.geo_group * geo_total) / denom
    };

    let seo_components = seo_c.to_json();
    let geo_components = geo_c.to_json();
    let mut flat = serde_json::Map::new();
    if let Value::Object(m) = &seo_components {
        flat.extend(m.clone());
    }
    if let Value::Object(m) = &geo_components {
        flat.extend(m.clone());
    }

    ScoreResult {
        seo_total: round2(seo_total.clamp(0.0, 100.0)),
        geo_total: round2(geo_total.clamp(0.0, 100.0)),
        total: round2(total.clamp(0.0, 100.0)),
        seo_components,
        geo_components,
        components_flat: Value::Object(flat),
    }
}

/// Persist a score row (LLD §7.5). Idempotent upsert keyed on video_id.
pub async fn persist(db: &Db, video_id: &str, r: &ScoreResult) -> Result<(), TubeforgeError> {
    db.upsert_score(
        video_id,
        r.seo_total,
        r.geo_total,
        r.total,
        &r.components_flat.to_string(),
    )
    .await
}

/// Score one stored video against the tracked keywords and persist it.
pub async fn score_video(
    db: &Db,
    bm25: &Bm25,
    video: &VideoRow,
    weights: &Weights,
) -> Result<ScoreResult, TubeforgeError> {
    let keywords: Vec<String> = db
        .list_keywords()
        .await?
        .into_iter()
        .map(|k| k.keyword)
        .collect();
    let tags: Vec<String> = serde_json::from_str(&video.tags).unwrap_or_default();
    let meta = geo::GeoMeta {
        published_at: video.published_at.clone(),
        recording_date: video.recording_date.clone(),
        recording_location_name: video.recording_location_name.clone(),
        recording_lat: video.recording_lat,
        recording_lng: video.recording_lng,
        topic_categories: serde_json::from_str(&video.topic_categories).unwrap_or_default(),
    };
    let r = compute_with_meta(
        &video.title,
        &video.description,
        &tags,
        &keywords,
        bm25,
        weights,
        Some(&video.video_id),
        &meta,
    );
    persist(db, &video.video_id, &r).await?;
    Ok(r)
}

/// Weighted mean over (component, value) pairs — LLD §7.4 formula. SEO and
/// GEO component key sets are disjoint, so summing both maps per key is safe
/// for either group.
fn weighted(items: &[(&'static str, f64)], w: &Weights) -> f64 {
    let num: f64 = items
        .iter()
        .map(|(k, v)| (w.seo_weight(k) + w.geo_weight(k)) * v)
        .sum();
    let denom: f64 = items.iter().map(|(k, _)| w.seo_weight(k) + w.geo_weight(k)).sum();
    if denom <= 0.0 {
        0.0
    } else {
        num / denom
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn effective_keywords_falls_back_to_title() {
        assert_eq!(effective_keywords(&[], "My Title"), vec!["My Title".to_string()]);
        assert_eq!(
            effective_keywords(&["a".to_string(), "b".to_string()], "My Title"),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    /// Envelope-adjacent JSON shape: flat components contain both groups.
    #[test]
    fn components_flat_merges_seo_and_geo() {
        let w = Weights::defaults();
        let dir = tempfile::tempdir().expect("tempdir");
        let index = crate::search::new_index(&dir.path().join("idx")).expect("index");
        let bm25 = Bm25::open(index).expect("bm25");
        let r = compute(
            "The Best Database Guide",
            "What is a database? How to build one. 0:00 intro",
            &["database".to_string(), "rust".to_string(), "guide".to_string()],
            &["database".to_string()],
            &bm25,
            &w,
            None,
        );
        assert!(r.seo_components.get("keyword_title").is_some());
        assert!(r.geo_components.get("entity_coverage").is_some());
        assert!(r.components_flat.get("keyword_title").is_some());
        assert!(r.components_flat.get("metadata_complete").is_some());
        // C1/C2 components flow through the composite and the flat map too —
        // zero for a metadata-less draft, present in the JSON.
        assert_eq!(r.geo_components.get("location_signal"), Some(&json!(0.0)));
        assert_eq!(r.geo_components.get("topic_relevance"), Some(&json!(0.0)));
        assert!(r.components_flat.get("location_signal").is_some());
        assert!(r.components_flat.get("topic_relevance").is_some());
        assert!((0.0..=100.0).contains(&r.seo_total));
        assert!((0.0..=100.0).contains(&r.geo_total));
        assert!((0.0..=100.0).contains(&r.total));
        assert_eq!(r.total, round2((r.seo_total + r.geo_total) / 2.0));
    }

    /// C1/C2 metadata flows into the composite: a stored video with a
    /// recorded location, near-publish recording date and matching topics
    /// scores above the metadata-less baseline.
    #[test]
    fn geo_meta_lifts_composite_score() {
        let w = Weights::defaults();
        let dir = tempfile::tempdir().expect("tempdir");
        let index = crate::search::new_index(&dir.path().join("idx")).expect("index");
        let bm25 = Bm25::open(index).expect("bm25");
        let meta = geo::GeoMeta {
            published_at: "2026-07-15T10:00:00Z".to_string(),
            recording_date: Some("2026-07-10T00:00:00Z".to_string()),
            recording_location_name: Some("Googleplex".to_string()),
            recording_lat: Some(37.422),
            recording_lng: Some(-122.084),
            topic_categories: vec![
                "https://en.wikipedia.org/wiki/Artificial_intelligence".to_string(),
            ],
        };
        let r = compute_with_meta(
            "Artificial Intelligence Guide",
            "What is AI? How to build with it.",
            &["ai".to_string()],
            &["artificial intelligence".to_string()],
            &bm25,
            &w,
            None,
            &meta,
        );
        assert_eq!(r.geo_components.get("location_signal"), Some(&json!(100.0)));
        assert_eq!(r.geo_components.get("topic_relevance"), Some(&json!(100.0)));
        let baseline = compute(
            "Artificial Intelligence Guide",
            "What is AI? How to build with it.",
            &["ai".to_string()],
            &["artificial intelligence".to_string()],
            &bm25,
            &w,
            None,
        );
        assert!(
            r.geo_total > baseline.geo_total,
            "meta must lift geo_total ({} vs {})",
            r.geo_total,
            baseline.geo_total
        );
    }

    #[test]
    fn envelope_json_shape() {
        let env = crate::output::Envelope::ok(json!({"data": 1}), None);
        let v = env.to_json();
        assert_eq!(v["ok"], true);
        assert!(v.get("data").is_some());
        assert!(v.get("meta").is_none());
        assert!(v.get("error").is_none());
        let err = crate::output::Envelope::error(&TubeforgeError::Usage("nope".into()));
        let v = err.to_json();
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"]["code"], "USAGE");
        assert!(v.get("data").is_none());
    }

    /// Silence unused-import lint for `util` in non-test builds is handled by
    /// the crate build; this test exercises the merge ordering helper.
    #[test]
    fn util_tokens_are_lowercase_alnum() {
        assert_eq!(
            crate::util::tokens("How-to Build: Rust DB!"),
            vec!["how", "to", "build", "rust", "db"]
        );
    }
}
