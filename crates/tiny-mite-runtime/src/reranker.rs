//! Reranker provider abstraction.
//!
//! Rerankers re-score retrieval candidates to improve relevance
//! ordering before context assembly.

use async_trait::async_trait;
use tiny_mite_domain::ModelId;

/// A single document-rerank candidate.
#[derive(Debug, Clone)]
pub struct RerankCandidate {
    /// Document identifier.
    pub id: String,
    /// Document text.
    pub content: String,
    /// Original retrieval score.
    pub original_score: f32,
    /// Reranked score (set after reranking).
    pub reranked_score: Option<f32>,
}

/// The result of a reranking operation.
#[derive(Debug, Clone)]
pub struct RerankResult {
    /// Reranked candidates, ordered by descending score.
    pub candidates: Vec<RerankCandidate>,
    /// Total elapsed milliseconds.
    pub elapsed_ms: f64,
}

/// Abstraction for reranking-capable backends.
#[async_trait]
pub trait RerankerProvider: Send + Sync {
    /// Rerank a set of candidates relative to a query.
    async fn rerank(
        &self,
        model_id: &ModelId,
        query: &str,
        candidates: Vec<RerankCandidate>,
    ) -> Result<RerankResult, RerankerError>;

    /// Check if a model supports reranking.
    async fn supports_reranking(&self, model_id: &ModelId) -> Result<bool, RerankerError>;
}

/// Errors specific to reranking operations.
#[derive(Debug, thiserror::Error)]
pub enum RerankerError {
    #[error("Model not found: {0}")]
    NotFound(ModelId),

    #[error("Model {0} does not support reranking")]
    Unsupported(ModelId),

    #[error("Internal error: {0}")]
    Internal(String),
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rerank_candidate_has_scores() {
        let c = RerankCandidate {
            id: "doc1".into(),
            content: "text".into(),
            original_score: 0.8,
            reranked_score: None,
        };
        assert_eq!(c.id, "doc1");
        assert!(c.reranked_score.is_none());
    }
}
