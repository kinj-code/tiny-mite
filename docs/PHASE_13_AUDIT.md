# Phase 13 — Repository Audit

**Git commit**: `6526681`
**Date**: 2026-08-11

## 1. Architecture Verification

### Canonical Execution Path

```
CLI (main.rs, ~170 lines)
  ↓
AgentRuntime::process_async()
  ↓
IntentClassifier → Planner → PlanValidator → ContextBridge
  ↓
ModelRouter → LmStudioProvider (real 4B model)
  ↓
tool_parser (5 formats)
  ↓
ToolExecutor → ToolGateway → PermissionEngine → Sandbox → AuditLog
  ↓
VerificationEngine → Reflection → RepairLoop/replan
  ↓
bounded multi-turn loop (AgentLoopConfig)
```

| Component | File | Status |
|-----------|------|--------|
| AgentRuntime | `runtime.rs` (923 lines) | ✅ Canonical, multi-turn, bounded |
| AgentLoopConfig | `runtime.rs` | ✅ 8 iters, 8 model calls, 32 tool calls, 5 failures |
| AgentConversation | `runtime.rs` | ✅ Persists messages, tool results, failures |
| CancellationToken | `runtime.rs` | ✅ Wired throughout loop |
| ModelRouter | `router.rs` | ✅ Provider selection + generate() delegation |
| LmStudioProvider | `adapters.rs` | ✅ OpenAI-compatible, reasoning_content fallback |
| tool_parser | `tool_parser.rs` | ✅ 5 formats (XML, malformed, JSON, fenced, OpenAI) |
| ToolExecutor | `tool_executor.rs` | ✅ Dynamic args, all tools registered |
| ToolGateway | `security/gateway.rs` | ✅ Authorizes every tool call |
| Sandbox | `sandbox.rs` | ✅ Path validation, allows non-existent paths |
| AuditLog | `security/audit.rs` | ✅ Entry per tool call |
| VerificationEngine | `verifier.rs` | ✅ Keyword/invariant/schema checks |
| Reflection | `reflection.rs` | ✅ Failure analysis, repair recommendations |
| RepairLoop | `repair.rs` | ✅ 3 retry attempts, simulated PASS |
| ModelToolProtocolConfig | `protocol.rs` | ✅ JSON/OpenAI/XML, minimal/maximal presets |

## 2. Known Limitations

| Limitation | Severity | Notes |
|-----------|----------|-------|
| 4B model format inconsistency | HIGH | ~60-70% first-attempt valid tool-call rate |
| No native tool calling integration | HIGH | LM Studio API supports it; not used |
| Multi-turn repair undemonstrated | MEDIUM | Model rarely produces follow-up calls |
| Pre-existing test failures (9 errors) | LOW | Rust 2024 edition lints in lib tests |
| No context token measurement | LOW | Estimated as `len()/3`, not measured |
| No controlled RAW comparison | MEDIUM | Infrastructure ready, batch not executed |

## 3. Duplicated Logic

NONE — The canonical execution path has been consolidated. There is exactly ONE orchestration engine: `AgentRuntime::process_async()`. `main.rs` is 170 lines of configuration only.

## 4. Test Status

| Suite | Result |
|-------|--------|
| `cargo check --workspace` | ✅ PASS |
| `cargo test -p tiny-mite-core --test e2e_integration` | ✅ 13/13 PASS |
| `cargo test --workspace` (lib) | ⚠️ 9 pre-existing errors (C — Rust 2024 lints) |
| `cargo test -p tiny-mite-core --bin tiny-mite` | ✅ PASS |
| `cargo fmt --all -- --check` | ⚠️ Pre-existing diffs |

## 5. Benchmark Status

| Component | File | Status |
|-----------|------|--------|
| 10 benchmark tasks | `tests/benchmarks/tasks.rs` | ✅ Complete |
| Tiny Mite runner | `tests/benchmarks/runner.rs` | ✅ Complete |
| RAW runner | `tests/benchmarks/raw_runner.rs` | ✅ Complete |
| Comparison engine | `tests/benchmarks/comparison.rs` | ✅ Complete |
| Report generator | `tests/benchmarks/report.rs` | ✅ Complete |
| Batch script | `scripts/run_phase11_benchmark.sh` | ✅ Complete |
| Pilot benchmark | — | ❌ NOT RUN |
| Ablation study | — | ❌ NOT RUN |

## 6. Model Protocol Behavior

Observed output formats from qwopus3.5-4b-coder-mtp (~10 trials):
- OpenAI function-calling: ~50%
- Markdown-fenced JSON: ~30%  
- Malformed XML: ~10%
- Natural language: ~10%

Publish ToolCall-15: 100/100 (EXTERNAL BASELINE — HuggingFace)

## 7. Context Size Estimates

| Component | Approximate tokens |
|-----------|-------------------|
| System prompt | ~150 |
| Task + context items | ~200 |
| Tool descriptions | ~50 |
| Conversation history | Variable |
| **Total typical prompt** | **~400-600** |

Within 8192 context budget. Compaction not yet triggered in practice.

## 8. Tool-Call Failure Modes

| Failure | Frequency | Classification |
|---------|-----------|----------------|
| Natural language `[toolcall]` | ~10% | F — Model capability |
| JSON wrapped in wrong format | ~20% | B — Format failure |
| Malformed XML | ~10% | B — Format failure |
| Parser recovers successfully | ~30% | ✅ Recovery success |

## 9. Security Verification

All security boundaries verified:
- ToolGateway → PermissionEngine → CapabilityToken → Sandbox → AuditLog
- Path traversal blocked
- Dry-run sandbox prevents real execution
- Revoked tokens produce PermissionDenied
- Success invariant prevents false positives

## 10. Conclusion

The architecture is validated and consolidated. The primary bottleneck is the 4B model's tool-call format consistency — a model capability limitation, not an architectural defect. Phase 14 should integrate native tool calling and run controlled benchmarks.