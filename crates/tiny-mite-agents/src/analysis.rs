//! Task analysis types — the structured output of task classification.
//!
//! Provides [`TaskAnalysis`] which captures everything the intelligence
//! engine needs to know about a task before planning begins.

use serde::{Deserialize, Serialize};

use super::complexity::ComplexityScore;
use super::intent::{Intent, TaskType};

/// Structured analysis of a user task.
///
/// Produced by [`IntentClassifier`](super::intent::IntentClassifier)
/// before any planning or execution begins. The analysis is deterministic
/// and does not require an LLM call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskAnalysis {
    /// The primary user intent.
    pub intent: Intent,
    /// The type of task this represents.
    pub task_type: TaskType,
    /// Multi-dimensional complexity estimate.
    pub complexity: ComplexityScore,
    /// Whether this task likely needs retrieval (RAG).
    pub requires_retrieval: bool,
    /// Whether this task likely needs tool execution.
    pub requires_tools: Vec<String>,
    /// Whether this task likely needs persistent memory access.
    pub requires_memory: bool,
    /// Whether streaming output is appropriate.
    pub requires_streaming: bool,
    /// Whether planning is required (multi-step tasks).
    pub requires_planning: bool,
    /// Whether reasoning/chain-of-thought is beneficial.
    pub requires_reasoning: bool,
    /// Estimated tokens needed for the full task.
    pub estimated_total_tokens: usize,
    /// Estimated tokens needed for the prompt alone.
    pub estimated_prompt_tokens: usize,
    /// How confident the classifier is (0.0–1.0).
    pub confidence: f32,
    /// Which model capabilities are needed.
    pub required_capabilities: Vec<String>,
    /// Human-readable reasoning for the classification.
    pub reasoning_hint: String,
    /// Risk level for this task (0 = none, 100 = critical).
    pub risk_score: u32,
    /// Whether to escalate to a more capable model.
    pub escalate: bool,
}

impl TaskAnalysis {
    /// Create a simple analysis for a basic task.
    #[must_use]
    pub fn simple(intent: Intent, task_type: TaskType) -> Self {
        Self {
            intent,
            task_type,
            complexity: ComplexityScore::simple(),
            requires_retrieval: false,
            requires_tools: Vec::new(),
            requires_memory: false,
            requires_streaming: false,
            requires_planning: false,
            requires_reasoning: false,
            estimated_total_tokens: 512,
            estimated_prompt_tokens: 256,
            confidence: 1.0,
            required_capabilities: vec!["text_generation".into()],
            reasoning_hint: "Simple task, no special requirements".into(),
            risk_score: 0,
            escalate: false,
        }
    }

    /// Returns `true` if this task can be handled without an LLM.
    #[must_use]
    pub fn can_handle_deterministically(&self) -> bool {
        !self.requires_reasoning
            && !self.requires_planning
            && self.requires_tools.is_empty()
            && self.intent == Intent::Question
            && self.estimated_total_tokens <= 512
    }

    /// Summary string for logging.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "intent={:?} type={:?} complexity={:.1} tools={} retrieval={} reasoning={} confidence={:.2}",
            self.intent,
            self.task_type,
            self.complexity.overall,
            self.requires_tools.len(),
            self.requires_retrieval,
            self.requires_reasoning,
            self.confidence
        )
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_analysis_defaults() {
        let analysis = TaskAnalysis::simple(Intent::Question, TaskType::FactualQuery);
        assert_eq!(analysis.intent, Intent::Question);
        assert!(!analysis.requires_planning);
        assert!(analysis.confidence > 0.9);
    }

    #[test]
    fn deterministic_detection() {
        let analysis = TaskAnalysis::simple(Intent::Question, TaskType::FactualQuery);
        assert!(analysis.can_handle_deterministically());
    }

    #[test]
    fn complex_task_needs_llm() {
        let mut analysis = TaskAnalysis::simple(Intent::CodeGeneration, TaskType::Implementation);
        analysis.requires_planning = true;
        analysis.requires_reasoning = true;
        assert!(!analysis.can_handle_deterministically());
    }

    #[test]
    fn summary_includes_key_fields() {
        let analysis = TaskAnalysis::simple(Intent::CodeGeneration, TaskType::Implementation);
        let summary = analysis.summary();
        assert!(summary.contains("CodeGeneration"));
        assert!(summary.contains("Implementation"));
    }
}
