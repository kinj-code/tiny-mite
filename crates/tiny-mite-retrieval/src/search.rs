//! Search implementations — lexical, hybrid, and ranking.
//!
//! Provides deterministic lexical search and a hybrid ranker that
//! combines lexical and vector search scores.

use std::collections::HashMap;

use crate::pipeline::RetrievalCandidate;

/// Lexical (keyword-based) search engine.
///
/// Performs deterministic TF-IDF-like matching without external dependencies.
pub struct LexicalSearcher {
    /// Index of document terms for fast lookup.
    index: HashMap<String, Vec<usize>>,
    /// Stored documents.
    documents: Vec<String>,
}

impl LexicalSearcher {
    /// Create a new empty searcher.
    #[must_use]
    pub fn new() -> Self {
        Self { index: HashMap::new(), documents: Vec::new() }
    }

    /// Index a set of documents.
    pub fn index_documents(&mut self, docs: Vec<String>) {
        self.documents = docs;
        self.index.clear();

        for (doc_id, doc) in self.documents.iter().enumerate() {
            for term in tokenize(doc) {
                self.index.entry(term).or_default().push(doc_id);
            }
        }
    }

    /// Search for documents matching a query.
    #[must_use]
    pub fn search(&self, query: &str, max_results: usize) -> Vec<RetrievalCandidate> {
        let query_terms: Vec<String> = tokenize(query);

        let mut scores: Vec<f32> = vec![0.0; self.documents.len()];

        for term in &query_terms {
            if let Some(doc_ids) = self.index.get(term) {
                // TF: how many times does this term appear in the query?
                let qtf = query_terms.iter().filter(|t| *t == term).count() as f32;

                for &doc_id in doc_ids {
                    // Count term frequency in document
                    let doc = &self.documents[doc_id];
                    let tf = tokenize(doc).iter().filter(|t| *t == term).count() as f32;
                    // Simple TF-IDF-like score
                    scores[doc_id] += tf * qtf;
                }
            }
        }

        // Collect top results
        let mut results: Vec<RetrievalCandidate> = self
            .documents
            .iter()
            .enumerate()
            .map(|(id, content)| RetrievalCandidate {
                id: format!("doc_{id}"),
                content: content.clone(),
                lexical_score: scores[id],
                vector_score: None,
                combined_score: scores[id],
            })
            .filter(|c| c.lexical_score > 0.0)
            .collect();

        results.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());
        results.truncate(max_results);
        results
    }

    /// Returns the number of indexed documents.
    #[must_use]
    pub fn doc_count(&self) -> usize {
        self.documents.len()
    }
}

impl Default for LexicalSearcher {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hybrid Ranker ─────────────────────────────────────────────────

/// Combines lexical and vector search scores into a final ranking.
pub struct HybridRanker {
    /// Weight given to lexical score (0.0–1.0).
    lexical_weight: f32,
    /// Weight given to vector score (0.0–1.0).
    vector_weight: f32,
}

impl HybridRanker {
    /// Create a new hybrid ranker with equal weights.
    #[must_use]
    pub fn new() -> Self {
        Self { lexical_weight: 0.5, vector_weight: 0.5 }
    }

    /// Set custom weights.
    #[must_use]
    pub fn with_weights(mut self, lexical: f32, vector: f32) -> Self {
        self.lexical_weight = lexical;
        self.vector_weight = vector;
        self
    }

    /// Rank candidates using hybrid scoring.
    pub fn rank(&self, candidates: &mut [RetrievalCandidate]) {
        for c in candidates.iter_mut() {
            let vector = c.vector_score.unwrap_or(0.0);
            c.combined_score = c.lexical_score * self.lexical_weight + vector * self.vector_weight;
        }
        candidates.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());
    }

    /// Rerank candidates (applies fresh scoring).
    pub fn rerank(&self, candidates: &mut [RetrievalCandidate]) {
        self.rank(candidates);
    }
}

impl Default for HybridRanker {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tokenizer ─────────────────────────────────────────────────────

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() >= 2)
        .map(|s| s.to_owned())
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_search_returns_results() {
        let mut searcher = LexicalSearcher::new();
        searcher.index_documents(vec![
            "Rust is a systems programming language".into(),
            "Python is great for data science".into(),
            "Rust and C++ are fast languages".into(),
        ]);

        let results = searcher.search("Rust programming", 5);
        assert!(!results.is_empty());
        assert!(results[0].content.contains("Rust"));
    }

    #[test]
    fn empty_query_returns_nothing() {
        let mut searcher = LexicalSearcher::new();
        searcher.index_documents(vec!["hello world".into()]);
        let results = searcher.search("xyz", 5);
        assert!(results.is_empty());
    }

    #[test]
    fn hybrid_ranker_combines_scores() {
        let ranker = HybridRanker::new();
        let mut candidates = vec![
            RetrievalCandidate {
                id: "d1".into(),
                content: "hello".into(),
                lexical_score: 0.8,
                vector_score: Some(0.2),
                combined_score: 0.0,
            },
            RetrievalCandidate {
                id: "d2".into(),
                content: "world".into(),
                lexical_score: 0.3,
                vector_score: Some(0.9),
                combined_score: 0.0,
            },
        ];
        ranker.rank(&mut candidates);
        assert!(candidates[0].combined_score > 0.0);
    }

    #[test]
    fn tokenize_splits_words() {
        let tokens = tokenize("Hello, World! Rust 2024");
        assert!(tokens.contains(&"hello".to_owned()));
        assert!(tokens.contains(&"world".to_owned()));
        assert!(tokens.contains(&"rust".to_owned()));
    }
}
