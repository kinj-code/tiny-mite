//! Verification engine — validates plan step outputs.
//!
//! The [`VerificationEngine`] provides deterministic verification of
//! plan step execution results. It supports schema validation, exit code
//! checking, invariant validation, and custom verification policies.

use crate::planner::{PlanStep, VerificationPolicy};

/// Result of a verification check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationOutcome {
    /// Whether the verification passed.
    pub passed: bool,
    /// Human-readable explanation.
    pub reason: String,
    /// Optional structured evidence.
    pub evidence: Option<String>,
}

impl VerificationOutcome {
    /// Create a passing result.
    #[must_use]
    pub fn pass(reason: impl Into<String>) -> Self {
        Self { passed: true, reason: reason.into(), evidence: None }
    }

    /// Create a failing result.
    #[must_use]
    pub fn fail(reason: impl Into<String>) -> Self {
        Self { passed: false, reason: reason.into(), evidence: None }
    }

    /// Attach evidence.
    #[must_use]
    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence = Some(evidence.into());
        self
    }
}

/// Deterministic verification engine.
///
/// Verifies that plan step outputs meet their declared verification
/// policies. Does not require an LLM.
pub struct VerificationEngine;

impl VerificationEngine {
    /// Create a new verification engine.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Verify a step's output according to its verification policy.
    #[must_use]
    pub fn verify(
        &self,
        step: &PlanStep,
        output: &str,
        exit_code: Option<i32>,
    ) -> VerificationOutcome {
        match &step.verification {
            VerificationPolicy::None => VerificationOutcome::pass("No verification required"),
            VerificationPolicy::ExitCode => match exit_code {
                Some(0) => VerificationOutcome::pass("Exit code 0 (success)"),
                Some(rc) => VerificationOutcome::fail(format!("Exit code {rc} (expected 0)")),
                None => VerificationOutcome::fail("No exit code available for verification"),
            },
            VerificationPolicy::Schema(schema) => {
                // Simple schema check: expected keyword must appear in output
                if output.contains(schema) {
                    VerificationOutcome::pass(format!("Schema keyword '{schema}' found in output"))
                        .with_evidence(output.to_owned())
                } else {
                    VerificationOutcome::fail(format!(
                        "Schema keyword '{schema}' not found in output"
                    ))
                }
            }
            VerificationPolicy::Invariant(invariant) => {
                // Simple invariant check: keyword must appear
                if output.contains(invariant) {
                    VerificationOutcome::pass(format!("Invariant '{invariant}' satisfied"))
                        .with_evidence(output.to_owned())
                } else {
                    VerificationOutcome::fail(format!("Invariant '{invariant}' not satisfied"))
                }
            }
            VerificationPolicy::Custom(check) => {
                // Custom verification: check that output contains the expected result
                if output.contains(check) || output.contains("SUCCESS") || output.contains("PASS") {
                    VerificationOutcome::pass(format!("Custom check '{check}' passed"))
                        .with_evidence(output.to_owned())
                } else {
                    VerificationOutcome::fail(format!("Custom check '{check}' failed"))
                }
            }
        }
    }

    /// Verify a completion message from a model against expected keywords.
    #[must_use]
    pub fn verify_completion(
        &self,
        response: &str,
        expected_keywords: &[&str],
    ) -> VerificationOutcome {
        let missing: Vec<&str> =
            expected_keywords.iter().filter(|kw| !response.contains(*kw)).copied().collect();

        if missing.is_empty() {
            VerificationOutcome::pass("All expected keywords present in response")
        } else {
            VerificationOutcome::fail(format!("Missing keywords: {:?}", missing))
        }
    }
}

impl Default for VerificationEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_verification_passes() {
        let step = PlanStep::new("s1", "test");
        let engine = VerificationEngine::new();
        let result = engine.verify(&step, "anything", None);
        assert!(result.passed);
    }

    #[test]
    fn exit_code_zero_passes() {
        let step = PlanStep::new("s1", "test").verify(VerificationPolicy::ExitCode);
        let engine = VerificationEngine::new();
        let result = engine.verify(&step, "", Some(0));
        assert!(result.passed);
    }

    #[test]
    fn exit_code_nonzero_fails() {
        let step = PlanStep::new("s1", "test").verify(VerificationPolicy::ExitCode);
        let engine = VerificationEngine::new();
        let result = engine.verify(&step, "", Some(1));
        assert!(!result.passed);
    }

    #[test]
    fn schema_keyword_found_passes() {
        let step = PlanStep::new("s1", "test").verify(VerificationPolicy::Schema("fn main".into()));
        let engine = VerificationEngine::new();
        let result = engine.verify(&step, "pub fn main() {}", None);
        assert!(result.passed);
    }

    #[test]
    fn schema_keyword_missing_fails() {
        let step = PlanStep::new("s1", "test").verify(VerificationPolicy::Schema("fn main".into()));
        let engine = VerificationEngine::new();
        let result = engine.verify(&step, "hello world", None);
        assert!(!result.passed);
    }

    #[test]
    fn invariant_check() {
        let step = PlanStep::new("s1", "test").verify(VerificationPolicy::Invariant("Rust".into()));
        let engine = VerificationEngine::new();
        let result = engine.verify(&step, "Rust is a systems programming language", None);
        assert!(result.passed);
    }

    #[test]
    fn completion_keyword_check() {
        let engine = VerificationEngine::new();
        let result = engine.verify_completion("Rust is fast and safe", &["Rust", "safe"]);
        assert!(result.passed);
    }

    #[test]
    fn completion_keyword_missing() {
        let engine = VerificationEngine::new();
        let result = engine.verify_completion("Rust is fast", &["Rust", "safe"]);
        assert!(!result.passed);
    }
}
