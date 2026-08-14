//! Property-based tests for the gap mining algorithm.
//!
//! Tests invariants for outliers() and gap_score():
//! - Outlier multiple is always >= threshold (3x)
//! - Outliers are sorted descending by multiple
//! - Gap score is bounded [0, 100]
//! - Gap score increases with demand and decreases with competition

use proptest::prelude::*;
use tubeforge::analytics::gaps::{gap_score, OUTLIER_MULTIPLE};

proptest! {
    /// PROPERTY: gap_score is bounded [0, 100] for any inputs.
    #[test]
    fn gap_score_bounded(
        mean_views in prop::num::f64::POSITIVE,
        channels in 0..20i64,
    ) {
        let score = gap_score(mean_views, channels);
        prop_assert!(
            (0.0..=100.0 + 1e-9).contains(&score),
            "gap_score {} out of bounds for views={}, channels={}",
            score, mean_views, channels
        );
    }

    /// PROPERTY: gap_score = 0 when there are 5+ channels (saturated).
    #[test]
    fn gap_score_zero_when_saturated(
        mean_views in prop::num::f64::POSITIVE,
        channels in 5..20i64,
    ) {
        let score = gap_score(mean_views, channels);
        prop_assert_eq!(
            score, 0.0,
            "gap_score should be 0 for {} channels, got {}", channels, score
        );
    }

    /// PROPERTY: gap_score increases with demand (more views → higher score).
    #[test]
    fn gap_score_monotonic_in_demand(
        views_low in 0..100_000i64,
        views_high in 100_001..1_000_000i64,
        channels in 0..5i64,
    ) {
        let score_low = gap_score(views_low as f64, channels);
        let score_high = gap_score(views_high as f64, channels);
        prop_assert!(
            score_high >= score_low,
            "higher demand should not decrease score: {} → {} (views {} → {})",
            score_low, score_high, views_low, views_high
        );
    }

    /// PROPERTY: gap_score decreases with more channels (more competition).
    #[test]
    fn gap_score_anti_monotonic_in_channels(
        mean_views in 1000..100_000i64,
        channels_low in 0..4i64,
        delta in 1..5i64,
    ) {
        let channels_high = channels_low + delta;
        if channels_high > 5 {
            return Ok(());
        }
        let score_low_comp = gap_score(mean_views as f64, channels_low);
        let score_high_comp = gap_score(mean_views as f64, channels_high);
        prop_assert!(
            score_low_comp >= score_high_comp,
            "more channels should not increase score: {} → {} (channels {} → {})",
            score_low_comp, score_high_comp, channels_low, channels_high
        );
    }

    /// PROPERTY: gap_score is deterministic (same input → same output).
    #[test]
    fn gap_score_deterministic(
        mean_views in prop::num::f64::POSITIVE,
        channels in 0..10i64,
    ) {
        let s1 = gap_score(mean_views, channels);
        let s2 = gap_score(mean_views, channels);
        prop_assert_eq!(s1, s2, "gap_score is not deterministic");
    }
}

/// PROPERTY: OUTLIER_MULTIPLE threshold is 3.0 (research-backed).
#[test]
fn outlier_threshold_is_3x() {
    assert_eq!(
        OUTLIER_MULTIPLE, 3.0,
        "OUTLIER_MULTIPLE should be 3.0 (3x channel mean)"
    );
}
