//! Task planner — transforms TaskAnalysis into executable plans.
//!
//! The [`Planner`] creates dependency-aware execution plans with
//! verification policies, retry policies, and step-level capabilities.

use serde::{Deserialize, Serialize};
use std::fmt;

use super::analysis::TaskAnalysis;

// ── Execution Policy ─────────────────────────────────────────────

/// How a plan step should be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionPolicy {
    /// Execute immediately in sequence.
    Sequential,
    /// Can execute in parallel with other independent steps.
    Parallel,
    /// Execute only on a specific condition.
    Conditional { condition: u32 },
    /// Skip if a previous step succeeded.
    SkipOnSuccess,
    /// Execute only if a previous step failed.
    OnFailure,
}

// ── Retry Policy ────────────────────────────────────────────────

/// How retries should be handled for a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryPolicy {
    /// Do not retry on failure.
    NoRetry,
    /// Retry up to N times.
    Retry(u32),
    /// Retry with exponential backoff.
    RetryWithBackoff { max_retries: u32, base_delay_ms: u64 },
}

// ── Verification Policy ─────────────────────────────────────────

/// How a step's output should be verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationPolicy {
    /// No verification needed.
    None,
    /// Verify output matches a schema.
    Schema(String),
    /// Verify a specific invariant holds.
    Invariant(String),
    /// Verify the step returned a success exit code.
    ExitCode,
    /// Custom verification logic.
    Custom(String),
}

// ── Plan Step ────────────────────────────────────────────────────

/// A single step in an execution plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    /// Unique step identifier.
    pub id: String,
    /// Human-readable description of what this step does.
    pub description: String,
    /// IDs of steps that must complete before this one.
    pub dependencies: Vec<String>,
    /// Required model capabilities for this step.
    pub required_capabilities: Vec<String>,
    /// Tools needed for this step.
    pub tools: Vec<String>,
    /// Expected output description.
    pub expected_output: String,
    /// How to verify this step's output.
    pub verification: VerificationPolicy,
    /// How to retry on failure.
    pub retry_policy: RetryPolicy,
    /// Execution policy for this step.
    pub execution_policy: ExecutionPolicy,
    /// Timeout in milliseconds (0 = no timeout).
    pub timeout_ms: u64,
    /// Priority (higher = more important).
    pub priority: u32,
    /// Estimated tokens for this step.
    pub estimated_tokens: usize,
}

impl PlanStep {
    /// Create a simple sequential step.
    #[must_use]
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            dependencies: Vec::new(),
            required_capabilities: vec!["text_generation".into()],
            tools: Vec::new(),
            expected_output: String::new(),
            verification: VerificationPolicy::None,
            retry_policy: RetryPolicy::NoRetry,
            execution_policy: ExecutionPolicy::Sequential,
            timeout_ms: 30000,
            priority: 0,
            estimated_tokens: 256,
        }
    }

    /// Add a dependency on another step.
    #[must_use]
    pub fn depends_on(mut self, step_id: impl Into<String>) -> Self {
        self.dependencies.push(step_id.into());
        self
    }

    /// Set the verification policy.
    #[must_use]
    pub fn verify(mut self, policy: VerificationPolicy) -> Self {
        self.verification = policy;
        self
    }

    /// Set retry policy.
    #[must_use]
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Set execution to parallel.
    #[must_use]
    pub fn parallel(mut self) -> Self {
        self.execution_policy = ExecutionPolicy::Parallel;
        self
    }

    /// Add required tools.
    #[must_use]
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }
}

// ── Plan ─────────────────────────────────────────────────────────

/// An executable plan containing ordered steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Unique plan identifier.
    pub id: String,
    /// The original task description.
    pub task_description: String,
    /// Ordered list of steps.
    pub steps: Vec<PlanStep>,
    /// The analysis that generated this plan.
    pub analysis_summary: String,
    /// Total estimated tokens across all steps.
    pub total_estimated_tokens: usize,
}

impl Plan {
    /// Create a new plan with no steps.
    #[must_use]
    pub fn new(id: impl Into<String>, task_description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            task_description: task_description.into(),
            steps: Vec::new(),
            analysis_summary: String::new(),
            total_estimated_tokens: 0,
        }
    }

    /// Add a step to the plan.
    pub fn add_step(&mut self, step: PlanStep) {
        self.total_estimated_tokens += step.estimated_tokens;
        self.steps.push(step);
    }

    /// Number of steps in the plan.
    #[must_use]
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Returns `true` if all step dependencies are satisfied.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let step_ids: std::collections::HashSet<&str> =
            self.steps.iter().map(|s| s.id.as_str()).collect();

        for step in &self.steps {
            for dep in &step.dependencies {
                if !step_ids.contains(dep.as_str()) {
                    return false;
                }
            }
        }
        true
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Plan: {} ({} steps, ~{} tokens)",
            self.id,
            self.steps.len(),
            self.total_estimated_tokens
        )?;
        for (i, step) in self.steps.iter().enumerate() {
            writeln!(f, "  {}. {} [{} deps]", i + 1, step.description, step.dependencies.len())?;
        }
        Ok(())
    }
}

// ── Planner ──────────────────────────────────────────────────────

/// Creates executable plans from task analyses.
pub struct Planner;

