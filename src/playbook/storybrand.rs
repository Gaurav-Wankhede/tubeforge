//! Donald Miller's StoryBrand (SB7 Framework) Narrative Architecture.
//!
//! Validates that video scripts position the viewer as the Hero and the
//! creator as the Guide.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoryBrandAudit {
    pub hero_defined: bool,
    pub problem_clear: bool,
    pub guide_present: bool,
    pub plan_provided: bool,
    pub cta_actionable: bool,
    pub stakes_articulated: bool,
    pub narrative_score: f64,
}

/// Audit script / packaging against the 7 StoryBrand elements.
pub fn audit_script(script_text: &str) -> StoryBrandAudit {
    let lower = script_text.to_lowercase();

    let hero_defined = lower.contains("you") || lower.contains("your");
    let problem_clear = lower.contains("problem") || lower.contains("bias") || lower.contains("fail") || lower.contains("lie");
    let guide_present = lower.contains("we") || lower.contains("blueprint") || lower.contains("system") || lower.contains("breakdown");
    let plan_provided = lower.contains("step") || lower.contains("framework") || lower.contains("architecture") || lower.contains("first");
    let cta_actionable = lower.contains("watch") || lower.contains("next") || lower.contains("check out") || lower.contains("click");
    let stakes_articulated = lower.contains("avoid") || lower.contains("cost") || lower.contains("master") || lower.contains("protect");

    let mut points: f64 = 20.0;
    if hero_defined { points += 15.0; }
    if problem_clear { points += 15.0; }
    if guide_present { points += 15.0; }
    if plan_provided { points += 15.0; }
    if cta_actionable { points += 10.0; }
    if stakes_articulated { points += 10.0; }

    StoryBrandAudit {
        hero_defined,
        problem_clear,
        guide_present,
        plan_provided,
        cta_actionable,
        stakes_articulated,
        narrative_score: points.clamp(0.0, 100.0),
    }
}
