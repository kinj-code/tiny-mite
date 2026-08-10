//! Task complexity estimation — multi-dimensional scoring.
//!
//! The [`TaskComplexityEstimator`] produces a [`ComplexityScore`] using
//! deterministic heuristics. No LLM required.

use serde::{Deserialize, Serialize};

use super::intent::Intent;

// ── Complexity Score ─────────────────────────────────────────────

/// Multi-dimensional complexity estimate for a task.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ComplexityScore {
    /// Overall complexity (0–100).
    pub overall: f32,
    /// How much reasoning depth is needed (0–100).
    pub reasoning_depth: f32,
    /// How much planning depth is needed (0–100).
    pub planning_depth: f32,
    /// How much context will be loaded (0–100).
    pub context_load: f32,
    /// How many tools are expected (0–100).
    pub tool_load: f32,
    /// Risk level (0–100).
    pub risk: f32,
    /// Whether the task can be parallelized (0–100).
    pub parallelism: f32,
    /// Confidence in this estimate (0.0–1.0).
    pub confidence: f32,
}

impl ComplexityScore {
    /// A default simple score for trivial tasks.
    #[must_use]
    pub fn simple() -> Self {
        Self {
            overall: 5.0,
            reasoning_depth: 0.0,
            planning_depth: 0.0,
            context_load: 5.0,
            tool_load: 0.0,
            risk: 0.0,
            parallelism: 0.0,
            confidence: 1.0,
        }
    }

    /// Returns `true` if the task is trivially simple.
    #[must_use]
    pub fn is_simple(&self) -> bool {
        self.overall < 20.0
    }

    /// Returns `true` if the task requires significant planning.
    #[must_use]
    pub fn needs_planning(&self) -> bool {
        self.planning_depth > 30.0 || self.overall > 50.0
    }
}

// ── Task Complexity Estimator ────────────────────────────────────

/// Deterministic complexity estimator.
pub struct TaskComplexityEstimator;

impl TaskComplexityEstimator {
    /// Create a new estimator.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Estimate complexity based on intent and features.
    #[must_use]
    pub fn estimate(
        &self,
        intent: Intent,
        requires_planning: bool,
        requires_reasoning: bool,
        tool_count: u32,
        estimated_tokens: usize,
    ) -> ComplexityScore {
        let intent_score: f32 = match intent {
            Intent::Question => 5.0,
            Intent::Explanation => 10.0,
            Intent::Summarization => 10.0,
            Intent::Creation => 20.0,
            Intent::CodeGeneration => 40.0,
            Intent::CodeReview => 35.0,
            Intent::Debugging => 50.0,
            Intent::Analysis => 25.0,
            Intent::Action => 60.0,
            Intent::Planning => 55.0,
            Intent::Unknown => 30.0,
        };

        let reasoning: f32 = if requires_reasoning { 60.0 } else { intent_score * 0.3_f32 };
        let planning: f32 = if requires_planning { 50.0 } else { intent_score * 0.2_f32 };

        let context: f32 = match estimated_tokens {
            0..=512 => 5.0,
            513..=2048 => 20.0,
            2049..=8192 => 50.0,
            _ => 80.0,
        };

        let tool: f32 = match tool_count {
            0 => 0.0,
            1 => 15.0,
            2..=3 => 35.0,
            _ => 60.0,
        };

        let risk: f32 = match intent {
            Intent::Action => 70.0,
            Intent::CodeGeneration if tool_count > 0 => 40.0,
            _ => 10.0,
        };

        let parallel: f32 = if requires_planning && tool_count >= 2 { 50.0 } else { 0.0 };

        let overall: f32 = (intent_score * 0.3_f32)
            + (reasoning * 0.2_f32)
            + (planning * 0.2_f32)
            + (context * 0.15_f32)
            + (tool * 0.1_f32)
            + (risk * 0.05_f32);

        ComplexityScore {
            overall: overall.min(100.0_f32),
            reasoning_depth: reasoning.min(100.0_f32),
            planning_depth: planning.min(100.0_f32),
            context_load: context.min(100.0_f32),
            tool_load: tool.min(100.0_f32),
            risk: risk.min(100.0_f32),
            parallelism: parallel.min(100.0_f32),
            confidence: if intent == Intent::Unknown { 0.5_f32 } else { 0.9_f32 },
        }
    }
}

impl Default for TaskComplexityEstimator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_task_is_simple() {
        let score = ComplexityScore::simple();
        assert!(score.is_simple());
        assert!(!score.needs_planning());
    }

    #[test]
    fn question_is_low_complexity() {
        let est = TaskComplexityEstimator::new();
        let score = est.estimate(Intent::Question, false, false, 0, 256);
        assert!(score.overall < 15.0);
    }

    #[test]
    fn debugging_is_high_complexity() {
        let est = TaskComplexityEstimator::new();
        let score = est.estimate(Intent::Debugging, true, true, 0, 1024);
        assert!(score.overall > 35.0);
        assert!(score.reasoning_depth > 40.0);
    }

    #[test]
    fn action_with_tools_is_highest() {
        let est = TaskComplexityEstimator::new();
        let score = est.estimate(Intent::Action, true, false, 3, 4096);
        assert!(score.overall > 40.0);
        assert!(score.risk > 50.0);
    }

    #[test]
    fn unknown_intent_lowers_confidence() {
        let est = TaskComplexityEstimator::new();
        let score = est.estimate(Intent::Unknown, false, false, 0, 256);
        assert!(score.confidence < 0.7);
    }

    #[test]
    fn parallel_detection() {
        let est = TaskComplexityEstimator::new();
        let parallel = est.estimate(Intent::CodeGeneration, true, false, 3, 2048);
        let serial = est.estimate(Intent::Question, false, false, 0, 256);
        assert!(parallel.parallelism > serial.parallelism);
    }
}
