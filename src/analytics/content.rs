//! Content generator — auto-draft Title / Description / Tags for a future
//! video from TubeForge data, enforcing empirical research packaging laws:
//! 1. Zero Colons (':') and Zero Pipes ('|') in titles.
//! 2. 45-Character Front-Loaded Mobile Viewport Rule.
//! 3. 5 Empirical Archetypes (Parenthetical Mechanism, Em-Dash Contrast, Mental Model, Blueprint, Case Study).
//! 4. Hero-Guide StoryBrand Description & High-Retention Hook Sequencing.

use serde::{Deserialize, Serialize};

/// One research-backed title variation with its archetype and mobile preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleVariation {
    pub archetype: String,
    pub title: String,
    pub mobile_preview_45: String,
    pub rationale: String,
}

/// A ready-to-use packaging draft for one future video.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDraft {
    pub topic: String,
    pub title: String,
    pub title_variations: Vec<TitleVariation>,
    pub mobile_preview_45: String,
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

/// Strict title sanitization: strips colons, replaces pipes, eliminates double spaces.
pub fn sanitize_title(raw: &str) -> String {
    let mut clean = raw
        .trim()
        .replace(" : ", " — ")
        .replace(": ", " — ")
        .replace(':', " — ")
        .replace(" | ", " — ")
        .replace('|', " — ");

    // Collapse multiple dashes or spaces
    while clean.contains("  ") {
        clean = clean.replace("  ", " ");
    }
    while clean.contains(" — — ") {
        clean = clean.replace(" — — ", " — ");
    }
    clean.trim().to_string()
}

