//! Plan validator — detects structural issues in execution plans.
//!
//! The [`PlanValidator`] checks plans for correctness before execution
//! without requiring an LLM. It detects circular dependencies, missing
//! tools, incompatible capabilities, and other structural issues.

use std::collections::{HashMap, HashSet};
use tiny_mite_runtime::ModelCapabilities;

use crate::planner::{Plan, PlanStep};

/// The result of validating a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    /// Whether the plan is valid (no errors).
    pub valid: bool,
    /// List of error messages.
    pub errors: Vec<String>,
    /// List of warnings (non-fatal issues).
    pub warnings: Vec<String>,
    /// Whether all required capabilities are available.
    pub capabilities_satisfied: bool,
}

impl ValidationResult {
    /// Create a clean validation result.
    #[must_use]
    pub fn new() -> Self {
        Self { valid: true, errors: Vec::new(), warnings: Vec::new(), capabilities_satisfied: true }
    }

    /// Returns `true` if there are no errors.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Validates execution plans for structural correctness.
pub struct PlanValidator;

impl PlanValidator {
    /// Create a new validator.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Validate a plan against available capabilities.
    #[must_use]
    pub fn validate(&self, plan: &Plan, capabilities: &ModelCapabilities) -> ValidationResult {
        let mut result = ValidationResult::new();

        // Check empty plan
        if plan.steps.is_empty() {
            result.errors.push("Plan has no steps".into());
            result.valid = false;
            return result;
        }

        // Build step ID set for dependency resolution
        let step_ids: HashSet<&str> = plan.steps.iter().map(|s| s.id.as_str()).collect();

        // Check each step
        for step in &plan.steps {
            self.validate_step(step, &step_ids, capabilities, &mut result);
        }

        // Check for circular dependencies
        if self.has_circular_dependencies(&plan.steps) {
            result.errors.push("Plan contains circular dependencies".into());
            result.valid = false;
        }

        // Check for orphan steps (no path from start)
        if self.has_orphan_steps(&plan.steps) {
            result.warnings.push(
                "Plan contains steps with no execution path from the start. Consider reorganizing."
                    .into(),
            );
        }

        result.valid = result.errors.is_empty();
        result
    }

    fn validate_step(
        &self,
        step: &PlanStep,
        step_ids: &HashSet<&str>,
        capabilities: &ModelCapabilities,
        result: &mut ValidationResult,
    ) {
        // Check dependencies exist
        for dep in &step.dependencies {
            if !step_ids.contains(dep.as_str()) {
                result
                    .errors
                    .push(format!("Step '{}' depends on non-existent step '{}'", step.id, dep));
                result.valid = false;
            }
        }

        // Check required capabilities against available ones
        for cap in &step.required_capabilities {
            let available = match cap.as_str() {
                "text_generation" => capabilities.text_generation,
                "chat" => capabilities.chat,
                "tool_calling" => capabilities.tool_calling,
                "structured_output" => capabilities.structured_output,
                "embeddings" => capabilities.embeddings,
                "reranking" => capabilities.reranking,
                "vision" => capabilities.vision,
                "audio" => capabilities.audio,
                "reasoning" => capabilities.reasoning,
                "speculative_decoding" => capabilities.speculative_decoding,
                "grammar_constrained_output" => capabilities.grammar_constrained_output,
                _ => false,
            };
            if !available {
                result.warnings.push(format!(
                    "Step '{}' requires capability '{}' which is not available",
                    step.id, cap
                ));
                result.capabilities_satisfied = false;
            }
        }
    }

    fn has_circular_dependencies(&self, steps: &[PlanStep]) -> bool {
        // Build adjacency list and check for cycles using DFS
        let adj: HashMap<&str, Vec<&str>> = steps
            .iter()
            .map(|s| (s.id.as_str(), s.dependencies.iter().map(|d| d.as_str()).collect()))
            .collect();

        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();

        for step in steps {
            if !visited.contains(step.id.as_str()) {
                if self.dfs_cycle_check(step.id.as_str(), &adj, &mut visited, &mut in_stack) {
                    return true;
                }
            }
        }
        false
    }

    fn dfs_cycle_check<'a>(
        &self,
        node: &'a str,
        adj: &HashMap<&str, Vec<&'a str>>,
        visited: &mut HashSet<&'a str>,
        in_stack: &mut HashSet<&'a str>,
    ) -> bool {
        visited.insert(node);
        in_stack.insert(node);

        if let Some(deps) = adj.get(node) {
            for &dep in deps {
                if in_stack.contains(dep) {
                    return true;
                }
                if !visited.contains(dep) {
                    if self.dfs_cycle_check(dep, adj, visited, in_stack) {
                        return true;
                    }
                }
            }
        }
        in_stack.remove(node);
        false
    }

