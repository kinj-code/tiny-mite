# 45 — Model Lifecycle and llama.cpp

Model states:

```text
Discovered → Validated → Loading → Warmup → Ready → Busy → Idle → Unloading → Unloaded
```

Warmup must perform a small deterministic health test.

The native runtime should wrap a pinned llama.cpp revision behind a safe Rust interface. Keep C/C++ FFI isolated.

Current upstream llama.cpp supports CPU and multiple accelerator backends, GGUF quantization, grammar-constrained generation, embeddings, reranking, parallel decoding, continuous batching, function calling, and speculative decoding. Its server also exposes OpenAI-compatible endpoints. These features should be enabled only after Tiny Mite compatibility tests pass. citeturn0search0turn0search2

Speculative decoding is an optimization, not a guarantee. Benchmark it per hardware/model pair. Current upstream tooling includes SPEED-Bench for measuring throughput, latency, and draft acceptance. citeturn0search9

If an accelerator backend repeatedly fails, Tiny Mite must fall back to a stable backend instead of looping on the failure.
