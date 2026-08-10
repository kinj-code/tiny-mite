//! Intent classification — deterministic keyword-driven heuristics.
//!
//! The [`IntentClassifier`] determines task intent, type, and
//! requirements using fast deterministic rules. No LLM call is
//! required — the classifier is a fallback that runs before
//! involving the model at all.

use serde::{Deserialize, Serialize};
use tiny_mite_runtime::ModelCapabilities;

use super::analysis::TaskAnalysis;
use super::complexity::{ComplexityScore, TaskComplexityEstimator};

// ── Intent ────────────────────────────────────────────────────────

/// Primary user intent categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Intent {
    /// Asking a factual question.
    Question,
    /// Requesting code generation or modification.
    CodeGeneration,
    /// Requesting code review or analysis.
    CodeReview,
    /// Requesting explanation of a concept or code.
    Explanation,
    /// Requesting summarization of content.
    Summarization,
    /// Requesting data analysis or transformation.
    Analysis,
    /// Requesting creative content generation.
    Creation,
    /// Requesting an action/command execution.
    Action,
    /// Requesting planning or task decomposition.
    Planning,
    /// Debugging a problem or error.
    Debugging,
    /// Unknown or ambiguous intent.
    Unknown,
}

// ── Task type ─────────────────────────────────────────────────────

/// Specific task type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskType {
    /// Simple factual lookup.
    FactualQuery,
    /// Writing or modifying code.
    Implementation,
    /// Designing architecture or planning.
    Design,
    /// Finding and fixing a bug.
    BugFix,
    /// Adding tests.
    TestWriting,
    /// Explaining existing code.
    CodeExplanation,
    /// Reviewing code for issues.
    CodeReview,
    /// Summarizing a document or conversation.
    Summarization,
    /// Analyzing data or logs.
    Analysis,
    /// General conversation or chitchat.
    Conversation,
    /// System operation or tool invocation.
    SystemAction,
    /// Unknown task type.
    Unknown,
}

// ── Intent Classifier ─────────────────────────────────────────────

/// Deterministic intent classifier.
///
/// Uses keyword matching and heuristic rules. When confidence is low,
/// the analysis flags `escalate = true` to signal that an LLM should
/// be consulted for better classification.
pub struct IntentClassifier {
    complexity_estimator: TaskComplexityEstimator,
}

impl IntentClassifier {
    /// Create a new classifier with the default complexity estimator.
    #[must_use]
    pub fn new() -> Self {
        Self { complexity_estimator: TaskComplexityEstimator::new() }
    }

    /// Analyze an input text and produce a structured task analysis.
    ///
    /// This is 100% deterministic — no LLM call is made.
    #[must_use]
    pub fn analyze(&self, input: &str) -> TaskAnalysis {
        let lower = input.to_lowercase();

        // ── Intent detection ───────────────────────────────────
        let intent = self.detect_intent(&lower);
        let task_type = self.detect_task_type(&lower, intent);

        // ── Feature detection ──────────────────────────────────
        let requires_retrieval = self.needs_retrieval(&lower);
        let requires_tools = self.detect_required_tools(&lower);
        let requires_planning = self.needs_planning(intent);
        let requires_reasoning = self.needs_reasoning(intent);
        let requires_memory = lower.contains("remember")
            || lower.contains("previous")
            || lower.contains("earlier")
            || lower.contains("project");
        let requires_streaming =
            lower.contains("long") || lower.contains("generate") || lower.contains("write");

        let risk_score = self.estimate_risk(intent, &requires_tools);

        // ── Token estimation ───────────────────────────────────
        let estimated_prompt = input.len().max(64);
        let estimated_total = estimated_prompt
            * if requires_planning {
                4
            } else if requires_reasoning {
                3
            } else {
                2
            };

        // ── Complexity ─────────────────────────────────────────
        let complexity = self.complexity_estimator.estimate(
            intent,
            requires_planning,
            requires_reasoning,
            requires_tools.len() as u32,
            estimated_total,
        );

        // ── Confidence ─────────────────────────────────────────
        let confidence = if intent == Intent::Unknown { 0.3 } else { 0.85 };

        // ── Required capabilities ──────────────────────────────
        let mut capabilities = vec!["text_generation".to_owned()];
        if requires_planning || requires_reasoning {
            capabilities.push("chat".to_owned());
        }
        if !requires_tools.is_empty() {
            capabilities.push("tool_calling".to_owned());
        }

        TaskAnalysis {
            intent,
            task_type,
            complexity,
            requires_retrieval,
            requires_tools,
            requires_memory,
            requires_streaming,
            requires_planning,
            requires_reasoning,
            estimated_total_tokens: estimated_total,
            estimated_prompt_tokens: estimated_prompt,
            confidence,
            required_capabilities: capabilities,
            reasoning_hint: self.build_hint(intent, task_type, confidence),
            risk_score,
            escalate: confidence < 0.6,
        }
    }

    // ── Private helpers ────────────────────────────────────────

    fn detect_intent(&self, text: &str) -> Intent {
        let patterns = [
            (
                Intent::CodeGeneration,
                &[
                    "write code",
                    "implement",
                    "create a function",
                    "build a",
                    "write a program",
                    "code a",
                    "add a function",
                    "refactor",
                ][..],
            ),
            (Intent::CodeReview, &["review", "code review", "audit", "check my code", "inspect"]),
            (
                Intent::Debugging,
                &["debug", "bug", "fix", "error", "broken", "not working", "crash"],
            ),
            (
                Intent::Explanation,
                &["explain", "what does", "how does", "why", "describe", "tell me about"],
            ),
            (Intent::Summarization, &["summarize", "summary", "tldr", "brief", "condense"]),
            (Intent::Analysis, &["analyze", "analysis", "metrics", "profile", "benchmark", "data"]),
            (
                Intent::Creation,
                &["write a poem", "write a story", "create a", "generate", "compose"],
            ),
            (
                Intent::Action,
                &["run", "execute", "deploy", "install", "setup", "configure", "build", "compile"],
            ),
            (Intent::Planning, &["plan", "architecture", "design", "outline", "steps", "strategy"]),
            (
                Intent::Question,
                &["what", "when", "where", "who", "how", "which", "can you", "could you"],
            ),
        ];

        for (intent, keywords) in &patterns {
            if keywords.iter().any(|k| text.contains(k)) {
                return *intent;
            }
        }
        Intent::Unknown
    }

