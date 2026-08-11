//! Model provider abstraction.
//!
//! Tiny Mite's provider interface isolates the rest of the system
//! from specific inference backends. Every provider — llama.cpp native,
//! Ollama adapter, LM Studio adapter — implements this trait.
//!
//! # Safety boundary
//!
//! Provider implementations that require `unsafe` (e.g. llama.cpp FFI)
//! must isolate those blocks behind safe wrappers documented with
//! invariants. The provider trait itself is pure safe Rust.

use std::fmt;

use async_trait::async_trait;
use tiny_mite_domain::ModelId;

use crate::inference::{InferenceRequest, InferenceResponse};
use crate::model::{Backend, DeviceInfo, ModelCapabilities, ModelInfo, ModelState};

// ── Provider errors ───────────────────────────────────────────────

/// Errors that can occur during provider operations.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// The requested model was not found.
    #[error("Model not found: {0}")]
    NotFound(ModelId),

    /// The model exists but is not in a usable state.
    #[error("Model {0} is not loadable (state: {1:?})")]
    NotLoadable(ModelId, ModelState),

    /// The model is not loaded.
    #[error("Model {0} is not loaded")]
    NotLoaded(ModelId),

    /// Insufficient resources to load the model.
    #[error("Insufficient resources: {reason}")]
    InsufficientResources { reason: String },

    /// The requested capability is not supported.
    #[error("Capability '{capability}' not supported")]
    UnsupportedCapability { capability: String },

    /// The backend is unavailable.
    #[error("Backend {backend:?} is not available")]
    BackendUnavailable { backend: Backend },

    /// An internal error occurred in the provider.
    #[error("Internal provider error: {0}")]
    Internal(String),

    /// The operation was cancelled.
    #[error("Operation cancelled")]
    Cancelled,

    /// The operation timed out.
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// The requested operation is not supported by this provider.
    #[error("Unsupported: {0}")]
    Unsupported(String),
}

// ── Model provider trait ─────────────────────────────────────────

/// Abstraction over a model inference backend.
///
/// Implementors provide model discovery, loading, inference, streaming,
/// and lifecycle management.
#[async_trait]
pub trait ModelProvider: Send + Sync + fmt::Debug {
    /// Return a unique name for this provider (e.g. "llama.cpp").
    fn name(&self) -> &'static str;

    /// Return the capabilities this provider advertises.
    fn provider_capabilities(&self) -> ModelCapabilities;

    /// Discover available models on this provider.
    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    /// Inspect a specific model by ID.
    async fn inspect(&self, id: &ModelId) -> Result<ModelInfo, ProviderError>;

    /// Load a model into memory.
    async fn load(&self, id: &ModelId) -> Result<ModelInfo, ProviderError>;

    /// Unload a model from memory.
    async fn unload(&self, id: &ModelId) -> Result<(), ProviderError>;

    /// Generate a response synchronously (non-streaming).
    async fn generate(
        &self,
        request: &InferenceRequest,
    ) -> Result<InferenceResponse, ProviderError>;

    /// Stream a response as incremental tokens.
    async fn stream(
        &self,
        request: &InferenceRequest,
        sink: tokio::sync::mpsc::Sender<InferenceResponse>,
    ) -> Result<(), ProviderError>;

    /// Cancel an active inference (by request/correlation ID).
    async fn cancel(&self, correlation_id: &str) -> Result<(), ProviderError>;

    /// Perform a health check on this provider.
    async fn health_check(&self) -> Result<(), ProviderError>;

    /// Return available compute devices.
    async fn list_devices(&self) -> Result<Vec<DeviceInfo>, ProviderError>;

    /// Estimate tokens for a prompt (token counting).
    async fn count_tokens(&self, model_id: &ModelId, text: &str) -> Result<usize, ProviderError>;
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::{ContextBudget, InferenceRequest, InferenceResponse};
    use crate::model::{ModelCapabilities, ModelInfo, ModelState};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tiny_mite_domain::ModelId;

    /// A mock provider for testing the abstraction layer.
    #[derive(Debug, Default)]
    struct MockProvider {
        models: Vec<ModelInfo>,
        loaded: std::sync::Mutex<std::collections::HashSet<ModelId>>,
        cancelled: AtomicBool,
    }

