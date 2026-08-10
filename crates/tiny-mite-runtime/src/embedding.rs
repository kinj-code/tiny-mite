//! Embedding provider abstraction.
//!
//! Embedding models convert text into dense vector representations
//! for semantic search, retrieval, and clustering.

use async_trait::async_trait;
use tiny_mite_domain::ModelId;

// ── Embedding errors ─────────────────────────────────────────────

/// Errors specific to embedding operations.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    /// Model not found.
    #[error("Model not found: {0}")]
    NotFound(ModelId),

    /// Model does not support embeddings.
    #[error("Model {0} does not support embeddings")]
    Unsupported(ModelId),

    /// Internal provider error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Embedding dimension mismatch.
    #[error("Expected dimension {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
}

// ── Embedding result ─────────────────────────────────────────────

/// The result of an embedding operation.
#[derive(Debug, Clone)]
pub struct EmbeddingResult {
    /// The embedding vector.
    pub vector: Vec<f32>,
    /// Number of dimensions.
    pub dimensions: usize,
    /// Number of input tokens.
    pub token_count: usize,
    /// Whether the embedding is normalized.
    pub normalized: bool,
    /// Model that produced this embedding.
    pub model_id: ModelId,
    /// Elapsed time in milliseconds.
    pub elapsed_ms: f64,
}

impl EmbeddingResult {
    /// Normalize the embedding vector to unit length.
    #[must_use]
    pub fn normalize(mut self) -> Self {
        let sum_sq: f32 = self.vector.iter().map(|x| x * x).sum();
        let norm = sum_sq.sqrt();
        if norm > 0.0 {
            for x in &mut self.vector {
                *x /= norm;
            }
        }
        self.normalized = true;
        self
    }

    /// Returns `true` if all values are finite.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.vector.iter().all(|x| x.is_finite())
    }
}

// ── Embedding provider trait ─────────────────────────────────────

/// Abstraction for embedding-capable backends.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate an embedding for a single text.
    async fn embed(
        &self,
        model_id: &ModelId,
        text: &str,
    ) -> Result<EmbeddingResult, EmbeddingError>;

    /// Generate embeddings for multiple texts (batched).
    async fn embed_batch(
        &self,
        model_id: &ModelId,
        texts: &[String],
    ) -> Result<Vec<EmbeddingResult>, EmbeddingError>;

    /// Check if a model supports embeddings.
    async fn supports_embedding(&self, model_id: &ModelId) -> Result<bool, EmbeddingError>;
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_embedding() {
        let result = EmbeddingResult {
            vector: vec![3.0_f32, 4.0_f32],
            dimensions: 2,
            token_count: 5,
            normalized: false,
            model_id: ModelId::new(),
            elapsed_ms: 10.0,
        };
        let normalized = result.normalize();
        assert!(normalized.normalized);
        let magnitude: f32 = normalized.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((magnitude - 1.0).abs() < 0.001);
    }

    #[test]
    fn finite_check() {
        let result = EmbeddingResult {
            vector: vec![1.0, f32::NAN],
            dimensions: 2,
            token_count: 1,
            normalized: false,
            model_id: ModelId::new(),
            elapsed_ms: 1.0,
        };
        assert!(!result.is_finite());
    }
}
