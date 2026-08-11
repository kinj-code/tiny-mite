//! Phase 11 — Benchmark Runner
//!
//! Executes benchmark tasks against Tiny Mite's AgentRuntime and records results.

use super::tasks::Task;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use tiny_mite_agents::{AgentRuntime, ToolExecutor};
use tiny_mite_runtime::{LmStudioProvider, ModelCapabilities, ModelRouter};
use tiny_mite_tools::{Sandbox, SandboxConfig};

/// A single benchmark trial result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialResult {
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
}

/// Benchmark configuration.
pub struct BenchmarkConfig {
    pub model_name: String,
    pub provider_url: String,
    pub trials_per_task: usize,
    pub work_dir: PathBuf,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            model_name: "qwopus3.5-4b-coder-mtp".into(),
            provider_url: "http://localhost:1234".into(),
            trials_per_task: 3,
            work_dir: PathBuf::from("/tmp"),
        }
    }
}

/// Executes benchmark tasks against the real model.
pub struct BenchmarkRunner {
    config: BenchmarkConfig,
}

impl BenchmarkRunner {
    pub fn new(config: BenchmarkConfig) -> Self {
        Self { config }
    }

    /// Run a single task for the configured number of trials.
    pub async fn run_task(&self, task: &Task) -> Vec<TrialResult> {
        let mut results = Vec::new();

        for trial_num in 1..=self.config.trials_per_task {
            println!("  Trial {}/{}...", trial_num, self.config.trials_per_task);

            let result = self.run_trial(task, trial_num).await;
            results.push(result);
        }

        results
    }

    /// Run multiple tasks and collect all results.
    pub async fn run_all(&self, tasks: &[Task]) -> Vec<TrialResult> {
        let mut all_results = Vec::new();

        for task in tasks {
            println!("Benchmark: {} ({:?})", task.id, task.difficulty);
            let results = Box::pin(self.run_task(task)).await;
            all_results.extend(results);
        }

        all_results
    }

    async fn run_trial(&self, task: &Task, trial_number: usize) -> TrialResult {
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

        let caps = ModelCapabilities {
            text_generation: true,
            chat: true,
            tool_calling: true,
            ..Default::default()
        };

        let mut router = ModelRouter::new();
        router.register("lmstudio", Box::new(provider)).await;
        let router = Arc::new(router);

        // ── Build AgentRuntime ────────────────────────
        let runtime = AgentRuntime::new(caps)
            .with_router(router)
            .with_tool_executor(tool_executor)
            .with_model_name(&self.config.model_name);

        // ── Execute ───────────────────────────────────
        let result = runtime.process_async(task.prompt).await;

        let elapsed = start.elapsed();

        // ── Validate ──────────────────────────────────
        let task_success = (task.validate)(&self.config.work_dir);

        // Tool success rate
        let tool_success_count = result.tool_results.iter()
            .filter(|t| matches!(t, tiny_mite_agents::ToolExecutionOutcome::Success { .. }))
            .count();
        let tool_success_rate = if result.tool_call_count > 0 {
            tool_success_count as f64 / result.tool_call_count as f64
        } else {
            0.0
        };

        TrialResult {
            task_id: task.id.to_string(),
            trial_number,
            success: task_success,
            model_calls: result.model_calls,
            tool_calls: result.tool_call_count,
            failures: result.conversation.failures,
            tool_success_rate,
            latency_ms: elapsed.as_secs_f64() * 1000.0,
            model_name: self.config.model_name.clone(),
            provider: "lmstudio".to_string(),
        }
    }
}

/// Quick diagnostic: test model connectivity.
pub async fn test_model_available(config: &BenchmarkConfig) -> bool {
    let provider = LmStudioProvider::new(&config.provider_url);
    provider.health_check().await.is_ok()
}