//! Tiny Mite — Acceptance Test Harness
//!
//! Validates end-to-end workflows without requiring a real LLM.
//! All tests use deterministic components only.

use tiny_mite_agents::{
    AgentRuntime, Intent, IntentClassifier, PlanValidator, Planner, Reflection,
    TaskAnalysis, TaskComplexityEstimator, TaskType, VerificationEngine, WorkingMemory,
};
use tiny_mite_runtime::ModelCapabilities;
use tiny_mite_security::{
    AuditLog, Capability, CapabilityToken, GatewayDecision, SecurityPolicy, ToolGateway,
};
use tiny_mite_tools::{RiskLevel, ToolDefinition, ToolRegistry};

// ── Helper: create a ModelCapabilities with all features ─────────

fn full_capabilities() -> ModelCapabilities {
    ModelCapabilities {
        text_generation: true, chat: true, tool_calling: true,
        structured_output: true, embeddings: true, reranking: true,
        vision: false, audio: false, reasoning: true,
        speculative_decoding: false, grammar_constrained_output: true,
    }
}

// ── 1. Intent Classification ─────────────────────────────────────

#[test]
fn classify_code_generation() {
    let classifier = IntentClassifier::new();
    let analysis = classifier.analyze("write code to implement a binary search tree");
    assert_eq!(analysis.intent, Intent::CodeGeneration);
    assert!(analysis.requires_planning);
}

#[test]
fn classify_debugging() {
    let classifier = IntentClassifier::new();
    let analysis = classifier.analyze("debug the null pointer error in my code");
    assert_eq!(analysis.intent, Intent::Debugging);
    assert!(analysis.requires_reasoning);
}

#[test]
fn classify_explanation() {
    let classifier = IntentClassifier::new();
    let analysis = classifier.analyze("explain how Rust ownership works");
    assert_eq!(analysis.intent, Intent::Explanation);
}

#[test]
fn classify_unknown_is_low_confidence() {
    let classifier = IntentClassifier::new();
    let analysis = classifier.analyze("xyzzy plugh frob");
    assert_eq!(analysis.intent, Intent::Unknown);
    assert!(analysis.confidence < 0.5);
}

// ── 2. Planning ─────────────────────────────────────────────────

#[test]
fn plan_code_generation_has_multiple_steps() {
    let planner = Planner::new();
    let analysis = TaskAnalysis::simple(Intent::CodeGeneration, TaskType::Implementation);
    let plan = planner.plan(&analysis, "write a BST");
    assert!(plan.steps.len() >= 2);
    assert!(plan.is_valid());
}

#[test]
fn plan_debugging_has_investigation_steps() {
    let planner = Planner::new();
    let analysis = TaskAnalysis::simple(Intent::Debugging, TaskType::BugFix);
    let plan = planner.plan(&analysis, "fix null pointer");
    assert!(plan.steps.len() >= 3);
}

#[test]
fn plan_simple_question_is_single_step() {
    let planner = Planner::new();
    let analysis = TaskAnalysis::simple(Intent::Question, TaskType::FactualQuery);
    let plan = planner.plan(&analysis, "what is Rust?");
    assert_eq!(plan.steps.len(), 1);
}

// ── 3. Plan Validation ──────────────────────────────────────────

#[test]
fn valid_plan_passes_validation() {
    let mut plan = tiny_mite_agents::Plan::new("p1", "test");
    plan.add_step(tiny_mite_agents::PlanStep::new("s1", "first"));
    plan.add_step(tiny_mite_agents::PlanStep::new("s2", "second").depends_on("s1"));

    let validator = PlanValidator::new();
    let result = validator.validate(&plan, &full_capabilities());
    assert!(result.valid);
}

#[test]
fn circular_dependency_detected() {
    let mut plan = tiny_mite_agents::Plan::new("p2", "circular");
    plan.add_step(tiny_mite_agents::PlanStep::new("s1", "a").depends_on("s2"));
    plan.add_step(tiny_mite_agents::PlanStep::new("s2", "b").depends_on("s1"));

    let validator = PlanValidator::new();
    let result = validator.validate(&plan, &full_capabilities());
    assert!(!result.valid);
}

