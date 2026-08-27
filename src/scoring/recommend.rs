//! Actionable recommendations (Phase 6.6 — vidIQ's "checklist" half).
//!
//! vidIQ's SEO scorecard is actionable: it tells you exactly what's missing
//! ("low description score? add more content. Missing tags? add what's
//! suggested"). This module derives the same checklist from the computed
//! components + raw inputs — every recommendation maps to a specific
//! component the creator can fix before publishing.

use crate::scoring::seo::{hashtag_count_score, title_length_score, SeoComponents};

/// One actionable recommendation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Recommendation {
    /// The component key this recommendation targets.
    pub component: &'static str,
    /// Imperative guidance ("Add 15-30 tags").
    pub message: String,
    /// 0-100 current value of the targeted component (for priority).
    pub current: f64,
}

/// Derive the checklist from the computed SEO components + raw inputs.
/// Ordered by severity (worst component first).
pub fn recommendations(
    seo: &SeoComponents,
    title: &str,
    desc: &str,
    tags: &[String],
    keywords: &[String],
) -> Vec<Recommendation> {
    let mut out: Vec<Recommendation> = Vec::new();

    if keywords.is_empty() {
        out.push(Recommendation {
            component: "keyword_title",
            message: "No target keywords given — pass --keywords so the title/description \
                      placement signals can be scored"
                .to_string(),
            current: seo.keyword_title,
        });
    }

    if seo.title_40_chars < 100.0 {
        out.push(Recommendation {
            component: "title_40_chars",
            message: "Put the primary keyword within the first 40 characters of the title \
                      (what search results actually show)"
                .to_string(),
            current: seo.title_40_chars,
        });
    }

    let tl = title_length_score(title.chars().count());
    if tl < 100.0 {
        out.push(Recommendation {
            component: "title_length",
            message: "Aim for a 35-49 character title strictly under 50 characters (mobile-safe CTR sweet spot)"
                .to_string(),
            current: tl,
        });
    }

    if seo.desc_first2lines < 100.0 {
        out.push(Recommendation {
            component: "desc_first2lines",
            message: "Open the description with the keyword in the first two lines — that is \
                      what YouTube crawls for context"
                .to_string(),
            current: seo.desc_first2lines,
        });
    }

    if seo.desc_length < 100.0 {
        let words = desc.split_whitespace().count();
        out.push(Recommendation {
            component: "desc_length",
            message: format!(
                "Description is {words} words — 200+ words is the sweet spot (add timestamps, \
                 links, and context)"
            ),
            current: seo.desc_length,
        });
    }

    if tags.is_empty() {
        out.push(Recommendation {
            component: "tags_quality",
            message: "No tags — add 15-30 relevant tags (5 minimum; every tag should be \
                      something you'd legitimately search for)"
                .to_string(),
            current: seo.tags_quality,
        });
    } else if !(5..=30).contains(&tags.len()) {
        out.push(Recommendation {
            component: "tags_quality",
            message: format!(
                "{} tags — target 15-30 (vidIQ minimum 5; 31+ dilutes relevance)",
                tags.len()
            ),
            current: seo.tags_quality,
        });
    }

    let hc = hashtag_count_score(desc);
    if hc < 100.0 {
        out.push(Recommendation {
            component: "hashtag_count",
            message: "Add 3-5 hashtags in the description (they help categorization; more \
                      than 5 dilutes impact)"
                .to_string(),
            current: hc,
        });
    }

    if seo.keyword_triple < 100.0 {
        out.push(Recommendation {
            component: "keyword_triple",
            message: "Use the same primary keyword in title AND description AND tags (vidIQ \
                      'triple keyword' — the three most important placements)"
                .to_string(),
            current: seo.keyword_triple,
        });
    }

    out.sort_by(|a, b| a.current.total_cmp(&b.current));
    out
}
