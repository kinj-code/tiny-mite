//! Phase 11.1 — RAW vs Tiny Mite Comparison Report Generator

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::benchmarks::runner::TrialResult;
use crate::benchmarks::raw_runner::RawTrialResult;

/// A per-task comparison between RAW and Tiny Mite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskComparison {
    pub task_id: String,
    pub raw_success_rate: f64,
    pub tinymite_success_rate: f64,
    pub absolute_delta_pp: f64,
    pub raw_avg_latency_ms: f64,
    pub tinymite_avg_latency_ms: f64,
    pub raw_avg_model_calls: f64,
    pub tinymite_avg_model_calls: f64,
    pub raw_avg_tool_calls: f64,
    pub tinymite_avg_tool_calls: f64,
    pub raw_trials: usize,
    pub tinymite_trials: usize,
}

/// Aggregate comparison across all tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateComparison {
    pub raw_overall_success_rate: f64,
    pub tinymite_overall_success_rate: f64,
    pub absolute_delta_pp: f64,
    pub raw_total_trials: usize,
    pub tinymite_total_trials: usize,
    pub raw_avg_latency_ms: f64,
    pub tinymite_avg_latency_ms: f64,
    pub per_task: Vec<TaskComparison>,
}

/// Generate a comparison report from RAW and TinyMite results.
pub fn compare(
    raw_results: &[RawTrialResult],
    tinymite_results: &[TrialResult],
) -> AggregateComparison {
    // Group by task_id
    let mut raw_by_task: HashMap<String, Vec<&RawTrialResult>> = HashMap::new();
    let mut tm_by_task: HashMap<String, Vec<&TrialResult>> = HashMap::new();

    for r in raw_results {
        raw_by_task.entry(r.task_id.clone()).or_default().push(r);
    }
    for r in tinymite_results {
        tm_by_task.entry(r.task_id.clone()).or_default().push(r);
    }

    let mut all_task_ids: Vec<String> = raw_by_task.keys().cloned().collect();
    for id in tm_by_task.keys() {
        if !all_task_ids.contains(id) {
            all_task_ids.push(id.clone());
        }
    }
    all_task_ids.sort();

    let mut per_task = Vec::new();

    for task_id in &all_task_ids {
        let raw = raw_by_task.get(task_id);
        let tm = tm_by_task.get(task_id);

        let raw_success_rate = raw.map(|trials| {
            trials.iter().filter(|t| t.success).count() as f64 / trials.len() as f64
        }).unwrap_or(0.0);

        let tinymite_success_rate = tm.map(|trials| {
            trials.iter().filter(|t| t.success).count() as f64 / trials.len() as f64
        }).unwrap_or(0.0);

        let raw_avg_latency = raw.map(|trials| {
            trials.iter().map(|t| t.latency_ms).sum::<f64>() / trials.len() as f64
        }).unwrap_or(0.0);

        let tinymite_avg_latency = tm.map(|trials| {
            trials.iter().map(|t| t.latency_ms).sum::<f64>() / trials.len() as f64
        }).unwrap_or(0.0);

        let raw_avg_model_calls = raw.map(|trials| {
            trials.iter().map(|t| t.model_calls as f64).sum::<f64>() / trials.len() as f64
        }).unwrap_or(0.0);

        let tinymite_avg_model_calls = tm.map(|trials| {
            trials.iter().map(|t| t.model_calls as f64).sum::<f64>() / trials.len() as f64
        }).unwrap_or(0.0);

        let raw_avg_tool_calls = raw.map(|trials| {
            trials.iter().map(|t| t.tool_calls as f64).sum::<f64>() / trials.len() as f64
        }).unwrap_or(0.0);

        let tinymite_avg_tool_calls = tm.map(|trials| {
            trials.iter().map(|t| t.tool_calls as f64).sum::<f64>() / trials.len() as f64
        }).unwrap_or(0.0);

        per_task.push(TaskComparison {
            task_id: task_id.clone(),
            raw_success_rate,
            tinymite_success_rate,
            absolute_delta_pp: (tinymite_success_rate - raw_success_rate) * 100.0,
            raw_avg_latency_ms: raw_avg_latency,
            tinymite_avg_latency_ms: tinymite_avg_latency,
            raw_avg_model_calls,
            tinymite_avg_model_calls,
            raw_avg_tool_calls,
            tinymite_avg_tool_calls,
            raw_trials: raw.map(|t| t.len()).unwrap_or(0),
            tinymite_trials: tm.map(|t| t.len()).unwrap_or(0),
        });
    }

    let raw_overall = raw_results.iter().filter(|r| r.success).count() as f64
        / raw_results.len().max(1) as f64;
    let tm_overall = tinymite_results.iter().filter(|r| r.success).count() as f64
        / tinymite_results.len().max(1) as f64;
    let raw_avg_lat = raw_results.iter().map(|r| r.latency_ms).sum::<f64>()
        / raw_results.len().max(1) as f64;
    let tm_avg_lat = tinymite_results.iter().map(|r| r.latency_ms).sum::<f64>()
        / tinymite_results.len().max(1) as f64;

    AggregateComparison {
        raw_overall_success_rate: raw_overall,
        tinymite_overall_success_rate: tm_overall,
        absolute_delta_pp: (tm_overall - raw_overall) * 100.0,
        raw_total_trials: raw_results.len(),
        tinymite_total_trials: tinymite_results.len(),
        raw_avg_latency_ms: raw_avg_lat,
        tinymite_avg_latency_ms: tm_avg_lat,
        per_task,
    }
}

/// Generate a Markdown comparison report from aggregate data.
pub fn comparison_markdown(comparison: &AggregateComparison) -> String {
    let mut md = String::new();
    md.push_str("# Phase 11.1 — RAW vs Tiny Mite Comparison\n\n");
    md.push_str(&format!(
        "**Model**: qwopus3.5-4b-coder-mtp | **Provider**: LM Studio\n\
         **RAW trials**: {} | **Tiny Mite trials**: {}\n\n",
        comparison.raw_total_trials, comparison.tinymite_total_trials,
    ));

    md.push_str("## Aggregate Results\n\n");
    md.push_str("| Metric | RAW | Tiny Mite | Delta |\n");
    md.push_str("|--------|-----|-----------|-------|\n");
    md.push_str(&format!(
        "| Success rate | {:.0}% | {:.0}% | {:+.1} pp |\n",
        comparison.raw_overall_success_rate * 100.0,
        comparison.tinymite_overall_success_rate * 100.0,
        comparison.absolute_delta_pp,
    ));
    md.push_str(&format!(
        "| Avg latency (ms) | {:.0} | {:.0} | |\n",
        comparison.raw_avg_latency_ms,
        comparison.tinymite_avg_latency_ms,
    ));
    md.push_str("\n## Per-Task Results\n\n");
    md.push_str("| Task | RAW | Tiny Mite | Delta |\n");
    md.push_str("|------|-----|-----------|-------|\n");
    for task in &comparison.per_task {
        let delta_str = if task.absolute_delta_pp > 0.0 {
            format!("**+{:.1} pp**", task.absolute_delta_pp)
        } else if task.absolute_delta_pp < 0.0 {
            format!("{:.1} pp", task.absolute_delta_pp)
        } else {
            "0".to_string()
        };
        md.push_str(&format!(
            "| {} | {:.0}% | {:.0}% | {} |\n",
            task.task_id,
            task.raw_success_rate * 100.0,
            task.tinymite_success_rate * 100.0,
            delta_str,
        ));
    }

    md
}