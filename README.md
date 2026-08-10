# Tiny Mite — Engineering Documentation

**Version:** 2.0.0 Architecture Baseline  
**Date:** 2026-08-08  
**Status:** Architecture / implementation source of truth  
**Theme:** Saturated Jade

Tiny Mite is an offline-first Intelligence Operating System for local language models, designed primarily for 3B–9B parameter models. Its purpose is not to make a model larger. Its purpose is to make a small model *behave larger* through orchestration, context engineering, retrieval, memory, planning, tools, verification, reflection, caching, and hardware-aware execution.

This documentation is intentionally written as an engineering blueprint for coding agents and humans. The implementation must follow the documents rather than improvising architecture.

## Core principle

> Quality first. Speed second. Efficiency always.

A fast wrong answer is a failure. A slower answer that is correct, verified, secure, and reproducible is preferable. Once quality gates pass, Tiny Mite should optimize latency and resource usage aggressively.

## Source of truth

1. `docs/00_MASTER_SPECIFICATION.md`
2. `BUILD_MANIFEST.md`
3. `AGENTS.md`
4. The relevant subsystem specification
5. Tests and executable behavior

When documents conflict, do not silently guess. Record the conflict, resolve it through an ADR, and update the source of truth.

## Recommended workspace

```text
TinyMite/
├── AGENTS.md
├── BUILD_MANIFEST.md
├── README.md
├── docs/
├── apps/
├── crates/
├── tests/
├── scripts/
├── config/
├── data/
└── plugins/
```

The project should be opened as the VS Code/Cline workspace. Documentation uses relative paths so the project can live anywhere.

## High-level loop

```text
User
  ↓
Intent / Request Gateway
  ↓
Context Manager
  ↓
Planner
  ↓
Memory + Retrieval
  ↓
Reasoning / Decision
  ↓
Tool Authorization
  ↓
Execution
  ↓
Verification
  ↓
Repair / Reflection
  ↓
Memory Update
  ↓
Response
```

The loop is event-driven. Components are services/modules; agents are task-oriented workers activated when needed. Tiny Mite is not a free-for-all swarm.

## Runtime philosophy

The native inference path is built around llama.cpp rather than reinventing an inference engine. External providers such as Ollama and LM Studio are supported through adapters. Cloud/OpenAI-compatible endpoints may be supported as optional providers, but offline/local operation remains first-class.

## Security philosophy

Every autonomous action is treated as a security boundary. Agents do not receive unrestricted shell, filesystem, network, or credential access. Permissions are scoped, auditable, revocable, and risk-aware.

## Documentation navigation

- Master architecture: `docs/00_MASTER_SPECIFICATION.md`
- System design: `docs/02_SYSTEM_DESIGN.md`
- Event architecture: `docs/04_EVENT_SYSTEM.md`
- Agent runtime: `docs/05_AGENT_SYSTEM.md`
- Memory: `docs/07_MEMORY_SYSTEM.md`
- Context: `docs/09_CONTEXT_ENGINE.md`
- Models: `docs/10_MODEL_RUNTIME.md`
- Tools: `docs/13_TOOL_SYSTEM.md`
- Security: `docs/18_SECURITY.md`
- Performance: `docs/24_PERFORMANCE.md`
- Testing: `docs/27_TESTING.md`
- Failure recovery: `docs/29_FAILURE_RECOVERY.md`
- Build process: `docs/30_BUILD_SYSTEM.md`
- AI builder instructions: `AGENTS.md`
- Progress: `BUILD_MANIFEST.md`