    fn detect_task_type(&self, text: &str, intent: Intent) -> TaskType {
        match intent {
            Intent::CodeGeneration => TaskType::Implementation,
            Intent::CodeReview => TaskType::CodeReview,
            Intent::Debugging => TaskType::BugFix,
            Intent::Explanation => TaskType::CodeExplanation,
            Intent::Summarization => TaskType::Summarization,
            Intent::Analysis => TaskType::Analysis,
            Intent::Creation => TaskType::Implementation,
            Intent::Action => TaskType::SystemAction,
            Intent::Planning => TaskType::Design,
            Intent::Question => {
                if text.contains("how") && (text.contains("code") || text.contains("write")) {
                    TaskType::Implementation
                } else {
                    TaskType::FactualQuery
                }
            }
            Intent::Unknown => TaskType::Unknown,
        }
    }

    fn needs_retrieval(&self, text: &str) -> bool {
        text.contains("find")
            || text.contains("search")
            || text.contains("look up")
            || text.contains("document")
            || text.contains("reference")
            || text.contains("project")
            || text.contains("codebase")
    }

    fn detect_required_tools(&self, text: &str) -> Vec<String> {
        let mut tools = Vec::new();
        if text.contains("compile") || text.contains("build") || text.contains("run") {
            tools.push("terminal".to_owned());
        }
        if text.contains("test") || text.contains("unit test") {
            tools.push("test-runner".to_owned());
        }
        if text.contains("file") || text.contains("write to") || text.contains("read") {
            tools.push("filesystem".to_owned());
        }
        if text.contains("git") || text.contains("commit") || text.contains("branch") {
            tools.push("git".to_owned());
        }
        tools
    }

    fn needs_planning(&self, intent: Intent) -> bool {
        matches!(intent, Intent::Planning | Intent::CodeGeneration | Intent::Action)
            || matches!(intent, Intent::Debugging)
    }

    fn needs_reasoning(&self, intent: Intent) -> bool {
        matches!(
            intent,
            Intent::Debugging | Intent::CodeReview | Intent::Analysis | Intent::Explanation
        )
    }

    fn estimate_risk(&self, intent: Intent, tools: &[String]) -> u32 {
        let base = match intent {
            Intent::Action => 60,
            Intent::CodeGeneration => 30,
            Intent::Debugging => 20,
            _ => 5,
        };
        let tool_bonus = tools.len() as u32 * 10;
        (base + tool_bonus).min(100)
    }

    fn build_hint(&self, intent: Intent, task_type: TaskType, confidence: f32) -> String {
        format!(
            "Intent classified as {:?}/{:?} with {:.0}% confidence via keyword heuristics.",
            intent,
            task_type,
            confidence * 100.0
        )
    }
}

impl Default for IntentClassifier {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_code_generation() {
        let c = IntentClassifier::new();
        let analysis = c.analyze("write code to implement a binary search tree");
        assert_eq!(analysis.intent, Intent::CodeGeneration);
        assert!(analysis.requires_planning);
    }

    #[test]
    fn detect_debugging() {
        let c = IntentClassifier::new();
        let analysis = c.analyze("my code is broken, help me debug the null pointer error");
        assert_eq!(analysis.intent, Intent::Debugging);
        assert!(analysis.requires_reasoning);
    }

    #[test]
    fn detect_explanation() {
        let c = IntentClassifier::new();
        let analysis = c.analyze("explain how Rust ownership works");
        assert_eq!(analysis.intent, Intent::Explanation);
    }

    #[test]
    fn detect_summarization() {
        let c = IntentClassifier::new();
        let analysis = c.analyze("summarize this document for me please");
        assert_eq!(analysis.intent, Intent::Summarization);
    }

    #[test]
    fn detect_planning() {
        let c = IntentClassifier::new();
        let analysis = c.analyze("plan the architecture for a microservice application");
        assert_eq!(analysis.intent, Intent::Planning);
        assert!(analysis.requires_planning);
    }

    #[test]
    fn unknown_input_is_low_confidence() {
        let c = IntentClassifier::new();
        let analysis = c.analyze("xyzzy plugh");
        assert_eq!(analysis.intent, Intent::Unknown);
        assert!(analysis.confidence < 0.5);
        assert!(analysis.escalate);
    }

    #[test]
    fn tool_detection_for_build() {
        let c = IntentClassifier::new();
        let analysis = c.analyze("build and compile the project and run tests");
        assert!(analysis.requires_tools.contains(&"terminal".to_owned()));
        assert!(analysis.requires_tools.contains(&"test-runner".to_owned()));
    }

    #[test]
    fn risk_is_higher_for_actions() {
        let c = IntentClassifier::new();
        let code = c.analyze("write a hello world function");
        let action = c.analyze("deploy to production and run the migration");
        assert!(action.risk_score > code.risk_score);
    }

    #[test]
    fn streaming_detection_for_generation() {
        let c = IntentClassifier::new();
        let analysis = c.analyze("write a long detailed article about ancient Rome");
        assert!(analysis.requires_streaming);
    }
}
