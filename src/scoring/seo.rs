//! SEO components (LLD §7.2). Every function is deterministic over its
//! inputs. BM25-derived components reuse `search::bm25` against the tantivy
//! corpus (self-excluded for stored videos); structural components are pure
//! string heuristics.
//!
//! Keyword queries: when no target keywords are given, the title itself is
//! the query (Phase 1 basic-mode semantics — corpus resonance of the title),
//! so the Phase 1 `keyword_title`/`keyword_desc`/`keyword_tags` signals keep
//! working for draft scoring.

use std::collections::HashSet;

use serde_json::{json, Value};

use crate::search::bm25::Bm25;
use crate::search::{FIELD_DESCRIPTION, FIELD_TAGS, FIELD_TITLE};
use crate::util;

/// k-scaling constant of the §7.2 formula sketch `min(1, bm25 / k) × 100`.
/// Baked (documented): tantivy BM25 scores at this corpus scale sit in
/// roughly 0.5–6, so k = 4 keeps the component sensitive without saturating.
pub const BM25_K: f64 = 4.0;

/// `title_front` position buckets (LLD §7.2).
const FRONT_NEAR: f64 = 100.0; // position <= 3
const FRONT_MID: f64 = 70.0; // position <= 7
const FRONT_FAR: f64 = 40.0; // elsewhere

/// Power words used by `title_hooks` (documented list).
const POWER_WORDS: [&str; 19] = [
    "ultimate", "best", "top", "complete", "proven", "secret", "tips", "tricks", "guide",
    "fast", "easy", "simple", "powerful", "essential", "surprising", "unexpected", "mistakes",
    "beginner", "advanced",
];

/// All ten SEO components as computed by `compute`.
#[derive(Debug, Clone)]
pub struct SeoComponents {
    pub keyword_title: f64,
    pub title_front: f64,
    pub title_length: f64,
    pub title_hooks: f64,
    pub keyword_desc: f64,
    pub desc_first150: f64,
    pub desc_structure: f64,
    pub tags_relevance: f64,
    pub tags_quality: f64,
    pub keyword_tags: f64,
}

