//! Inference request/response types and context budgeting.
//!
//! Structured request/response types isolate the provider abstraction
//! from raw strings and enable typed tool calls, structured output,
//! and observability.

use serde::{Deserialize, Serialize};
use std::fmt;
use tiny_mite_domain::{CorrelationId, ModelId, TaskId};

// ---------------------------------------------------------------------------
// Context budget
// ---------------------------------------------------------------------------

/// Tracks and enforces token budget for a single inference request.
///
/// The context manager should allocate token budgets before sending
/// requests to the model. This type helps enforce those budgets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBudget {
    /// Maximum total tokens (prompt + output).
    pub max_total_tokens: usize,
    /// Tokens reserved for system instructions.
    pub system_tokens: usize,
    /// Tokens reserved for conversation history.
    pub conversation_tokens: usize,
    /// Tokens reserved for retrieved context.
    pub retrieval_tokens: usize,
    /// Tokens reserved for tool definitions.
    pub tool_tokens: usize,
    /// Tokens reserved for output generation.
    pub output_tokens: usize,
}

impl ContextBudget {
    /// Create a new budget with the given total context window.
    ///
    /// Splits conservatively: reserves 20% for output, 10% for system,
    /// 10% for tools, leaving 60% for conversation + retrieval.
    #[must_use]
    pub fn new(max_total_tokens: usize) -> Self {
        let output = (max_total_tokens as f64 * 0.20) as usize;
        let system = (max_total_tokens as f64 * 0.10) as usize;
        let tool = (max_total_tokens as f64 * 0.10) as usize;
        // Remaining: conversation + retrieval
        let remaining = max_total_tokens.saturating_sub(output + system + tool);
        let conversation = remaining / 2;
        let retrieval = remaining - conversation;

        Self {
            max_total_tokens,
            system_tokens: system,
            conversation_tokens: conversation,
            retrieval_tokens: retrieval,
            tool_tokens: tool,
            output_tokens: output,
        }
    }

    /// Total allocated (non-output) tokens.
    #[must_use]
    pub fn allocated_input_tokens(&self) -> usize {
        self.system_tokens + self.conversation_tokens + self.retrieval_tokens + self.tool_tokens
    }

    /// Whether a given number of additional tokens would fit within the budget.
    #[must_use]
    pub fn can_fit(&self, additional_tokens: usize) -> bool {
        self.allocated_input_tokens() + additional_tokens <= self.max_total_tokens
    }
}

impl fmt::Display for ContextBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "budget: total={}, system={}, conv={}, retrieval={}, tools={}, output={}",
            self.max_total_tokens,
            self.system_tokens,
            self.conversation_tokens,
            self.retrieval_tokens,
            self.tool_tokens,
            self.output_tokens
        )
    }
}

// ---------------------------------------------------------------------------
// Inference request
// ---------------------------------------------------------------------------

/// A structured request for model inference.
///
/// # Security
///
/// The `prompt` field may contain arbitrary text. Producers are responsible
/// for avoiding sensitive data in prompts that would be logged.
/// The `system_prompt` is treated as instructions, not user data.
#[derive(Debug, Clone)]
pub struct InferenceRequest {
    /// Model to use for inference.
    pub model_id: ModelId,
    /// The main prompt or conversation text.
    pub prompt: String,
    /// Optional system-level instructions.
    pub system_prompt: Option<String>,
    /// Maximum tokens to generate.
    pub max_tokens: usize,
    /// Sampling temperature (0.0–2.0).
    pub temperature: f32,
    /// Nucleus sampling threshold (None = no top-p).
    pub top_p: Option<f32>,
    /// Top-k sampling (None = no top-k).
    pub top_k: Option<usize>,
    /// Random seed (None = random).
    pub seed: Option<u64>,
    /// Stop sequences that halt generation.
    pub stop_sequences: Vec<String>,
    /// Grammar for constrained output.
    pub grammar: Option<String>,
    /// Tool definitions for function calling.
    pub tools: Vec<ToolDefinition>,
    /// Correlation ID for tracing.
    pub correlation_id: Option<CorrelationId>,
    /// Task ID for scheduler integration.
    pub task_id: Option<TaskId>,
    /// Timeout in milliseconds (None = no timeout).
    pub timeout_ms: Option<u64>,
    /// Context budget for this request.
    pub context_budget: ContextBudget,
}

// ---------------------------------------------------------------------------
// Tool definition
// ---------------------------------------------------------------------------

/// A tool available to the model during inference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the tool's parameters.
    pub parameters: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Tool call (from model output)
// ---------------------------------------------------------------------------

