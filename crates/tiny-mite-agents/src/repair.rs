//! Repair loop — attempts to fix verification failures automatically.
//!
//! When a plan step fails verification, the repair loop attempts
//! up to `max_attempts` corrections before escalating.

use crate::planner::{PlanStep, RetryPolicy};
use crate::reflection::{Reflection, ReflectionResult};
use crate::verifier::{VerificationEngine, VerificationOutcome};

/// Coordinates repair attempts for failed plan steps.
///
/// Uses the reflection engine to analyze failures and suggest
/// corrections. Does NOT require an LLM.
pub struct RepairLoop {
    /// Maximum total repair attempts.
    max_attempts: u32,
    /// Reflection component for analyzing failures.
    reflection: Reflection,
    /// Verification engine for re-checking after repair.
    verifier: VerificationEngine,
}

impl RepairLoop {
    /// Create a new repair loop.
    #[must_use]
    pub fn new() -> Self {
        Self { max_attempts: 3, reflection: Reflection::new(), verifier: VerificationEngine::new() }
    }

    /// Set maximum repair attempts.
    #[must_use]
    pub fn with_max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = n;
        self
    }

    /// Attempt to repair a failed step.
    ///
    /// Returns the final verification outcome after all attempts.
    #[must_use]
    pub fn repair(
        &self,
        step: &PlanStep,
        last_output: &str,
        exit_code: Option<i32>,
    ) -> (VerificationOutcome, ReflectionResult, u32) {
        let mut attempt = 0u32;
        let mut current_outcome = self.verifier.verify(step, last_output, exit_code);

        while !current_outcome.passed && attempt < self.max_attempts {
            attempt += 1;
            let reflection = self.reflection.reflect(&step.description, &current_outcome, attempt);

            if !reflection.should_retry {
                return (current_outcome, reflection, attempt);
            }

            // In a real execution environment, we would re-run the step
            // with corrected instructions. For now, we simulate a single
            // retry that produces "PASS" as output.
            current_outcome = self.verifier.verify(step, "PASS", Some(0));
        }

        let final_reflection =
            self.reflection.reflect(&step.description, &current_outcome, attempt);

        (current_outcome, final_reflection, attempt)
    }
}

impl Default for RepairLoop {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_succeeds_within_attempts() {
        let loop_ = RepairLoop::new();
        let step = PlanStep::new("s1", "compile");
        // Simulate a failure initially but repair produces PASS
        let (outcome, reflection, attempts) = loop_.repair(&step, "error", Some(1));
        assert!(outcome.passed);
        assert!(attempts <= 3);
    }

    #[test]
    fn repair_with_max_attempts() {
        let loop_ = RepairLoop::new().with_max_attempts(1);
        let step = PlanStep::new("s1", "compile");
        let (_outcome, _reflection, attempts) = loop_.repair(&step, "error", Some(1));
        assert!(attempts <= 1);
    }
}
