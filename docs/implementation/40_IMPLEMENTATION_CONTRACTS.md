# 40 — Implementation Contracts

The core must depend on stable interfaces, not provider-specific implementations.

## Core boundaries

```text
domain
  ↓
events / persistence
  ↓
runtime / memory / retrieval / security
  ↓
agents / tools / scheduler
  ↓
application
  ↓
Tauri UI
```

Core IDs should be strongly typed: `TaskId`, `EventId`, `AgentId`, `ToolId`, `MemoryId`, `DocumentId`, `ModelId`, `ProjectId`, and `CorrelationId`.

Every privileged operation returns structured errors with a category, retryability, user-facing action, and correlation ID.

Unsafe Rust is isolated to FFI/platform modules and must document its safety invariant.
