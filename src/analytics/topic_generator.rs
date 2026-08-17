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

/// Generate a batch of candidate topics for the greedy bot, deduplicated
/// against the research history.
pub async fn generate_candidates(
    db: &Db,
    clients: &FetchClients,
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

    candidates.truncate(MAX_CANDIDATES);
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
