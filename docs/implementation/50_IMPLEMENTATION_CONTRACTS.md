# Tiny Mite — Implementation Contracts

> **Status**: Live · **Version**: 0.1.0 · **Last updated**: 2026-08-10

This document defines the stable contracts that every Tiny Mite subsystem must adhere to. Contracts that change require an ADR and documentation update.

## 1. Core Interfaces

### 1.1 ModelProvider

```rust
#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn provider_capabilities(&self) -> ModelCapabilities;

    async fn discover_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    async fn inspect(&self, id: &ModelId) -> Result<ModelInfo, ProviderError>;
    async fn load(&self, id: &ModelId) -> Result<ModelInfo, ProviderError>;
    async fn unload(&self, id: &ModelId) -> Result<(), ProviderError>;
    async fn generate(&self, request: &InferenceRequest) -> Result<InferenceResponse, ProviderError>;
    async fn stream(&self, request: &InferenceRequest, sink: mpsc::Sender<InferenceResponse>) -> Result<(), ProviderError>;
    async fn cancel(&self, correlation_id: &str) -> Result<(), ProviderError>;
    async fn health_check(&self) -> Result<(), ProviderError>;
    async fn list_devices(&self) -> Result<Vec<DeviceInfo>, ProviderError>;
    async fn count_tokens(&self, model_id: &ModelId, text: &str) -> Result<usize, ProviderError>;
}
```

**Contract**: Every inference backend MUST implement this trait. The runtime NEVER calls model-specific code directly. Adding a new backend requires only implementing this trait.

**Implementations**: `OllamaProvider`, `LmStudioProvider`, `OpenAiProvider`, `NativeLlamaCppProvider` (experimental).

### 1.2 EmbeddingProvider

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, model_id: &ModelId, text: &str) -> Result<EmbeddingResult, EmbeddingError>;
    async fn embed_batch(&self, model_id: &ModelId, texts: &[String]) -> Result<Vec<EmbeddingResult>, EmbeddingError>;
    async fn supports_embedding(&self, model_id: &ModelId) -> Result<bool, EmbeddingError>;
}
```

### 1.3 RerankerProvider

```rust
#[async_trait]
pub trait RerankerProvider: Send + Sync {
    async fn rerank(&self, model_id: &ModelId, query: &str, candidates: Vec<RerankCandidate>) -> Result<RerankResult, RerankerError>;
    async fn supports_reranking(&self, model_id: &ModelId) -> Result<bool, RerankerError>;
}
```

### 1.4 EventBus

```rust
pub trait EventBus: Send + Sync {
    fn publish(&self, event: EventEnvelope);
    fn subscribe(&self, event_type: &str, handler: Box<dyn EventHandler>);
    fn replay(&self, since: DateTime<Utc>) -> Vec<EventEnvelope>;
}
```

**Contract**: All inter-module communication goes through the event bus. Modules never reference each other directly except through shared domain types.

### 1.5 Tool interface

Every tool MUST declare:
- `ToolDefinition` with input schema, output schema, risk level, capabilities
- Execution gated through `ToolGateway.authorize()` returning `GatewayDecision`
- All executions produce `AuditEntry` in the `AuditLog`

---

## 2. Event / Task Schemas

All events use `EventEnvelope` with typed payloads:

| Event | Schema | Producer | Consumers |
|-------|--------|----------|-----------|
| `task.created` | TaskId, description, timestamp | API layer | Scheduler, Timeline UI |
| `task.analyzed` | TaskAnalysis, correlation_id | IntentClassifier | Planner, ContextEngine |
| `plan.created` | Plan, task_id | Planner | AgentRuntime, Timeline UI |
| `step.started` | PlanStep, task_id | AgentRuntime | AgentPanel UI |
| `step.completed` | PlanStep, VerificationOutcome | AgentRuntime | Reflection, RepairLoop |
| `step.failed` | PlanStep, error, retry_count | AgentRuntime | Reflection, RepairLoop |
| `tool.requested` | ToolDefinition, CapabilityToken | AgentRuntime | ToolGateway, ApprovalManager |
| `tool.completed` | ToolResult, duration_ms | ToolGateway | AgentRuntime |
| `verification.failed` | PlanStep, reason | VerificationEngine | Reflection |
| `reflection.completed` | ReflectionResult | Reflection | RepairLoop |
| `task.completed` | TaskResult, duration_ms | AgentRuntime | MemoryConsolidation, Timeline UI |
| `security.audit` | AuditEntry | ToolGateway, SecurityPolicy | SecurityCenter UI |

**Contract**: All events carry a `correlation_id` for tracing. Event payloads are versioned serde structs.

---

## 3. Context Budget Algorithm

The context budget algorithm determines what fits in the model's context window:

```
budget = model_context_limit - reserved_output - reserved_tool_calls - safety_margin

