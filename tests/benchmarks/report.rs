//! Phase 11 — Benchmark Report Generator

use super::runner::TrialResult;
use std::fs;

/// Generate a complete benchmark report as Markdown.
pub fn generate_markdown(results: &[TrialResult], total_trials: usize) -> String {
    let mut report = String::new();

    report.push_str("# Phase 11 Benchmark Report\n\n");
    report.push_str(&format!("**Model**: {}  \n", results.first().map(|r| &r.model_name).unwrap_or(&"unknown".to_string())));
    report.push_str(&format!("**Provider**: {}  \n", results.first().map(|r| &r.provider).unwrap_or(&"unknown".to_string())));
    report.push_str(&format!("**Total trials**: {}  \n\n", total_trials));

    // ── Aggregate metrics ──────────────────────────
    let success_count = results.iter().filter(|r| r.success).count();
    let avg_model_calls: f64 = results.iter().map(|r| r.model_calls as f64).sum::<f64>() / results.len() as f64;
    let avg_tool_calls: f64 = results.iter().map(|r| r.tool_calls as f64).sum::<f64>() / results.len() as f64;
    let avg_latency: f64 = results.iter().map(|r| r.latency_ms).sum::<f64>() / results.len() as f64;
    let avg_tool_success: f64 = results.iter().map(|r| r.tool_success_rate).sum::<f64>() / results.len() as f64;

    report.push_str("## Aggregate Results\n\n");
    report.push_str("| Metric | Value |\n");
    report.push_str("|--------|-------|\n");
    report.push_str(&format!("| Task success rate | {:.1}% ({}/{}) |\n", 
        (success_count as f64 / results.len() as f64) * 100.0, success_count, results.len()));
    report.push_str(&format!("| Average model calls | {:.1} |\n", avg_model_calls));
    report.push_str(&format!("| Average tool calls | {:.1} |\n", avg_tool_calls));
    report.push_str(&format!("| Average latency (ms) | {:.0} |\n", avg_latency));
    report.push_str(&format!("| Average tool success rate | {:.1}% |\n\n", avg_tool_success * 100.0));

    // ── Per-task breakdown ──────────────────────────
    report.push_str("## Per-Task Results\n\n");
    for result in results {
        report.push_str(&format!(
            "### {} (trial {})\n\n\
             | Metric | Value |\n\
             |--------|-------|\n\
             | Task success | {} |\n\
             | Model calls | {} |\n\
             | Tool calls | {} |\n\
             | Failures | {} |\n\
             | Tool success rate | {:.1}% |\n\
             | Latency (ms) | {:.0} |\n\n",
            result.task_id, result.trial_number,
            if result.success { "✅ PASS" } else { "❌ FAIL" },
            result.model_calls,
            result.tool_calls,
            result.failures,
            result.tool_success_rate * 100.0,
            result.latency_ms,
        ));
    }

    report
}

/// Save results as JSON for machine readability.
pub fn save_json(results: &[TrialResult], path: &str) {
    if let Ok(json) = serde_json::to_string_pretty(results) {
        let _ = fs::create_dir_all(std::path::Path::new(path).parent().unwrap_or(std::path::Path::new(".")));
        let _ = fs::write(path, json);
    }
}

/// Save report as Markdown.
pub fn save_markdown(report: &str, path: &str) {
    let _ = fs::create_dir_all(std::path::Path::new(path).parent().unwrap_or(std::path::Path::new(".")));
    let _ = fs::write(path, report);
}