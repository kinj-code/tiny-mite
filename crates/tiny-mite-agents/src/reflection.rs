//! Reflection — structured analysis of task success and failure.
//!
//! The [`Reflection`] component produces structured reflection results
//! when verification fails or confidence is low. It does NOT require
//! an LLM — it uses deterministic analysis of the available evidence.

use serde::{Deserialize, Serialize};

use crate::verifier::VerificationOutcome;

/// The result of a reflection pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReflectionResult {
    /// Whether the reflection produced a useful correction.
    pub has_correction: bool,
    /// What worked well.
    pub what_worked: Vec<String>,
    /// What failed.
    pub what_failed: Vec<String>,
    /// Likely cause of any failures.
    pub likely_cause: Option<String>,
    /// Suggested correction.
    pub correction: Option<String>,
    /// Whether to retry the failed step.
    pub should_retry: bool,
    /// Suggested changes to the plan.
    pub plan_changes: Vec<String>,
    /// Confidence in this reflection (0.0–1.0).
    pub confidence: f32,
    /// Whether to escalate to a human or more capable model.
    pub escalate: bool,
}

impl ReflectionResult {
    /// Create a passing reflection (nothing to fix).
    #[must_use]
    pub fn nothing_to_report() -> Self {
        Self {
            has_correction: false,
            what_worked: vec!["All checks passed".into()],
            what_failed: Vec::new(),
            likely_cause: None,
            correction: None,
            should_retry: false,
            plan_changes: Vec::new(),
            confidence: 1.0,
            escalate: false,
        }
    }

    /// Create a reflection from a verification failure.
    #[must_use]
    pub fn from_failure(verification: &VerificationOutcome, step_description: &str) -> Self {
        Self {
            has_correction: true,
            what_worked: Vec::new(),
            what_failed: vec![format!(
                "Step '{}' verification failed: {}",
                step_description, verification.reason
            )],
            likely_cause: Some("Output did not meet verification criteria".into()),
            correction: Some(format!(
                "Re-run step '{}' with corrected instructions",
                step_description
            )),
            should_retry: true,
            plan_changes: vec![format!("Add retry for step '{}'", step_description)],
            confidence: 0.7,
            escalate: false,
        }
    }
}

/// Reflective analysis component.
///
/// Produces structured reflection results based on task outcomes.
/// Designed to be deterministic and cheap — no LLM calls required.
pub struct Reflection;

impl Reflection {
    /// Create a new reflection instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Analyze a step's verification outcome and produce a reflection.
    #[must_use]
    pub fn reflect(
        &self,
        step_description: &str,
        verification: &VerificationOutcome,
        attempt_count: u32,
    ) -> ReflectionResult {
        if verification.passed {
            return ReflectionResult::nothing_to_report();
        }

        let mut result = ReflectionResult::from_failure(verification, step_description);

        // Multiple failures suggest a deeper issue
        if attempt_count >= 3 {
            result.likely_cause =
                Some("Multiple retry attempts failed. Root cause may be structural.".into());
            result.should_retry = false;
            result.escalate = true;
            result.confidence = 0.4;
            result.correction = Some(
                "Escalate to human review or more capable model for root cause analysis".into(),
            );
        }

        // If exit code was non-zero, suggest debugging
        if verification.reason.contains("Exit code") {
            result.correction =
                Some("Command failed with non-zero exit code. Check error output and fix.".into());
        }

        result
    }

    /// Reflect on an entire plan execution.
    #[must_use]
    pub fn reflect_on_plan(
        &self,
        failed_steps: &[(&str, &VerificationOutcome)],
        passed_steps: &[&str],
        total_steps: usize,
    ) -> ReflectionResult {
        if failed_steps.is_empty() {
            return ReflectionResult::nothing_to_report();
        }

        let failure_rate = failed_steps.len() as f32 / total_steps.max(1) as f32;

        ReflectionResult {
            has_correction: true,
            what_worked: passed_steps
                .iter()
                .map(|s| format!("Step '{}' completed successfully", s))
                .collect(),
            what_failed: failed_steps
                .iter()
                .map(|(id, outcome)| format!("Step '{}' failed: {}", id, outcome.reason))
                .collect(),
            likely_cause: if failure_rate > 0.5 {
                Some("High failure rate suggests plan is too ambitious or resources are insufficient".into())
            } else {
                Some("Some individual steps need correction".into())
            },
            correction: Some("Re-plan the failed steps with more conservative constraints".into()),
            should_retry: failure_rate < 0.5,
            plan_changes: failed_steps
                .iter()
                .map(|(id, _)| format!("Revise step '{}' with explicit error handling", id))
                .collect(),
            confidence: (1.0 - failure_rate).max(0.1),
            escalate: failure_rate > 0.5,
        }
    }
}

impl Default for Reflection {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_report_when_no_issues() {
        let result = ReflectionResult::nothing_to_report();
        assert!(!result.has_correction);
        assert!(!result.should_retry);
    }

    #[test]
    fn failure_produces_correction() {
        let outcome = VerificationOutcome::fail("Exit code 1 (expected 0)");
        let result = ReflectionResult::from_failure(&outcome, "compile");
        assert!(result.has_correction);
        assert!(result.should_retry);
    }

    #[test]
    fn reflection_suggests_retry_on_first_failure() {
        let reflection = Reflection::new();
        let outcome = VerificationOutcome::fail("Exit code 1");
        let result = reflection.reflect("compile", &outcome, 1);
        assert!(result.should_retry);
        assert!(!result.escalate);
    }

    #[test]
    fn multiple_failures_escalate() {
        let reflection = Reflection::new();
        let outcome = VerificationOutcome::fail("Still failing");
        let result = reflection.reflect("compile", &outcome, 4);
        assert!(!result.should_retry);
        assert!(result.escalate);
    }

    #[test]
    fn plan_reflection_with_partial_failures() {
        let reflection = Reflection::new();
        let outcome = VerificationOutcome::fail("bad output");
        let failed = vec![("s2", &outcome)];
        let passed = vec!["s1", "s3"];
        let result = reflection.reflect_on_plan(&failed, &passed, 4);
        assert!(result.has_correction);
        assert!(result.should_retry); // only 1/4 failed
        assert!(!result.escalate);
    }

    #[test]
    fn plan_reflection_high_failure_rate_escalates() {
        let reflection = Reflection::new();
        let outcome = VerificationOutcome::fail("broken");
        let failed = vec![("s1", &outcome), ("s2", &outcome), ("s3", &outcome)];
        let passed: Vec<&str> = Vec::new();
        let result = reflection.reflect_on_plan(&failed, &passed, 4);
        assert!(result.escalate);
        assert!(!result.should_retry);
    }
}
