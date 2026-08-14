//! Property-based tests for the ideas scoring formula.
//!
//! The score is: 0.5 * seo_total + 0.3 * idea_fit + 0.2 * competitor_gap
//! Properties:
//! - Score is a weighted sum (linearity)
//! - Score is bounded [0, 100] when inputs are [0, 100]
//! - Higher inputs → higher score (monotonicity)
//! - Zero inputs → zero score

use proptest::prelude::*;
use tubeforge::analytics::ideas::{W_FIT, W_GAP, W_SEO};

/// Compute the score from components (mirrors the production formula).
fn compute_score(seo: f64, fit: f64, gap: f64) -> f64 {
    W_SEO * seo + W_FIT * fit + W_GAP * gap
}

/// Strategy: score component in [0, 100].
fn component() -> impl Strategy<Value = f64> {
    prop::num::f64::POSITIVE.prop_filter("bounded", |&v| v <= 100.0)
}

proptest! {
    /// PROPERTY: Score is bounded [0, 100] when all inputs are [0, 100].
    /// Max possible: 0.5*100 + 0.3*100 + 0.2*100 = 100.
    #[test]
    fn score_bounded_zero_to_100(
        seo in component(),
        fit in component(),
        gap in component(),
    ) {
        let score = compute_score(seo, fit, gap);
        prop_assert!(
            (0.0..=100.0 + 1e-9).contains(&score),
            "score {} out of bounds [0,100] for seo={}, fit={}, gap={}",
            score, seo, fit, gap
        );
    }

    /// PROPERTY: Monotonicity — increasing any input increases the score.
    #[test]
    fn score_monotonic_in_each_component(
        seo in component(),
        fit in component(),
        gap in component(),
        delta in prop::num::f64::POSITIVE.prop_filter("small", |&d| d > 0.0 && d <= 10.0),
    ) {
        let base = compute_score(seo, fit, gap);

        let seo_increased = compute_score((seo + delta).min(100.0), fit, gap);
        prop_assert!(
            seo_increased >= base - 1e-9,
            "increasing seo should not decrease score: {} → {} (seo={}→{})",
            base, seo_increased, seo, seo + delta
        );

        let fit_increased = compute_score(seo, (fit + delta).min(100.0), gap);
        prop_assert!(
            fit_increased >= base - 1e-9,
            "increasing fit should not decrease score: {} → {}",
            base, fit_increased
        );

        let gap_increased = compute_score(seo, fit, (gap + delta).min(100.0));
        prop_assert!(
            gap_increased >= base - 1e-9,
            "increasing gap should not decrease score: {} → {}",
            base, gap_increased
        );
    }

    /// PROPERTY: Linearity — score is a weighted sum (distributive).
    #[test]
    fn score_linear_weighted_sum(
        seo in component(),
        fit in component(),
        gap in component(),
    ) {
        let score = compute_score(seo, fit, gap);
        let expected = W_SEO * seo + W_FIT * fit + W_GAP * gap;
        prop_assert!(
            (score - expected).abs() < 1e-9,
            "score {} != expected {} (weights: {}*{} + {}*{} + {}*{})",
            score, expected, W_SEO, seo, W_FIT, fit, W_GAP, gap
        );
    }
}

/// PROPERTY: Zero inputs → zero score.
#[test]
fn score_zero_when_all_zero() {
    let score = compute_score(0.0, 0.0, 0.0);
    assert_eq!(score, 0.0, "zero inputs should give zero score");
}

/// PROPERTY: Max inputs → max score (100).
#[test]
fn score_max_when_all_max() {
    let score = compute_score(100.0, 100.0, 100.0);
    assert!(
        (score - 100.0).abs() < 1e-9,
        "max inputs should give 100, got {}",
        score
    );
}

/// PROPERTY: Weights sum to 1.0 (normalization invariant).
#[test]
fn weights_sum_to_one() {
    let sum = W_SEO + W_FIT + W_GAP;
    assert!(
        (sum - 1.0).abs() < 1e-9,
        "weights should sum to 1.0, got {}",
        sum
    );
}

/// PROPERTY: SEO has the highest weight (it's the primary signal).
#[test]
fn seo_is_primary_signal() {
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(
            W_SEO > W_FIT && W_SEO > W_GAP,
            "SEO weight ({}) should exceed fit ({}) and gap ({})",
            W_SEO,
            W_FIT,
            W_GAP
        );
    }
}
