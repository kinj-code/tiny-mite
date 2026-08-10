//! Tiny Mite model runtime
//!
//! Provider abstraction, model lifecycle, inference types, and context budgeting.
//!
//! # Architecture
//!
//! ```text
//! Tiny Mite Core → ModelProvider trait → llama.cpp / Ollama / LM Studio
//!                   ↑
//!           ModelInfo, InferenceRequest, InferenceResponse, ContextBudget
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod adapters;
pub mod cache;
pub mod compaction;
pub mod context;
pub mod embedding;
pub mod grammar;
pub mod inference;
pub mod model;
pub mod provider;
pub mod reliability;
pub mod reranker;
pub mod router;
pub mod streaming;

pub use cache::{LatencyTracer, LruCache, MemoryPressureManager, RuntimeCaches};
pub use compaction::{CompactionResult, CompactionStrategy, ContextCompactor};
pub use reliability::{AdaptiveConcurrency, CrashRecovery, ModelLifecycleConfig, TaskCheckpoint};
pub use router::ModelRouter;

pub use adapters::{LmStudioProvider, OllamaProvider, OpenAiProvider};
pub use context::{ContextCompiler, ContextItem, ContextItemType, ContextWindow};
pub use embedding::{EmbeddingError, EmbeddingProvider, EmbeddingResult};
pub use grammar::{
    GrammarConstraint, HardwareCapabilities, JsonSchemaConstraint, SpeculativeDecodingConfig,
};
pub use inference::{
    ContextBudget, InferenceRequest, InferenceResponse, SamplingConfig, ToolCall, ToolDefinition,
};
pub use model::{Backend, DeviceInfo, ModelCapabilities, ModelInfo, ModelState};
pub use provider::{ModelProvider, ProviderError};
pub use reranker::{RerankCandidate, RerankResult, RerankerError, RerankerProvider};
pub use streaming::{StreamingConfig, StreamingSession};
