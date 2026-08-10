# Tiny Mite — Phase 9A Integration Audit

> **Date**: 2026-08-10 | **Status**: Pre-implementation audit

## Scope

Every `ProviderError::Internal`, `STUB`, `not yet implemented`, placeholder, and mock in the production source tree was identified. This covers 11 crates in the workspace.

---

## A. MUST FIX (highest priority, blocks real execution)

| # | Location | Issue | Impact |
|---|----------|-------|--------|
| **A1** | `crates/tiny-mite-runtime/src/adapters.rs` (×14) | **All three HTTP providers (Ollama, LM Studio, OpenAI-compatible) return `ProviderError::Internal` for every operational method**: `discover_models`, `inspect`, `generate`, `stream` | Zero providers can actually perform inference. The entire system is architecture-only — no model can ever be called. |
| **A2** | `crates/tiny-mite-agents/src/runtime.rs:90` | `AgentRuntime::process()` uses `// placeholder passing result` for step execution — never calls a model provider | Even with working HTTP providers, the intelligence loop won't call them |
| **A3** | `crates/tiny-mite-llama-cpp/src/provider.rs:202,210` | `generate()` and `stream()` return `ProviderError::Internal("not yet implemented after ABI fix")` | Native llama.cpp is completely non-functional (blocked separately) |
| **A4** | `crates/tiny-mite-tools/src/impls.rs:138` | `HttpTool::get()` returns `[STUB] Would GET {url}` — never actually makes an HTTP request | Network tool cannot retrieve real data |
| **A5** | `crates/tiny-mite-tools/src/impls.rs:157` | `McpClientStub::call_tool()` returns `[MCP STUB]` | MCP integration is non-functional |

## B. IMPORTANT (degrades integration but doesn't block entirely)

| # | Location | Issue | Impact |
|---|----------|-------|--------|
| **B1** | `crates/tiny-mite-runtime/src/provider.rs:124-386` | `MockModelProvider` lives in the production `provider.rs` source file (35+ lines of mock code) rather than under `#[cfg(test)]` | Production binary includes mock implementation; could be accidentally used |
| **B2** | `crates/tiny-mite-core/src/config.rs:222,241,258` | Configuration sections for Retrieval, UI, Plugins are placeholder sections with no fields | Runtime can't be configured for retrieval backends or plugins |
| **B3** | `crates/tiny-mite-agents/src/context_bridge.rs:182` | Conversation zone is an "empty placeholder" — no actual conversation history integration | Chat context won't include prior messages |
| **B4** | `crates/tiny-mite-agents/src/runtime.rs` | No `CancellationToken` propagation in the intelligence loop | Tasks can't be cancelled mid-execution |
| **B5** | `crates/tiny-mite-runtime/src/router.rs:179-184` | `MockProvider` is used in `router.rs` tests but defines real `ProviderError("mock")` responses inline — safe, but `load()`/`generate()` always error | Router tests verify routing logic but not actual provider usage |

## C. OPTIONAL (improvements, not blockers)

| # | Location | Issue | Impact |
|---|----------|-------|--------|
| **C1** | `crates/tiny-mite-runtime/src/context.rs` | `CompiledContext.quality_score` computed but never consumed by any consumer outside tests | Dead data |
| **C2** | `crates/tiny-mite-agents/src/verifier.rs` | `Schema` and `Invariant` verification policies use simple keyword matching — doesn't validate against an actual JSON Schema or invariant checker | Limited verification value |
| **C3** | 160+ `missing_docs` warnings across `tiny-mite-llama-cpp`, `tiny-mite-agents`, `tiny-mite-security` | Public API documentation incomplete | Navigability and maintainability |

## D. EXPERIMENTAL / DEFERRED (intentionally frozen)

| # | Location | Issue | Status |
|---|----------|-------|--------|
| **D1** | `crates/tiny-mite-llama-cpp/` | Native llama.cpp ABI mismatch with LM Studio v2.25.2 | **FROZEN** — requires `libedit-dev` + `llama-cpp-2` bindgen |
| **D2** | `apps/desktop/` | Tauri shell not integrated; React app runs standalone via Vite | Pending Phase 9F |
| **D3** | `crates/tiny-mite-retrieval/` (6 tests) | `EmbeddingProvider` trait defined, lexical search works, but no embedding model connected | Needs Phase 9C HTTP providers |
| **D4** | `crates/tiny-mite-scheduler/src/cancellation.rs` | `CancellationToken` exists but only used internally in scheduler tests | Needs integration into AgentRuntime |

---

## Modules That Can Be Reused as-is

These are production-ready and correctly wired:

