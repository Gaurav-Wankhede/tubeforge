//! Forecast module — future-facing analysis over stored research history.
//!
//! Adds a lightweight **time-series forecasting layer** on top of the structured
//! DB (LLD §8). The DB stores *what happened* (keyword_research snapshots,
//! channel_snapshots); this module extrapolates *what is likely next* so the
//! system can auto-pick "which video topic to make next" and auto-draft
//! title/description/tags from forecast + score signals.
//!
//! Method (research-backed, 2026): **weighted linear regression (OLS) on
//! elapsed-time** is the most robust choice for sparse, irregularly-spaced
//! data (2–10 points). Hand-rolled — no external ML crate. Future upgrade path
//! is Holt's linear trend via `augurs-ets` once series exceed ~15–20 points.
//!
//! Reliability is honest: n<3 → no forecast; |t-stat| < 2.0 → verdict defaults
//! to FLAT; reliability tier reflects points + fit (LOW/MEDIUM/HIGH). The
//! current corpus is intra-day noise, so most verdicts are LOW confidence —
//! the tool says so rather than fake precision.

use serde::{Deserialize, Serialize};

/// Recency half-life (days): weights decay ~exp(-k·age). 30-day half-life.
const RECENCY_HALF_LIFE_DAYS: f64 = 30.0;
/// Default forecast horizon (days).
pub const DEFAULT_HORIZON_DAYS: f64 = 7.0;
/// % change over the horizon that counts as a "meaningful" trend.
const TREND_THRESHOLD_PCT: f64 = 10.0;
/// Minimum |t-statistic| to call a slope significant (≈95% with few dof).
const T_STAT_SIGNIFICANT: f64 = 2.0;
/// Minimum points to produce a forecast.
pub const MIN_POINTS: usize = 3;
/// Only apply recency weighting once enough points exist (else it ≈ last 2 pts).
const RECENCY_MIN_POINTS: usize = 5;

/// Trend verdict for a forecasted series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrendVerdict {
    Rising,
    Flat,
    Falling,
}

/// Reliability tier of a forecast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reliability {
    Low,
    Medium,
    High,
}

/// The result of forecasting one series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Forecast {
    pub verdict: TrendVerdict,
    /// Predicted value at the horizon.
    pub next_estimate: Option<f64>,
    /// Slope per day.
    pub slope_per_day: f64,
    /// Percent change over the horizon (comparable across metrics).
    pub pct_over_horizon: f64,
    pub reliability: Reliability,
    pub t_statistic: f64,
    pub r_squared: f64,
    pub points: usize,
}

/// A single (elapsed_days, value) point.
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub days: f64,
    pub value: f64,
}

