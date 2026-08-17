//! Packaging-psychology scoring (PRD v4.2 supporting layer).
//!
//! A quantified detector/scorer for the high-CTR title patterns used by the
//! researched creators (Dan Martell, Alex Hormozi, Jeff Su, Liam Ottley):
//!
//! 1. **Time-anchor** — "Give me 60 seconds…", "in 5 minutes".
//! 2. **Precise non-round number + extreme outcome** — "The 7 Brutal Truths…",
//!    "37,000 views in 30 days".
//! 3. **Income/wealth claim** — "earn $10k/month", "make money".
//! 4. **Forbidden-knowledge / "feels illegal" frame** — "nobody tells you",
//!    "feels illegal", "the secret that…".
//! 5. **How-to + identity/age constraint** — "How to write for beginners",
//!    "for people over 40".
//!
//! Design contract (PRD v4.2): SEO Strategy is the PRIMARY product; this
//! psychology score is a **supporting** signal that boosts CTR/ranking
//! potential. It is computed separately and surfaced alongside (not blended
//! into) the SEO/GEO totals, so the SEO score stays the honest, primary
//! rank-or-not signal and the psychology score explains CTR lift.

use serde::{Deserialize, Serialize};

/// The five researched-creator title formulas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleFormula {
    TimeAnchor,
    PreciseNumber,
    IncomeClaim,
    ForbiddenKnowledge,
    HowToIdentity,
}

impl TitleFormula {
    pub const ALL: [TitleFormula; 5] = [
        TitleFormula::TimeAnchor,
        TitleFormula::PreciseNumber,
        TitleFormula::IncomeClaim,
        TitleFormula::ForbiddenKnowledge,
        TitleFormula::HowToIdentity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TitleFormula::TimeAnchor => "Time anchor",
            TitleFormula::PreciseNumber => "Precise number + extreme outcome",
            TitleFormula::IncomeClaim => "Income / wealth claim",
            TitleFormula::ForbiddenKnowledge => "Forbidden knowledge / feels illegal",
            TitleFormula::HowToIdentity => "How-to + identity / age constraint",
        }
    }
}

/// Result of scoring one title against the psychology patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PsychScore {
    /// 0..=100 composite. 0 = no high-CTR psychology detected (pure search
    /// style). Not blended into the SEO total — a separate supporting signal.
    pub total: f64,
    /// Which formulas were detected in this title.
    pub detected: Vec<TitleFormula>,
    /// Per-formula evidence: the matched fragment (first match per formula).
    pub evidence: Vec<(TitleFormula, String)>,
}

/// Detect the psychology formulas present in `title`. Returns the detected
/// formulas in canonical order, with the matched fragment as evidence.
pub fn detect(title: &str) -> Vec<(TitleFormula, String)> {
    let lower = title.to_lowercase();
    let mut out: Vec<(TitleFormula, String)> = Vec::new();
    for f in TitleFormula::ALL {
        if let Some(ev) = evidence(f, &lower, title) {
            out.push((f, ev));
        }
    }
    out
}

/// Compute the 0..=100 psychology score for a title.
///
/// Each detected formula contributes a base 20 points; precise-number gets a
/// small bonus when it's paired with an extreme-outcome framing (the strongest
/// Martell/Hormozi pattern), capped at 100.
pub fn score(title: &str) -> PsychScore {
    let detected_ev = detect(title);
    let detected: Vec<TitleFormula> = detected_ev.iter().map(|(f, _)| *f).collect();
    let raw = detected_ev.len() as f64 * 20.0;
    let bonus = if detected.contains(&TitleFormula::PreciseNumber)
        && extreme_outcome_present(&title.to_lowercase())
    {
        10.0
    } else {
        0.0
    };
    PsychScore {
        total: (raw + bonus).min(100.0).round(),
        detected,
        evidence: detected_ev,
    }
}

/// Whether the title carries an "extreme outcome" framing (numbers, gains,
/// harsh truths, etc.) — boosts the precise-number pattern.
fn extreme_outcome_present(lower: &str) -> bool {
    const OUTCOME_WORDS: [&str; 14] = [
        "truth",
        "brutal",
        "mistake",
        "mistakes",
        "secret",
        "earn",
        "views",
        "subscribers",
        "money",
        "income",
        "million",
        "thousand",
        "double",
        "grow",
    ];
    OUTCOME_WORDS.iter().any(|w| lower.contains(w))
}

