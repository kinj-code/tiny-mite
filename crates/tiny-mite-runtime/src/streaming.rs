//! Streaming inference — incremental token delivery over channels.
//!
//! Defines the streaming contract between providers and consumers.
//! Provider-agnostic; works with any ModelProvider implementation.

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::inference::{InferenceRequest, InferenceResponse};

/// Configuration for streaming generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    /// Whether streaming is enabled.
    pub enabled: bool,
    /// Maximum tokens per streamed chunk.
    pub chunk_size: usize,
    /// Whether to emit partial tokens or full words.
    pub emit_partial_tokens: bool,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self { enabled: true, chunk_size: 1, emit_partial_tokens: true }
    }
}

/// A streaming session that yields incremental tokens.
///
/// Created by a [`ModelProvider`](crate::ModelProvider) when
/// `stream()` is called. Consumers call `recv()` to get the next token.
pub struct StreamingSession {
    /// Channel receiver for streaming responses.
    rx: mpsc::Receiver<InferenceResponse>,
    /// Configuration for this session.
    config: StreamingConfig,
    /// Whether the session has been closed.
    closed: bool,
}

impl StreamingSession {
    /// Create a new streaming session from a channel receiver.
    #[must_use]
    pub fn new(rx: mpsc::Receiver<InferenceResponse>, config: StreamingConfig) -> Self {
        Self { rx, config, closed: false }
    }

    /// Receive the next token or completion signal.
    ///
    /// Returns `None` when the stream is exhausted or closed.
    pub async fn recv(&mut self) -> Option<InferenceResponse> {
        if self.closed {
            return None;
        }
        let response = self.rx.recv().await?;
        if response.is_finished() {
            self.closed = true;
        }
        Some(response)
    }

    /// Returns `true` if the stream has finished.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Get the streaming config.
    #[must_use]
    pub fn config(&self) -> &StreamingConfig {
        &self.config
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use tiny_mite_domain::ModelId;

    #[test]
    fn streaming_config_defaults() {
        let config = StreamingConfig::default();
        assert!(config.enabled);
        assert_eq!(config.chunk_size, 1);
    }

    #[tokio::test]
    async fn streaming_session_receives_tokens() {
        let (tx, rx) = mpsc::channel(16);
        let mut session = StreamingSession::new(rx, StreamingConfig::default());

        // Send a partial token
        tx.send(InferenceResponse {
            id: "test".into(),
            model_id: ModelId::new(),
            text: "Hello ".into(),
            finish_reason: String::new(),
            prompt_tokens: 0,
            generated_tokens: 1,
            total_tokens: 1,
            elapsed_ms: 0.0,
            correlation_id: None,
            tool_calls: Vec::new(),
            structured_output: None,
        })
        .await
        .unwrap();

        let token = session.recv().await;
        assert!(token.is_some());
        assert_eq!(token.unwrap().text, "Hello ");
        assert!(!session.is_closed());
    }

    #[tokio::test]
    async fn streaming_session_detects_finish() {
        let (tx, rx) = mpsc::channel(16);
        let mut session = StreamingSession::new(rx, StreamingConfig::default());

        // Send a finished response
        tx.send(InferenceResponse {
            id: "test".into(),
            model_id: ModelId::new(),
            text: "Done".into(),
            finish_reason: "stop".into(),
            prompt_tokens: 5,
            generated_tokens: 1,
            total_tokens: 6,
            elapsed_ms: 100.0,
            correlation_id: None,
            tool_calls: Vec::new(),
            structured_output: None,
        })
        .await
        .unwrap();

        let token = session.recv().await.unwrap();
        assert!(token.is_finished());
        assert!(session.is_closed());
    }
}