/// Fit a weighted linear trend `value ~ a + b·days` and forecast the horizon.
///
/// - n < `MIN_POINTS` → `None` (can't distinguish signal from noise).
/// - Recency weights `exp(-k·age)` applied only when n ≥ `RECENCY_MIN_POINTS`.
/// - Verdict: |t-stat| < 2.0 → FLAT regardless of slope; else compare
///   `pct_over_horizon` against ±`TREND_THRESHOLD_PCT`.
/// - Reliability: LOW (n<4 or |t|<1.5) · MEDIUM (n 4–7, |t|≥2) · HIGH (n≥8, |t|≥2).
pub fn forecast(points: &[Point], horizon_days: f64) -> Option<Forecast> {
    let n = points.len();
    if n < MIN_POINTS {
        return None;
    }
    let horizon = if horizon_days <= 0.0 {
        DEFAULT_HORIZON_DAYS
    } else {
        horizon_days
    };

    // Recency weights (only when enough points).
    let t_max = points.iter().map(|p| p.days).fold(0.0_f64, f64::max);
    let k = std::f64::consts::LN_2 / RECENCY_HALF_LIFE_DAYS;
    let weights: Vec<f64> = if n >= RECENCY_MIN_POINTS {
        points
            .iter()
            .map(|p| (-k * (t_max - p.days)).exp())
            .collect()
    } else {
        vec![1.0; n]
    };

    // Weighted least squares.
    let wsum: f64 = weights.iter().sum();
    let t_bar = points
        .iter()
        .zip(&weights)
        .map(|(p, w)| w * p.days)
        .sum::<f64>()
        / wsum;
    let v_bar = points
        .iter()
        .zip(&weights)
        .map(|(p, w)| w * p.value)
        .sum::<f64>()
        / wsum;
    let sxx: f64 = points
        .iter()
        .zip(&weights)
        .map(|(p, w)| w * (p.days - t_bar).powi(2))
        .sum();
    let sxy: f64 = points
        .iter()
        .zip(&weights)
        .map(|(p, w)| w * (p.days - t_bar) * (p.value - v_bar))
        .sum();
    if sxx.abs() < 1e-12 {
        return None; // all points share one timestamp — no trend axis.
    }

    let slope = sxy / sxx;
    let intercept = v_bar - slope * t_bar;

    // Predicted values + residual variance.
    let mut sse = 0.0_f64;
    let mut syy = 0.0_f64;
    for (p, w) in points.iter().zip(&weights) {
        let yhat = intercept + slope * p.days;
        sse += w * (p.value - yhat).powi(2);
        syy += w * (p.value - v_bar).powi(2);
    }
    let r2 = if syy.abs() < 1e-12 {
        0.0
    } else {
        sxy * sxy / (sxx * syy)
    };
    let dof = (n as f64) - 2.0;
    let var = if dof > 0.0 { sse / dof } else { 0.0 };
    let se_slope = (var / sxx).sqrt();
    // A zero residual-variance fit (se_slope == 0) means the slope is perfectly
    // determined — treat it as unboundedly significant rather than non-significant.
    // Use a large finite sentinel so JSON serialization stays valid.
    let t_stat = if se_slope > 1e-12 {
        slope / se_slope
    } else if slope.abs() > 1e-12 {
        slope.signum() * f64::MAX
    } else {
        0.0
    };

    // Next-period estimate + % change over horizon.
    let t_pred = t_max + horizon;
    let next = intercept + slope * t_pred;
    let pct = if v_bar.abs() > 1e-12 {
        slope * horizon / v_bar.abs() * 100.0
    } else {
        0.0
    };

    // Verdict (robust: significant slope gate).
    let significant = t_stat.abs() >= T_STAT_SIGNIFICANT;
    let verdict = if significant && pct > TREND_THRESHOLD_PCT {
        TrendVerdict::Rising
    } else if significant && pct < -TREND_THRESHOLD_PCT {
        TrendVerdict::Falling
    } else {
        TrendVerdict::Flat
    };

    // Reliability tier.
    let reliability = if n < 4 || t_stat.abs() < 1.5 {
        Reliability::Low
    } else if n >= 8 && t_stat.abs() >= T_STAT_SIGNIFICANT {
        Reliability::High
    } else {
        Reliability::Medium
    };

    Some(Forecast {
        verdict,
        next_estimate: Some(next),
        slope_per_day: slope,
        pct_over_horizon: pct,
        reliability,
        t_statistic: t_stat,
        r_squared: r2,
        points: n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(vals: &[(f64, f64)]) -> Vec<Point> {
        vals.iter()
            .map(|&(d, v)| Point { days: d, value: v })
            .collect()
    }

    #[test]
    fn fewer_than_3_points_is_none() {
        assert!(forecast(&pts(&[(0.0, 10.0), (1.0, 11.0)]), 7.0).is_none());
        assert!(forecast(&[], 7.0).is_none());
    }

    #[test]
    fn rising_series_is_rising() {
        let f = forecast(
            &pts(&[
                (0.0, 10.0),
                (1.0, 12.0),
                (2.0, 14.0),
                (3.0, 16.0),
                (4.0, 18.0),
            ]),
            7.0,
        )
        .unwrap();
        assert_eq!(f.verdict, TrendVerdict::Rising);
        assert!(f.slope_per_day > 0.0);
        assert!(f.next_estimate.unwrap() > 18.0);
    }

    #[test]
    fn falling_series_is_falling() {
        let f = forecast(
            &pts(&[
                (0.0, 50.0),
                (1.0, 46.0),
                (2.0, 42.0),
                (3.0, 38.0),
                (4.0, 34.0),
            ]),
            7.0,
        )
        .unwrap();
        assert_eq!(f.verdict, TrendVerdict::Falling);
        assert!(f.next_estimate.unwrap() < 34.0);
    }

    #[test]
    fn flat_noisy_series_is_flat() {
        let f = forecast(
            &pts(&[(0.0, 10.0), (1.0, 11.0), (2.0, 9.0), (3.0, 10.5)]),
            7.0,
        )
        .unwrap();
        assert_eq!(f.verdict, TrendVerdict::Flat);
    }

    #[test]
    fn all_same_time_is_none() {
        assert!(forecast(&pts(&[(0.0, 10.0), (0.0, 11.0), (0.0, 9.0)]), 7.0).is_none());
    }
}
