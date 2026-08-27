//! Alex Hormozi's $100M Offers Value Equation Engine.
//!
//! Evaluates content on the Attention Value Equation:
//! Score = (Dream Outcome x Perceived Likelihood) / (Time Delay x Effort & Sacrifice)

/// Compute the 0..=100 Value Equation score for title and description.
pub fn score(title: &str, description: &str) -> f64 {
    let lower_title = title.to_lowercase();
    let lower_desc = description.to_lowercase();
    let combined = format!("{lower_title} {lower_desc}");

    // 1. Dream Outcome (0..=10)
    let dream_keywords = [
        "master", "build", "scale", "earn", "grow", "solve", "dominate",
        "secret", "system", "blueprint", "complete", "architecture", "teardown",
        "breakthrough", "transform", "ultimate", "win"
    ];
    let mut dream_score: f64 = 4.0;
    for kw in &dream_keywords {
        if lower_title.contains(kw) { dream_score += 2.0; }
        else if lower_desc.contains(kw) { dream_score += 0.5; }
    }
    dream_score = dream_score.clamp(1.0, 10.0);

    // 2. Perceived Likelihood of Success (0..=10)
    let proof_keywords = [
        "step-by-step", "proven", "benchmark", "experiment", "evidence",
        "visual", "deep-dive", "explained", "summary", "how to", "breakdown",
        "guide", "in 10 minutes", "in 5 minutes", "illustrated"
    ];
    let mut proof_score: f64 = 4.0;
    for kw in &proof_keywords {
        if lower_title.contains(kw) { proof_score += 2.0; }
        else if lower_desc.contains(kw) { proof_score += 0.5; }
    }
    proof_score = proof_score.clamp(1.0, 10.0);

    // 3. Time Delay Friction (Lower is better, scale 1..=5)
    let time_savers = ["minutes", "fast", "quick", "instant", "short", "speed", "rapid"];
    let mut time_friction: f64 = 3.0;
    for kw in &time_savers {
        if lower_title.contains(kw) { time_friction -= 0.8; }
    }
    time_friction = time_friction.clamp(1.0, 5.0);

    // 4. Effort & Sacrifice Friction (Lower is better, scale 1..=5)
    let effort_reducers = ["without", "zero", "no ", "easy", "simple", "effortless", "eliminated"];
    let mut effort_friction: f64 = 3.0;
    for kw in &effort_reducers {
        if combined.contains(kw) { effort_friction -= 0.7; }
    }
    effort_friction = effort_friction.clamp(1.0, 5.0);

    // Composite Value Score (Normalized to 0..=100)
    let raw = (dream_score * proof_score) / (time_friction * effort_friction);
    (raw * 8.5).clamp(10.0, 100.0)
}
