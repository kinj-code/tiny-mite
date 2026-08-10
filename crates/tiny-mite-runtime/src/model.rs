//! Model abstraction — identity, capabilities, lifecycle, and metadata.
//!
//! # Model states
//!
//! ```text
//! Discovered → Validated → Loading → Warmup → Ready → Busy → Idle → Unloading → Unloaded
//!                                                     ↓
//!                                                   Failed
//! ```

use serde::{Deserialize, Serialize};
use tiny_mite_domain::ModelId;

// ---------------------------------------------------------------------------
// Model identity
// ---------------------------------------------------------------------------

/// Full model metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Unique model identifier.
    pub id: ModelId,
    /// Provider name (e.g. "llama.cpp", "ollama").
    pub provider: String,
    /// Human-readable model name.
    pub name: String,
    /// Model family (e.g. "llama", "mistral", "qwen").
    pub family: Option<String>,
    /// Model architecture (e.g. "llama", "falcon", "gpt-neox").
    pub architecture: Option<String>,
    /// File format (e.g. "gguf", "safetensors").
    pub format: Option<String>,
    /// Quantization level (e.g. "Q4_K_M", "Q8_0", "BF16").
    pub quantization: Option<String>,
    /// Estimated parameter count (e.g. 3 for 3B, 7 for 7B).
    pub parameter_size_billions: Option<f32>,
    /// Maximum context length in tokens (0 = unknown).
    pub max_context_length: usize,
    /// Path to the model file on disk (if local).
    pub file_path: Option<String>,
    /// Capabilities the model supports.
    pub capabilities: ModelCapabilities,
    /// Estimated RAM required to load the model (bytes, 0 = unknown).
    pub estimated_ram_bytes: u64,
    /// Current lifecycle state.
    pub state: ModelState,
}

impl Default for ModelInfo {
    fn default() -> Self {
        Self {
            id: ModelId::new(),
            provider: String::new(),
            name: String::new(),
            family: None,
            architecture: None,
            format: None,
            quantization: None,
            parameter_size_billions: None,
            max_context_length: 0,
            file_path: None,
            capabilities: ModelCapabilities::default(),
            estimated_ram_bytes: 0,
            state: ModelState::Unloaded,
        }
    }
}

// ---------------------------------------------------------------------------
// Model capabilities
// ---------------------------------------------------------------------------

/// What a model can do.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct ModelCapabilities {
    /// Basic text generation.
    pub text_generation: bool,
    /// Multi-turn chat/conversation.
    pub chat: bool,
    /// Native tool/function calling.
    pub tool_calling: bool,
    /// Grammar-constrained / structured JSON output.
    pub structured_output: bool,
    /// Embedding generation.
    pub embeddings: bool,
    /// Reranking / scoring.
    pub reranking: bool,
    /// Vision/image input.
    pub vision: bool,
    /// Audio input or output.
    pub audio: bool,
    /// Complex reasoning (chain-of-thought, etc.).
    pub reasoning: bool,
    /// Speculative decoding (draft model).
    pub speculative_decoding: bool,
    /// Grammar-constrained generation.
    pub grammar_constrained_output: bool,
}

impl ModelCapabilities {
    /// Returns `true` if the model can generate text.
    #[must_use]
    pub fn can_generate(&self) -> bool {
        self.text_generation || self.chat
    }

    /// Returns `true` if the model supports tool calling.
    #[must_use]
    pub fn can_use_tools(&self) -> bool {
        self.tool_calling || self.structured_output
    }
}

// ---------------------------------------------------------------------------
// Model state (lifecycle)
// ---------------------------------------------------------------------------

/// Where a model is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelState {
    /// Model file found but not yet validated.
    Discovered,
    /// Model file has been validated but not loaded.
    Validated,
    /// Model is being loaded into memory.
    Loading,
    /// Warmup/health-check in progress.
    Warmup,
    /// Model is loaded and ready for inference.
    Ready,
    /// Model is actively processing an inference request.
    Busy,
    /// Model is loaded but idle.
    Idle,
    /// Model is being unloaded from memory.
    Unloading,
    /// Model is not loaded.
    Unloaded,
    /// An error occurred (loading, validation, etc.).
    Failed,
}

impl ModelState {
    /// Returns `true` if the model is available for inference.
    #[must_use]
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Ready | Self::Idle | Self::Busy)
    }

    /// Returns `true` if the model is in a terminal/inactive state.
    #[must_use]
    pub fn is_inactive(&self) -> bool {
        matches!(self, Self::Unloaded | Self::Failed)
    }
}

// ---------------------------------------------------------------------------
// Backend / device information
// ---------------------------------------------------------------------------

/// An accelerator backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// CPU-only inference.
    Cpu,
    /// Vulkan compute.
    Vulkan,
    /// NVIDIA CUDA.
    Cuda,
    /// Apple Metal.
    Metal,
    /// AMD ROCm/HIP.
    Hip,
    /// Intel SYCL / oneAPI.
    Sycl,
    /// Unknown or custom backend.
    Other,
}

/// Information about a compute device available for inference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Unique device identifier within the backend.
    pub device_id: u32,
    /// Human-readable device name.
    pub name: String,
    /// The backend this device belongs to.
    pub backend: Backend,
    /// Available memory on the device (bytes, 0 = unknown).
    pub memory_bytes: u64,
    /// Whether the device is healthy and usable.
    pub available: bool,
    /// Whether this is the recommended default device.
    pub recommended: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_state_is_available() {
        assert!(ModelState::Ready.is_available());
        assert!(ModelState::Idle.is_available());
        assert!(ModelState::Busy.is_available());
        assert!(!ModelState::Loading.is_available());
        assert!(!ModelState::Unloaded.is_available());
        assert!(!ModelState::Failed.is_available());
    }

    #[test]
    fn model_state_is_inactive() {
        assert!(!ModelState::Ready.is_inactive());
        assert!(ModelState::Unloaded.is_inactive());
        assert!(ModelState::Failed.is_inactive());
    }

    #[test]
    fn capabilities_default() {
        let cap = ModelCapabilities::default();
        assert!(!cap.text_generation);
        assert!(!cap.can_generate());
    }

    #[test]
    fn capabilities_detect_generation() {
        let cap = ModelCapabilities {
            text_generation: true,
            ..Default::default()
        };
        assert!(cap.can_generate());
    }

    #[test]
    fn capabilities_detect_tool_use() {
        let cap = ModelCapabilities {
            tool_calling: true,
            ..Default::default()
        };
        assert!(cap.can_use_tools());
    }
}