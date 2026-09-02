//! Pillar 3: Hyper-Specificity & Category Beachhead (Crossing the Chasm)
//!
//! Broad topics ("Rust Security") compete with giants. Hyper-specific
//! mechanics ("Rust Build Script Credential Stealing") command 100% CTR
//! from a desperate sub-niche. This module scores specificity so every
//! video targets a razor-sharp friction point before expanding outward.

use serde::{Deserialize, Serialize};

use crate::search::bm25::Bm25;
use crate::storage::db::VideoRow;
use crate::util;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeachheadScore {
    pub total: f64,
    pub token_specificity: f64,
    pub competition_weakness: f64,
    pub intent_sharpness: f64,
    pub verdict: String,
    pub is_beachhead: bool,
}

/// Score a topic's beachhead potential.
///
/// - `token_specificity`: 1–2 tokens = broad (20), 3 = medium (60), 4+ = hyper-specific (90–100)
/// - `competition_weakness`: fewer corpus matches → weaker giants → higher score
/// - `intent_sharpness`: friction-point suffixes (+20 bonus)
pub fn score_topic(topic: &str, bm25: &Bm25, videos: &[VideoRow]) -> BeachheadScore {
    let toks = util::tokens(topic);
    let token_specificity = match toks.len() {
        0 => 0.0,
        1 => 20.0,
        2 => 35.0,
        3 => 65.0,
        4 => 85.0,
        _ => 100.0,
    };
    let doc_hits = bm25.matches(crate::search::FIELD_TITLE, topic).len() as f64;
    let total = videos.len().max(1) as f64;
    let df_ratio = doc_hits / total;
    let competition_weakness = if df_ratio <= 0.02 {
        100.0
    } else if df_ratio <= 0.05 {
        80.0
    } else if df_ratio <= 0.10 {
        55.0
    } else if df_ratio <= 0.20 {
        30.0
    } else {
        10.0
    };
    let sharp_terms = [
        "attack", "stealing", "leak", "panic", "deadlock", "bypass", "injection", "overflow",
        "exploit", "forgery", "credential", "sandbox", "isolation", "forensics",
    ];
    let lower = topic.to_lowercase();
    let has_sharp = sharp_terms.iter().any(|t| lower.contains(t));
    let intent_sharpness = if has_sharp { 100.0 } else { 40.0 };

    let total = ((token_specificity * 0.40 + competition_weakness * 0.40 + intent_sharpness * 0.20) as f64).round();
    let is_beachhead = total >= 70.0 && toks.len() >= 3 && competition_weakness >= 55.0;
    let verdict = if is_beachhead {
        "beachhead — hyper-specific, low giant competition"
    } else if token_specificity < 50.0 {
        "too broad — giants own this, narrow to a mechanic"
    } else {
        "contested — viable but giants present"
    }
    .to_string();
    BeachheadScore {
        total,
        token_specificity,
        competition_weakness,
        intent_sharpness,
        verdict,
        is_beachhead,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm_with(titles: &[&str]) -> (tempfile::TempDir, crate::search::bm25::Bm25, Vec<VideoRow>) {
        let dir = tempfile::tempdir().unwrap();
        let idx = crate::search::new_index(&dir.path().join("idx")).unwrap();
        let mut w = idx.writer(15_000_000);
        let _ = &w;
        let mut vids = Vec::new();
        for (i, t) in titles.iter().enumerate() {
            let vid = VideoRow {
                video_id: format!("v{i}"),
                title: t.to_string(),
                description: String::new(),
                tags: "[]".into(),
                published_at: "2026-01-01T00:00:00Z".into(),
                ..Default::default()
            };
            w.add_document(crate::search::index::VideoDoc { video_id: vid.video_id.clone(), channel_id: None, title: vid.title.clone(), description: vid.description.clone(), tags: vec![], published_at: None }).unwrap();
            vids.push(vid);
        }
        w.commit().unwrap();
        let bm = crate::search::bm25::Bm25::open(idx).unwrap();
        (dir, bm, vids)
    }

    #[test]
    fn broad_topic_fails_beachhead() {
        let (_d, bm, vids) = bm_with(&["Rust Security Guide", "Rust Security Audit", "Rust Security Basics"]);
        let s = score_topic("Rust Security", &bm, &vids);
        assert!(!s.is_beachhead);
        assert!(s.token_specificity < 50.0);
    }

    #[test]
    fn hyper_specific_passes() {
        let (_d, bm, vids) = bm_with(&["Python Security Guide", "Tokio Auth Guide", "Generic Topic"]);
        let s = score_topic("Rust Build Script Credential Stealing", &bm, &vids);
        assert!(s.is_beachhead);
        assert_eq!(s.token_specificity, 100.0); // 5 tokens
    }

    #[test]
    fn friction_suffix_boosts_intent() {
        let (_d, bm, vids) = bm_with(&["Some Video"]);
        let a = score_topic("Rust Build Script Credential Stealing", &bm, &vids);
        let b = score_topic("Rust Build Script Overview", &bm, &vids);
        assert!(a.intent_sharpness > b.intent_sharpness);
    }
}
