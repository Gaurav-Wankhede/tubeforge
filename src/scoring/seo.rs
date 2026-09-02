//! SEO components (LLD §7.2). Every function is deterministic over its
//! inputs. BM25-derived components reuse `search::bm25` against the tantivy
//! corpus (self-excluded for stored videos); structural components are pure
//! string heuristics.
//!
//! Keyword queries: when no target keywords are given, the title itself is
//! the query (Phase 1 basic-mode semantics — corpus resonance of the title),
//! so the Phase 1 `keyword_title`/`keyword_desc`/`keyword_tags` signals keep
//! working for draft scoring.

use std::collections::{HashMap, HashSet};

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
    "ultimate",
    "best",
    "top",
    "complete",
    "proven",
    "secret",
    "tips",
    "tricks",
    "guide",
    "fast",
    "easy",
    "simple",
    "powerful",
    "essential",
    "surprising",
    "unexpected",
    "mistakes",
    "beginner",
    "advanced",
];

/// All SEO components as computed by `compute`. Phase 6.6 added
/// five vidIQ-benchmark components (`title_40_chars`, `desc_first2lines`,
/// `desc_length`, `hashtag_count`, `keyword_triple`) alongside the original
/// ten (LLD §7.2). Phase 7 adds three graph-based components
/// (`tag_authority`, `topic_dominance`, `keyword_competition`).
#[derive(Debug, Clone)]
pub struct SeoComponents {
    pub keyword_title: f64,
    pub title_front: f64,
    pub title_length: f64,
    pub title_hooks: f64,
    pub title_40_chars: f64,
    pub keyword_desc: f64,
    pub desc_first150: f64,
    pub desc_first2lines: f64,
    pub desc_length: f64,
    pub desc_structure: f64,
    pub tags_relevance: f64,
    pub tags_quality: f64,
    pub keyword_tags: f64,
    pub hashtag_count: f64,
    pub keyword_triple: f64,
    /// Phase 7: tag authority — weighted by channel centrality.
    pub tag_authority: f64,
    /// Phase 7: topic dominance — channel's share of the topic cluster.
    pub topic_dominance: f64,
    /// Phase 7: keyword competition — incumbent authority for the keyword.
    pub keyword_competition: f64,
}

