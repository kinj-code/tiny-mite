//! Tiny Mite — CLI Entry Point
//!
//! Configures providers, sandbox, and tools, then delegates all
//! orchestration to [`AgentRuntime::process_async`].

use std::io::{self, Write};
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

    // ── Agent stats ───────────────────────────────────────
    trace.push_str(&format!(
        "\n[AGENT]\niteration={}/{}\nmodel_calls={}\ntool_calls={}\nfailures={}\nstuck={}\ncancelled={}\nelapsed_ms={:.0}\n\n",
        result.iterations,
        result.iterations,
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

// ── Interactive mode ──────────────────────────────────────────────

fn interactive_mode(runtime: &AgentRuntime) {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  Tiny Mite v0.1 — Local AI Coding Agent                  ║");
    println!("║  Type /help for commands, /quit to exit                  ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    loop {
        print!(">>> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        match input {
            "/quit" | "/exit" | "/q" => {
                println!("Goodbye!");
                break;
            }
            "/help" | "/h" => {
                println!("Tiny Mite — Interactive Mode");
                println!("  Type any coding task and Tiny Mite will execute it using a local LLM.");
                println!("  Example: Create a file called hello.txt containing Hello World");
                println!();
                println!("  Commands:");
                println!("    /help, /h    — Show this help");
                println!("    /quit, /q    — Exit");
                println!("    /model NAME  — Switch model (requires LM Studio reload)");
                println!("    /status      — Show agent status");
                println!();
                continue;
            }
            "/status" => {
                println!("Agent Status:");
                println!("  Provider: LM Studio (http://localhost:1234/v1)");
                println!("  Model: qwopus3.5-4b-coder-mtp (default)");
                println!("  Max iterations: 8, Max model calls: 8, Max tool calls: 32");
                println!("  Timeout: 300s");
                println!();
                continue;
            }
            s if s.starts_with("/model") => {
                println!("Model switching requires restart. Use --model flag:");
                println!("  tiny-mite --model <model-name>");
                continue;
            }
            _ => {}
        }

        // Execute the task
        println!("Working... (this may take 15-60 seconds with a local LLM)\n");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let task = input.to_string();
        let result = rt.block_on(async {
            runtime.process_async(&task).await
        });

        let trace = format_trace(&task, &result);
        println!("{trace}");
        println!("───────────────────────────────────────────────────────────\n");
    }
}

// ── Main ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = std::env::args().collect();

    // ── Configure sandbox (used by both modes) ───────────────
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

    // ── Configure provider ──────────────────────────────────
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

    // Default model
    let model_name = if args.len() >= 3 && args[1] == "--model" {
        args[2].clone()
    } else {
        "qwopus3.5-4b-coder-mtp".to_string()
    };

    // ── Build AgentRuntime ──────────────────────────────────
    let runtime = AgentRuntime::new(caps)
        .with_router(router)
        .with_tool_executor(tool_executor)
        .with_model_name(&model_name);

    // ── Interactive or one-shot mode ────────────────────────
    if args.len() < 2 {
        // No arguments: interactive REPL mode
        interactive_mode(&runtime);
    } else if args[1] == "--model" && args.len() < 4 {
        eprintln!("Usage: tiny-mite --model <model-name> <task description>");
        eprintln!("  Or:  tiny-mite (interactive mode)");
        std::process::exit(1);
    } else if args[1] == "--model" {
        // tiny-mite --model <name> <task>
        let task = args[3..].join(" ");
        let result = runtime.process_async(&task).await;
        let trace = format_trace(&task, &result);
        println!("{trace}");
    } else {
        // tiny-mite <task>
        let task = args[1..].join(" ");
        let result = runtime.process_async(&task).await;
        let trace = format_trace(&task, &result);
        println!("{trace}");
    }
}