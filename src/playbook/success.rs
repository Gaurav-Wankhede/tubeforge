//! Chip & Dan Heath's Made to Stick (SUCCESs Framework).
//!
//! Evaluates content on the 6 SUCCESs dimensions:
//! Simple, Unexpected, Concrete, Credible, Emotional, Stories.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuccessDimensions {
    pub simple: f64,
    pub unexpected: f64,
    pub concrete: f64,
    pub credible: f64,
    pub emotional: f64,
    pub stories: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SuccessResult {
    pub total: f64,
    pub unexpected: f64,
    pub concrete: f64,
    pub dimensions: SuccessDimensions,
}

/// Compute the 0..=100 SUCCESs score across title, description, and hook.
pub fn score(title: &str, description: &str, hook: Option<&str>) -> SuccessResult {
    let lower_title = title.to_lowercase();
    let lower_desc = description.to_lowercase();
    let hook_text = hook.unwrap_or("").to_lowercase();
    let text = format!("{lower_title} {lower_desc} {hook_text}");

    // 1. Simple (Length & Core focus: 30-50 chars title ideal, concise focus)
    let char_len = title.len();
    let simple_score = if (30..=50).contains(&char_len) && !title.contains(':') {
        90.0
    } else if char_len < 30 {
        75.0
    } else {
        55.0
    };

    // 2. Unexpected (Curiosity gap, counter-intuitive phrasing, paradoxes)
    let unexpected_triggers = [
        "why", "how", "lies to you", "broken", "stop", "never", "truth",
        "secret", "nobody", "actually", "fails", "myth", "real reason", "warning"
    ];
    let mut unexpected_score: f64 = 40.0;
    for trig in &unexpected_triggers {
        if lower_title.contains(trig) { unexpected_score += 25.0; }
        else if text.contains(trig) { unexpected_score += 8.0; }
    }
    unexpected_score = unexpected_score.clamp(20.0, 100.0);

    // 3. Concrete (Tangible nouns vs abstract fluff)
    let concrete_nouns = [
        "brain", "neuron", "silicon", "compiler", "chip", "money", "dollar",
        "habit", "code", "memory", "database", "vector", "rust", "scale", "system"
    ];
    let abstract_fluff = ["methodology", "paradigm", "synergy", "holistic", "utilize", "frameworks"];
    let mut concrete_score: f64 = 50.0;
    for n in &concrete_nouns {
        if text.contains(n) { concrete_score += 15.0; }
    }
    for f in &abstract_fluff {
        if text.contains(f) { concrete_score -= 15.0; }
    }
    concrete_score = concrete_score.clamp(20.0, 100.0);

    // 4. Credible (Authority, empirical research, experiments, numbers)
    let credibility_markers = [
        "kahneman", "cialdini", "nobel", "experiment", "study", "rfc", "benchmark",
        "data", "measured", "7 laws", "10 minutes", "100%", "tested"
    ];
    let mut credible_score: f64 = 40.0;
    for m in &credibility_markers {
        if text.contains(m) { credible_score += 20.0; }
    }
    credible_score = credible_score.clamp(20.0, 100.0);

    // 5. Emotional (Stakes, loss aversion, survival, empowerment)
    let emotional_triggers = [
        "lies", "fail", "break", "trap", "danger", "vulnerability", "master",
        "power", "freedom", "fear", "waste", "cost", "protect", "win"
    ];
    let mut emotional_score: f64 = 45.0;
    for e in &emotional_triggers {
        if text.contains(e) { emotional_score += 18.0; }
    }
    emotional_score = emotional_score.clamp(20.0, 100.0);

    // 6. Stories (Hero arc, conflict, journey)
    let story_triggers = ["when", "how to", "case study", "teardown", "story", "inside", "journey"];
    let mut story_score: f64 = 50.0;
    for s in &story_triggers {
        if text.contains(s) { story_score += 15.0; }
    }
    story_score = story_score.clamp(20.0, 100.0);

    let total = (simple_score * 0.20
        + unexpected_score * 0.25
        + concrete_score * 0.15
        + credible_score * 0.15
        + emotional_score * 0.15
        + story_score * 0.10)
        .clamp(0.0, 100.0);

    SuccessResult {
        total,
        unexpected: unexpected_score,
        concrete: concrete_score,
        dimensions: SuccessDimensions {
            simple: simple_score,
            unexpected: unexpected_score,
            concrete: concrete_score,
            credible: credible_score,
            emotional: emotional_score,
            stories: story_score,
        },
    }
}