/// Capitalize topic words cleanly for high visual CTR readability.
fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let lower = w.to_lowercase();
            if ["and", "or", "the", "in", "on", "at", "to", "for", "with", "a", "an", "vs"].contains(&lower.as_str()) {
                lower
            } else {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Generate the 4 empirical title archetypes based on topic characteristics.
pub fn build_title_archetypes(topic: &str, angle: &str) -> (String, Vec<TitleVariation>) {
    let clean_topic = title_case(topic.trim().trim_end_matches('.'));
    let mut variations = Vec::new();

    // Archetype 1: Technical Mechanism (Parenthetical Hook) — The Gold Standard for CTR
    let title_1 = sanitize_title(&format!("How {clean_topic} Works (Inside the Architecture)"));
    let prev_1 = preview_45(&title_1);
    variations.push(TitleVariation {
        archetype: "Technical Mechanism (Parenthetical)".to_string(),
        title: title_1.clone(),
        mobile_preview_45: prev_1,
        rationale: "Parentheses act as an insider whisper, teasing the deep technical mechanism while keeping the first 45 characters clear.".to_string(),
    });

    // Archetype 2: Contrarian Warning / High-Stakes Consequence (Em-Dash)
    let title_2 = sanitize_title(&format!("Why Most Devs Fail with {clean_topic} — And What to Do Instead"));
    let prev_2 = preview_45(&title_2);
    variations.push(TitleVariation {
        archetype: "Contrarian Warning (Em-Dash)".to_string(),
        title: title_2,
        mobile_preview_45: prev_2,
        rationale: "Em-dash sets a dramatic breath pause separating the high-risk problem from the actionable architectural solution.".to_string(),
    });

    // Archetype 3: Complete Mental Model (Zero-Fluff Educational)
    let title_3 = sanitize_title(&format!("{clean_topic} Explained with Zero Fluff (Full Mental Model)"));
    let prev_3 = preview_45(&title_3);
    variations.push(TitleVariation {
        archetype: "High-Retention Mental Model".to_string(),
        title: title_3,
        mobile_preview_45: prev_3,
        rationale: "Appeals to serious developers seeking high-signal, zero-fluff conceptual mastery over superficial tutorials.".to_string(),
    });

    // Archetype 4: Production Blueprint / Complete Guide (Bracketed Asset)
    let title_4 = sanitize_title(&format!("Master {clean_topic} in 2026 [Full System Blueprint]"));
    let prev_4 = preview_45(&title_4);
    variations.push(TitleVariation {
        archetype: "System Blueprint (Bracketed Asset)".to_string(),
        title: title_4,
        mobile_preview_45: prev_4,
        rationale: "Square brackets declare a tangible, high-value asset, signaling immediate pragmatic utility.".to_string(),
    });

    // Primary selection based on demand angle
    let primary_title = match angle {
        "rising" => title_1.clone(),
        "high_demand" => sanitize_title(&format!("Why {clean_topic} Matters (Complete Architectural Deep Dive)")),
        _ => title_1,
    };

    (primary_title, variations)
}

/// Compute the exact 45-character mobile truncation preview.
pub fn preview_45(title: &str) -> String {
    if title.chars().count() <= 45 {
        title.to_string()
    } else {
        let cut: String = title.chars().take(42).collect();
        format!("{}...", cut.trim_end())
    }
}

/// Build a keyword-first, StoryBrand-structured description draft:
/// Hook -> Hero Problem -> Architectural Guide -> Timestamps Blueprint -> Zero-colon Hashtags.
fn build_description(input: &DraftInput) -> String {
    let mut out = String::new();
    let topic_clean = title_case(input.topic.trim());

    // 1. First 125 chars: High-Curiosity Mobile Search Snippet (Zero Colons)
    out.push_str(&format!(
        "Master {topic_clean} with zero fluff — inside the core architecture, real-world trade-offs, and production engineering practices.\n\n"
    ));

    // 2. StoryBrand Problem-Solution Setup
    out.push_str(&format!(
        "Most tutorials explain {topic_clean} with superficial syntax without showing what happens under the hood. In this deep-dive, we reverse-engineer the core mental model, benchmark the trade-offs, and show you exactly how to apply it in production systems.\n\n"
    ));

    // 3. Concrete Value Breakdown
    out.push_str("WHAT YOU WILL LEARN\n");
    out.push_str(&format!("- The core architectural mechanism behind {topic_clean}\n"));
    out.push_str("- The critical mistakes that cause production failures and how to prevent them\n");
    out.push_str("- Performance benchmarks and memory isolation patterns\n");
    out.push_str("- Clean production code examples and architectural blueprints\n\n");

    // 4. Research Telemetry Context
    if let Some(opp) = input.opportunity_score {
        out.push_str(&format!("Corpus Research: Opportunity {:.0}/100", opp));
        if let Some(comp) = input.competition_score {
            out.push_str(&format!(" · Competition {:.0}/100", comp));
        }
        if let Some(views) = input.serp_mean_views {
            out.push_str(&format!(" · Competitor Average {} views", fmt_views(views)));
        }
        out.push_str("\n\n");
    }

    // 5. Semantic Tags Covered
    if !input.suggested_tags.is_empty() {
        out.push_str("Keywords Covered: ");
        out.push_str(
            &input
                .suggested_tags
                .iter()
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str("\n\n");
    }

    // 6. Search Hashtags (Zero Colons)
    let tag_clean = input.topic.replace([' ', '-', ':'], "");
    out.push_str(&format!("#{tag_clean} #SoftwareEngineering #ArchitectureExplained"));

    out
}

/// Build tags: primary keyword first, then suggested, then related — deduplicated.
fn build_tags(input: &DraftInput) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let primary = input.topic.trim().to_string();
    if !primary.is_empty() {
        tags.push(primary);
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

/// Generate a full research-backed packaging draft.
pub fn generate(input: &DraftInput) -> ContentDraft {
    let angle = demand_angle(input);
    let (title, title_variations) = build_title_archetypes(&input.topic, &angle);
    let mobile_preview_45 = preview_45(&title);
    let description = build_description(input);
    let tags = build_tags(input);

    ContentDraft {
        topic: input.topic.clone(),
        title,
        title_variations,
        mobile_preview_45,
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

    fn sample_input() -> DraftInput {
        DraftInput {
            topic: "Linux Memory Isolation".to_string(),
            volume_label: Some("High".to_string()),
            opportunity_score: Some(78.5),
            competition_score: Some(34.2),
            serp_mean_views: Some(42_000.0),
            verdict: Some("rising".to_string()),
            suggested_tags: vec!["linux kernel".into(), "memory management".into(), "virtual memory".into()],
            related_keywords: vec!["how memory isolation works".into()],
        }
    }

    #[test]
    fn title_has_zero_colons_and_zero_pipes() {
        let d = generate(&sample_input());
        assert!(!d.title.contains(':'));
        assert!(!d.title.contains('|'));
        for var in &d.title_variations {
            assert!(!var.title.contains(':'));
            assert!(!var.title.contains('|'));
        }
    }

    #[test]
    fn mobile_preview_is_bounded_to_45_chars() {
        let d = generate(&sample_input());
        assert!(d.mobile_preview_45.chars().count() <= 45);
    }
}
