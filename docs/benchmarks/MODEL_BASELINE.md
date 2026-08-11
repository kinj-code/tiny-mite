# Qwopus3.5-4B-Coder-MTP — Published Model Baseline

Source: https://huggingface.co/Jackrong/Qwopus3.5-4B-Coder-MTP-GGUF

## Published Benchmarks (External Evaluation)

| Benchmark | Qwopus3.5-4B-Coder-MTP | Qwen3.5-4B-MTP (reference) |
|-----------|------------------------|---------------------------|
| Suite Average | **82.0%** | 74.0% |
| BugFind-15 | **71/100** | 52/100 |
| HermesAgent-20 | **64/100** | 61/100 |
| ToolCall-15 | **100/100** | 90/100 |
| InstructFollow-15 | **93/100** | 93/100 |

## Published Evaluation Configuration

- MTP n=2
- temperature=1.0
- top_p=0.95
- up to 3 attempts per scenario
- scenario counted correct if any attempt passes
- evaluated through LM Studio/local setup

## Key Observations

1. **ToolCall-15: 100/100** — The model has perfect tool-calling ability in controlled tests
2. **BugFind-15: 71/100** — Strong debugging capability, room for improvement in repair
3. **HermesAgent-20: 64/100** — Agent workflow is the weakest area
4. **InstructFollow-15: 93/100** — Excellent instruction following

## Implications for Tiny Mite

Since the model already has perfect tool-calling (100/100), Tiny Mite's value is NOT in basic tool execution.

Tiny Mite's potential value lies in:
1. **Improving agent workflow** (HermesAgent-20: 64/100 → target higher)
2. **Enabling multi-turn autonomous repair** (not measured in published benchmarks)
3. **Providing context management** for long-running tasks
4. **Security enforcement** during autonomous operation
5. **Bounded execution** with loop detection and cancellation
6. **Tool-call recovery** when malformed calls occur

## Phase 11 Research Question

> "How much additional autonomous task-completion capability does Tiny Mite provide
> on top of an already strong 4B agentic coding model?"

The metric is **autonomous project success rate** — a task only passes if the
resulting project actually satisfies its validation tests (not the model's text answer).

## Benchmark Configuration (Tiny Mite Phase 11)

| Parameter | Value |
|-----------|-------|
| Model | qwopus3.5-4b-coder-mtp |
| Provider | LM Studio (http://localhost:1234/v1) |
| Temperature | 0.7 |
| Max tokens | 2048 |
| Max iterations | 8 |
| Max model calls | 8 |
| Max tool calls | 32 |
| Max failures | 5 |
| Context budget | 8192 tokens |
| Sandbox | /tmp, current directory |

## Note

These published numbers are EXTERNAL BASELINE DATA from the model card.
They are NOT Tiny Mite benchmark results.
Phase 11 must establish its own reproducible measurements.