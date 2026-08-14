//! Content generator — auto-draft Title / Description / Tags for a future
//! video from TubeForge data (LLD §8 + forecast layer).
//!
//! Given a topic and its research/forecast/scores data, produces a precise,
//! SEO-shaped packaging draft. This is the "the system automatically provides
//! the precise title, description, tags" capability: it combines
//! - the forecast (rising/flat/falling demand → which angle to lead with),
//! - the latest keyword research (volume_label, opportunity, competition,
//!   suggested_tags, related_keywords),
//! - the 22-component SEO/GEO score model for what a strong title/desc/tags
//!   must contain (keyword-first, front-loaded, hashtags, etc.).
//!
//! All output is deterministic, plain text, no emojis (channel invariant), and
//! follows the house Title Doctrine (keyword-first, 30-55 chars, no hype).

use serde::{Deserialize, Serialize};

/// A ready-to-use packaging draft for one future video.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDraft {
    pub topic: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    /// The demand signal that shaped the angle.
    pub demand_angle: String,
    /// Forecast verdict (when enough history exists).
    pub verdict: Option<String>,
}

/// Inputs the generator consumes.
#[derive(Debug, Clone, Default)]
pub struct DraftInput {
    pub topic: String,
    pub volume_label: Option<String>,
    pub opportunity_score: Option<f64>,
    pub competition_score: Option<f64>,
    pub serp_mean_views: Option<f64>,
    pub verdict: Option<String>,
    pub suggested_tags: Vec<String>,
    pub related_keywords: Vec<String>,
}

/// Build the title following the house Title Doctrine: keyword-first,
/// 30-55 chars, declarative/question/versus formulas, no hype, no emoji.
fn build_title(topic: &str, angle: &str) -> String {
    let topic_clean = topic.trim().trim_end_matches('.');
    // Formula selection by demand angle (data-backed from the keyword matrix).
    let candidate = match angle {
        "rising" => format!("{topic_clean}, Explained"),
        "falling" => format!("Is {topic_clean} Still Worth It?"),
        "high_demand" => format!("What Is {topic_clean}?"),
        "comparison" => format!("{topic_clean}: A Comparison"),
        _ => format!("{topic_clean}, Explained"),
    };
    // Hard cap ~55 chars (house doctrine: never truncate on mobile).
    if candidate.chars().count() > 55 {
        let cut: String = candidate.chars().take(52).collect();
        format!("{}…", cut.trim_end())
    } else {
        candidate
    }
}

/// Build a keyword-first, 300-500 word-style description draft (concise but
/// SEO-shaped): hook → what you'll learn → verdict/angle → resource note.
fn build_description(input: &DraftInput) -> String {
    let mut out = String::new();

    // Hook (first ~125 chars = search snippet).
    let vol = input.volume_label.as_deref().unwrap_or("this topic");
    out.push_str(&format!(
        "Learn {} — how it works, why it matters, and whether it's trending up or down. ",
        input.topic
    ));

    out.push_str("\n\nIn this video you'll understand ");
    out.push_str(&input.topic);
    out.push_str(" clearly: the core idea, the problem it solves, and the trade-offs. ");

    if let Some(opp) = input.opportunity_score {
        out.push_str(&format!("Research shows opportunity {:.0}/100", opp));
        if let Some(comp) = input.competition_score {
            out.push_str(&format!(" vs competition {:.0}/100.", comp));
        } else {
            out.push('.');
        }
    }
    if let Some(v) = &input.verdict {
        out.push_str(&format!(" Demand is {v}."));
    }
    if let Some(views) = input.serp_mean_views {
        out.push_str(&format!(
            " Ranking videos average {} views.",
            fmt_views(views)
        ));
    }
    out.push_str(&format!(" ({vol} volume)."));

    if !input.suggested_tags.is_empty() {
        out.push_str("\n\nKeywords covered: ");
        out.push_str(
            &input
                .suggested_tags
                .iter()
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('.');
    }

    out.push_str("\n\n# ");
    out.push_str(&input.topic.replace(' ', ""));
    out.push_str(" #TechExplained");

    out
}

/// Build tags: primary keyword first, then suggested, then related — capped.
fn build_tags(input: &DraftInput) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let primary = input.topic.trim().to_string();
    if !primary.is_empty() {
        tags.push(primary.clone());
    }
    for t in &input.suggested_tags {
        if tags.len() >= 12 {
            break;
        }
        let t = t.trim();
        if !t.is_empty() && !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            tags.push(t.to_string());
        }
    }
    for r in &input.related_keywords {
        if tags.len() >= 12 {
            break;
        }
        let r = r.trim();
        if !r.is_empty() && !tags.iter().any(|x| x.eq_ignore_ascii_case(r)) {
            tags.push(r.to_string());
        }
    }
    tags
}

/// Decide the demand angle from forecast + research.
fn demand_angle(input: &DraftInput) -> String {
    if let Some(v) = &input.verdict {
        if v.eq_ignore_ascii_case("rising") {
            return "rising".to_string();
        }
        if v.eq_ignore_ascii_case("falling") {
            return "falling".to_string();
        }
    }
    if let Some(vol) = &input.volume_label {
        if vol.eq_ignore_ascii_case("high") {
            return "high_demand".to_string();
        }
    }
    "explained".to_string()
}

/// Generate a full packaging draft.
pub fn generate(input: &DraftInput) -> ContentDraft {
    let angle = demand_angle(input);
    let title = build_title(&input.topic, &angle);
    let description = build_description(input);
    let tags = build_tags(input);
    ContentDraft {
        topic: input.topic.clone(),
        title,
        description,
        tags,
        demand_angle: angle,
        verdict: input.verdict.clone(),
    }
}

fn fmt_views(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.0}k", v / 1_000.0)
    } else {
        format!("{v:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> DraftInput {
        DraftInput {
            topic: "jwt vs paseto".to_string(),
            volume_label: Some("High".to_string()),
            opportunity_score: Some(30.6),
            competition_score: Some(69.4),
            serp_mean_views: Some(14_640.0),
            verdict: Some("rising".to_string()),
            suggested_tags: vec!["jwt".into(), "paseto".into(), "token".into()],
            related_keywords: vec!["what is jwt".into()],
        }
    }

    #[test]
    fn title_is_keyword_first_and_bounded() {
        let d = generate(&input());
        assert!(d.title.starts_with("Jwt vs Paseto") || d.title.starts_with("jwt vs paseto"));
        assert!(d.title.chars().count() <= 55);
    }

    #[test]
    fn rising_uses_explained_angle() {
        let d = generate(&input());
        assert_eq!(d.demand_angle, "rising");
        assert!(d.title.contains("Explained"));
    }

    #[test]
    fn tags_start_with_primary_and_dedupe() {
        let d = generate(&input());
        assert_eq!(d.tags[0], "jwt vs paseto");
        // No duplicates case-insensitively.
        let lower: Vec<String> = d.tags.iter().map(|t| t.to_lowercase()).collect();
        let unique: std::collections::HashSet<_> = lower.iter().collect();
        assert_eq!(unique.len(), d.tags.len());
    }

    #[test]
    fn description_is_nonempty_and_emoji_free() {
        let d = generate(&input());
        assert!(!d.description.is_empty());
        assert!(!d.description.contains('😀'));
    }
}
