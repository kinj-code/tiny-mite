# Tiny Mite Build Manifest

This is the implementation control plane. The coding agent must update it after verified work.

## Status legend

`[ ]` pending · `[~]` active · `[x]` verified · `[!]` blocked · `[?]` design decision

## Phase 0 — Governance and foundation

- [x] Documentation baseline
- [x] Agent instructions
- [x] Build manifest
- [x] Repository scaffolding
- [x] Rust workspace
- [x] Frontend workspace
- [x] CI pipeline
- [x] Formatting/linting
- [x] Dependency policy

## Phase 1 — Core runtime

- [x] Configuration service
- [x] Structured logging
- [x] Error taxonomy
- [x] Event bus
- [x] Event persistence/replay
- [x] Scheduler
- [x] Task registry
- [x] Cancellation system
- [x] Resource manager

## Phase 2 — Model runtime

- [x] Model abstraction
- [!] Native llama.cpp provider (experimental — ABI blocked on LM Studio v2.25.2 struct mismatch)
- [x] GGUF model discovery
- [x] Model metadata inspection
- [x] Context/session manager
- [x] Streaming
- [x] Grammar-constrained output
- [x] Structured tool-call output
- [x] Embedding provider
- [x] Reranker provider
- [x] Speculative decoding support
- [x] Hardware capability detection
- [x] Ollama adapter
- [x] LM Studio adapter
- [x] Generic OpenAI-compatible adapter

## Phase 3 — Intelligence

- [x] Intent classification
- [x] Task complexity estimator
- [x] Planner
- [x] Plan validator
- [x] Agent runtime
- [x] Agent registry
- [~] Context engine (extensive in tiny-mite-runtime)
- [x] Working memory
- [x] Episodic memory
- [x] Semantic memory
- [x] Procedural memory
- [x] Project memory
- [x] Memory consolidation
- [x] Retrieval pipeline
- [x] Hybrid search
- [x] Reranking
- [x] Reflection
- [x] Verification
- [x] Repair loop

## Phase 4 — Tools and execution

- [x] Tool registry
- [x] Tool schemas
- [x] Permission engine
- [x] Filesystem tools
- [x] Search tools
- [x] Shell tool
- [x] Git tool
- [x] Compiler/test runner
- [x] HTTP/network tool
- [x] MCP client
- [x] Sandbox
- [x] Dry-run mode
- [x] Approval UI

## Phase 5 — Security

- [x] Threat model
- [x] Secret store
- [x] Capability tokens
- [x] Prompt-injection defenses
- [x] Memory poisoning defenses
- [x] Tool-output validation
- [x] Network policy
- [x] Filesystem policy
- [x] Audit log
- [x] Security test suite
- [ ] Red-team harness

## Phase 6 — Desktop UX

- [~] Tauri shell (Vite + React scaffold ready, Tauri integration pending)
- [x] Chat UI
- [x] Task timeline
- [x] Agent activity panel
- [x] Context inspector
- [x] Memory inspector
- [x] Model manager
- [x] Tool permission center
- [x] Security center
- [x] Settings
- [x] Diagnostics
- [x] Saturated Jade theme

## Phase 7 — Reliability and performance

- [x] Crash recovery
- [x] Durable task state
- [x] Model unload/reload
- [x] Memory pressure manager
- [x] Context compaction
- [x] Cache layers
- [x] Benchmark suite
- [x] Hardware profiles
- [x] Adaptive concurrency
- [x] Latency tracing

## Phase 8 — Release

- [x] Linux packaging
- [x] Windows packaging
- [x] macOS packaging
- [ ] Upgrade/migration system
- [ ] Documentation validation
- [ ] Security audit
- [ ] Performance acceptance
- [ ] Release candidate
- [ ] Final release

## Definition of done

A feature is complete only when its documentation, implementation, tests, observability, security behavior, failure behavior, and acceptance criteria are all satisfied.


## Phase 0.5 — Implementation-ready contracts

- [x] Core interfaces
- [x] Event/task schemas
- [x] Context budget algorithm
- [x] Resource scheduler
- [x] Model lifecycle
- [x] Tauri IPC
- [x] MCP/security integration
- [x] Acceptance test harness
- [x] Fresh-agent build runbook
