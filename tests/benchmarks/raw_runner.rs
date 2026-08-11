//! Phase 11.1 — RAW Model Benchmark Runner
//!
//! Executes benchmark tasks directly against the model WITHOUT Tiny Mite orchestration.
//! Uses no AgentRuntime, Planner, ContextBridge, Reflection, or RepairLoop.
//! Only uses LmStudioProvider + tool_parser + ToolExecutor (sandboxed).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use tiny_mite_domain::ModelId;
use tiny_mite_runtime::{
    ContextBudget, InferenceRequest, LmStudioProvider, ModelCapabilities,
};
use tiny_mite_agents::{
    ToolExecutor, ToolExecutionOutcome, parse_tool_calls,
};
use tiny_mite_tools::{Sandbox, SandboxConfig};
use serde::{Deserialize, Serialize};

use crate::benchmarks::tasks::Task;

/// A raw model trial result (same format as TinyMite TrialResult).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTrialResult {
    pub task_id: String,
    pub trial_number: usize,
    pub success: bool,
    pub model_calls: u32,
    pub tool_calls: usize,
    pub failures: usize,
    pub tool_success_rate: f64,
    pub latency_ms: f64,
    pub model_name: String,
    pub provider: String,
    pub mode: String, // "raw" or "tinymite"
}

/// Configuration for raw model benchmarking.
pub struct RawBenchmarkConfig {
    pub model_name: String,
    pub provider_url: String,
    pub trials_per_task: usize,
    pub work_dir: PathBuf,
    pub temperature: f32,
    pub max_tokens: usize,
    pub max_iterations: usize,
}

impl Default for RawBenchmarkConfig {
    fn default() -> Self {
        Self {
            model_name: "qwopus3.5-4b-coder-mtp".into(),
            provider_url: "http://localhost:1234".into(),
            trials_per_task: 3,
            work_dir: PathBuf::from("/tmp"),
            temperature: 0.7,
            max_tokens: 2048,
            max_iterations: 8,
        }
    }
}

/// Executes RAW model benchmarks (no Tiny Mite orchestration).
pub struct RawBenchmarkRunner {
    config: RawBenchmarkConfig,
}

impl RawBenchmarkRunner {
    pub fn new(config: RawBenchmarkConfig) -> Self {
        Self { config }
    }

    /// Run all tasks for the configured number of trials.
    pub async fn run_all(&self, tasks: &[Task]) -> Vec<RawTrialResult> {
        let mut all_results = Vec::new();
        for task in tasks {
            println!("RAW Benchmark: {} ({:?})", task.id, task.difficulty);
            for trial in 1..=self.config.trials_per_task {
                println!("  Trial {}/{}...", trial, self.config.trials_per_task);
                let result = self.run_trial(task, trial).await;
                all_results.push(result);
            }
        }
        all_results
    }

    async fn run_trial(&self, task: &Task, trial_number: usize) -> RawTrialResult {
        let start = Instant::now();

        // ── Setup sandbox ──────────────────────────────
        let sandbox = Sandbox::new(SandboxConfig {
            allowed_paths: vec![
                self.config.work_dir.clone(),
                PathBuf::from("/tmp"),
            ],
            allow_shell: true,
            allow_network: false,
            max_runtime_ms: task.timeout.as_millis() as u64,
        });

        let mut tool_executor = ToolExecutor::new(sandbox);
        tool_executor.register_standard_tools();
        let tool_executor = Arc::new(Mutex::new(tool_executor));

        // ── Setup provider ────────────────────────────
        let provider = LmStudioProvider::new(&self.config.provider_url);

        // ── RAW conversation loop (no AgentRuntime) ──
        let mut model_calls: u32 = 0;
        let mut tool_calls: usize = 0;
        let mut failures: usize = 0;
        let mut conversation = String::new();

        // System prompt (same as Tiny Mite's)
        let system_prompt = format!(
            "You are a coding assistant with access to tools.\n\
             Tools: write_file(path,content) read_file(path) shell(cmd) run_tests search(query) list_files(path)\n\
             To call a tool, output EXACTLY:\n\
             [{{\"name\":\"tool_name\",\"arguments\":{{\"arg1\":\"val1\",\"arg2\":\"val2\"}}}}]\n\n\
             User task: {}",
            task.prompt
        );

        let mut current_prompt = system_prompt.clone();

        for _iteration in 0..self.config.max_iterations {
            let request = InferenceRequest {
                model_id: ModelId::new(),
                model_name: self.config.model_name.clone(),
                prompt: current_prompt.clone(),
                system_prompt: Some(
                    "Output tool calls as JSON. Do not explain, just output the tool call.".into(),
                ),
                max_tokens: self.config.max_tokens,
                temperature: self.config.temperature,
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

            let response = match provider.generate(&request).await {
                Ok(r) => r,
                Err(_) => break,
            };
            model_calls += 1;

            let parsed = parse_tool_calls(&response.text);
            if parsed.is_empty() {
                // Try repair feedback
                if _iteration < self.config.max_iterations - 1 {
                    current_prompt = format!(
                        "{}\n\nYour tool call could not be parsed. Use: [{{\"name\":\"TOOL\",\"arguments\":{{\"arg\":\"val\"}}}}]\nRetry: {}",
                        conversation, task.prompt
                    );
                    continue;
                }
                break;
            }

            for tc in &parsed {
                let step = tiny_mite_agents::PlanStep::new(
                    format!("raw_tool_{}", tool_calls),
                    format!("Execute {} with {:?}", tc.name, tc.args),
                )
                .with_tools(vec![tc.name.clone()])
                .with_args(tc.args.clone());

                let mut executor = tool_executor.lock().await;
                let outcome = executor.execute_for_step(&step, task.prompt).await;

                let tool_success = matches!(&outcome, ToolExecutionOutcome::Success { result, .. } if result.success);
                if !tool_success { failures += 1; }
                tool_calls += 1;

                let output = match &outcome {
                    ToolExecutionOutcome::Success { result, .. } => result.output.clone(),
                    other => format!("{:?}", other),
                };

                conversation.push_str(&format!(
                    "\nTool {} returned: {}", tc.name, output
                ));
            }

            current_prompt = format!(
                "{}\n\nContinue the task. Output another tool call if more work is needed, or say DONE.",
                conversation
            );
        }

        let elapsed = start.elapsed();
        let task_success = (task.validate)(&self.config.work_dir);

        let tool_success_rate = if tool_calls > 0 {
            (tool_calls.saturating_sub(failures)) as f64 / tool_calls as f64
        } else {
            0.0
        };

        RawTrialResult {
            task_id: task.id.to_string(),
            trial_number,
            success: task_success,
            model_calls,
            tool_calls,
            failures,
            tool_success_rate,
            latency_ms: elapsed.as_secs_f64() * 1000.0,
            model_name: self.config.model_name.clone(),
            provider: "lmstudio".into(),
            mode: "raw".into(),
        }
    }
}