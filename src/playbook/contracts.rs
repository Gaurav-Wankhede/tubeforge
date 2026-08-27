//! Founder Playbook Prompt Contract Generator.
//!
//! Synthesizes behavioral, architectural, and cognitive directives into
//! deterministic execution prompts for autonomous coding agents.

/// Generate a production-ready Founder Playbook contract prompt for a ticket.
pub fn generate_contract(
    channel: &str,
    topic: &str,
    target_keyword: &str,
    title: &str,
    framework: &str,
) -> String {
    format!(
        r#"# FOUNDER PLAYBOOK PRODUCTION CONTRACT
CHANNEL: {channel}
TOPIC: {topic}
KEYWORD: {target_keyword}
APPROVED TITLE (<50 chars, 0 colons): "{title}"
STRATEGIC FRAMEWORK: {framework}

[CORE DIRECTIVES]
1. Domain Grounding: Every visual beat MUST derive from the authentic empirical mechanism of {topic}.
2. Value Equation: Maximize Dream Outcome & Perceived Likelihood; Minimize viewer Time Delay and Effort.
3. SUCCESs Hook: Open with a Loewenstein Information Gap within the first 3 seconds.
4. Clean Typography: Strict ban on acronyms in VO script; 100% full phonetic expansions.
5. Canvas: Open black canvas #000000; continuous GSAP SVG morphs."#
    )
}
