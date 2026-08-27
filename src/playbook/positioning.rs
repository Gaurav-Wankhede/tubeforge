//! April Dunford's Positioning & W. Chan Kim's Blue Ocean Strategy (ERRC Grid).
//!
//! Provides differentiated value theme analysis and competitive positioning metrics.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrcGrid {
    pub eliminate: Vec<String>,
    pub reduce: Vec<String>,
    pub raise: Vec<String>,
    pub create: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PositioningScore {
    pub differentiation_score: f64,
    pub category_clarity: f64,
    pub errc_grid: ErrcGrid,
}

/// Evaluate positioning strength and generate default ERRC recommendations.
pub fn evaluate_positioning(topic: &str, target_audience: &str) -> PositioningScore {
    let mut errc = ErrcGrid::default();
    errc.eliminate.push("Generic presentation slide bullet points".to_string());
    errc.reduce.push("Unnecessary filler intros and background music".to_string());
    errc.raise.push("Domain-grounded vector architectural precision".to_string());
    errc.create.push("Continuous deterministic GSAP state morphs on open black canvas".to_string());

    let differentiation = if topic.contains("Rust") || topic.contains("Compiler") || topic.contains("Psychology") {
        88.0
    } else {
        70.0
    };

    let clarity = if !target_audience.is_empty() { 90.0 } else { 65.0 };

    PositioningScore {
        differentiation_score: differentiation,
        category_clarity: clarity,
        errc_grid: errc,
    }
}