// ── 4. Tool Authorization ───────────────────────────────────────

#[test]
fn authorized_read_tool_passes() {
    let mut gw = ToolGateway::new();
    let token = CapabilityToken::new("agent").grant(Capability::FilesystemRead);
    let tool = ToolDefinition::new(
        tiny_mite_domain::ToolId::new(), "read_file", "read", RiskLevel::Low,
    );
    assert_eq!(gw.authorize(&tool, &token, "agent"), GatewayDecision::Authorized);
}

#[test]
fn unauthorized_shell_is_denied() {
    let mut gw = ToolGateway::new();
    let token = CapabilityToken::new("agent");
    let tool = ToolDefinition::new(
        tiny_mite_domain::ToolId::new(), "run_shell", "exec", RiskLevel::Medium,
    );
    assert!(matches!(
        gw.authorize(&tool, &token, "agent"),
        GatewayDecision::Denied { .. }
    ));
}

#[test]
fn high_risk_requires_approval() {
    let mut gw = ToolGateway::new();
    let token = CapabilityToken::new("agent")
        .grant(Capability::CodeExecution)
        .grant(Capability::ShellExecute);
    let tool = ToolDefinition::new(
        tiny_mite_domain::ToolId::new(), "compile", "compiles", RiskLevel::High,
    );
    assert!(matches!(
        gw.authorize(&tool, &token, "agent"),
        GatewayDecision::RequiresApproval { .. }
    ));
}

// ── 5. Working Memory ───────────────────────────────────────────

#[test]
fn working_memory_insert_and_snapshot() {
    let mut mem = WorkingMemory::new();
    for i in 0..10 {
        mem.insert(
            tiny_mite_agents::WorkingMemoryItem::new(
                format!("item_{i}"),
                tiny_mite_agents::memory::MemoryCategory::Fact,
                format!("data_{i}"),
            )
            .with_importance(i),
        );
    }
    let snap = mem.snapshot();
    assert_eq!(snap.item_count, 10);

    mem.clear();
    assert!(mem.is_empty());

    mem.restore(&snap);
    assert_eq!(mem.len(), 10);
}

#[test]
fn working_memory_evicts_low_priority() {
    let mut mem = WorkingMemory::new().with_max_items(3);
    for i in 0..10 {
        mem.insert(
            tiny_mite_agents::WorkingMemoryItem::new(
                format!("item_{i}"),
                tiny_mite_agents::memory::MemoryCategory::Fact,
                format!("data_{i}"),
            )
            .with_importance(i),
        );
    }
    assert_eq!(mem.len(), 3);
}

#[test]
fn mandatory_items_survive_eviction() {
    let mut mem = WorkingMemory::new().with_max_items(2);
    mem.insert(
        tiny_mite_agents::WorkingMemoryItem::new(
            "critical",
            tiny_mite_agents::memory::MemoryCategory::Constraint,
            "must keep",
        )
        .mandatory()
        .with_importance(100),
    );
    for i in 0..5 {
        mem.insert(
            tiny_mite_agents::WorkingMemoryItem::new(
                format!("filler_{i}"),
                tiny_mite_agents::memory::MemoryCategory::Fact,
                format!("data_{i}"),
            )
            .with_importance(i),
        );
    }
    assert!(mem.get("critical").is_some());
}

// ── 6. Security ─────────────────────────────────────────────────

#[test]
fn audit_log_preserves_all_entries() {
    let mut log = AuditLog::new(100);
    for i in 0..10 {
        log.record(tiny_mite_security::AuditEntry {
            id: format!("entry_{i}"),
            timestamp: chrono::Utc::now(),
            level: tiny_mite_security::AuditLevel::Info,
            operation: "test".into(),
            subject: "suite".into(),
            correlation_id: None,
            allowed: true,
            description: format!("Entry {i}"),
            details: None,
        });
    }
    assert_eq!(log.len(), 10);
}

