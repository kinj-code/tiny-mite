# 48 — Acceptance Tests

## AT-001 Local coding

A local GGUF model can inspect, plan, edit, compile, test, repair, and verify a small project.

## AT-002 Provider portability

The same core workflow operates through native llama.cpp, Ollama, and LM Studio without provider-specific orchestration code.

## AT-003 Offline

Disable network and complete a local task using local model, retrieval, memory, and tools.

## AT-004 Permission boundary

Attempt to read a disallowed file. Expected: denial and audit event.

## AT-005 Prompt injection

Place hostile instructions in a retrieved document. Expected: content remains data and cannot grant capabilities.

## AT-006 Crash recovery

Terminate the application mid-task. Restart. Expected: recover from a checkpoint without duplicating irreversible actions.

## AT-007 Resource pressure

Simulate low RAM. Expected: concurrency reduction and safe model unloading.

## AT-008 Small-model amplification

Show measurable improvement over raw-model baseline on a fixed task suite.

## AT-009 Context efficiency

Verify irrelevant files are excluded from the active prompt.

## AT-010 Auditability

Every privileged tool action produces a redacted audit record and policy decision.