/// The identity/audience constraint words for the how-to pattern.
const IDENTITY_WORDS: [&str; 8] = [
    "beginners",
    "developers",
    "engineers",
    "startups",
    "over 40",
    "over 30",
    "for kids",
    "for women",
];

/// Match a single formula against the lowercased title, returning the matched
/// fragment (from the original casing) as evidence.
fn evidence(f: TitleFormula, lower: &str, original: &str) -> Option<String> {
    let frag = match f {
        TitleFormula::TimeAnchor => {
            const WORDS: [&str; 10] = [
                "second", "seconds", "minute", "minutes", "hour", "hours", "in 60", "in 30",
                "in 10", "fast",
            ];
            first_match(lower, &WORDS)
        }
        TitleFormula::PreciseNumber => precise_number(lower),
        TitleFormula::IncomeClaim => {
            const WORDS: [&str; 8] = [
                "$", "money", "income", "earn", "salary", "revenue", "million", "profit",
            ];
            first_match(lower, &WORDS)
        }
        TitleFormula::ForbiddenKnowledge => {
            const WORDS: [&str; 13] = [
                "feels illegal",
                "feel illegal",
                "nobody tells",
                "no one tells",
                "they don't want you",
                "the secret",
                "secret that",
                "hidden",
                "forbidden",
                "not allowed",
                "never told",
                "aren't told",
                "aren’t told",
            ];
            first_match(lower, &WORDS)
        }
        TitleFormula::HowToIdentity => {
            let has_how = lower.contains("how to") || lower.contains("how i");
            let identity = IDENTITY_WORDS.iter().find(|w| lower.contains(**w)).copied();
            match (has_how, identity) {
                (true, _) => Some("how-to"),
                (false, Some(_)) => Some("identity"),
                (false, None) => None,
            }
        }
    };
    frag.map(|m| matched_fragment(original, m))
}

/// Find the first keyword present in `lower`, returned lowercased.
fn first_match(lower: &str, words: &'static [&'static str]) -> Option<&'static str> {
    words.iter().find(|w| lower.contains(**w)).copied()
}

