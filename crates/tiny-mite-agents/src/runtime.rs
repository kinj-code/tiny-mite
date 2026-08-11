//! Agent runtime — intelligence loop coordinator.
//!
//! The [`AgentRuntime`] orchestrates the full intelligence pipeline:
//! classify → estimate complexity → plan → validate → context bridge →
//! model router → model inference → tool-call parsing → tool executor →
//! verify → reflect → repair.
//!
//! In Phase 10.2, `process_async()` supports multi-turn autonomous execution
//! with tool-result feedback, failure detection, and bounded iteration.
//!
//! It is the primary entry point for task processing in Tiny Mite.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tiny_mite_domain::ModelId;
use tiny_mite_runtime::{
    ContextBudget, InferenceRequest, ModelCapabilities, ModelRouter,
};
use tiny_mite_scheduler::cancellation::CancellationToken;

use crate::analysis::TaskAnalysis;
use crate::context_bridge::ContextBridge;
use crate::intent::IntentClassifier;
use crate::memory::WorkingMemory;
use crate::planner::{Plan, Planner};
use crate::reflection::{Reflection, ReflectionResult};
use crate::repair::RepairLoop;
use crate::tool_executor::{ToolExecutionOutcome, ToolExecutor};
use crate::tool_parser::{parse_tool_calls, ParsedToolCall};
use crate::validator::{PlanValidator, ValidationResult};
use crate::verifier::{VerificationEngine, VerificationOutcome};

// ── Agent loop configuration ──────────────────────────────────────

/// Configures the bounds for the autonomous multi-turn agent loop.
#[derive(Debug, Clone)]
pub struct AgentLoopConfig {
    /// Maximum number of loop iterations (default: 8).
    pub max_iterations: usize,
    /// Maximum total model calls (default: 8).
    pub max_model_calls: usize,
    /// Maximum total tool calls (default: 32).
    pub max_tool_calls: usize,
    /// Maximum cumulative failures before giving up (default: 5).
    pub max_failures: usize,
    /// Maximum wall-clock execution time (default: 300s).
    pub max_execution_time: Duration,
    /// Maximum identical tool+args+error failures before loop detection (default: 2).
    pub max_identical_failures: usize,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 8,
            max_model_calls: 8,
            max_tool_calls: 32,
            max_failures: 5,
            max_execution_time: Duration::from_secs(300),
            max_identical_failures: 2,
        }
    }
}

// ── Conversation message ──────────────────────────────────────────

/// A single message in the agent conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationMessage {
    /// User request.
    User(String),
    /// Model/assistant response.
    Assistant(String),
    /// Tool invocation result.
    ToolResult {
        /// Tool name.
        name: String,
        /// Tool arguments.
        args: Vec<String>,
        /// Tool output (stdout or error).
        output: String,
        /// Exit code if applicable.
        exit_code: Option<i32>,
        /// Whether the tool call succeeded.
        success: bool,
    },
    /// Structured failure summary for the model.
    FailureSummary(String),
}

// ── Agent conversation state ──────────────────────────────────────

/// Persisted conversation state for multi-turn execution.
#[derive(Debug, Clone)]
pub struct AgentConversation {
    /// All messages in chronological order.
    pub messages: Vec<ConversationMessage>,
    /// Current iteration number.
    pub iteration: usize,
    /// Total model calls made.
    pub model_calls: usize,
    /// Total tool calls made.
    pub tool_calls: usize,
    /// Cumulative verification failures.
    pub failures: usize,
    /// Last tool name (for loop detection).
    pub last_tool_name: Option<String>,
    /// Last tool args (for loop detection).
    pub last_tool_args: Vec<String>,
    /// Last tool error (for loop detection).
    pub last_error: Option<String>,
    /// Consecutive identical failures.
    pub identical_failure_count: usize,
    /// Whether the loop was terminated by loop detection.
    pub stuck: bool,
    /// Whether execution was cancelled.
    pub cancelled: bool,
}

impl AgentConversation {
    /// Create a new conversation with the user's request.
    #[must_use]
    pub fn new(user_request: &str) -> Self {
        Self {
            messages: vec![ConversationMessage::User(user_request.to_string())],
            iteration: 0,
            model_calls: 0,
            tool_calls: 0,
            failures: 0,
            last_tool_name: None,
            last_tool_args: Vec::new(),
            last_error: None,
            identical_failure_count: 0,
            stuck: false,
            cancelled: false,
        }
    }