/// A tool call extracted from model output.
///
/// This is DATA, not AUTHORITY. The Tool Gateway must authorize
/// execution separately.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    /// Unique ID of this tool call instance.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Arguments as JSON.
    pub arguments: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Inference response
// ---------------------------------------------------------------------------

/// A response from a model inference.
///
/// # Streaming
///
/// During streaming, each incremental response contains partial text
/// in `text` and may have an empty `finish_reason`. The final response
/// has a non-empty `finish_reason`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResponse {
    /// Unique ID for this response (or correlation ID during streaming).
    pub id: String,
    /// The model that generated this response.
    pub model_id: ModelId,
    /// Generated text (can be partial during streaming).
    pub text: String,
    /// Why generation stopped ("stop", "length", "tool_calls", etc.).
    pub finish_reason: String,
    /// Number of tokens in the prompt.
    pub prompt_tokens: usize,
    /// Number of tokens generated.
    pub generated_tokens: usize,
    /// Total tokens used (prompt + generated).
    pub total_tokens: usize,
    /// Total elapsed wall time in milliseconds.
    pub elapsed_ms: f64,
    /// Correlation ID for tracing.
    pub correlation_id: Option<CorrelationId>,
    /// Tool calls extracted from the response.
    pub tool_calls: Vec<ToolCall>,
    /// Structured output (if grammar/structured output was requested).
    pub structured_output: Option<serde_json::Value>,
}

impl InferenceResponse {
    /// Returns `true` if this is a partial/streaming response.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        self.finish_reason.is_empty()
    }

    /// Returns `true` if generation completed normally.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        !self.finish_reason.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Sampling config
// ---------------------------------------------------------------------------

/// Reusable sampling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingConfig {
    /// Temperature.
    pub temperature: f32,
    /// Top-p.
    pub top_p: Option<f32>,
    /// Top-k.
    pub top_k: Option<usize>,
    /// Maximum tokens to generate.
    pub max_tokens: usize,
    /// Seed.
    pub seed: Option<u64>,
    /// Stop sequences.
    pub stop_sequences: Vec<String>,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: None,
            top_k: None,
            max_tokens: 512,
            seed: None,
            stop_sequences: Vec::new(),
        }
    }
}

impl SamplingConfig {
    /// Apply the sampling config to an inference request builder.
    #[must_use]
    pub fn apply_to(&self) -> (f32, Option<f32>, Option<usize>, usize, Option<u64>, Vec<String>) {
        (
            self.temperature,
            self.top_p,
            self.top_k,
            self.max_tokens,
            self.seed,
            self.stop_sequences.clone(),
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_budget_splits_reasonably() {
        let budget = ContextBudget::new(8192);
        // 20% for output
        assert_eq!(budget.output_tokens, 1638);
        // 10% each for system and tools
        assert_eq!(budget.system_tokens, 819);
        assert_eq!(budget.tool_tokens, 819);
        // Remaining split between conversation and retrieval
        assert!(budget.conversation_tokens > 0);
        assert!(budget.retrieval_tokens > 0);
        // Sum should equal max
        let total = budget.system_tokens
            + budget.conversation_tokens
            + budget.retrieval_tokens
            + budget.tool_tokens
            + budget.output_tokens;
        assert_eq!(total, 8192);
    }

    #[test]
    fn context_budget_can_fit() {
        let budget = ContextBudget::new(4096);
        assert!(budget.can_fit(0));
        assert!(budget.can_fit(100));
        // Very large addition should still return false
        assert!(!budget.can_fit(5000));
    }

    #[test]
    fn response_is_partial_detection() {
        let partial = InferenceResponse {
            id: "1".into(),
            model_id: ModelId::new(),
            text: "Hello ".into(),
            finish_reason: "".into(),
            prompt_tokens: 1,
            generated_tokens: 1,
            total_tokens: 2,
            elapsed_ms: 10.0,
            correlation_id: None,
            tool_calls: Vec::new(),
            structured_output: None,
        };
        assert!(partial.is_partial());
        assert!(!partial.is_finished());
    }

    #[test]
    fn response_is_finished_detection() {
        let finished = InferenceResponse {
            id: "2".into(),
            model_id: ModelId::new(),
            text: "World".into(),
            finish_reason: "stop".into(),
            prompt_tokens: 1,
            generated_tokens: 1,
            total_tokens: 2,
            elapsed_ms: 10.0,
            correlation_id: None,
            tool_calls: Vec::new(),
            structured_output: None,
        };
        assert!(!finished.is_partial());
        assert!(finished.is_finished());
    }

    #[test]
    fn sampling_config_defaults() {
        let cfg = SamplingConfig::default();
        assert_eq!(cfg.temperature, 0.7);
        assert_eq!(cfg.max_tokens, 512);
    }
}