impl SeoComponents {
    /// (key, value) pairs in canonical order — the weighted-sum input.
    pub fn values(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("keyword_title", self.keyword_title),
            ("title_front", self.title_front),
            ("title_length", self.title_length),
            ("title_hooks", self.title_hooks),
            ("keyword_desc", self.keyword_desc),
            ("desc_first150", self.desc_first150),
            ("desc_structure", self.desc_structure),
            ("tags_relevance", self.tags_relevance),
            ("tags_quality", self.tags_quality),
            ("keyword_tags", self.keyword_tags),
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

/// Full SEO pass: BM25 components + structural components.
pub fn compute(
    title: &str,
    desc: &str,
    tags: &[String],
    keywords: &[String],
    bm25: &Bm25,
    exclude_video_id: Option<&str>,
) -> SeoComponents {
    let query = keywords.join(" ");
    SeoComponents {
        keyword_title: keyword_score(bm25.corpus_resonance(FIELD_TITLE, &query, exclude_video_id)),
        title_front: title_front_score(title, keywords),
        title_length: title_length_score(title.chars().count()),
        title_hooks: title_hooks_score(title),
        keyword_desc: keyword_score(bm25.corpus_resonance(
            FIELD_DESCRIPTION,
            &query,
            exclude_video_id,
        )),
        desc_first150: desc_first150_score(desc, keywords),
        desc_structure: desc_structure_score(desc),
        tags_relevance: tags_relevance_score(title, desc, tags),
        tags_quality: tags_quality_score(title, desc, tags),
        keyword_tags: keyword_score(bm25.corpus_resonance(FIELD_TAGS, &query, exclude_video_id)),
    }
}

/// LLD §7.2: `min(1, bm25 / k) × 100`.
pub fn keyword_score(raw_bm25: f64) -> f64 {
    (raw_bm25 / BM25_K).min(1.0) * 100.0
}

/// LLD §7.2 `title_front`: first keyword token position in the title —
/// `pos<=3 → 100; <=7 → 70; else 40`; 0 when the keyword is absent.
pub fn title_front_score(title: &str, keywords: &[String]) -> f64 {
    let title_tokens = util::tokens(title);
    let mut best: Option<usize> = None;
    for kw in keywords {
        for kwt in util::tokens(kw) {
            if let Some(pos) = title_tokens.iter().position(|t| t == &kwt) {
                best = Some(best.map_or(pos, |b| b.min(pos)));
            }
        }
    }
    match best {
        None => 0.0,
        Some(pos) => {
            let p = pos + 1;
            if p <= 3 {
                FRONT_NEAR
            } else if p <= 7 {
                FRONT_MID
            } else {
                FRONT_FAR
            }
        }
    }
}

/// LLD §7.2 `title_length`: ideal 40–60 chars → 100; piecewise falloff
/// (10→60, 40→90, 60→100, 80→70, 120→40, longer → 40, shorter than 10 → 30).
pub fn title_length_score(len: usize) -> f64 {
    let len = len as f64;
    if (40.0..=60.0).contains(&len) {
        100.0
    } else if len < 10.0 {
        30.0
    } else if len < 40.0 {
        60.0 + (len - 10.0) / 30.0 * 30.0 // 10→60 .. 40→90
    } else if len < 80.0 {
        100.0 - (len - 60.0) / 20.0 * 30.0 // 60→100 .. 80→70
    } else if len < 120.0 {
        70.0 - (len - 80.0) / 40.0 * 30.0 // 80→70 .. 120→40
    } else {
        40.0
    }
}

/// LLD §7.2 `title_hooks`: +25 per hook category (numbers, power words,
/// "how to", brackets), capped at 100.
pub fn title_hooks_score(title: &str) -> f64 {
    let t = title.to_lowercase();
    let mut hits = 0.0f64;
    if t.chars().any(|c| c.is_ascii_digit()) {
        hits += 25.0;
    }
    if t.contains("how to") {
        hits += 25.0;
    }
    if t.contains('[') || t.contains(']') || t.contains('(') || t.contains(')') {
        hits += 25.0;
    }
    let words: Vec<&str> = t
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .collect();
    if POWER_WORDS.iter().any(|pw| words.contains(pw)) {
        hits += 25.0;
    }
    hits.min(100.0)
}

/// LLD §7.2 `desc_first150`: keyword token in the first 150 chars → 100;
/// elsewhere in the description → 60; absent → 0.
pub fn desc_first150_score(desc: &str, keywords: &[String]) -> f64 {
    let kw_tokens: HashSet<String> = keywords
        .iter()
        .flat_map(|k| util::tokens(k))
        .collect();
    if kw_tokens.is_empty() {
        return 0.0;
    }
    let head: HashSet<String> = util::tokens(&desc.chars().take(150).collect::<String>())
        .into_iter()
        .collect();
    if head.intersection(&kw_tokens).next().is_some() {
        100.0
    } else {
        let all: HashSet<String> = util::tokens(desc).into_iter().collect();
        if all.intersection(&kw_tokens).next().is_some() {
            60.0
        } else {
            0.0
        }
    }
}

/// LLD §7.2 `desc_structure` checklist: ≥2 lines, bullets, hashtags, and
/// numbered/step markers → 25 each.
pub fn desc_structure_score(desc: &str) -> f64 {
    let lines: Vec<&str> = desc
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let mut score = 0.0;
    if lines.len() >= 2 {
        score += 25.0;
    }
    if lines
        .iter()
        .any(|l| l.starts_with('-') || l.starts_with('*') || l.starts_with('•'))
    {
        score += 25.0;
    }
    if desc.contains('#') {
        score += 25.0;
    }
    if lines.iter().any(|l| {
        l.chars().next().is_some_and(|c| c.is_ascii_digit())
            || l.to_lowercase().starts_with("step")
    }) {
        score += 25.0;
    }
    score
}

/// LLD §7.2 `tags_relevance`: Jaccard overlap of tag tokens vs (title+desc)
/// tokens × 100.
pub fn tags_relevance_score(title: &str, desc: &str, tags: &[String]) -> f64 {
    let tag_tokens: HashSet<String> = tags.iter().flat_map(|t| util::tokens(t)).collect();
    let content: HashSet<String> = util::tokens(title)
        .into_iter()
        .chain(util::tokens(desc))
        .collect();
    if tag_tokens.is_empty() || content.is_empty() {
        return 0.0;
    }
    let inter = tag_tokens.intersection(&content).count();
    let union = tag_tokens.len() + content.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64 * 100.0
    }
}

/// LLD §7.2 `tags_quality` checklist: count in [3,8] → +50, first tag matches
/// content → +25, ≥2 tag tokens match content → +25.
pub fn tags_quality_score(title: &str, desc: &str, tags: &[String]) -> f64 {
    if tags.is_empty() {
        return 0.0;
    }
    let mut score = 0.0;
    if (3..=8).contains(&tags.len()) {
        score += 50.0;
    }
    let content: HashSet<String> = util::tokens(title)
        .into_iter()
        .chain(util::tokens(desc))
        .collect();
    let tag_tokens: HashSet<String> = tags.iter().flat_map(|t| util::tokens(t)).collect();
    if let Some(first) = tags.first() {
        if util::tokens(first).iter().any(|t| content.contains(t)) {
            score += 25.0;
        }
    }
    if tag_tokens.intersection(&content).count() >= 2 {
        score += 25.0;
    }
    score
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BM25 k-scaling golden vector: raw → component.
    #[test]
    fn keyword_score_golden_vectors() {
        assert_eq!(keyword_score(0.0), 0.0);
        assert_eq!(keyword_score(2.0), 50.0);
        assert_eq!(keyword_score(4.0), 100.0);
        assert_eq!(keyword_score(8.0), 100.0, "capped at 1.0");
        assert_eq!(keyword_score(0.5), 12.5);
    }

    #[test]
    fn title_front_golden_vectors() {
        let kw = |s: &str| vec![s.to_string()];
        assert_eq!(title_front_score("Database Guide", &kw("database")), 100.0);
        assert_eq!(
            title_front_score("The Complete Guide to Databases", &kw("databases")),
            70.0, // position 5 → <=7
        );
        assert_eq!(
            title_front_score("An In Depth Look At Database Internals", &kw("database")),
            70.0, // position 6 → <=7
        );
        assert_eq!(
            title_front_score("An Extra Long Intro That Goes On For A While Database", &kw("database")),
            40.0, // position 11 → else
        );
        assert_eq!(title_front_score("Nothing To See", &kw("database")), 0.0);
    }

    #[test]
    fn title_length_golden_vectors() {
        assert_eq!(title_length_score(40), 100.0);
        assert_eq!(title_length_score(60), 100.0);
        assert_eq!(title_length_score(9), 30.0);
        assert_eq!(title_length_score(10), 60.0);
        assert_eq!(title_length_score(39), 89.0); // linear ramp 10→60 .. 40→90
        assert_eq!(title_length_score(80), 70.0);
        assert_eq!(title_length_score(120), 40.0);
        assert_eq!(title_length_score(300), 40.0);
    }

    #[test]
    fn title_hooks_golden_vectors() {
        assert_eq!(title_hooks_score("No Hooks Present Here"), 0.0);
        assert_eq!(title_hooks_score("5 Best Ways To Win"), 50.0); // digits + power
        assert_eq!(title_hooks_score("How to Build It"), 25.0);
        assert_eq!(title_hooks_score("The [Ultimate] 100x Guide"), 75.0); // 3 categories
        assert_eq!(title_hooks_score("Ultimate (Beginner) Tips"), 50.0); // brackets + power
        assert_eq!(title_hooks_score("10 Ultimate [Tips] How to Win"), 100.0); // cap
    }

    #[test]
    fn desc_first150_golden_vectors() {
        let kw = |s: &str| vec![s.to_string()];
        let mut early = "word ".repeat(28); // 140 chars of filler
        early.push_str("database appears early");
        assert_eq!(desc_first150_score(&early, &kw("database")), 100.0);

        let late = format!("{}database", "word ".repeat(100)); // after 150
        assert_eq!(desc_first150_score(&late, &kw("database")), 60.0);

        assert_eq!(desc_first150_score("no keywords in here", &kw("zzz")), 0.0);
        assert_eq!(desc_first150_score("", &kw("database")), 0.0);
    }

    #[test]
    fn desc_structure_golden_vectors() {
        assert_eq!(desc_structure_score("one line"), 0.0);
        assert_eq!(desc_structure_score("line one\nline two"), 25.0);
        let rich = "Overview\n- bullet one\n- bullet two\nStep 1: do it\n#tags\n0:00 intro";
        assert_eq!(desc_structure_score(rich), 100.0);
    }

    #[test]
    fn tags_relevance_golden_vectors() {
        // tags {database, rust} vs content {rust, database, guide} → 2/3.
        let tags = vec!["database".to_string(), "rust".to_string()];
        let s = tags_relevance_score("Rust Database Guide", "", &tags);
        assert!((s - 200.0 / 3.0).abs() < 1e-9, "got {s}");
        assert_eq!(tags_relevance_score("No Overlap", "", &tags), 0.0);
        assert_eq!(tags_relevance_score("x", "", &[]), 0.0);
    }

    #[test]
    fn tags_quality_golden_vectors() {
        let good = vec![
            "rust".to_string(),
            "database".to_string(),
            "guide".to_string(),
            "tutorial".to_string(),
        ];
        // count in [3,8] → 50; first tag in content → 25; >=2 tag tokens → 25.
        assert_eq!(tags_quality_score("Rust Database Guide", "", &good), 100.0);
        let too_few = vec!["rust".to_string()];
        assert_eq!(tags_quality_score("Rust Database Guide", "", &too_few), 25.0);
        let unrelated = vec![
            "x".to_string(),
            "y".to_string(),
            "z".to_string(),
            "w".to_string(),
        ];
        assert_eq!(tags_quality_score("Rust Database Guide", "", &unrelated), 50.0);
        assert_eq!(tags_quality_score("t", "", &[]), 0.0);
    }
}
