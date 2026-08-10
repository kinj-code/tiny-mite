//! Retrieval pipeline — query analysis, expansion, candidate retrieval, ranking.
//!
//! Provider-independent; works with lexical search, vector search, and hybrid.

use serde::{Deserialize, Serialize};

/// A retrieved document candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalCandidate {
    pub id: String,
    pub content: String,
    pub lexical_score: f32,
    pub vector_score: Option<f32>,
    pub combined_score: f32,
}

/// A structured search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub original: String,
    pub expanded_terms: Vec<String>,
    pub max_results: usize,
}

impl SearchQuery {
    /// Create a new search query.
    #[must_use]
    pub fn new(query: impl Into<String>) -> Self {
        let q = query.into();
        let expanded = expand_query(&q);
        Self { original: q, expanded_terms: expanded, max_results: 20 }
    }
}

/// The retrieval pipeline.
pub struct RetrievalPipeline {
    query_analysis_enabled: bool,
    dedup_enabled: bool,
}

impl RetrievalPipeline {
    /// Create a new pipeline.
    #[must_use]
    pub fn new() -> Self {
        Self { query_analysis_enabled: true, dedup_enabled: true }
    }

    /// Analyze a raw query into structured form.
    #[must_use]
    pub fn analyze_query(&self, raw: &str) -> SearchQuery {
        SearchQuery::new(raw)
    }

    /// Deduplicate retrieval candidates.
    pub fn deduplicate(&self, candidates: Vec<RetrievalCandidate>) -> Vec<RetrievalCandidate> {
        let mut seen = std::collections::HashSet::new();
        candidates.into_iter().filter(|c| seen.insert(c.content.clone())).collect()
    }
}

impl Default for RetrievalPipeline {
    fn default() -> Self {
        Self::new()
    }
}

fn expand_query(query: &str) -> Vec<String> {
    // Simple: split into terms
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_expands() {
        let q = SearchQuery::new("Rust programming");
        assert!(q.expanded_terms.contains(&"rust".to_owned()));
        assert!(q.expanded_terms.contains(&"programming".to_owned()));
    }

    #[test]
    fn dedup_removes_duplicates() {
        let pipeline = RetrievalPipeline::new();
        let candidates = vec![
            RetrievalCandidate {
                id: "1".into(),
                content: "a".into(),
                lexical_score: 1.0,
                vector_score: None,
                combined_score: 1.0,
            },
            RetrievalCandidate {
                id: "2".into(),
                content: "a".into(),
                lexical_score: 0.5,
                vector_score: None,
                combined_score: 0.5,
            },
            RetrievalCandidate {
                id: "3".into(),
                content: "b".into(),
                lexical_score: 0.8,
                vector_score: None,
                combined_score: 0.8,
            },
        ];
        let deduped = pipeline.deduplicate(candidates);
        assert_eq!(deduped.len(), 2);
    }
}
