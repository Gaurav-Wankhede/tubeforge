//! Chronological Evolution Decode (DecodingYT 3-phase law)
//!
//! Videos sorted by `published_at` are split into chronological thirds:
//! - Phase 1 FOUNDATION (steal+hack, broad, !! caps)
//! - Phase 2 GROWTH (volume + proof stacking, TimeAnchor arrives)
//! - Phase 3 MASTERY (authoritative How-To + Number, zero-colon, 55-char)
//! TubeForge quantifies the evolution so TECHVERSE/BOOKVERSE can skip Phase 1.

use serde::{Deserialize, Serialize};

use crate::scoring::psych;
use crate::storage::db::VideoRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseStats {
    pub phase: String,
    pub count: usize,
    pub avg_title_len: f64,
    pub how_to_ratio: f64,
    pub number_ratio: f64,
    pub colon_ratio: f64,
    pub avg_psych: f64,
    pub total_views: i64,
    pub date_from: String,
    pub date_to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChronologyReport {
    pub phases: Vec<PhaseStats>,
    pub evolution: String,
    pub current_phase: String,
    pub recommendation: String,
}

pub fn decode(mut videos: Vec<VideoRow>) -> ChronologyReport {
    videos.sort_by(|a, b| a.published_at.cmp(&b.published_at));
    if videos.is_empty() {
        return ChronologyReport {
            phases: vec![],
            evolution: "no videos — publish 10-15 to train neural vector".into(),
            current_phase: "foundation".into(),
            recommendation: "Phase 1: steal proven structures, avoid broad topics".into(),
        };
    }
    let n = videos.len();
    let chunk = (n as f64 / 3.0).ceil() as usize;
    let slices: Vec<&[VideoRow]> = vec![
        &videos[0..chunk.min(n)],
        &videos[chunk.min(n)..(2 * chunk).min(n)],
        &videos[(2 * chunk).min(n)..n],
    ];
    let labels = ["Phase 1 — Foundation (Steal+Hack)", "Phase 2 — Growth (Volume+Proof)", "Phase 3 — Mastery (Authority System)"];
    let mut phases = Vec::new();
    for (i, slice) in slices.iter().enumerate() {
        if slice.is_empty() { continue; }
        phases.push(stats_for(labels[i].to_string(), slice));
    }
    let current_phase = if n < 15 {
        "foundation"
    } else if n < 40 {
        "growth"
    } else {
        "mastery"
    }
    .to_string();
    let evolution = if phases.len() == 3 {
        let p1 = &phases[0];
        let p3 = &phases[2];
        if p3.how_to_ratio > p1.how_to_ratio && p3.colon_ratio < p1.colon_ratio {
            "evolved: hack caps → authoritative How-To, zero-colon discipline".into()
        } else if p3.avg_psych > p1.avg_psych {
            "evolved: generic → precise-number + HowOpener lift".into()
        } else {
            "stable pattern — consider narrowing to beachhead mechanics".into()
        }
    } else {
        "collecting — need 3 phases for evolution signal".into()
    };
    let recommendation = match current_phase.as_str() {
        "foundation" => "Ship 10-15, copy proven titles (steal like artist), add 1 USP — skip broad Gaming/DEAD topics".into(),
        "growth" => "Stack proof titles (How I Got 500K with ONLY 30) + TimeAnchor (15 Days, FAST) → build pillar FULL COURSE".into(),
        _ => "Systematize: 45% How-To + 22% Number listicle, 55 chars, one-word caps, feeders → pillar loop".into(),
    };
    ChronologyReport { phases, evolution, current_phase, recommendation }
}

fn stats_for(label: String, slice: &[VideoRow]) -> PhaseStats {
    let count = slice.len();
    let total_len: usize = slice.iter().map(|v| v.title.chars().count()).sum();
    let avg_title_len = if count > 0 { total_len as f64 / count as f64 } else { 0.0 };
    let how = slice.iter().filter(|v| v.title.to_lowercase().starts_with("how ")).count() as f64 / count as f64;
    let numbers = slice.iter().filter(|v| v.title.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)).count() as f64 / count as f64;
    let colons = slice.iter().filter(|v| v.title.contains(':')).count() as f64 / count as f64;
    let psych_sum: f64 = slice.iter().map(|v| psych::score(&v.title).total).sum();
    let avg_psych = if count > 0 { psych_sum / count as f64 } else { 0.0 };
    let total_views: i64 = slice.iter().filter_map(|v| v.view_count).sum();
    let date_from = slice.first().map(|v| v.published_at.chars().take(10).collect()).unwrap_or_default();
    let date_to = slice.last().map(|v| v.published_at.chars().take(10).collect()).unwrap_or_default();
    PhaseStats {
        phase: label,
        count,
        avg_title_len: (avg_title_len * 10.0).round() / 10.0,
        how_to_ratio: (how * 100.0).round() / 100.0,
        number_ratio: (numbers * 100.0).round() / 100.0,
        colon_ratio: (colons * 100.0).round() / 100.0,
        avg_psych: (avg_psych * 10.0).round() / 10.0,
        total_views,
        date_from,
        date_to,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vid(title: &str, pub_at: &str, views: i64) -> VideoRow {
        VideoRow { video_id: title.into(), title: title.into(), published_at: pub_at.into(), view_count: Some(views), ..Default::default() }
    }

    #[test]
    fn empty_is_foundation() {
        let r = decode(vec![]);
        assert_eq!(r.current_phase, "foundation");
        assert!(r.phases.is_empty());
    }

    #[test]
    fn three_phases_split() {
        let mut vids = vec![];
        for i in 0..9 {
            vids.push(vid(&format!("How to Test {}", i), &format!("2026-01-0{}T00:00:00Z", i+1), 100));
        }
        let r = decode(vids);
        assert_eq!(r.phases.len(), 3);
        assert_eq!(r.phases[0].count, 3);
        assert_eq!(r.phases[2].count, 3);
    }

    #[test]
    fn evolution_detects_authority() {
        let mut vids = vec![];
        for i in 0..6 { vids.push(vid("HACKED!! SECRET!!", &format!("2026-01-0{}T00:00:00Z", i+1), 10)); }
        for i in 6..12 { vids.push(vid("How to Do X", &format!("2026-01-{}T00:00:00Z", i+1), 10)); }
        for i in 12..18 { vids.push(vid("How to Do X Properly", &format!("2026-01-{}T00:00:00Z", i+1), 10)); }
        let r = decode(vids);
        assert!(r.evolution.contains("evolved") || r.evolution.contains("stable"));
    }
}
