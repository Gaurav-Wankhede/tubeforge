//! Action layer (VidIQ-style): turns raw computed scores into **what to DO**.
//!
//! The underlying analytics compute 0-100 scores (audit components, SEO
//! scores, opportunity scores). This module reframes them as ranked,
//! actionable next steps for the creator's OWN channel — not competitor
//! observations. Each action has:
//! - `what`  : the concrete fix ("Rewrite title to 20-60 chars")
//! - `why`   : the data backing it ("title was 110 chars, hurts CTR")
//! - `impact`: high/medium/low (how much it moves growth)
//! - `effort`: low/medium/high (how hard it is to do)

use serde::{Deserialize, Serialize};

/// Severity/impact of an action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Impact {
    High,
    Medium,
    Low,
}

/// One actionable recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub area: String,
    pub what: String,
    pub why: String,
    pub impact: Impact,
    pub score: f64,
}

/// Priority = impact weight * (1 - score/100). High-impact + low-score first.
pub fn priority(impact: &Impact, score: f64) -> f64 {
    let w = match impact {
        Impact::High => 1.0,
        Impact::Medium => 0.6,
        Impact::Low => 0.3,
    };
    w * (1.0 - (score.clamp(0.0, 100.0) / 100.0))
}

/// Sort actions by priority (highest first).
pub fn sort_actions(actions: &mut [Action]) {
    actions.sort_by(|a, b| {
        priority(&b.impact, b.score)
            .partial_cmp(&priority(&a.impact, a.score))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Turn a ChannelAudit into ranked, actionable fixes. Weak components
/// (score < 70) become concrete "DO THIS" actions ordered by impact×gap.
pub fn from_audit(audit: &crate::analytics::audit::ChannelAudit) -> Vec<Action> {
    let mut actions = Vec::new();
    for c in &audit.components {
        if c.score >= 70.0 {
            continue; // already strong — no action
        }
        let (impact, what, why) = match c.name.as_str() {
            "metadata" => (
                Impact::High,
                "Rewrite video titles to 20-60 chars with the keyword up front; add 300+ char descriptions",
                format!("metadata score {:.0}/100 — weak titles/descriptions hurt CTR and ranking", c.score),
            ),
            "consistency" => (
                Impact::High,
                "Publish on a fixed cadence (data: weekly beats sporadic 5x; 12+/mo grows subs 66% faster)",
                format!("consistency {:.0}/100 — irregular upload gaps hurt the algorithm", c.score),
            ),
            "engagement" => (
                Impact::High,
                "Improve hooks/retention (hold 50% in first 30s) and ask for comments (comments weigh 3x likes)",
                format!("engagement {:.0}/100 — low comments×3+likes vs views", c.score),
            ),
            "tags" => (
                Impact::Medium,
                "Use 5-30 relevant tags per video (5 min / 15-30 optimal)",
                format!("tag usage {:.0}/100 — too few or poorly-diversified tags", c.score),
            ),
            "series" => (
                Impact::Medium,
                "Structure content as a series/playlist to boost session time",
                format!("series strength {:.0}/100 — few episodic videos", c.score),
            ),
            "authority" => (
                Impact::Low,
                "Cross-promote and build watch time to grow authority (subs + views vs competitors)",
                format!("authority {:.0}/100 — channel scale is a lagging factor", c.score),
            ),
            _ => (Impact::Low, "Review channel health", format!("{:.0}/100", c.score)),
        };
        actions.push(Action {
            area: c.name.clone(),
            what: what.into(),
            why,
            impact,
            score: c.score,
        });
    }
    sort_actions(&mut actions);
    actions
}

/// VidIQ View Prediction tier from an opportunity score (0-100).
/// "Very High"/"High"/"Medium"/"Low" — how likely the topic is to generate
/// views for the creator's channel, given demand + competition + fit.
pub fn view_prediction(opportunity: f64, fit: f64) -> &'static str {
    let opp = opportunity.clamp(0.0, 100.0);
    let fit_ok = fit >= 40.0;
    if opp >= 70.0 && fit_ok {
        "Very High"
    } else if opp >= 50.0 {
        "High"
    } else if opp >= 30.0 {
        "Medium"
    } else {
        "Low"
    }
}

/// Plain-language "why make THIS" for an idea/recommendation.
pub fn why_make(opportunity: f64, competition: f64, volume: &str, demand_matches: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    if opportunity >= 70.0 && competition < 60.0 {
        parts
            .push("high demand with weak competition — a well-optimized video can win".to_string());
    } else if opportunity >= 50.0 {
        parts.push("solid demand, winnable with a sharper angle".to_string());
    } else {
        parts.push("moderate or saturated demand — needs a clear angle".to_string());
    }
    parts.push(format!("{} search volume", volume.to_lowercase()));
    parts.push(format!("competition {competition:.0}/100"));
    if demand_matches > 0 {
        parts.push(format!("{demand_matches} ranking videos currently match"));
    }
    parts.join("; ")
}

/// Reframe the coverage map into actionable "topic you should win" entries.
/// A coverage topic is an opportunity when it has real demand (mean_views)
/// and FEW channels cover it (weak supply = a gap you can fill). Returns the
/// top `limit` opportunities, each with a concrete angle to win it.
pub fn gap_opportunities(
    topics: &[crate::analytics::gaps::CoverageTopic],
    limit: usize,
) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    for t in topics
        .iter()
        .filter(|t| t.score >= 30.0 && t.mean_views > 0.0)
    {
        let angle = if t.no_short {
            "Make a Short version — this topic has no Short in the corpus yet".to_string()
        } else if t.is_series {
            format!(
                "Start a series on {} — episodic content boosts session time",
                t.topic
            )
        } else {
            format!(
                "Cover {} with a fresh angle — only {} channel(s) own it",
                t.topic, t.channels
            )
        };
        out.push(serde_json::json!({
            "topic": t.topic,
            "score": t.score,
            "demand_views": t.mean_views,
            "channels_covering": t.channels,
            "action": angle,
            "prediction": view_prediction(t.score, 50.0),
        }));
    }
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::audit::{AuditComponent, ChannelAudit};
    use crate::analytics::gaps::CoverageTopic;

    #[test]
    fn view_prediction_tiers() {
        assert_eq!(view_prediction(80.0, 50.0), "Very High");
        assert_eq!(view_prediction(60.0, 50.0), "High");
        assert_eq!(view_prediction(40.0, 50.0), "Medium");
        assert_eq!(view_prediction(20.0, 50.0), "Low");
        // Even high opportunity with weak fit drops out of Very High.
        assert_eq!(view_prediction(80.0, 30.0), "High");
    }

    #[test]
    fn priority_orders_by_impact_and_gap() {
        // High impact + low score > Low impact + low score.
        let high = priority(&Impact::High, 30.0);
        let low = priority(&Impact::Low, 30.0);
        assert!(high > low);
        // Same impact: lower score (bigger gap) ranks first.
        let weak = priority(&Impact::High, 20.0);
        let stronger = priority(&Impact::High, 60.0);
        assert!(weak > stronger);
    }

    #[test]
    fn audit_actions_only_for_weak_components() {
        let audit = ChannelAudit {
            channel_id: "x".into(),
            channel_name: "x".into(),
            total_score: 60.0,
            grade: "C".into(),
            verdict: "".into(),
            components: vec![
                AuditComponent {
                    name: "metadata".into(),
                    score: 50.0,
                    weight: 0.3,
                    detail: "".into(),
                },
                AuditComponent {
                    name: "consistency".into(),
                    score: 90.0,
                    weight: 0.15,
                    detail: "".into(),
                },
            ],
        };
        let actions = from_audit(&audit);
        // Only the weak component (metadata, 50 < 70) yields an action.
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].area, "metadata");
        assert_eq!(actions[0].impact, Impact::High);
    }

    #[test]
    fn gap_opportunities_reframe_coverage() {
        let topics = vec![
            CoverageTopic {
                topic: "rust async".into(),
                videos: 12,
                channels: 2,
                mean_views: 80_000.0,
                newest_at: None,
                no_short: true,
                is_series: false,
                score: 60.0,
                covering_channels: vec![],
            },
            CoverageTopic {
                topic: "saturated".into(),
                videos: 50,
                channels: 8,
                mean_views: 10_000.0,
                newest_at: None,
                no_short: false,
                is_series: false,
                score: 5.0,
                covering_channels: vec![],
            },
        ];
        let opps = gap_opportunities(&topics, 10);
        // Only the score>=30 topic (rust async) becomes an opportunity.
        assert_eq!(opps.len(), 1);
        assert_eq!(opps[0]["topic"], "rust async");
        assert!(opps[0]["action"].as_str().unwrap().contains("Short"));
    }
}