impl Planner {
    /// Create a new planner.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Generate a plan from a task analysis.
    ///
    /// The plan is deterministic based on the analysis — no LLM required.
    #[must_use]
    pub fn plan(&self, analysis: &TaskAnalysis, task_description: &str) -> Plan {
        let mut plan = Plan::new("plan_1", task_description);
        plan.analysis_summary = analysis.summary();

        match analysis.intent {
            super::intent::Intent::CodeGeneration => {
                plan.add_step(
                    PlanStep::new("step_1", "Analyze requirements and design solution").verify(
                        VerificationPolicy::Invariant("Design covers all requirements".into()),
                    ),
                );
                plan.add_step(
                    PlanStep::new("step_2", "Implement the solution").depends_on("step_1").retry(
                        RetryPolicy::RetryWithBackoff { max_retries: 3, base_delay_ms: 1000 },
                    ),
                );
                if analysis.requires_tools.contains(&"test-runner".to_owned()) {
                    plan.add_step(
                        PlanStep::new("step_3", "Run test suite and verify")
                            .depends_on("step_2")
                            .verify(VerificationPolicy::ExitCode),
                    );
                }
            }
            super::intent::Intent::Debugging => {
                plan.add_step(PlanStep::new("step_1", "Reproduce and isolate the bug"));
                plan.add_step(PlanStep::new("step_2", "Identify root cause").depends_on("step_1"));
                plan.add_step(PlanStep::new("step_3", "Implement fix").depends_on("step_2"));
                plan.add_step(
                    PlanStep::new("step_4", "Verify fix and run tests")
                        .depends_on("step_3")
                        .verify(VerificationPolicy::ExitCode),
                );
            }
            super::intent::Intent::Planning => {
                plan.add_step(PlanStep::new("step_1", "Analyze requirements and constraints"));
                plan.add_step(
                    PlanStep::new("step_2", "Research existing solutions and patterns")
                        .depends_on("step_1"),
                );
                plan.add_step(
                    PlanStep::new("step_3", "Design high-level architecture").depends_on("step_2"),
                );
                plan.add_step(
                    PlanStep::new("step_4", "Detail component design").depends_on("step_3"),
                );
                plan.add_step(
                    PlanStep::new("step_5", "Review and validate design")
                        .depends_on("step_4")
                        .verify(VerificationPolicy::Invariant(
                            "Design is complete and consistent".into(),
                        )),
                );
            }
            super::intent::Intent::Action => {
                plan.add_step(
                    PlanStep::new("step_1", "Validate prerequisites and permissions")
                        .verify(VerificationPolicy::Invariant("All prerequisites met".into())),
                );
                plan.add_step(
                    PlanStep::new("step_2", "Execute the action")
                        .depends_on("step_1")
                        .with_tools(analysis.requires_tools.clone())
                        .retry(RetryPolicy::Retry(2)),
                );
                plan.add_step(
                    PlanStep::new("step_3", "Verify action result")
                        .depends_on("step_2")
                        .verify(VerificationPolicy::ExitCode),
                );
            }
            _ => {
                // Simple sequential plan for questions, explanations, etc.
                if analysis.requires_reasoning {
                    plan.add_step(PlanStep::new(
                        "step_1",
                        "Analyze the question and gather context",
                    ));
                    plan.add_step(
                        PlanStep::new("step_2", "Reason through the answer").depends_on("step_1"),
                    );
                    plan.add_step(
                        PlanStep::new("step_3", "Formulate and verify response")
                            .depends_on("step_2"),
                    );
                } else {
                    plan.add_step(PlanStep::new("step_1", "Answer directly"));
                }
            }
        }

        plan
    }
}

impl Default for Planner {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::TaskAnalysis;
    use crate::intent::{Intent, TaskType};

    #[test]
    fn plan_step_builder() {
        let step = PlanStep::new("s1", "test step")
            .depends_on("s0")
            .verify(VerificationPolicy::ExitCode)
            .retry(RetryPolicy::Retry(3))
            .parallel();
        assert_eq!(step.id, "s1");
        assert_eq!(step.dependencies, vec!["s0"]);
        assert_eq!(step.execution_policy, ExecutionPolicy::Parallel);
    }

    #[test]
    fn plan_validity_check() {
        let mut plan = Plan::new("p1", "test plan");
        plan.add_step(PlanStep::new("s1", "first"));
        plan.add_step(PlanStep::new("s2", "second").depends_on("s1"));
        assert!(plan.is_valid());

        let mut invalid = Plan::new("p2", "bad refs");
        invalid.add_step(PlanStep::new("s1", "first").depends_on("missing"));
        assert!(!invalid.is_valid());
    }

    #[test]
    fn code_generation_plan_has_multiple_steps() {
        let planner = Planner::new();
        let mut analysis = TaskAnalysis::simple(Intent::CodeGeneration, TaskType::Implementation);
        analysis.requires_tools = vec!["test-runner".into()];
        let plan = planner.plan(&analysis, "write a BST");
        assert!(plan.step_count() >= 2);
    }

    #[test]
    fn debugging_plan_has_bug_fix_steps() {
        let planner = Planner::new();
        let mut analysis = TaskAnalysis::simple(Intent::Debugging, TaskType::BugFix);
        analysis.requires_reasoning = true;
        let plan = planner.plan(&analysis, "fix null pointer");
        assert!(plan.step_count() >= 3);
    }

    #[test]
    fn planning_task_includes_design_review() {
        let planner = Planner::new();
        let mut analysis = TaskAnalysis::simple(Intent::Planning, TaskType::Design);
        analysis.requires_planning = true;
        let plan = planner.plan(&analysis, "design a system");
        assert!(plan.steps.iter().any(|s| s.description.contains("Review")));
    }

    #[test]
    fn display_format_includes_steps() {
        let plan = Planner::new()
            .plan(&TaskAnalysis::simple(Intent::Question, TaskType::FactualQuery), "what is Rust?");
        let display = format!("{plan}");
        assert!(display.contains("1 steps"));
    }
}