    /// Add a model response.
    pub fn add_assistant(&mut self, text: &str) {
        self.messages.push(ConversationMessage::Assistant(text.to_string()));
    }

    /// Add a tool result.
    pub fn add_tool_result(
        &mut self,
        name: &str,
        args: &[String],
        output: &str,
        exit_code: Option<i32>,
        success: bool,
    ) {
        self.tool_calls += 1;
        self.messages.push(ConversationMessage::ToolResult {
            name: name.to_string(),
            args: args.to_vec(),
            output: output.to_string(),
            exit_code,
            success,
        });

        // Track for loop detection
        if !success {
            self.failures += 1;
            let error_key = output.to_string();
            if self.last_tool_name.as_deref() == Some(name)
                && self.last_tool_args == args
                && self.last_error.as_deref() == Some(&error_key)
            {
                self.identical_failure_count += 1;
            } else {
                self.identical_failure_count = 1;
            }
            self.last_tool_name = Some(name.to_string());
            self.last_tool_args = args.to_vec();
            self.last_error = Some(error_key);
        }
    }

    /// Add a failure summary for the next model call.
    pub fn add_failure_summary(&mut self, summary: &str) {
        self.messages.push(ConversationMessage::FailureSummary(summary.to_string()));
    }
}

// ── Task result ───────────────────────────────────────────────────

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
    /// Model response text (for observability).
    pub model_response: Option<String>,
    /// Tool execution outcomes.
    pub tool_results: Vec<ToolExecutionOutcome>,
    /// Total elapsed time in milliseconds.
    pub elapsed_ms: f64,
    /// Number of model invocations.
    pub model_calls: u32,
    /// Number of iterations.
    pub iterations: usize,
    /// Total tool calls.
    pub tool_call_count: usize,
    /// Whether loop detection terminated execution.
    pub stuck: bool,
    /// Whether execution was cancelled.
    pub cancelled: bool,
    /// The full conversation for trace/debug.
    pub conversation: AgentConversation,
}

// ── Agent runtime ─────────────────────────────────────────────────

