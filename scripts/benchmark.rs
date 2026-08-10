//! Tiny Mite benchmark harness — measures key runtime operations.
//!
//! Run with:
//! ```bash
//! cargo run --example benchmark --release
//! ```

use std::time::Instant;

fn main() {
    println!("=== Tiny Mite Benchmark Suite ===\n");

    // ── Intent classification benchmark ────────────────────────
    let classifier = tiny_mite_agents::IntentClassifier::new();
    let test_inputs = [
        "write code to implement a binary search tree",
        "explain how Rust ownership works",
        "my code is broken, help me debug the null pointer error",
        "what is the capital of France?",
        "build and compile the project and run tests",
        "plan the architecture for a microservice application",
        "summarize this document for me please",
    ];

    let t0 = Instant::now();
    for _ in 0..1000 {
        for input in &test_inputs {
            let _ = classifier.analyze(input);
        }
    }
    let elapsed = t0.elapsed();
    println!(
        "IntentClassification: {:.2} µs/call ({} inputs × 1000 iterations)",
        elapsed.as_micros() as f64 / (7.0 * 1000.0),
        test_inputs.len()
    );

    // ── Complexity estimation benchmark ────────────────────────
    let estimator = tiny_mite_agents::TaskComplexityEstimator::new();
    let t0 = Instant::now();
    for _ in 0..10_000 {
        let _ = estimator.estimate(
            tiny_mite_agents::Intent::CodeGeneration,
            true,
            true,
            3,
            4096,
        );
    }
    println!(
        "ComplexityEstimation: {:.2} µs/call (10000 iterations)",
        t0.elapsed().as_micros() as f64 / 10_000.0
    );

    // ── Planning benchmark ─────────────────────────────────────
    let planner = tiny_mite_agents::Planner::new();
    let t0 = Instant::now();
    for _ in 0..10_000 {
        let analysis = tiny_mite_agents::TaskAnalysis::simple(
            tiny_mite_agents::Intent::Debugging,
            tiny_mite_agents::TaskType::BugFix,
        );
        let _ = planner.plan(&analysis, "fix null pointer");
    }
    println!(
        "Planning: {:.2} µs/call (10000 iterations)",
        t0.elapsed().as_micros() as f64 / 10_000.0
    );

    // ── Working memory benchmark ───────────────────────────────
    let t0 = Instant::now();
    for _ in 0..1000 {
        let mut mem = tiny_mite_agents::WorkingMemory::new();
        for i in 0..50 {
            mem.insert(
                tiny_mite_agents::WorkingMemoryItem::new(
                    format!("item_{i}"),
                    tiny_mite_agents::memory::MemoryCategory::Fact,
                    format!("data_{i}"),
                )
                .with_importance(i as u32),
            );
        }
        let _snap = mem.snapshot();
    }
    println!(
        "WorkingMemory insert+snapshot (50 items): {:.2} µs/call (1000 iterations)",
        t0.elapsed().as_micros() as f64 / 1000.0
    );

    // ── LRU cache benchmark ────────────────────────────────────
    let t0 = Instant::now();
    let mut cache = tiny_mite_runtime::LruCache::<String, i32>::new(256);
    for i in 0..100_000 {
        cache.insert(format!("key_{}", i % 512), i);
        let _ = cache.get(&format!("key_{}", (i * 7 + 3) % 512));
    }
    println!(
        "LruCache: {:.2} µs/op (100000 ops)",
        t0.elapsed().as_micros() as f64 / 100_000.0
    );

    // ── Security token benchmark ───────────────────────────────
    use tiny_mite_security::Capability;
    use tiny_mite_security::CapabilityToken;
    let t0 = Instant::now();
    for _ in 0..10_000 {
        let token = CapabilityToken::new("agent")
            .grant(Capability::FilesystemRead)
            .grant(Capability::ShellExecute);
        let _ = token.has(Capability::FilesystemRead);
        let _ = token.is_valid();
    }
    println!(
        "CapabilityToken create+check: {:.2} µs/call (10000 iterations)",
        t0.elapsed().as_micros() as f64 / 10_000.0
    );

    // ── Audit log benchmark ────────────────────────────────────
    use tiny_mite_security::{AuditEntry, AuditLevel, AuditLog};
    let t0 = Instant::now();
    let mut log = AuditLog::new(10_000);
    for i in 0..10_000 {
        log.record(AuditEntry {
            id: format!("entry_{i}"),
            timestamp: chrono::Utc::now(),
            level: if i % 3 == 0 { AuditLevel::Warning } else { AuditLevel::Info },
            operation: "benchmark".into(),
            subject: "suite".into(),
            correlation_id: None,
            allowed: i % 5 != 0,
            description: format!("Benchmark entry {i}"),
            details: None,
        });
    }
    println!(
        "AuditLog record: {:.2} µs/entry (10000 entries)",
        t0.elapsed().as_micros() as f64 / 10_000.0
    );

    // ── Context compaction benchmark ───────────────────────────
    use tiny_mite_runtime::context::{ContextItem, ContextItemType, Authority};
    let t0 = Instant::now();
    for _ in 0..1000 {
        let items: Vec<ContextItem> = (0..100)
            .map(|i| ContextItem {
                id: format!("ctx_{i}"),
                item_type: ContextItemType::UserMessage,
                content: "x".repeat(300),
                token_count: 100,
                priority: (i % 10) as u32,
                relevance: 50,
                authority: Authority::User,
                pinned: i < 5,
                sensitivity: tiny_mite_runtime::context::Sensitivity::Public,
                timestamp: chrono::Utc::now(),
                metadata: Default::default(),
            })
            .collect();
        let compactor = tiny_mite_runtime::compaction::ContextCompactor::new(
            5000,
            tiny_mite_runtime::compaction::CompactionStrategy::DropLowPriority,
        );
        let _ = compactor.compact(items);
    }
    println!(
        "ContextCompaction (100 items → 5000 token budget): {:.2} µs/call (1000 iterations)",
        t0.elapsed().as_micros() as f64 / 1000.0
    );

    // ── Tool registry benchmark ────────────────────────────────
    use tiny_mite_domain::ToolId;
    use tiny_mite_tools::{RiskLevel, ToolDefinition, ToolRegistry};
    let t0 = Instant::now();
    for _ in 0..1000 {
        let mut reg = ToolRegistry::new();
        for i in 0..20 {
            reg.register(ToolDefinition::new(
                ToolId::new(),
                format!("tool_{i}"),
                "benchmark tool",
                if i < 5 { RiskLevel::Low } else { RiskLevel::Medium },
            ));
        }
        let _ = reg.find_by_capability("code_execution");
    }
    println!(
        "ToolRegistry register+query (20 tools): {:.2} µs/call (1000 iterations)",
        t0.elapsed().as_micros() as f64 / 1000.0
    );

    println!("\n=== Benchmark Complete ===");
}