impl SeoComponents {
    /// (key, value) pairs in canonical order — the weighted-sum input.
    pub fn values(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("keyword_title", self.keyword_title),
            ("title_front", self.title_front),
            ("title_length", self.title_length),
            ("title_hooks", self.title_hooks),
            ("title_40_chars", self.title_40_chars),
            ("keyword_desc", self.keyword_desc),
            ("desc_first150", self.desc_first150),
            ("desc_first2lines", self.desc_first2lines),
            ("desc_length", self.desc_length),
            ("desc_structure", self.desc_structure),
            ("tags_relevance", self.tags_relevance),
            ("tags_quality", self.tags_quality),
            ("keyword_tags", self.keyword_tags),
            ("hashtag_count", self.hashtag_count),
            ("keyword_triple", self.keyword_triple),
            ("tag_authority", self.tag_authority),
            ("topic_dominance", self.topic_dominance),
            ("keyword_competition", self.keyword_competition),
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
/// Graph-based components default to 0 when no graph data is provided.
pub fn compute(
    title: &str,
    desc: &str,
    tags: &[String],
    keywords: &[String],
    bm25: &Bm25,
    exclude_video_id: Option<&str>,
) -> SeoComponents {
    compute_with_graph(
        title,
        desc,
        tags,
        keywords,
        bm25,
        exclude_video_id,
        None,
        None,
        None,
        None,
    )
}

/// Full SEO pass with graph data. The three graph-based components are
/// computed only when the corresponding graph signals are provided:
/// - `tag_authority_scores`: tag → authority score (from channel centrality)
/// - `topic_dominance_scores`: (channel_id, topic) → dominance score
/// - `kw_channel_edges`: keyword-channel dominance edges
///
/// When any of these is `None`, the corresponding component scores 0.
pub fn compute_with_graph(
    title: &str,
    desc: &str,
    tags: &[String],
    keywords: &[String],
    bm25: &Bm25,
    exclude_video_id: Option<&str>,
    tag_authority_scores: Option<&HashMap<String, f64>>,
    topic_dominance_scores: Option<&HashMap<(String, String), f64>>,
    kw_channel_edges: Option<&[crate::analytics::graph::KwChannelEdge]>,
    channel_id: Option<&str>,
) -> SeoComponents {
    let query = keywords.join(" ");

    // Compute graph-based components
    let tag_authority = compute_tag_authority(tags, tag_authority_scores);
    let topic_dominance = compute_topic_dominance(title, channel_id, topic_dominance_scores);
    let keyword_competition = compute_keyword_competition(keywords, kw_channel_edges);

    SeoComponents {
        keyword_title: keyword_score(bm25.corpus_resonance(FIELD_TITLE, &query, exclude_video_id)),
        title_front: title_front_score(title, keywords),
        title_length: title_length_score(title.chars().count()),
        title_hooks: title_hooks_score(title),
        title_40_chars: title_40_chars_score(title, keywords),
        keyword_desc: keyword_score(bm25.corpus_resonance(
            FIELD_DESCRIPTION,
            &query,
            exclude_video_id,
        )),
        desc_first150: desc_first150_score(desc, keywords),
        desc_first2lines: desc_first2lines_score(desc, keywords),
        desc_length: desc_length_score(desc),
        desc_structure: desc_structure_score(desc),
        tags_relevance: tags_relevance_score(title, desc, tags),
        tags_quality: tags_quality_score(title, desc, tags),
        keyword_tags: keyword_score(bm25.corpus_resonance(FIELD_TAGS, &query, exclude_video_id)),
        hashtag_count: hashtag_count_score(desc),
        keyword_triple: keyword_triple_score(title, desc, tags, keywords),
        tag_authority,
        topic_dominance,
        keyword_competition,
    }
}

/// Phase 7 `tag_authority`: the mean authority score of the video's tags,
/// weighted by channel centrality. Tags used by high-centrality channels
/// score higher. Normalized to 0-100.
fn compute_tag_authority(
    tags: &[String],
    tag_authority_scores: Option<&HashMap<String, f64>>,
) -> f64 {
    let scores = match tag_authority_scores {
        Some(s) => s,
        None => return 0.0,
    };
    if tags.is_empty() {
        return 0.0;
    }
    let mut total = 0.0;
    let mut count = 0;
    for t in tags {
        let t_lower = t.to_lowercase();
        if let Some(&score) = scores.get(&t_lower) {
            total += score;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (total / count as f64).min(100.0)
    }
}

/// Phase 7 `topic_dominance`: the channel's dominance in the video's topic
/// clusters. Computed as the max dominance score across all title tokens
/// that are also topic clusters. Normalized to 0-100.
fn compute_topic_dominance(
    title: &str,
    channel_id: Option<&str>,
    topic_dominance_scores: Option<&HashMap<(String, String), f64>>,
) -> f64 {
    let scores = match topic_dominance_scores {
        Some(s) => s,
        None => return 0.0,
    };
    let cid = match channel_id {
        Some(c) => c,
        None => return 0.0,
    };
    let title_tokens: HashSet<String> = util::tokens(title).into_iter().collect();
    let mut best = 0.0;
    for token in &title_tokens {
        if let Some(&score) = scores.get(&(cid.to_string(), token.clone())) {
            if score > best {
                best = score;
            }
        }
    }
    best.min(100.0)
}

/// Phase 7 `keyword_competition`: incumbent authority for the target keyword.
/// Higher when dominant channels already own the keyword (harder to rank).
/// Scored as the max dominance across the keyword's channel edges.
/// Normalized to 0-100 (high = competitive, low = opportunity).
fn compute_keyword_competition(
    keywords: &[String],
    kw_channel_edges: Option<&[crate::analytics::graph::KwChannelEdge]>,
) -> f64 {
    let edges = match kw_channel_edges {
        Some(e) => e,
        None => return 0.0,
    };
    if keywords.is_empty() {
        return 0.0;
    }
    let mut best = 0.0;
    for kw in keywords {
        for edge in edges {
            if edge.keyword == *kw && edge.dominance > best {
                best = edge.dominance;
            }
        }
    }
    (best * 100.0).min(100.0)
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
/// (10→60, 35..=49→100, 50..=60→90, 80→70, 120→40, longer → 40, shorter than 10 → 30).
pub fn title_length_score(len: usize) -> f64 {
    let len = len as f64;
    if (35.0..=49.0).contains(&len) {
        100.0
    } else if (50.0..=60.0).contains(&len) {
        90.0
    } else if len < 10.0 {
        30.0
    } else if len < 35.0 {
        60.0 + (len - 10.0) / 25.0 * 35.0
    } else if len < 80.0 {
        90.0 - (len - 60.0) / 20.0 * 20.0
    } else if len < 120.0 {
        70.0 - (len - 80.0) / 40.0 * 30.0
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
    let kw_tokens: HashSet<String> = keywords.iter().flat_map(|k| util::tokens(k)).collect();
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

/// Phase 6.6 `title_40_chars`: vidIQ's top signal — the primary keyword
/// must appear within the first 40 characters of the title (what search
/// results actually show). Keyword token in first 40 chars → 100; elsewhere
/// in the title → 60; absent → 0.
pub fn title_40_chars_score(title: &str, keywords: &[String]) -> f64 {
    let kw_tokens: HashSet<String> = keywords.iter().flat_map(|k| util::tokens(k)).collect();
    if kw_tokens.is_empty() {
        return 0.0;
    }
    let head: HashSet<String> = util::tokens(&title.chars().take(40).collect::<String>())
        .into_iter()
        .collect();
    if head.intersection(&kw_tokens).next().is_some() {
        100.0
    } else {
        let all: HashSet<String> = util::tokens(title).into_iter().collect();
        if all.intersection(&kw_tokens).next().is_some() {
            60.0
        } else {
            0.0
        }
    }
}

/// Phase 6.6 `desc_first2lines`: Alan Spicer's guidance — the first two
/// lines of the description are what YouTube crawls for context. Keyword
/// token in the first two non-empty lines → 100; elsewhere → 60; absent → 0.
pub fn desc_first2lines_score(desc: &str, keywords: &[String]) -> f64 {
    let kw_tokens: HashSet<String> = keywords.iter().flat_map(|k| util::tokens(k)).collect();
    if kw_tokens.is_empty() {
        return 0.0;
    }
    let first_two: String = desc
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let head: HashSet<String> = util::tokens(&first_two).into_iter().collect();
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

/// Phase 6.6 `desc_length`: 200+ words is the sweet spot (Alan Spicer).
/// Piecewise: <50 → 40, 50-99 → 60, 100-149 → 75, 150-199 → 90, 200+ → 100.
pub fn desc_length_score(desc: &str) -> f64 {
    let words = desc.split_whitespace().count();
    match words {
        0..=49 => 40.0,
        50..=99 => 60.0,
        100..=149 => 75.0,
        150..=199 => 90.0,
        _ => 100.0,
    }
}

/// Phase 6.6 `hashtag_count`: 3–5 hashtags is the sweet spot (Alan Spicer);
/// 1–2 or 6–8 are suboptimal but not fatal; 0 or 9+ hurt.
pub fn hashtag_count_score(desc: &str) -> f64 {
    let count = desc
        .split_whitespace()
        .filter(|w| w.starts_with('#') && w.chars().count() > 1)
        .count();
    match count {
        3..=5 => 100.0,
        1..=2 | 6..=8 => 60.0,
        0 => 0.0,
        _ => 40.0,
    }
}

/// Phase 6.6 `keyword_triple`: the same keyword used in the three most
/// important places — title, description, AND tags (vidIQ "Triple Keywords").
/// A keyword present in all three → 100; two placements → 60; one → 30.
/// Scored per-keyword, best placement count wins.
pub fn keyword_triple_score(title: &str, desc: &str, tags: &[String], keywords: &[String]) -> f64 {
    let title_t: HashSet<String> = util::tokens(title).into_iter().collect();
    let desc_t: HashSet<String> = util::tokens(desc).into_iter().collect();
    let tag_t: HashSet<String> = tags.iter().flat_map(|t| util::tokens(t)).collect();
    let mut best = 0usize;
    for kw in keywords {
        let kw_t = util::tokens(kw);
        if kw_t.is_empty() {
            continue;
        }
        let mut placements = 0usize;
        if kw_t.iter().all(|t| title_t.contains(t)) {
            placements += 1;
        }
        if kw_t.iter().all(|t| desc_t.contains(t)) {
            placements += 1;
        }
        if kw_t.iter().all(|t| tag_t.contains(t)) {
            placements += 1;
        }
        best = best.max(placements);
    }
    match best {
        3 => 100.0,
        2 => 60.0,
        1 => 30.0,
        _ => 0.0,
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
        l.chars().next().is_some_and(|c| c.is_ascii_digit()) || l.to_lowercase().starts_with("step")
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

/// Phase 6.6 `tags_quality` (vidIQ-family rework): tag count 15–30 is the
/// optimal band (Alan Spicer) → +50; 5–14 (vidIQ minimum is 5) or 31–50
/// (too many dilutes) → +35; 1–4 → +15; 0 → 0. Plus: first tag matches
/// content → +25, ≥2 tag tokens match content → +25.
pub fn tags_quality_score(title: &str, desc: &str, tags: &[String]) -> f64 {
    if tags.is_empty() {
        return 0.0;
    }
    let mut score = 0.0;
    match tags.len() {
        15..=30 => score += 50.0,
        5..=14 | 31..=50 => score += 35.0,
        _ => score += 15.0, // 1-4 or 51+ — too few / tag stuffing
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
            title_front_score(
                "An Extra Long Intro That Goes On For A While Database",
                &kw("database")
            ),
            40.0, // position 11 → else
        );
        assert_eq!(title_front_score("Nothing To See", &kw("database")), 0.0);
    }

    #[test]
    fn title_length_golden_vectors() {
        assert_eq!(title_length_score(40), 100.0);
        assert_eq!(title_length_score(60), 90.0);
        assert_eq!(title_length_score(9), 30.0);
        assert_eq!(title_length_score(10), 60.0);
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
        // 4 tags → now the 1-4 band → 15; first tag in content → 25;
        // >=2 tag tokens → 25 → 65.
        let few = vec![
            "rust".to_string(),
            "database".to_string(),
            "guide".to_string(),
            "tutorial".to_string(),
        ];
        assert_eq!(
            tags_quality_score("Rust Database Guide", "", &few),
            65.0,
            "4 tags: thin band + both content matches"
        );
        // 15-30 tags → optimal band → 50 + first tag 25 + >=2 tokens 25 = 100.
        let mut optimal: Vec<String> = vec![
            "rust".to_string(),
            "database".to_string(),
            "guide".to_string(),
        ];
        optimal.extend((0..12).map(|i| format!("related{i}")));
        assert_eq!(optimal.len(), 15);
        assert_eq!(
            tags_quality_score("Rust Database Guide", "", &optimal),
            100.0,
            "15 tags with content matches is the optimal band"
        );
        let too_few = vec!["rust".to_string()];
        assert_eq!(
            tags_quality_score("Rust Database Guide", "", &too_few),
            40.0 // 15 + first-tag 25
        );
        let unrelated = vec![
            "x".to_string(),
            "y".to_string(),
            "z".to_string(),
            "w".to_string(),
        ];
        assert_eq!(
            tags_quality_score("Rust Database Guide", "", &unrelated),
            15.0 // 1-4 band, no content matches
        );
        assert_eq!(tags_quality_score("t", "", &[]), 0.0);
    }

    #[test]
    fn title_40_chars_golden_vectors() {
        let kw = |s: &str| vec![s.to_string()];
        // "Database" within first 40 chars.
        assert_eq!(
            title_40_chars_score(
                "Database Guide: Everything You Need to Know",
                &kw("database")
            ),
            100.0
        );
        // Keyword after char 40 but present.
        assert_eq!(
            title_40_chars_score(
                "An Extra Long Title That Goes On And On And On database internals",
                &kw("database")
            ),
            60.0
        );
        assert_eq!(title_40_chars_score("Nothing to see", &kw("zzz")), 0.0);
        assert_eq!(title_40_chars_score("", &kw("database")), 0.0);
        assert_eq!(title_40_chars_score("Database", &[]), 0.0);
    }

    #[test]
    fn desc_first2lines_golden_vectors() {
        let kw = |s: &str| vec![s.to_string()];
        assert_eq!(
            desc_first2lines_score(
                "In this video, database tuning.\nThen more.",
                &kw("database")
            ),
            100.0
        );
        // Keyword in line 3 → not first two lines → 60.
        assert_eq!(
            desc_first2lines_score(
                "First line without it.\nSecond line plain.\nThird line database here.",
                &kw("database")
            ),
            60.0
        );
        assert_eq!(desc_first2lines_score("nothing here", &kw("zzz")), 0.0);
        assert_eq!(desc_first2lines_score("", &kw("database")), 0.0);
    }

    #[test]
    fn desc_length_golden_vectors() {
        let words_50 = vec!["word"; 50].join(" ");
        let words_200 = vec!["word"; 200].join(" ");
        assert_eq!(desc_length_score(""), 40.0);
        assert_eq!(desc_length_score(&words_50), 60.0);
        assert_eq!(desc_length_score(&vec!["word"; 120].join(" ")), 75.0);
        assert_eq!(desc_length_score(&vec!["word"; 180].join(" ")), 90.0);
        assert_eq!(desc_length_score(&words_200), 100.0);
    }

    #[test]
    fn hashtag_count_golden_vectors() {
        assert_eq!(hashtag_count_score(""), 0.0);
        assert_eq!(hashtag_count_score("#one #two"), 60.0);
        assert_eq!(hashtag_count_score("#one #two #three"), 100.0);
        assert_eq!(hashtag_count_score("#a #b #c #d #e #f"), 60.0);
        assert_eq!(hashtag_count_score("#a #b #c #d #e #f #g #h #i #j"), 40.0);
        // "#" alone is not a hashtag.
        assert_eq!(hashtag_count_score("# #two"), 60.0);
    }

    #[test]
    fn keyword_triple_golden_vectors() {
        let kw = vec!["rust database".to_string()];
        let tags = vec!["rust".to_string(), "database".to_string()];
        // Keyword in title + desc + tags → triple.
        assert_eq!(
            keyword_triple_score(
                "Rust Database Guide",
                "A rust database walkthrough.",
                &tags,
                &kw
            ),
            100.0
        );
        // Title + tags only (no desc) → 60.
        assert_eq!(
            keyword_triple_score("Rust Database Guide", "", &tags, &kw),
            60.0
        );
        // Title only → 30.
        assert_eq!(
            keyword_triple_score("Rust Database Guide", "", &[], &kw),
            30.0
        );
        assert_eq!(keyword_triple_score("Nothing", "", &[], &kw), 0.0);
        assert_eq!(keyword_triple_score("Rust", "", &tags, &[]), 0.0);
    }

    // --- Phase 7 graph-based component tests ---

    #[test]
    fn tag_authority_zero_when_no_graph_data() {
        let tags = vec!["rust".to_string()];
        assert_eq!(compute_tag_authority(&tags, None), 0.0);
    }

    #[test]
    fn tag_authority_mean_of_tag_scores() {
        let tags = vec!["rust".to_string(), "async".to_string()];
        let mut scores = HashMap::new();
        scores.insert("rust".to_string(), 80.0);
        scores.insert("async".to_string(), 60.0);
        // Mean = 70.0
        assert_eq!(compute_tag_authority(&tags, Some(&scores)), 70.0);
    }

    #[test]
    fn tag_authority_case_insensitive() {
        let tags = vec!["Rust".to_string()];
        let mut scores = HashMap::new();
        scores.insert("rust".to_string(), 80.0);
        assert_eq!(compute_tag_authority(&tags, Some(&scores)), 80.0);
    }

    #[test]
    fn tag_authority_empty_tags() {
        let tags: Vec<String> = vec![];
        let mut scores = HashMap::new();
        scores.insert("rust".to_string(), 80.0);
        assert_eq!(compute_tag_authority(&tags, Some(&scores)), 0.0);
    }

    #[test]
    fn topic_dominance_zero_when_no_graph_data() {
        assert_eq!(compute_topic_dominance("Rust guide", Some("A"), None), 0.0);
    }

    #[test]
    fn topic_dominance_max_across_title_tokens() {
        let mut scores = HashMap::new();
        scores.insert(("A".to_string(), "rust".to_string()), 75.0);
        scores.insert(("A".to_string(), "guide".to_string()), 30.0);
        // Max across title tokens = 75.0
        assert_eq!(
            compute_topic_dominance("Rust guide", Some("A"), Some(&scores)),
            75.0
        );
    }

    #[test]
    fn topic_dominance_zero_when_no_channel() {
        let mut scores = HashMap::new();
        scores.insert(("A".to_string(), "rust".to_string()), 75.0);
        assert_eq!(
            compute_topic_dominance("Rust guide", None, Some(&scores)),
            0.0
        );
    }

    #[test]
    fn keyword_competition_zero_when_no_graph_data() {
        let kw = vec!["rust".to_string()];
        assert_eq!(compute_keyword_competition(&kw, None), 0.0);
    }

    #[test]
    fn keyword_competition_max_dominance() {
        use crate::analytics::graph::KwChannelEdge;
        let kw = vec!["rust".to_string()];
        let edges = vec![
            KwChannelEdge {
                keyword: "rust".to_string(),
                channel_id: "A".to_string(),
                dominance: 0.8,
                video_count: 5,
                mean_views: 10000.0,
            },
            KwChannelEdge {
                keyword: "rust".to_string(),
                channel_id: "B".to_string(),
                dominance: 0.3,
                video_count: 2,
                mean_views: 5000.0,
            },
        ];
        // Max dominance = 0.8 → 80.0
        assert_eq!(compute_keyword_competition(&kw, Some(&edges)), 80.0);
    }

    #[test]
    fn compute_with_graph_includes_all_components() {
        let dir = tempfile::tempdir().expect("tempdir");
        let index = crate::search::new_index(&dir.path().join("idx")).expect("index");
        let bm25 = Bm25::open(index).expect("bm25");

        let mut tag_auth = HashMap::new();
        tag_auth.insert("rust".to_string(), 80.0);

        let mut topic_dom = HashMap::new();
        topic_dom.insert(("A".to_string(), "rust".to_string()), 75.0);

        let result = compute_with_graph(
            "Rust async guide",
            "A rust async walkthrough.",
            &["rust".to_string(), "async".to_string()],
            &["rust".to_string()],
            &bm25,
            None,
            Some(&tag_auth),
            Some(&topic_dom),
            None,
            Some("A"),
        );

        assert!(result.tag_authority > 0.0, "tag_authority should be > 0");
        assert!(
            result.topic_dominance > 0.0,
            "topic_dominance should be > 0"
        );
    }
}
