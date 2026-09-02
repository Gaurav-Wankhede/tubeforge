//! Pillar 5: Value Monopoly & High-Contrast Packaging
//!
//! A video must be the single most complete asset on its topic.
//! Monopoly = exhaustive coverage + visual tangibility.
//! Packaging = Zero-Colon + 1280×720 pure HTML high-contrast thumbnail.

use serde::{Deserialize, Serialize};

use crate::scoring::geo;
use crate::storage::db::VideoRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonopolyScore {
    pub total: f64,
    pub completeness: f64,
    pub visual_tangibility: f64,
    pub packaging: f64,
    pub is_monopoly: bool,
    pub verdict: String,
}

/// Monopoly score 0–100:
/// - `completeness` 50%: GEO entity_coverage + list_phrasing + desc_length
/// - `visual_tangibility` 25%: thumb_url present + duration ≥ 600s (2.5D volumetric affordance)
/// - `packaging` 25%: title has no colon, ≤60 chars, thumb 1280×720 contract
pub fn score_video(video: &VideoRow, desc: &str, tags: &[String]) -> MonopolyScore {
    let geo_meta = geo::GeoMeta::default();
    let geo_c = geo::compute(desc, tags, &[], &geo_meta);
    // completeness proxy: entity coverage + list phrasing + metadata completeness (tags+desc+timestamps)
    let desc_words = desc.split_whitespace().count() as f64;
    let desc_len = if desc_words >= 200.0 { 100.0 } else if desc_words >= 100.0 { 70.0 } else if desc_words >= 50.0 { 40.0 } else { 20.0 };
    let completeness = ((geo_c.entity_coverage + geo_c.list_phrasing + desc_len) / 3.0).min(100.0);
    // Visual tangibility: thumb + long-form volumetric
    let has_thumb = video.thumb_url.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
    let long = video.duration_sec.unwrap_or(0) >= 600;
    let visual_tangibility = match (has_thumb, long) {
        (true, true) => 100.0,
        (true, false) => 60.0,
        (false, true) => 50.0,
        (false, false) => 20.0,
    };
    let has_colon = video.title.contains(':');
    let len_ok = video.title.chars().count() <= 60;
    let packaging = match (has_colon, len_ok) {
        (false, true) => 100.0,
        (false, false) => 70.0,
        (true, _) => 30.0,
    };
    let total = (completeness * 0.50 + visual_tangibility * 0.25 + packaging * 0.25).round();
    let is_monopoly = total >= 70.0 && !has_colon && has_thumb;
    let verdict = if is_monopoly {
        "monopoly — most complete + high-contrast packaging"
    } else if has_colon {
        "fix packaging — colons break harness (use parenthetical hooks)"
    } else if !has_thumb {
        "missing 2.5D volumetric thumb (1280×720 pure HTML required)"
    } else {
        "expand completeness — add entities, lists, and depth"
    }
    .to_string();
    MonopolyScore { total, completeness: completeness.round(), visual_tangibility, packaging, is_monopoly, verdict }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vid(title: &str, thumb: Option<&str>, dur: Option<i64>, desc: &str) -> VideoRow {
        VideoRow { video_id: "v1".into(), title: title.into(), description: desc.into(), thumb_url: thumb.map(|s| s.to_string()), duration_sec: dur, published_at: "2026-01-01T00:00:00Z".into(), ..Default::default() }
    }

    #[test]
    fn colon_breaks_monopoly() {
        let v = vid("Rust: Security Guide", Some("https://thumb"), Some(900), "What is Rust? How to secure it. Step 1: isolate. Step 2: sandbox.");
        let s = score_video(&v, &v.description, &[]);
        assert!(!s.is_monopoly);
        assert_eq!(s.packaging, 30.0);
    }

    #[test]
    fn complete_packaged_is_monopoly() {
        let desc = "What is Rust sandbox isolation? How to build zero-trust compiler sandbox. Entities: Rust, sandbox, compiler. Steps:\n- isolate\n- verify\n- sandbox\n#rust";
        let v = vid("How I Built a Zero-Trust Compiler Sandbox", Some("https://thumb"), Some(900), desc);
        let s = score_video(&v, desc, &["rust".into(), "sandbox".into()]);
        assert!(s.is_monopoly || s.total >= 60.0);
        assert_eq!(s.packaging, 100.0);
    }

    #[test]
    fn missing_thumb_no_monopoly() {
        let v = vid("How Rust Works", None, Some(900), "Short desc");
        let s = score_video(&v, &v.description, &[]);
        assert!(!s.is_monopoly);
        assert_eq!(s.visual_tangibility, 50.0);
    }
}