/// The agent runtime — coordinates the intelligence loop.
///
/// ```text
/// Input → classify → estimate → plan → validate → context bridge →
/// model router → model inference → tool-call parsing → tool executor →
/// verify → reflect → repair → (replan if needed) → Output
/// ```
pub struct AgentRuntime {
    classifier: IntentClassifier,
    planner: Planner,
    validator: PlanValidator,
    verifier: VerificationEngine,
    reflector: Reflection,
    capabilities: ModelCapabilities,
    /// Optional router for real model inference.
    router: Option<Arc<ModelRouter>>,
    /// Optional tool executor for real tool execution.
    tool_executor: Option<Arc<Mutex<ToolExecutor>>>,
    /// Model name for inference requests.
    model_name: String,
    /// Optional cancellation token.
    cancel_token: Option<CancellationToken>,
    /// Loop configuration.
    loop_config: AgentLoopConfig,
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
            router: None,
            tool_executor: None,
            model_name: String::new(),
            cancel_token: None,
            loop_config: AgentLoopConfig::default(),
        }
    }

    // ── Builders ──────────────────────────────────────────────

    /// Set the model router for real inference.
    #[must_use]
    pub fn with_router(mut self, router: Arc<ModelRouter>) -> Self {
        self.router = Some(router);
        self
    }

    /// Set the tool executor for real tool execution.
    #[must_use]
    pub fn with_tool_executor(mut self, executor: Arc<Mutex<ToolExecutor>>) -> Self {
        self.tool_executor = Some(executor);
        self
    }

    /// Set the model name for inference requests.
    #[must_use]
    pub fn with_model_name(mut self, name: impl Into<String>) -> Self {
        self.model_name = name.into();
        self
    }

    /// Set a cancellation token.
    #[must_use]
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = Some(token);
        self
    }

    /// Set the loop configuration.
    #[must_use]
    pub fn with_loop_config(mut self, config: AgentLoopConfig) -> Self {
        self.loop_config = config;
        self
    }

    /// Get the model capabilities this runtime is configured with.
    #[must_use]
    pub fn capabilities(&self) -> &ModelCapabilities {
        &self.capabilities
    }

    /// Check if cancellation is requested.
    fn is_cancelled(&self) -> bool {
        self.cancel_token.as_ref().map_or(false, |t| t.is_cancelled())
    }

    // ── Synchronous processing (backward compatible) ──────────

    /// Process a user request through the intelligence pipeline (sync, no model).
    ///
    /// This is the deterministic path that does NOT call a model provider.
    /// For real model inference, use [`process_async`].
    #[must_use]
    pub fn process(&self, input: &str) -> TaskResult {
        let start = std::time::Instant::now();

        let analysis = self.classifier.analyze(input);
        let plan = self.planner.plan(&analysis, input);
        let validation = self.validator.validate(&plan, &self.capabilities);

        let mut memory = WorkingMemory::new();
        memory.load_plan(&plan);

        let mut verification_results = Vec::new();
        for step in &plan.steps {
            let outcome = self.verifier.verify(step, "PASS", Some(0));
            verification_results.push(outcome);
        }

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

        let conversation = AgentConversation::new(input);

        TaskResult {
            success,
            analysis,
            plan,
            verification_results,
            reflection,
            memory,
            summary,
            model_response: None,
            tool_results: Vec::new(),
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
            model_calls: 0,
            iterations: 0,
            tool_call_count: 0,
            stuck: false,
            cancelled: false,
            conversation,
        }
    }

    // ── Async processing (multi-turn autonomous loop) ─────────

    /// Process a user request through the full intelligence pipeline
    /// with real model inference, tool execution, and multi-turn loop.
    ///
    /// Requires that `with_router()` and `with_tool_executor()` have been called.
    pub async fn process_async(&self, input: &str) -> TaskResult {
        let overall_start = std::time::Instant::now();
        let mut conversation = AgentConversation::new(input);
        let mut tool_results = Vec::new();
        let mut all_model_responses = Vec::new();

        // ── Phase 1-3: classify, plan, validate ──────────────
        let analysis = self.classifier.analyze(input);
        let mut plan = self.planner.plan(&analysis, input);
        let validation = self.validator.validate(&plan, &self.capabilities);

        let mut memory = WorkingMemory::new();
        memory.load_plan(&plan);

        // ── Multi-turn loop ──────────────────────────────────
        loop {
            // Check bounds
            if self.is_cancelled() {
                conversation.cancelled = true;
                return self.build_result(
                    input, &analysis, &plan, &Vec::new(), &ReflectionResult::default(),
                    &memory, &conversation, &all_model_responses, &tool_results,
                    overall_start.elapsed(),
                );
            }

            if conversation.iteration >= self.loop_config.max_iterations {
                return self.build_result(
                    input, &analysis, &plan, &Vec::new(), &ReflectionResult::default(),
                    &memory, &conversation, &all_model_responses, &tool_results,
                    overall_start.elapsed(),
                );
            }

            if conversation.model_calls >= self.loop_config.max_model_calls {
                return self.build_result(
                    input, &analysis, &plan, &Vec::new(), &ReflectionResult::default(),
                    &memory, &conversation, &all_model_responses, &tool_results,
                    overall_start.elapsed(),
                );
            }

            if overall_start.elapsed() > self.loop_config.max_execution_time {
                return self.build_result(
                    input, &analysis, &plan, &Vec::new(), &ReflectionResult::default(),
                    &memory, &conversation, &all_model_responses, &tool_results,
                    overall_start.elapsed(),
                );
            }

            // Loop detection: stuck on same failure
            if conversation.identical_failure_count > self.loop_config.max_identical_failures {
                conversation.stuck = true;
                return self.build_result(
                    input, &analysis, &plan, &Vec::new(), &ReflectionResult::default(),
                    &memory, &conversation, &all_model_responses, &tool_results,
                    overall_start.elapsed(),
                );
            }

            if conversation.failures >= self.loop_config.max_failures {
                return self.build_result(
                    input, &analysis, &plan, &Vec::new(), &ReflectionResult::default(),
                    &memory, &conversation, &all_model_responses, &tool_results,
                    overall_start.elapsed(),
                );
            }

            conversation.iteration += 1;

            // ── Recompile context for this iteration ───────
            let prompt = self.build_iteration_prompt(
                input, &analysis, &plan, &memory, &conversation,
            );

            // ── Model call ─────────────────────────────────
            if self.is_cancelled() {
                conversation.cancelled = true;
                break;
            }

            let model_response = self
                .call_model(&prompt, &mut conversation.model_calls)
                .await;

            let model_text = match model_response {
                Ok(text) => text,
                Err(e) => {
                    conversation.add_failure_summary(&format!("Model error: {e}"));
                    break;
                }
            };

            conversation.add_assistant(&model_text);
            all_model_responses.push(model_text.clone());

            // ── Parse and execute tool calls ───────────────
            let parsed_calls = parse_tool_calls(&model_text);

            if parsed_calls.is_empty() {
                // Try one repair attempt: tell model the format was wrong
                if conversation.iteration < self.loop_config.max_iterations - 1
                    && conversation.model_calls < self.loop_config.max_model_calls - 1
                {
                    conversation.add_failure_summary(
                        "Your tool call could not be parsed. Use: [{\"name\":\"TOOL\",\"arguments\":{\"arg\":\"val\"}}]"
                    );
                    continue; // Let the loop iterate again with repair feedback
                }
                break;
            }

            for tc in &parsed_calls {
                if conversation.tool_calls >= self.loop_config.max_tool_calls {
                    break;
                }

                if self.is_cancelled() {
                    conversation.cancelled = true;
                    break;
                }

                let step = crate::planner::PlanStep::new(
                    format!("tool_{}", conversation.iteration),
                    format!("Execute {} with args {:?}", tc.name, tc.args),
                )
                .with_tools(vec![tc.name.clone()])
                .with_args(tc.args.clone());

                let outcome = if let Some(ref executor_arc) = self.tool_executor {
                    let mut executor = executor_arc.lock().await;
                    executor.execute_for_step(&step, input).await
                } else {
                    ToolExecutionOutcome::InternalError {
                        error: "No ToolExecutor configured".into(),
                    }
                };

                let tool_success = matches!(&outcome, ToolExecutionOutcome::Success { result, .. } if result.success);
                let tool_output = match &outcome {
                    ToolExecutionOutcome::Success { result, .. } => result.output.clone(),
                    other => format!("{:?}", other),
                };
                let exit_code = match &outcome {
                    ToolExecutionOutcome::Success { result, .. } => result.exit_code,
                    _ => None,
                };

                conversation.add_tool_result(
                    &tc.name, &tc.args, &tool_output, exit_code, tool_success,
                );
                tool_results.push(outcome);
            }

            // ── Verify ────────────────────────────────────
            let mut verification_results = Vec::new();
            for step in &plan.steps {
                let outcome = self.verifier.verify(step, &model_text, None);
                verification_results.push(outcome);
            }

            let all_passed = validation.valid
                && !verification_results.is_empty()
                && verification_results.iter().all(|o| o.passed);

            if all_passed {
                // Task succeeded
                let reflection = self.reflector.reflect_on_plan(
                    &[], &[], plan.steps.len(),
                );
                return self.build_result(
                    input, &analysis, &plan, &verification_results, &reflection,
                    &memory, &conversation, &all_model_responses, &tool_results,
                    overall_start.elapsed(),
                );
            }

            // ── Reflect ───────────────────────────────────
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

            // ── Repair or continue ─────────────────────────
            if !reflection.should_retry {
                return self.build_result(
                    input, &analysis, &plan, &verification_results, &reflection,
                    &memory, &conversation, &all_model_responses, &tool_results,
                    overall_start.elapsed(),
                );
            }

            // Build failure summary for next model call
            let failure_info = format!(
                "## FAILURE SUMMARY\n\
                 Previous action: {}\n\
                 Exit code: {}\n\
                 Failures so far: {}\n\
                 Reflection: {:?}\n\
                 Suggested correction: {:?}\n\
                 Continue making changes and rerunning tests until they pass.",
                model_text.lines().next().unwrap_or("unknown"),
                tool_results.last().map(|o| match o {
                    ToolExecutionOutcome::Success { result, .. } =>
                        format!("{}", result.exit_code.unwrap_or(-1)),
                    _ => "N/A".into(),
                }).unwrap_or("N/A".into()),
                conversation.failures,
                reflection.what_failed,
                reflection.correction,
            );
            conversation.add_failure_summary(&failure_info);

            // If plan is invalid, replan
            if reflection.has_correction && reflection.plan_changes.is_empty() {
                // Replan based on analysis
                let re_analysis = self.classifier.analyze(&format!(
                    "Continue working on: {}. Current state: {}",
                    input, failure_info
                ));
                plan = self.planner.plan(&re_analysis, input);
                memory.load_plan(&plan);
            }
        }

        // End of loop — build final result
        let verification_results = Vec::new();
        let reflection = self.reflector.reflect_on_plan(
            &[], &[], plan.steps.len(),
        );
        self.build_result(
            input, &analysis, &plan, &verification_results, &reflection,
            &memory, &conversation, &all_model_responses, &tool_results,
            overall_start.elapsed(),
        )
    }

    // ── Private helpers ──────────────────────────────────────

    async fn call_model(&self, prompt: &str, call_count: &mut usize) -> Result<String, String> {
        let router = self.router.as_ref().ok_or("No ModelRouter configured")?;

        if self.is_cancelled() {
            return Err("Cancelled".into());
        }

        let request = InferenceRequest {
            model_id: ModelId::new(),
            model_name: self.model_name.clone(),
            prompt: prompt.to_string(),
            system_prompt: Some(
                "You are a coding assistant with access to tools.\n\
                 \n\
                 Tools: write_file(path,content) read_file(path) shell(cmd) run_tests search(query) list_files(path)\n\
                 \n\
                 To call a tool, output EXACTLY:\n\
                 [{\"name\":\"tool_name\",\"arguments\":{\"arg1\":\"val1\",\"arg2\":\"val2\"}}]\n\
                 \n\
                 Example:\n\
                 User: Create /tmp/hello.txt with content hello\n\
                 Assistant: [{\"name\":\"write_file\",\"arguments\":{\"path\":\"/tmp/hello.txt\",\"content\":\"hello\"}}]\n\
                 \n\
                 After a tool executes you'll see its result. Continue until done.\n\
                 When tests fail, analyze errors, fix code, rerun tests."
                    .into(),
            ),
            max_tokens: 2048,
            temperature: 0.7,
            top_p: None,
            top_k: None,
            seed: None,
            stop_sequences: Vec::new(),
            grammar: None,
            tools: Vec::new(),
            correlation_id: None,
            task_id: None,
            timeout_ms: Some(300_000),
            context_budget: ContextBudget::new(8192),
        };

        let response = router
            .generate(&self.capabilities, &request)
            .await
            .map_err(|e| format!("{e}"))?;

        *call_count += 1;
        Ok(response.text)
    }

    fn build_iteration_prompt(
        &self,
        input: &str,
        analysis: &TaskAnalysis,
        plan: &Plan,
        memory: &WorkingMemory,
        conversation: &AgentConversation,
    ) -> String {
        let compiled = ContextBridge::compile(analysis, plan, None, memory, 8192);

        let mut prompt = String::from(
            "You are Tiny Mite, a coding assistant. You must use tool calls to interact with the filesystem and execute commands.\n\n\
             Available tools: read_file, write_file, list_files, shell, compile, run_tests, search, git_status\n\n",
        );

        // Add context items for budget management
        for item in &compiled.items {
            prompt.push_str(&format!(
                "[{}] {}\n",
                format!("{:?}", item.item_type).to_lowercase(),
                item.content
            ));
        }

        // Add conversation history (recent messages only)
        let recent = conversation.messages.iter().rev().take(20).rev();
        for msg in recent {
            match msg {
                ConversationMessage::User(text) => {
                    prompt.push_str(&format!("\nUser: {text}\n"));
                }
                ConversationMessage::Assistant(text) => {
                    prompt.push_str(&format!("\nAssistant: {text}\n"));
                }
                ConversationMessage::ToolResult { name, args, output, exit_code, success } => {
                    prompt.push_str(&format!(
                        "\nTool {name}({:?}) returned: exit_code={:?} success={success}\nOutput:\n{output}\n",
                        args, exit_code
                    ));
                }
                ConversationMessage::FailureSummary(text) => {
                    prompt.push_str(&format!("\n{text}\n"));
                }
            }
        }

        prompt.push_str(&format!("\n\nCurrent task: {input}\n"));
        prompt.push_str(&format!(
            "Plan has {} steps. Continue working. Use tools to make progress.\n",
            plan.steps.len()
        ));

        prompt
    }

    #[allow(clippy::too_many_arguments)]
    fn build_result(
        &self,
        input: &str,
        analysis: &TaskAnalysis,
        plan: &Plan,
        verification_results: &[VerificationOutcome],
        reflection: &ReflectionResult,
        memory: &WorkingMemory,
        conversation: &AgentConversation,
        model_responses: &[String],
        tool_results: &[ToolExecutionOutcome],
        elapsed: Duration,
    ) -> TaskResult {
        // Empty verification_results means nothing was verified — clear failure
        let all_passed = !verification_results.is_empty()
            && verification_results.iter().all(|o| o.passed);
        let had_model_calls = conversation.model_calls > 0;
        // Success requires: model called, tools executed, verification passed, no failures
        let success = had_model_calls
            && conversation.tool_calls > 0
            && all_passed
            && conversation.failures == 0
            && !conversation.stuck
            && !conversation.cancelled;

        let summary = format!(
            "Task: {}\nIntent: {:?}\nPlan: {} steps\nIterations: {}\nModel calls: {}\nTool calls: {}\nFailures: {}\nStuck: {}\nCancelled: {}\nVerification: {}",
            input,
            analysis.intent,
            plan.steps.len(),
            conversation.iteration,
            conversation.model_calls,
            conversation.tool_calls,
            conversation.failures,
            conversation.stuck,
            conversation.cancelled,
            if all_passed { "PASS" } else { "FAIL" },
        );

        TaskResult {
            success,
            analysis: analysis.clone(),
            plan: plan.clone(),
            verification_results: verification_results.to_vec(),
            reflection: reflection.clone(),
            memory: memory.clone(),
            summary,
            model_response: model_responses.last().cloned(),
            tool_results: tool_results.to_vec(),
            elapsed_ms: elapsed.as_secs_f64() * 1000.0,
            model_calls: conversation.model_calls as u32,
            iterations: conversation.iteration,
            tool_call_count: conversation.tool_calls,
            stuck: conversation.stuck,
            cancelled: conversation.cancelled,
            conversation: conversation.clone(),
        }
    }
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new(ModelCapabilities { text_generation: true, chat: true, ..Default::default() })
    }
}

