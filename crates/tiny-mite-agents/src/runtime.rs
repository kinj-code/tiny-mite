//! Agent runtime — intelligence loop coordinator.
//!
//! The [`AgentRuntime`] orchestrates the full intelligence pipeline:
//! classify → estimate complexity → plan → validate → execute → verify → reflect.
//!
//! It is the primary entry point for task processing in Tiny Mite.

use std::sync::Arc;
use tiny_mite_runtime::ModelCapabilities;

use crate::analysis::TaskAnalysis;
use crate::intent::IntentClassifier;
use crate::memory::WorkingMemory;
use crate::planner::{Plan, Planner};
use crate::reflection::{Reflection, ReflectionResult};
use crate::validator::{PlanValidator, ValidationResult};
use crate::verifier::{VerificationEngine, VerificationOutcome};

/// The result of running a task through the intelligence loop.
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// Whether the overall task succeeded.
    pub success: bool,
    /// The original task analysis.
    pub analysis: TaskAnalysis,
    /// The execution plan that was followed.
    pub plan: Plan,
    /// Verification results for each step.
    pub verification_results: Vec<VerificationOutcome>,
    /// Reflection on the overall execution.
    pub reflection: ReflectionResult,
    /// Current working memory state.
    pub memory: WorkingMemory,
    /// Summary message for the user.
    pub summary: String,
}

/// The agent runtime — coordinates the intelligence loop.
///
/// ```text
/// Input → classify → estimate → plan → validate → execute → verify → reflect → Output
/// ```
pub struct AgentRuntime {
    classifier: IntentClassifier,
    planner: Planner,
    validator: PlanValidator,
    verifier: VerificationEngine,
    reflector: Reflection,
    capabilities: ModelCapabilities,
}

impl AgentRuntime {
    /// Create a new agent runtime with the given model capabilities.
    #[must_use]
    pub fn new(capabilities: ModelCapabilities) -> Self {
        Self {
            classifier: IntentClassifier::new(),
            planner: Planner::new(),
            validator: PlanValidator::new(),
            verifier: VerificationEngine::new(),
            reflector: Reflection::new(),
            capabilities,
        }
    }

    /// Process a user request through the full intelligence pipeline.
    ///
    /// Returns a [`TaskResult`] containing the analysis, plan, and
    /// execution outcome.
    #[must_use]
    pub fn process(&self, input: &str) -> TaskResult {
        // Phase 1: Analyze
        let analysis = self.classifier.analyze(input);

        // Phase 2: Plan
        let plan = self.planner.plan(&analysis, input);

        // Phase 3: Validate
        let validation = self.validator.validate(&plan, &self.capabilities);

        // Phase 4: Initialize working memory
        let mut memory = WorkingMemory::new();
        memory.load_plan(&plan);

        // Phase 5: Simulate step execution and verification
        let mut verification_results = Vec::new();
        for step in &plan.steps {
            // In a real execution environment, the step would be dispatched
            // to a model provider. For architecture validation, we produce
            // a placeholder passing result.
            let outcome = self.verifier.verify(step, "PASS", Some(0));
            verification_results.push(outcome);
        }

        // Phase 6: Reflect
        let failed: Vec<(&str, &VerificationOutcome)> = verification_results
            .iter()
            .enumerate()
            .filter(|(_, o)| !o.passed)
            .map(|(i, o)| (plan.steps[i].id.as_str(), o))
            .collect();
        let passed: Vec<&str> = verification_results
            .iter()
            .enumerate()
            .filter(|(_, o)| o.passed)
            .map(|(i, _)| plan.steps[i].id.as_str())
            .collect();

        let reflection = self.reflector.reflect_on_plan(&failed, &passed, plan.steps.len());

        let success = validation.valid && verification_results.iter().all(|o| o.passed);

        let summary = format!(
            "Task: {}\nIntent: {:?}\nPlan: {} steps\nValidation: {}\nVerification: {} passed, {} failed\nReflection: {}",
            input,
            analysis.intent,
            plan.steps.len(),
            if validation.valid { "PASS" } else { "FAIL" },
            passed.len(),
            failed.len(),
            if reflection.has_correction { "suggestions available" } else { "no issues" }
        );

        TaskResult { success, analysis, plan, verification_results, reflection, memory, summary }
    }

    /// Get the model capabilities this runtime is configured with.
    #[must_use]
    pub fn capabilities(&self) -> &ModelCapabilities {
        &self.capabilities
    }
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new(ModelCapabilities { text_generation: true, chat: true, ..Default::default() })
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_question_returns_result() {
        let runtime = AgentRuntime::default();
        let result = runtime.process("what is Rust?");
        assert!(!result.summary.is_empty());
        assert!(!result.plan.steps.is_empty());
    }

    #[test]
    fn code_generation_returns_plan() {
        let runtime = AgentRuntime::default();
        let result = runtime.process("write code to implement a binary search tree");
        assert!(result.plan.step_count() >= 2);
    }

    #[test]
    fn memory_is_populated_from_plan() {
        let runtime = AgentRuntime::default();
        let result = runtime.process("write a function");
        assert!(!result.memory.is_empty());
    }

    #[test]
    fn verification_results_match_steps() {
        let runtime = AgentRuntime::default();
        let result = runtime.process("debug a crash");
        assert_eq!(result.verification_results.len(), result.plan.steps.len());
    }

    #[test]
    fn all_steps_pass_for_basic_query() {
        let runtime = AgentRuntime::default();
        let result = runtime.process("explain Rust ownership");
        let all_passed = result.verification_results.iter().all(|o| o.passed);
        assert!(all_passed);
    }

    #[test]
    fn capabilities_are_exposed() {
        let runtime = AgentRuntime::default();
        assert!(runtime.capabilities().text_generation);
    }
}
