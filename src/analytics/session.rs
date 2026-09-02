//! Pillar 4: The Next-Hour Session Time Multiplier (Pillar Video Loops)
//!
//! YouTube optimizes Total Platform Session Time over single views.
//! Chaining shorter videos to a long pillar masterwork triggers
//! algorithmic recommendation cascades. TubeForge bridges the Kanban
//! catalog (`status done`) with deterministic end-screen linking.

use serde::{Deserialize, Serialize};

use crate::storage::db::{KanbanTicketRow, VideoRow};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionChain {
    pub pillar_video_id: String,
    pub pillar_title: String,
    pub pillar_duration_sec: i64,
    pub feeders: Vec<Feeder>,
    pub multiplier: f64,
    pub verdict: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feeder {
    pub video_id: String,
    pub title: String,
    pub duration_sec: i64,
    pub role: String,
}

/// Build deterministic session chains: shortest → pillar.
///
/// - Pillar = longest `done` video or longest video ≥ 600s (10m).
/// - Feeders = `done` tickets / short videos (<600s) that share topic tokens.
/// - Multiplier = `1.0 + 0.15 * feeder_count` capped at 2.0 (15% per feeder)
pub fn build_chains(videos: &[VideoRow], tickets: &[KanbanTicketRow]) -> Vec<SessionChain> {
    // Pillar candidates: videos ≥ 600s sorted longest-first
    let mut pillars: Vec<&VideoRow> = videos.iter().filter(|v| v.duration_sec.unwrap_or(0) >= 600).collect();
    pillars.sort_by_key(|v| std::cmp::Reverse(v.duration_sec.unwrap_or(0)));
    // Feeder pool: tickets done + short videos
    let mut feeders: Vec<Feeder> = Vec::new();
    for t in tickets.iter().filter(|t| t.status == "done") {
        feeders.push(Feeder {
            video_id: t.video_id.clone().unwrap_or_else(|| t.ticket_id.clone()),
            title: t.title.clone(),
            duration_sec: t.optimal_duration_sec.unwrap_or(300),
            role: "feeder".into(),
        });
    }
    for v in videos.iter().filter(|v| v.duration_sec.unwrap_or(0) < 600 && v.duration_sec.unwrap_or(0) > 0) {
        feeders.push(Feeder {
            video_id: v.video_id.clone(),
            title: v.title.clone(),
            duration_sec: v.duration_sec.unwrap_or(300),
            role: "feeder".into(),
        });
    }
    let mut chains = Vec::new();
    for pillar in pillars.into_iter().take(3) {
        let ptoks: std::collections::HashSet<String> = crate::util::tokens(&pillar.title).into_iter().collect();
        let mut matched: Vec<Feeder> = feeders
            .iter()
            .filter(|f| {
                let ftoks: std::collections::HashSet<String> = crate::util::tokens(&f.title).into_iter().collect();
                ptoks.intersection(&ftoks).count() >= 1
            })
            .cloned()
            .collect();
        matched.truncate(5);
        let multiplier = (1.0 + matched.len() as f64 * 0.15).min(2.0);
        let verdict = if matched.len() >= 3 {
            "cascade ready — 3+ feeders → pillar triggers recommendation cascade"
        } else if matched.len() >= 1 {
            "linked — add 2 more feeders for cascade"
        } else {
            "orphan pillar — no feeder chain, no session multiplier"
        }
        .to_string();
        chains.push(SessionChain {
            pillar_video_id: pillar.video_id.clone(),
            pillar_title: pillar.title.clone(),
            pillar_duration_sec: pillar.duration_sec.unwrap_or(0),
            feeders: matched,
            multiplier: (multiplier * 100.0).round() / 100.0,
            verdict,
        });
    }
    chains
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vid(id: &str, title: &str, dur: i64) -> VideoRow {
        VideoRow { video_id: id.into(), title: title.into(), duration_sec: Some(dur), published_at: "2026-01-01T00:00:00Z".into(), ..Default::default() }
    }
    fn ticket(id: &str, title: &str, dur: i64) -> KanbanTicketRow {
        KanbanTicketRow {
            ticket_id: id.into(), title: title.into(), channel: "TECHVERSE".into(),
            status: "done".into(), optimal_duration_sec: Some(dur),
            topic: None, framework: None, target_keyword: None, youtube_url: None, video_id: None, research_ref: None, notes: None,
            created_at: "2026-01-01T00:00:00Z".into(), updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn orphan_pillar_no_feeders() {
        let vids = vec![vid("p1", "Zero-Trust Compiler Sandbox Deep Dive", 900)];
        let chains = build_chains(&vids, &[]);
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].feeders.len(), 0);
        assert_eq!(chains[0].multiplier, 1.0);
    }

    #[test]
    fn feeder_chain_multiplies() {
        let vids = vec![
            vid("p1", "Rust Sandbox Compiler Deep Dive", 900),
            vid("s1", "Rust Sandbox Bypass Attack", 300),
            vid("s2", "Rust Sandbox Isolation Explained", 250),
        ];
        let tickets = vec![ticket("t1", "Rust Sandbox Forensics Part 1", 400)];
        let chains = build_chains(&vids, &tickets);
        assert!(chains[0].feeders.len() >= 2);
        assert!(chains[0].multiplier > 1.0);
    }

    #[test]
    fn short_videos_not_pillars() {
        let vids = vec![vid("s1", "Short Clip", 45), vid("s2", "Another Short", 50)];
        let chains = build_chains(&vids, &[]);
        assert!(chains.is_empty());
    }
}