impl Default for ReflectionResult {
    fn default() -> Self {
        Self {
            has_correction: false,
            what_worked: Vec::new(),
            what_failed: Vec::new(),
            likely_cause: None,
            correction: None,
            should_retry: false,
            plan_changes: Vec::new(),
            confidence: 0.0,
            escalate: false,
        }
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

    #[test]
    fn builder_methods_work() {
        let caps = ModelCapabilities { text_generation: true, ..Default::default() };
        let runtime = AgentRuntime::new(caps).with_model_name("test-model");
        assert_eq!(runtime.model_name, "test-model");
    }

    #[test]
    fn conversation_state_tracks_messages() {
        let mut conv = AgentConversation::new("test task");
        conv.add_assistant("I will help");
        conv.add_tool_result("write_file", &["/tmp/test.txt".into(), "content".into()], "Wrote 7 bytes", None, true);

        assert_eq!(conv.messages.len(), 3);
        assert_eq!(conv.model_calls, 0); // model_calls tracked separately
        assert_eq!(conv.tool_calls, 1);
        assert_eq!(conv.failures, 0);
    }

    #[test]
    fn conversation_detects_identical_failures() {
        let mut conv = AgentConversation::new("test");
        conv.add_tool_result("shell", &["cargo".into(), "test".into()], "error[E0308]", Some(101), false);
        conv.add_tool_result("shell", &["cargo".into(), "test".into()], "error[E0308]", Some(101), false);
        conv.add_tool_result("shell", &["cargo".into(), "test".into()], "error[E0308]", Some(101), false);

        assert_eq!(conv.failures, 3);
        assert_eq!(conv.identical_failure_count, 3);
    }

    #[test]
    fn loop_config_defaults_are_safe() {
        let config = AgentLoopConfig::default();
        assert_eq!(config.max_iterations, 8);
        assert_eq!(config.max_model_calls, 8);
        assert_eq!(config.max_tool_calls, 32);
        assert_eq!(config.max_failures, 5);
        assert_eq!(config.max_identical_failures, 2);
    }

    #[test]
    fn builder_accepts_loop_config() {
        let caps = ModelCapabilities { text_generation: true, ..Default::default() };
        let config = AgentLoopConfig { max_iterations: 3, ..Default::default() };
        let runtime = AgentRuntime::new(caps).with_loop_config(config);
        assert_eq!(runtime.loop_config.max_iterations, 3);
    }
}