/// Detect a precise, non-round number (2-3 digit run, allowing thousands
/// separators) in the title.
fn precise_number(lower: &str) -> Option<&'static str> {
    // Only match 2-3 digit "small precise" numbers (round 10s like "10" are
    // deliberately excluded — the pattern is about non-round precision).
    let has_digit = lower.chars().any(|c| c.is_ascii_digit());
    if !has_digit {
        return None;
    }
    // Crude but effective: a run like `N` where N is 2-3 digits not ending
    // in a round 0, OR a 4+ digit number (37,000).
    let chars: Vec<char> = lower.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut j = i;
            let mut digits = String::new();
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == ',') {
                if chars[j].is_ascii_digit() {
                    digits.push(chars[j]);
                }
                j += 1;
            }
            if is_precise_number(&digits) {
                return Some("precise-number");
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

/// A "precise" number: a single non-zero digit (7), a 2-3 digit number NOT
/// ending in 0 (13, 37, 275), or any 4+ digit quantity (37,000). Round tens
/// (10, 20, 100) are deliberately excluded — the pattern is about precision.
fn is_precise_number(s: &str) -> bool {
    let n = s.len();
    if n == 0 {
        return false;
    }
    if n == 1 {
        let d = s.chars().next().unwrap_or('0');
        return d != '0'; // 1..=9 are precise
    }
    if n >= 4 {
        return true; // 37,000 / 1,234 — precise quantity
    }
    let last = s.chars().last().unwrap_or('0');
    !(last == '0')
}

/// Recover the original-cased fragment matched by a lowercased keyword. For
/// the number case there is no clean 1:1 keyword, so we return the pattern
/// name itself (callers display evidence; a number title has obvious
/// evidence). We return `matched` unchanged — callers use it as a label.
fn matched_fragment(_original: &str, matched: &str) -> String {
    matched.to_string()
}

/// Generate ranked high-CTR title variants for a topic (Martell/Hormozi-style).
/// `outcome` is an optional extreme-outcome phrase (e.g. "the brutal truth").
pub fn variants(topic: &str, outcome: Option<&str>) -> Vec<String> {
    let t = topic.trim().trim_end_matches('.');
    if t.is_empty() {
        return Vec::new();
    }
    let oc = outcome.unwrap_or("");
    let mut v: Vec<String> = Vec::new();
    // 1. Time anchor
    v.push(format!("Give Me 60 Seconds on {t}"));
    // 2. Precise number + outcome
    if !oc.is_empty() {
        v.push(format!("The 7 {oc}s About {t}"));
    } else {
        v.push(format!("7 Things I Wish I Knew About {t}"));
    }
    // 3. Forbidden knowledge
    v.push(format!("The Secret About {t} Nobody Tells You"));
    // 4. How-to + identity
    v.push(format!("How to Master {t} (for Beginners)"));
    // 5. Income frame (only when an outcome is supplied)
    if !oc.is_empty() {
        v.push(format!("How {t} {oc} (My Exact Process)"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::{prop_assert, prop_assert_eq};

    #[test]
    fn time_anchor_detected() {
        let s = score("Give Me 60 Seconds on Rust Databases");
        assert!(s.detected.contains(&TitleFormula::TimeAnchor));
        assert!(s.total >= 20.0);
    }

    #[test]
    fn precise_number_with_outcome_bonus() {
        let s = score("The 7 Brutal Truths About Marketing");
        assert!(s.detected.contains(&TitleFormula::PreciseNumber));
        // 20 base + 10 extreme-outcome bonus.
        assert_eq!(s.total, 30.0);
    }

    #[test]
    fn income_claim_detected() {
        let s = score("How I Earn $10,000 a Month");
        assert!(s.detected.contains(&TitleFormula::IncomeClaim));
    }

    #[test]
    fn forbidden_knowledge_detected() {
        let s = score("The Secret About Youtube Nobody Tells You");
        assert!(s.detected.contains(&TitleFormula::ForbiddenKnowledge));
    }

    #[test]
    fn how_to_identity_detected() {
        let s = score("How to Build a SaaS for Beginners");
        assert!(s.detected.contains(&TitleFormula::HowToIdentity));
    }

    #[test]
    fn round_numbers_are_not_precise() {
        // "10" ends in 0 → not precise. No psychology detected.
        let s = score("10 Tips for Marketing");
        assert!(!s.detected.contains(&TitleFormula::PreciseNumber));
        assert_eq!(s.total, 0.0);
    }

    #[test]
    fn empty_title_scores_zero() {
        let s = score("");
        assert_eq!(s.total, 0.0);
        assert!(s.detected.is_empty());
    }

    #[test]
    fn multiple_patterns_accumulate_capped_at_100() {
        let s =
            score("The 7 Secrets That Feel Illegal to Earn $1M — Nobody Tells You in 60 Seconds");
        // precise + forbidden + income + time-anchor + (how-to absent) = 80 + bonus.
        assert!(s.total >= 80.0);
        assert!(s.total <= 100.0);
    }

    #[test]
    fn variants_are_nonempty_and_formulaic() {
        let v = variants("Rust", Some("the brutal truth"));
        assert!(!v.is_empty());
        assert!(v.iter().any(|t| t.contains("60 Seconds")));
        assert!(v.iter().any(|t| t.contains("Secret")));
        assert!(v.iter().any(|t| t.contains("Nobody Tells You")));
        assert!(v.iter().any(|t| t.contains("Beginners")));
    }

    proptest::proptest! {
        // Any title scores within [0,100] and detected evidence is non-empty.
        #[test]
        fn score_is_bounded_and_consistent(
            title in ".*",
        ) {
            let s = score(&title);
            prop_assert!((0.0..=100.0).contains(&s.total), "score in range");
            prop_assert_eq!(s.detected.len(), s.evidence.len(), "detected == evidence");
            // A detected formula always contributes exactly 20 (plus bonus).
            let base = s.detected.len() as f64 * 20.0;
            prop_assert!(s.total >= base, "total >= base sum");
        }
    }

    proptest::proptest! {
        // Variants are always generated for a non-empty topic and non-empty.
        #[test]
        fn variants_are_never_empty_for_valid_topic(
            topic in "[A-Za-z][A-Za-z ]{0,39}",
        ) {
            let v = variants(&topic, Some("the truth"));
            prop_assert!(!v.is_empty(), "variants for {topic:?}");
            for t in &v {
                prop_assert!(!t.trim().is_empty(), "variant not blank");
            }
        }
    }
}