    impl MockProvider {
        fn new(models: Vec<ModelInfo>) -> Self {
            Self {
                models,
                loaded: std::sync::Mutex::new(std::collections::HashSet::new()),
                cancelled: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl ModelProvider for MockProvider {
        fn name(&self) -> &'static str {
            "mock"
        }

        fn provider_capabilities(&self) -> ModelCapabilities {
            ModelCapabilities { text_generation: true, chat: true, ..Default::default() }
        }

        async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(self.models.clone())
        }

        async fn inspect(&self, id: &ModelId) -> Result<ModelInfo, ProviderError> {
            self.models.iter().find(|m| &m.id == id).cloned().ok_or(ProviderError::NotFound(*id))
        }

        async fn load(&self, id: &ModelId) -> Result<ModelInfo, ProviderError> {
            let _model = self
                .inspect(id)
                .await
                .map_err(|_| ProviderError::NotLoadable(*id, ModelState::Unloaded))?;
            self.loaded.lock().unwrap().insert(*id);
            let mut info = self.models.iter().find(|m| &m.id == id).unwrap().clone();
            info.state = ModelState::Ready;
            Ok(info)
        }

        async fn unload(&self, id: &ModelId) -> Result<(), ProviderError> {
            self.loaded.lock().unwrap().remove(id);
            Ok(())
        }

        async fn generate(
            &self,
            _request: &InferenceRequest,
        ) -> Result<InferenceResponse, ProviderError> {
            Ok(InferenceResponse {
                id: tiny_mite_domain::EventId::new().to_string(),
                model_id: ModelId::new(),
                text: "Hello from mock provider".to_owned(),
                finish_reason: "stop".to_owned(),
                prompt_tokens: 10,
                generated_tokens: 5,
                total_tokens: 15,
                elapsed_ms: 100.0,
                correlation_id: None,
                tool_calls: Vec::new(),
                structured_output: None,
            })
        }

        async fn stream(
            &self,
            request: &InferenceRequest,
            sink: tokio::sync::mpsc::Sender<InferenceResponse>,
        ) -> Result<(), ProviderError> {
            let words = ["Hello", "from", "mock", "provider"];
            for word in &words {
                if self.cancelled.load(Ordering::Acquire) {
                    return Err(ProviderError::Cancelled);
                }
                let _ = sink
                    .send(InferenceResponse {
                        id: request.correlation_id.map(|c| c.to_string()).unwrap_or_default(),
                        model_id: request.model_id,
                        text: format!("{word} "),
                        finish_reason: if word == words.last().unwrap() {
                            "stop".to_owned()
                        } else {
                            "".to_owned()
                        },
                        prompt_tokens: 10,
                        generated_tokens: 1,
                        total_tokens: 11,
                        elapsed_ms: 10.0,
                        correlation_id: None,
                        tool_calls: Vec::new(),
                        structured_output: None,
                    })
                    .await;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            Ok(())
        }

        async fn cancel(&self, _correlation_id: &str) -> Result<(), ProviderError> {
            self.cancelled.store(true, Ordering::Release);
            Ok(())
        }

        async fn health_check(&self) -> Result<(), ProviderError> {
            Ok(())
        }

        async fn list_devices(&self) -> Result<Vec<DeviceInfo>, ProviderError> {
            Ok(vec![DeviceInfo {
                device_id: 0,
                name: "mock-cpu".into(),
                backend: Backend::Cpu,
                memory_bytes: 8_589_934_592,
                available: true,
                recommended: true,
            }])
        }

        async fn count_tokens(
            &self,
            _model_id: &ModelId,
            text: &str,
        ) -> Result<usize, ProviderError> {
            Ok(text.split_whitespace().count())
        }
    }

    fn make_model(id: ModelId) -> ModelInfo {
        ModelInfo {
            id,
            provider: "mock".into(),
            name: format!("model-{id}"),
            family: Some("test".into()),
            architecture: Some("mock".into()),
            format: Some("gguf".into()),
            quantization: Some("Q4_K_M".into()),
            parameter_size_billions: Some(3.0),
            max_context_length: 8192,
            file_path: None,
            capabilities: ModelCapabilities {
                text_generation: true,
                chat: true,
                ..Default::default()
            },
            estimated_ram_bytes: 2_000_000_000,
            state: ModelState::Unloaded,
        }
    }

    #[tokio::test]
    async fn provider_discover_returns_models() {
        let model = make_model(ModelId::new());
        let provider = MockProvider::new(vec![model.clone()]);
        let models = provider.discover_models().await.expect("discover");
        assert!(!models.is_empty());
        assert_eq!(models[0].id, model.id);
    }

    #[tokio::test]
    async fn provider_inspect_found() {
        let model = make_model(ModelId::new());
        let provider = MockProvider::new(vec![model.clone()]);
        let info = provider.inspect(&model.id).await.expect("inspect");
        assert_eq!(info.name, model.name);
    }

    #[tokio::test]
    async fn provider_inspect_not_found() {
        let provider = MockProvider::new(vec![]);
        let result = provider.inspect(&ModelId::new()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn provider_load_and_unload() {
        let model = make_model(ModelId::new());
        let id = model.id;
        let provider = MockProvider::new(vec![model]);

        let loaded = provider.load(&id).await.expect("load");
        assert_eq!(loaded.state, ModelState::Ready);

        provider.unload(&id).await.expect("unload");
    }

    #[tokio::test]
    async fn provider_load_nonexistent_fails() {
        let provider = MockProvider::new(vec![]);
        let result = provider.load(&ModelId::new()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn provider_generate_returns_response() {
        let model = make_model(ModelId::new());
        let id = model.id;
        let provider = MockProvider::new(vec![model]);
        provider.load(&id).await.expect("load");

        let request = InferenceRequest {
            model_id: id,
            model_name: "test-model".into(),
            prompt: "Hello".to_owned(),
            system_prompt: None,
            max_tokens: 100,
            temperature: 0.7,
            top_p: None,
            top_k: None,
            seed: None,
            stop_sequences: Vec::new(),
            grammar: None,
            tools: Vec::new(),
            correlation_id: None,
            task_id: None,
            timeout_ms: None,
            context_budget: ContextBudget::new(8192),
        };

        let response = provider.generate(&request).await.expect("generate");
        assert!(!response.text.is_empty());
    }

    #[tokio::test]
    async fn provider_stream_sends_tokens() {
        let model = make_model(ModelId::new());
        let id = model.id;
        let provider = MockProvider::new(vec![model]);
        provider.load(&id).await.expect("load");

        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let request = InferenceRequest {
            model_id: id,
            model_name: "test-model".into(),
            prompt: "Hi".to_owned(),
            system_prompt: None,
            max_tokens: 50,
            temperature: 0.5,
            top_p: None,
            top_k: None,
            seed: None,
            stop_sequences: Vec::new(),
            grammar: None,
            tools: Vec::new(),
            correlation_id: None,
            task_id: None,
            timeout_ms: None,
            context_budget: ContextBudget::new(8192),
        };

        provider.stream(&request, tx).await.expect("stream");

        let mut tokens = Vec::new();
        while let Some(resp) = rx.recv().await {
            tokens.push(resp.text);
        }

        assert!(!tokens.is_empty());
        assert!(tokens.iter().any(|t| t.contains("mock")));
    }

    #[tokio::test]
    async fn provider_cancel_stops_stream() {
        let model = make_model(ModelId::new());
        let id = model.id;
        let provider = MockProvider::new(vec![model]);
        provider.load(&id).await.expect("load");

        // Use channel to coordinate — the stream will check cancelled
        // flag which we set by calling cancel()
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let request = InferenceRequest {
            model_id: id,
            model_name: "test-model".into(),
            prompt: "test".to_owned(),
            system_prompt: None,
            max_tokens: 50,
            temperature: 0.5,
            top_p: None,
            top_k: None,
            seed: None,
            stop_sequences: Vec::new(),
            grammar: None,
            tools: Vec::new(),
            correlation_id: Some(tiny_mite_domain::CorrelationId::new()),
            task_id: None,
            timeout_ms: None,
            context_budget: ContextBudget::new(8192),
        };

        // Cancel before streaming — stream should return Cancelled error
        provider.cancel("any").await.expect("cancel");
        let result = provider.stream(&request, tx).await;
        assert!(result.is_err());
        match result {
            Err(ProviderError::Cancelled) => {}
            other => panic!("Expected Cancelled, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn provider_count_tokens() {
        let model = make_model(ModelId::new());
        let provider = MockProvider::new(vec![model.clone()]);
        let count = provider.count_tokens(&model.id, "hello world test").await.expect("count");
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn provider_health_check() {
        let provider = MockProvider::new(vec![]);
        provider.health_check().await.expect("healthy");
    }
}
