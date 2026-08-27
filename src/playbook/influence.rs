//! Robert Cialdini's 7 Levers of Influence & Compliance.
//!
//! Scans content packaging and architecture for persuasion triggers:
//! Reciprocity, Commitment/Consistency, Social Proof, Authority, Liking, Scarcity, Unity.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InfluenceDetection {
    pub reciprocity: bool,
    pub commitment: bool,
    pub social_proof: bool,
    pub authority: bool,
    pub liking: bool,
    pub scarcity: bool,
    pub unity: bool,
    pub score: f64,
}

/// Detect active Cialdini levers in content.
pub fn detect(title: &str, description: &str) -> InfluenceDetection {
    let lower_title = title.to_lowercase();
    let lower_desc = description.to_lowercase();
    let text = format!("{lower_title} {lower_desc}");

    // 1. Reciprocity: Generous blueprints, free frameworks, instant value
    let reciprocity = text.contains("free") || text.contains("blueprint") || text.contains("framework") || text.contains("summary") || text.contains("breakdown");

    // 2. Commitment & Consistency: Logical progression, micro-agreements
    let commitment = text.contains("step-by-step") || text.contains("why you") || text.contains("always") || text.contains("how to");

    // 3. Social Proof: Consensus, popularity, widespread adoption
    let social_proof = text.contains("everyone") || text.contains("why people") || text.contains("industry standard") || text.contains("million") || text.contains("bestseller");

    // 4. Authority: Recognized researchers, Nobel laureates, benchmarks, RFCs
    let authority = text.contains("kahneman") || text.contains("cialdini") || text.contains("compiler") || text.contains("nobel") || text.contains("doctor") || text.contains("expert");

    // 5. Liking & Relatability: Shared struggles, mistakes, empathy
    let liking = text.contains("your brain") || text.contains("our") || text.contains("we make") || text.contains("mistake") || text.contains("honest");

    // 6. Scarcity & Exclusivity: Hidden secrets, rare insights, under-the-hood
    let scarcity = text.contains("nobody") || text.contains("secret") || text.contains("hidden") || text.contains("finally") || text.contains("under the hood") || text.contains("teardown");

    // 7. Unity: Shared tribal identity
    let unity = text.contains("engineers") || text.contains("founders") || text.contains("rustaceans") || text.contains("developers") || text.contains("for startups");

    let mut count = 0;
    if reciprocity { count += 1; }
    if commitment { count += 1; }
    if social_proof { count += 1; }
    if authority { count += 1; }
    if liking { count += 1; }
    if scarcity { count += 1; }
    if unity { count += 1; }

    let score = (count as f64 * 18.0).clamp(20.0, 100.0);

    InfluenceDetection {
        reciprocity,
        commitment,
        social_proof,
        authority,
        liking,
        scarcity,
        unity,
        score,
    }
}
