//! Greedy bot topic generator: picks the next topic to research by combining
//! autocomplete suggestions, competitor tags, and related-keyword drift.

use std::collections::HashSet;

use crate::error::TubeforgeError;
use crate::fetch::{youtube_suggestions, FetchClients};
use crate::storage::db::Db;

/// Maximum number of candidate topics returned per generation round.
const MAX_CANDIDATES: usize = 50;

/// A candidate topic the greedy bot can research next.
#[derive(Debug, Clone)]
pub struct TopicCandidate {
    pub topic: String,
    pub source: TopicSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TopicSource {
    Autocomplete,
    CompetitorTag,
    RelatedKeyword,
    SeedDrift,
}

impl std::fmt::Display for TopicSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TopicSource::Autocomplete => write!(f, "autocomplete"),
            TopicSource::CompetitorTag => write!(f, "competitor_tag"),
            TopicSource::RelatedKeyword => write!(f, "related_keyword"),
            TopicSource::SeedDrift => write!(f, "seed_drift"),
        }
    }
}

/// Niche guard: a topic passes when ANY configured term appears in it
/// (case-insensitive substring). An empty term list disables filtering so
/// behavior is identical to pre-guard releases.
fn is_on_niche(topic: &str, niche_terms: &[String]) -> bool {
    if niche_terms.is_empty() {
        return true;
    }
    let lower = topic.to_lowercase();
    niche_terms.iter().any(|term| lower.contains(term))
}

/// Generate a batch of candidate topics for the greedy bot, deduplicated
/// against the research history and filtered to the configured niche.
pub async fn generate_candidates(
    db: &Db,
    clients: &FetchClients,
    niche_terms: &[String],
) -> Result<Vec<TopicCandidate>, TubeforgeError> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates: Vec<TopicCandidate> = Vec::new();

    // 1) Autocomplete suggestions from tracked keywords
    let keywords = db.list_keywords().await?;
    for kw_row in &keywords {
        let suggestions = youtube_suggestions(clients, &kw_row.keyword).await;
        for sug in suggestions {
            let lower = sug.to_lowercase();
            if seen.insert(lower.clone()) {
                candidates.push(TopicCandidate {
                    topic: sug,
                    source: TopicSource::Autocomplete,
                });
            }
        }
    }

    // 2) Competitor tags (top 50 from the table, ordered by avg_views desc)
    let competitor_tags = db.greedy_top_competitor_tags(50).await?;
    for tag in &competitor_tags {
        let lower = tag.to_lowercase();
        if seen.insert(lower) {
            candidates.push(TopicCandidate {
                topic: tag.clone(),
                source: TopicSource::CompetitorTag,
            });
        }
    }

    // 3) Related keywords from recent keyword_research rows
    let recent_research = db.greedy_recent_related_keywords(20).await?;
    for rk in &recent_research {
        let lower = rk.to_lowercase();
        if seen.insert(lower) {
            candidates.push(TopicCandidate {
                topic: rk.clone(),
                source: TopicSource::RelatedKeyword,
            });
        }
    }

    // 4) Seed drift: autocomplete off the seed terms themselves
    let seeds = db.greedy_active_seeds().await?;
    for seed in &seeds {
        let suggestions = youtube_suggestions(clients, seed).await;
        for sug in suggestions {
            let lower = sug.to_lowercase();
            if seen.insert(lower) {
                candidates.push(TopicCandidate {
                    topic: sug,
                    source: TopicSource::SeedDrift,
                });
            }
        }
    }

    // 5) Niche guard: drop off-niche drift BEFORE truncation so every
    //    surviving candidate is worth research spend.
    if !niche_terms.is_empty() {
        candidates.retain(|c| is_on_niche(&c.topic, niche_terms));
    }

    candidates.truncate(MAX_CANDIDATES);
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prop_assert;
    use proptest::prop_assert_eq;

    #[test]
    fn source_display_roundtrip() {
        for src in [
            TopicSource::Autocomplete,
            TopicSource::CompetitorTag,
            TopicSource::RelatedKeyword,
            TopicSource::SeedDrift,
        ] {
            let s = src.to_string();
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn topic_candidate_clone() {
        let c = TopicCandidate {
            topic: "rust tutorial".into(),
            source: TopicSource::Autocomplete,
        };
        let c2 = c.clone();
        assert_eq!(c.topic, c2.topic);
    }

    #[test]
    fn niche_guard_blocks_known_drift() {
        // Regression: greedy researched music-band drift off an MCP/AI channel.
        let terms = vec!["rust".to_string(), "mcp".to_string(), "ai".to_string()];
        assert!(is_on_niche("build an MCP server in Rust", &terms));
        assert!(is_on_niche("AI engineering roadmap", &terms));
        assert!(!is_on_niche("massive attack angel acapella", &terms));
        assert!(!is_on_niche("best coffee grinders 2026", &terms));
    }

    proptest::proptest! {
        // Property: empty term list accepts everything (legacy parity).
        #[test]
        fn empty_terms_accept_all(topic in "\\PC{0,80}") {
            prop_assert!(is_on_niche(&topic, &[]));
        }

        // Property: case never flips a verdict (filter is case-insensitive).
        #[test]
        fn verdict_case_insensitive(
            topic in "[a-zA-Z0-9 +\\-]{0,60}",
            term in "[a-zA-Z]{1,10}",
        ) {
            let terms = vec![term.to_lowercase()];
            let flipped: String = topic
                .chars()
                .map(|c| if c.is_ascii_alphabetic() {
                    (c as u8 ^ 0x20) as char
                } else {
                    c
                })
                .collect();
            prop_assert_eq!(
                is_on_niche(&topic, &terms),
                is_on_niche(&flipped, &terms)
            );
        }

        // Property: no false accepts — a non-empty accept implies some term
        // IS a case-insensitive substring of the topic.
        #[test]
        fn accept_implies_term_present(
            topic in "[a-zA-Z0-9 +\\-]{0,60}",
            t1 in "[a-z]{1,8}",
            t2 in "[a-z]{1,8}",
        ) {
            let terms = vec![t1.clone(), t2];
            let lower = topic.to_lowercase();
            if is_on_niche(&topic, &terms) {
                prop_assert!(
                    terms.iter().any(|t| lower.contains(t.as_str())),
                    "accepted {topic:?} without any matching term"
                );
            }
        }

        // Property: filtering is idempotent (A→B == A).
        #[test]
        fn filter_idempotent(
            topics in proptest::collection::vec("[a-zA-Z0-9 +\\-]{0,30}", 0..20),
            t in "[a-z]{1,8}",
        ) {
            let terms = vec![t];
            let once: Vec<String> = topics
                .iter()
                .filter(|x| is_on_niche(x, &terms))
                .cloned()
                .collect();
            let twice: Vec<String> = once
                .iter()
                .filter(|x| is_on_niche(x, &terms))
                .cloned()
                .collect();
            prop_assert_eq!(once.len(), twice.len());
        }
    }
}