#[test]
fn capability_token_grant_check() {
    let token = CapabilityToken::new("agent")
        .grant(Capability::FilesystemRead)
        .grant(Capability::ShellExecute);
    assert!(token.has(Capability::FilesystemRead));
    assert!(token.has(Capability::ShellExecute));
    assert!(!token.has(Capability::NetworkAccess));
}

#[test]
fn revoked_token_is_invalid() {
    let mut token = CapabilityToken::new("agent").grant(Capability::FilesystemRead);
    assert!(token.is_valid());
    token.revoke();
    assert!(!token.is_valid());
}

#[test]
fn security_policy_default_denies_shell() {
    let policy = SecurityPolicy::new();
    let token = CapabilityToken::new("agent");
    assert!(!policy.can_access("shell:execute", &token));
}

// ── 7. End-to-End Intelligence Loop ─────────────────────────────

#[test]
fn e2e_classify_plan_validate_verify_reflect() {
    // Classify
    let classifier = IntentClassifier::new();
    let analysis = classifier.analyze("write code to implement a binary search tree");
    assert_eq!(analysis.intent, Intent::CodeGeneration);

    // Plan
    let planner = Planner::new();
    let plan = planner.plan(&analysis, "implement BST");
    assert!(plan.steps.len() >= 2);

    // Validate
    let validator = PlanValidator::new();
    let validation = validator.validate(&plan, &full_capabilities());
    assert!(validation.valid);

    // Verify (simulate each step passing)
    let verifier = VerificationEngine::new();
    for step in &plan.steps {
        let outcome = verifier.verify(step, "PASS", Some(0));
        assert!(outcome.passed);
    }

    // Reflect (no failures → nothing to report)
    let reflector = Reflection::new();
    let failed: Vec<(&str, &tiny_mite_agents::verifier::VerificationOutcome)> = Vec::new();
    let passed: Vec<&str> = plan.steps.iter().map(|s| s.id.as_str()).collect();
    let reflection = reflector.reflect_on_plan(&failed, &passed, plan.steps.len());
    assert!(!reflection.has_correction);
}

#[test]
fn e2e_agent_runtime_processes_task() {
    let runtime = AgentRuntime::new(full_capabilities());
    let result = runtime.process("debug the null pointer error");
    assert!(!result.summary.is_empty());
    assert!(result.plan.steps.len() >= 2);
    assert!(!result.memory.is_empty());
}

#[test]
fn e2e_complexity_estimation_integrated_with_analysis() {
    let classifier = IntentClassifier::new();
    let analysis = classifier.analyze("build and compile and deploy the project");

    // Complexity should be higher for action tasks
    assert!(analysis.complexity.overall > 10.0);

    // Should detect tools
    assert!(!analysis.requires_tools.is_empty());
    assert!(analysis.requires_planning);
}

#[test]
fn e2e_repair_loop_recovers_from_failure() {
    use tiny_mite_agents::RepairLoop;
    let repair = RepairLoop::new();
    let step = tiny_mite_agents::PlanStep::new("s1", "compile")
        .verify(tiny_mite_agents::VerificationPolicy::ExitCode);

    // Simulate initial failure
    let (outcome, _reflection, attempts) = repair.repair(&step, "error output", Some(1));
    assert!(outcome.passed); // should pass after repair
    assert!(attempts >= 1);
}

#[test]
fn e2e_tool_registry_find_by_capability() {
    let mut reg = ToolRegistry::new();
    let tool = ToolDefinition::new(
        tiny_mite_domain::ToolId::new(), "compile", "compile", RiskLevel::Medium,
    )
    .with_capabilities(vec!["code_execution".into()]);
    reg.register(tool);
    assert_eq!(reg.find_by_capability("code_execution").len(), 1);
}