    fn has_orphan_steps(&self, steps: &[PlanStep]) -> bool {
        // A step is orphaned if it has no path from a root (no-dependency) step
        let has_deps: HashSet<&str> =
            steps.iter().flat_map(|s| s.dependencies.iter().map(|d| d.as_str())).collect();

        // Root steps = no dependencies
        let roots: HashSet<&str> =
            steps.iter().filter(|s| s.dependencies.is_empty()).map(|s| s.id.as_str()).collect();

        if roots.is_empty() {
            return true; // all steps have deps = no start point
        }

        // Build reverse adjacency (who depends on whom)
        let mut reachable = HashSet::new();
        let mut stack: Vec<&str> = roots.iter().copied().collect();
        let adj: HashMap<&str, Vec<&str>> = steps
            .iter()
            .map(|s| (s.id.as_str(), s.dependencies.iter().map(|d| d.as_str()).collect()))
            .collect();
        let mut rev: HashMap<&str, Vec<&str>> = HashMap::new();
        for s in steps {
            for dep in &s.dependencies {
                rev.entry(dep.as_str()).or_default().push(s.id.as_str());
            }
        }

        while let Some(node) = stack.pop() {
            if reachable.insert(node) {
                if let Some(children) = rev.get(node) {
                    for &child in children {
                        if !reachable.contains(child) {
                            stack.push(child);
                        }
                    }
                }
            }
        }

        let all_ids: HashSet<&str> = steps.iter().map(|s| s.id.as_str()).collect();
        reachable != all_ids
    }
}

impl Default for PlanValidator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn test_caps() -> ModelCapabilities {
        ModelCapabilities {
            text_generation: true,
            chat: true,
            tool_calling: true,
            ..Default::default()
        }
    }

    #[test]
    fn valid_plan_passes() {
        let mut plan = Plan::new("p1", "test");
        plan.add_step(PlanStep::new("s1", "first"));
        plan.add_step(PlanStep::new("s2", "second").depends_on("s1"));

        let result = PlanValidator::new().validate(&plan, &test_caps());
        assert!(result.is_ok());
        assert!(result.valid);
    }

    #[test]
    fn missing_dependency_detected() {
        let mut plan = Plan::new("p2", "test");
        plan.add_step(PlanStep::new("s1", "first").depends_on("missing"));

        let result = PlanValidator::new().validate(&plan, &test_caps());
        assert!(!result.is_ok());
    }

    #[test]
    fn circular_dependency_detected() {
        let mut plan = Plan::new("p3", "test");
        plan.add_step(PlanStep::new("s1", "first").depends_on("s2"));
        plan.add_step(PlanStep::new("s2", "second").depends_on("s1"));

        let result = PlanValidator::new().validate(&plan, &test_caps());
        assert!(!result.valid);
    }

    #[test]
    fn empty_plan_fails() {
        let plan = Plan::new("p4", "empty");
        let result = PlanValidator::new().validate(&plan, &test_caps());
        assert!(!result.valid);
    }

    #[test]
    fn missing_capability_warns() {
        let mut plan = Plan::new("p5", "need embedding");
        let mut step = PlanStep::new("s1", "embed");
        step.required_capabilities = vec!["embeddings".into()];
        plan.add_step(step);

        let caps = ModelCapabilities { text_generation: true, ..Default::default() };
        let result = PlanValidator::new().validate(&plan, &caps);
        assert!(result.is_ok()); // warnings only, not errors
        assert!(!result.capabilities_satisfied);
    }

    #[test]
    fn no_circular_in_linear_plan() {
        let mut plan = Plan::new("p6", "linear");
        plan.add_step(PlanStep::new("s1", "a"));
        plan.add_step(PlanStep::new("s2", "b").depends_on("s1"));
        plan.add_step(PlanStep::new("s3", "c").depends_on("s2"));

        let result = PlanValidator::new().validate(&plan, &test_caps());
        assert!(result.valid);
    }

    #[test]
    fn all_deps_connected() {
        let mut plan = Plan::new("p7", "connected");
        // All steps are roots (no deps) — no orphans
        plan.add_step(PlanStep::new("s1", "a"));
        plan.add_step(PlanStep::new("s2", "b"));

        let result = PlanValidator::new().validate(&plan, &test_caps());
        assert!(result.valid);
    }
}
