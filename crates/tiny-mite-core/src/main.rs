//! Tiny Mite — CLI Entry Point
//!
//! Configures providers, sandbox, and tools, then delegates all
//! orchestration to [`AgentRuntime::process_async`].

use std::sync::Arc;
use tokio::sync::Mutex;

use tiny_mite_agents::{
    AgentRuntime, ToolExecutionOutcome, ToolExecutor,
};
use tiny_mite_runtime::{
    LmStudioProvider, ModelCapabilities, ModelRouter,
};
use tiny_mite_tools::{Sandbox, SandboxConfig};

// ── Observability trace formatting ────────────────────────────────

fn format_trace(input: &str, result: &tiny_mite_agents::TaskResult) -> String {
    let mut trace = String::new();

    trace.push_str(&format!(
        "[REQUEST]\n{input}\n\n\
         [INTENT]\n{:?}\nconfidence={:.2}\ntype={:?}\n\n\
         [COMPLEXITY]\ntotal={:.1}\ntools={:?}\nrisk={:.1}\n\n\
         [PLAN]\n{} steps\n",
        result.analysis.intent,
        result.analysis.confidence,
        result.analysis.task_type,
        result.analysis.complexity.overall,
        result.analysis.requires_tools,
        result.analysis.risk_score,
        result.plan.steps.len(),
    ));

    for (i, step) in result.plan.steps.iter().enumerate() {
        trace.push_str(&format!(
            "  {}. {} [deps:{} tools:{}]\n",
            i + 1,
            step.description,
            step.dependencies.len(),
            step.tools.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }

    // ── Conversation messages ──────────────────────────────
    for msg in &result.conversation.messages {
        match msg {
            tiny_mite_agents::ConversationMessage::User(text) => {
                // Skip initial user message (already shown as REQUEST)
                if text == input { continue; }
                trace.push_str(&format!("\n[USER]\n{text}\n"));
            }
            tiny_mite_agents::ConversationMessage::Assistant(text) => {
                trace.push_str(&format!("\n[MODEL RESPONSE]\n{text}\n"));
            }
            tiny_mite_agents::ConversationMessage::ToolResult { name, args, output, exit_code, success } => {
                trace.push_str(&format!(
                    "[TOOL]\n{} {} exit_code={:?} success={}\n[TOOL RESULT]\n{}\n",
                    name,
                    serde_json::to_string(args).unwrap_or_default(),
                    exit_code,
                    success,
                    output
                ));
            }
            tiny_mite_agents::ConversationMessage::FailureSummary(text) => {
                trace.push_str(&format!("\n[FAILURE SUMMARY]\n{text}\n"));
            }
        }
    }

    // ── Tool results ───────────────────────────────────────
    for (i, tr) in result.tool_results.iter().enumerate() {
        match tr {
            ToolExecutionOutcome::Success { result: tool_result, .. } => {
                if i < result.tool_results.len() - 1 { continue; } // already shown in conversation
                trace.push_str(&format!(
                    "[TOOL RESULT]\nsuccess={}\noutput=\n{}\n",
                    tool_result.success, tool_result.output
                ));
            }
            other => {
                trace.push_str(&format!("[TOOL]\nerror={:?}\n", other));
            }
        }
    }

    trace.push_str(&format!(
        "\n[VERIFICATION]\npassed={}\n",
        result.verification_results.iter().filter(|o| o.passed).count()
    ));

    for (i, outcome) in result.verification_results.iter().enumerate() {
        trace.push_str(&format!(
            "  step_{}: {} ({})\n",
            i + 1,
            if outcome.passed { "PASS" } else { "FAIL" },
            outcome.reason
        ));
    }

    trace.push_str(&format!(
        "\n[REFLECTION]\ncorrection={}\nshould_retry={}\ncorrection_detail={:?}\n\n",
        result.reflection.has_correction,
        result.reflection.should_retry,
        result.reflection.correction
    ));

    trace.push_str(&format!(
        "[AGENT]\niteration={}/{}\nmodel_calls={}\ntool_calls={}\nfailures={}\nstuck={}\ncancelled={}\nelapsed_ms={:.0}\n\n",
        result.iterations,
        result.iterations,  // max is tracked internally
        result.model_calls,
        result.tool_call_count,
        result.conversation.failures,
        result.stuck,
        result.cancelled,
        result.elapsed_ms,
    ));

    trace.push_str(&format!(
        "[FINAL]\nsuccess={}\ntotal_latency_ms={:.0}\n",
        result.success,
        result.elapsed_ms,
    ));

    trace
}

// ── Main ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: tiny-mite <task description>");
        eprintln!("  Or:  tiny-mite --model <model-name> <task description>");
        eprintln!("\nDefault model: qwopus3.5-4b-coder-mtp");
        eprintln!("Provider: LM Studio at http://localhost:1234/v1");
        std::process::exit(1);
    }

    let (model_name, task) = if args[1] == "--model" && args.len() >= 4 {
        (args[2].clone(), args[3..].join(" "))
    } else {
        ("qwopus3.5-4b-coder-mtp".to_string(), args[1..].join(" "))
    };

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Tiny Mite — Phase 10.2 Autonomous Agent Loop            ║");
    println!("║  Model: {:<48} ║", model_name);
    println!("║  Provider: LM Studio (http://localhost:1234/v1)          ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    // ── Configure sandbox ──────────────────────────────────
    let sandbox = Sandbox::new(SandboxConfig {
        allowed_paths: vec![
            std::path::PathBuf::from("/tmp"),
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        ],
        allow_shell: true,
        allow_network: false,
        max_runtime_ms: 120_000,
    });

    let mut tool_executor = ToolExecutor::new(sandbox);
    tool_executor.register_standard_tools();
    let tool_executor = Arc::new(Mutex::new(tool_executor));

    // ── Configure provider ─────────────────────────────────
    let provider = LmStudioProvider::new("http://localhost:1234");

    let caps = ModelCapabilities {
        text_generation: true,
        chat: true,
        tool_calling: true,
        ..Default::default()
    };

    let mut router = ModelRouter::new();
    router.register("lmstudio", Box::new(provider)).await;
    let router = Arc::new(router);

    // ── Build AgentRuntime — the canonical execution engine ─
    let runtime = AgentRuntime::new(caps)
        .with_router(router)
        .with_tool_executor(tool_executor)
        .with_model_name(&model_name);

    // ── Execute ────────────────────────────────────────────
    let result = runtime.process_async(&task).await;
    let trace = format_trace(&task, &result);

    println!("{trace}");
    println!("═══════════════════════════════════════════════════════════");
    println!("Execution complete.");
}