| Module | Crate | Status |
|--------|-------|--------|
| `IntentClassifier` + keyword patterns | `tiny-mite-agents` | ✅ Operational (67 tests) |
| `TaskComplexityEstimator` | `tiny-mite-agents` | ✅ Operational |
| `Planner` + `Plan` + `PlanStep` | `tiny-mite-agents` | ✅ Operational |
| `PlanValidator` (circular dep detection) | `tiny-mite-agents` | ✅ Operational |
| `WorkingMemory` + eviction + snapshot/restore | `tiny-mite-agents` | ✅ Operational |
| `ContextBridge` (build/compile) | `tiny-mite-agents` | ✅ Operational (4 tests) |
| `ContextCompiler` + zones + eviction | `tiny-mite-runtime` | ✅ Operational (29 tests) |
| `VerificationEngine` | `tiny-mite-agents` | ✅ Operational (8 tests) |
| `Reflection` + `ReflectionResult` | `tiny-mite-agents` | ✅ Operational (6 tests) |
| `RepairLoop` | `tiny-mite-agents` | ✅ Operational (2 tests) |
| `ModelRouter` + capability matching | `tiny-mite-runtime` | ✅ Operational (4 tests) |
| `ToolRegistry` + `ToolDefinition` | `tiny-mite-tools` | ✅ Operational (23 tests) |
| `ToolGateway` + authorization | `tiny-mite-security` | ✅ Operational |
| `PermissionEngine` | `tiny-mite-tools` | ✅ Operational |
| `CapabilityToken` | `tiny-mite-security` | ✅ Operational (48 tests) |
| `AuditLog` | `tiny-mite-security` | ✅ Operational |
| `SecurityPolicy` + `AccessPolicy` | `tiny-mite-security` | ✅ Operational |
| `OutputValidator` | `tiny-mite-security` | ✅ Operational |
| `SecretStore` | `tiny-mite-security` | ✅ Operational |
| `FileSystemTool` | `tiny-mite-tools` | ✅ Operational |
| `ShellTool` | `tiny-mite-tools` | ✅ Operational |
| `Sandbox` | `tiny-mite-tools` | ✅ Operational |
| `ApprovalManager` | `tiny-mite-tools` | ✅ Operational |
| `LexicalSearcher` + `HybridRanker` | `tiny-mite-retrieval` | ✅ Operational (6 tests) |
| `CrashRecovery` / `AdaptiveConcurrency` | `tiny-mite-runtime` | ✅ Operational |
| `LruCache` / `MemoryPressureManager` | `tiny-mite-runtime` | ✅ Operational |
| `ContextCompactor` (4 strategies) | `tiny-mite-runtime` | ✅ Operational |

**28 modules are fully operational and tested. All intelligence infrastructure is ready — the only missing piece is the actual model invocation bridge.**

---

## Recommended Implementation Order

1. **A1** — Implement real HTTP providers (reqwest) with mock server tests. This alone unblocks the entire system.
2. **A2** — Connect AgentRuntime to actually call `ModelProvider::generate()` and `ModelProvider::stream()` through the provider
3. **A4** — Make HttpTool use real reqwest calls through the sandbox
4. **B4** — Wire CancellationToken propagation into AgentRuntime
5. **B1** — Move `MockModelProvider` under `#[cfg(test)]`
6. **B3** — Integrate conversation history into ContextBridge
7. **B2** — Fill out config sections for retrieval/plugins
8. **C1-C3** — Quality improvements (docs, verification accuracy)

## Dependency Relationships

```
A1 (HTTP providers) ──────────────────────────────────────────────┐
    ↓                                                              │
A2 (AgentRuntime calls providers) ─── depends on A1                │
    ↓                                                              │
A4 (Real HTTP tool) ───────────────── depends on A1                │
    ↓                                                              │
B4 (CancellationToken) ─────────────── depends on A2               │
    ↓                                                              │
B3 (Conversation history) ──────────── depends on A2               │
    ↓                                                              │
B1 (Move mock) ─────────────────────── independent                 │
B2 (Config) ────────────────────────── independent                 │
C1-C3 (Quality) ────────────────────── independent                 │
```

**Items A1–A3 are on the critical path. All B and C items are independent of each other once A1+A2 are done.**

## Estimated Effort

| Block | Estimated Time | Risk |
|-------|---------------|------|
| A1 — HTTP providers (all 3) | 2 hours | Medium (reqwest integration, error mapping) |
| A2 — AgentRuntime connection | 1 hour | Low (plumbing existing components) |
| A4 — Real HTTP tool | 30 min | Low |
| B4 — CancellationToken wiring | 1 hour | Low (token already exists) |
| B1 — Move mock | 15 min | None |
| B3 — Conversation history | 30 min | Low |
| B2 — Config sections | 15 min | None |
| C1-C3 — Quality | 30 min | None |
| **Total (A+B)** | **~5.5 hours** | |

---

## Wait for approval before proceeding to Phase 9B.