for each candidate (sorted by priority descending):
    if candidate.is_mandatory:
        add to context
    elif budget_remaining >= candidate.token_count:
        add to context
    elif can_compress(candidate):
        compress and add
    else:
        add to omitted list
```

**Contract**: The algorithm must be deterministic. Budget calculation is in `ContextCompiler::compile()`. Compression strategies: `DropOldest`, `DropLowPriority`, `TruncateToHeadline`, `MergeAdjacent`.

---

## 4. Resource Scheduler

The resource scheduler prevents Tiny Mite from exhausting the host machine:

```rust
pub struct ResourceScheduler {
    max_concurrent_tasks: usize,     // default: 4
    max_loaded_models: usize,        // default: 2
    max_concurrent_http: usize,      // default: 8
    memory_high_water_mark: u64,     // bytes
    cpu_target: f32,                 // 0.0-1.0
}
```

**Contract**: When resources are constrained, the scheduler reduces concurrency, pauses low-priority work, unloads idle models, then resumes when pressure relieves.

---

## 5. Model Lifecycle

```
Discovery → inspect GGUF → classify capabilities
    ↓
Load → llama_model_load_from_file → create context
    ↓
Ready → generate / stream / embed / tokenize
    ↓
Idle (timeout) → unload → free context → free model
    ↓
Reload (on demand) → load again
```

**Contract**: Model state machine is in `ModelState` enum. The native llama.cpp provider follows this lifecycle. Ollama/LM Studio adapters treat `load()`/`unload()` as no-ops since the server manages models.

---

## 6. Tauri IPC

The desktop frontend communicates with the Rust backend via Tauri commands:

| Command | Direction | Purpose |
|---------|-----------|---------|
| `send_message` | UI → Core | Submit user input |
| `subscribe_events` | Core → UI | Stream task/agent events |
| `approve_tool` | UI → Core | Approve pending tool execution |
| `deny_tool` | UI → Core | Deny pending tool execution |
| `get_models` | UI → Core | List available models |
| `load_model` | UI → Core | Load a model |
| `get_audit_log` | UI → Core | Retrieve audit entries |
| `update_settings` | UI → Core | Update configuration |

**Contract**: All IPC uses JSON serialization. Sensitive data (secrets) is never transmitted over IPC. The Tauri shell is currently in Vite+React scaffold; Tauri integration is pending.

---

## 7. MCP / Security Integration

Model Context Protocol servers operate under the same security model as local tools:

- Each MCP server receives a `CapabilityToken` with scoped permissions
- MCP tool calls go through `ToolGateway.authorize()`
- All MCP interactions are audited
- MCP servers cannot grant themselves capabilities
- Network access for MCP is explicitly granted per server

**Contract**: MCP client stub in `McpClientStub`. Full integration deferred to Phase 5.5.

---

## 8. Acceptance Test Harness

Location: `tests/acceptance/`

The acceptance test harness validates end-to-end workflows without requiring a real model:

```bash
cargo test --test acceptance
```

### Covered scenarios:

1. **Intent classification**: Input → correct intent (9 test inputs)
2. **Planning**: Intent → valid plan with dependencies
3. **Tool authorization**: Token checks → correct GatewayDecision
4. **Context budgeting**: Items → Compacted output under budget
5. **Working memory**: Insert/evict/snapshot/restore cycles
6. **Security**: Capability checks, audit log, secret redaction
7. **End-to-end**: classify → plan → validate → execute → verify → reflect → repair

**Contract**: All acceptance tests must pass before a release is cut.

---

## 9. Fresh-Agent Build Runbook

Location: `docs/FRESH_AGENT_BUILD_RUNBOOK.md`

A new coding agent starting from scratch must:

1. Read `AGENTS.md`
2. Read `BUILD_MANIFEST.md`
3. Read this document (`docs/implementation/50_IMPLEMENTATION_CONTRACTS.md`)
4. Run `cargo check --workspace` to verify the build
5. Run `cargo test --workspace` to verify all 301 tests pass
6. Run `cargo fmt --all -- --check` to verify formatting
7. Read the subsystem document for the next unfinished task
8. Begin implementation

**Contract**: The runbook must be followed before any code changes.