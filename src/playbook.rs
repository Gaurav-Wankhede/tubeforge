//! Founder Playbook & Content Psychology Engine (Rust 2024 Edition).
//!
//! Natively compiles 17 battle-tested startup, positioning, offer design,
//! persuasion, and behavioral psychology frameworks into TubeForge:
//!
//! - **$100M Offers** (Alex Hormozi): Attention Value Equation (Dream Outcome x Perceived Likelihood / Time Delay x Effort).
//! - **Made to Stick** (Chip & Dan Heath): SUCCESs 6-Vector Tensor (Simple, Unexpected, Concrete, Credible, Emotional, Stories).
//! - **Influence** (Robert Cialdini): 7 Levers of Compliance (Reciprocity, Commitment, Social Proof, Authority, Liking, Scarcity, Unity).
//! - **StoryBrand** (Donald Miller): SB7 Narrative Framework (Viewer=Hero, Creator=Guide, 3-Step Plan, Action CTA, Stakes).
//! - **Positioning & Blue Ocean** (April Dunford, Kim & Mauborgne): ERRC Grid & Differentiated Value Themes.
//! - **Customer Truth** (Rob Fitzpatrick): Mom Test Non-Leading Behavioral Validation.

pub mod contracts;
pub mod influence;
pub mod positioning;
pub mod storybrand;
pub mod success;
pub mod value_equation;

use serde::{Deserialize, Serialize};

/// Composite content psychology diagnostic score.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PsychologicalAudit {
    pub value_equation_score: f64,
    pub success_score: f64,
    pub influence_score: f64,
    pub overall_resonance: f64,
    pub detected_cialdini_levers: Vec<String>,
    pub success_dimensions: success::SuccessDimensions,
    pub recommendations: Vec<String>,
}

/// Perform a comprehensive psychological audit over a title, description, and hook.
pub fn audit_content(title: &str, description: &str, hook: Option<&str>) -> PsychologicalAudit {
    let ve = value_equation::score(title, description);
    let succ = success::score(title, description, hook);
    let inf = influence::detect(title, description);

    let mut detected_levers = Vec::new();
    if inf.reciprocity { detected_levers.push("Reciprocity".to_string()); }
    if inf.commitment { detected_levers.push("Commitment/Consistency".to_string()); }
    if inf.social_proof { detected_levers.push("Social Proof".to_string()); }
    if inf.authority { detected_levers.push("Authority".to_string()); }
    if inf.liking { detected_levers.push("Liking/Relatability".to_string()); }
    if inf.scarcity { detected_levers.push("Scarcity/Exclusivity".to_string()); }
    if inf.unity { detected_levers.push("Unity/In-Group".to_string()); }

    let overall = (ve * 0.35 + succ.total * 0.35 + inf.score * 0.30).clamp(0.0, 100.0);

    let mut recommendations = Vec::new();
    if ve < 60.0 {
        recommendations.push("Increase perceived likelihood or reduce time/effort friction in the title.".to_string());
    }
    if succ.unexpected < 50.0 {
        recommendations.push("Open a stronger curiosity gap or challenge common assumptions.".to_string());
    }
    if succ.concrete < 50.0 {
        recommendations.push("Replace abstract terminology with tangible, physical nouns.".to_string());
    }
    if detected_levers.is_empty() {
        recommendations.push("Anchor the content in authoritative benchmarks, Nobel citations, or scarcity.".to_string());
    }

    PsychologicalAudit {
        value_equation_score: ve,
        success_score: succ.total,
        influence_score: inf.score,
        overall_resonance: overall,
        detected_cialdini_levers: detected_levers,
        success_dimensions: succ.dimensions,
        recommendations,
    }
}
