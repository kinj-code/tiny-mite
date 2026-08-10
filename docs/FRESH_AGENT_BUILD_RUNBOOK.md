# Tiny Mite — Fresh Agent Build Runbook

A new coding agent starting from scratch must follow this sequence.

## Prerequisites

- Rust toolchain (stable, edition 2024)
- Node.js 20+ (for desktop frontend)
- Git
- Linux/macOS/Windows (testing on Linux)

## Bootstrap sequence

### Step 1: Read the governance documents

```bash
cat AGENTS.md
cat BUILD_MANIFEST.md
cat docs/implementation/50_IMPLEMENTATION_CONTRACTS.md
```

### Step 2: Verify the workspace compiles

```bash
cargo check --workspace
```

Expected: 0 errors. Warnings from `tiny-mite-llama-cpp` and `tiny-mite-agents` are acceptable.

### Step 3: Verify all tests pass

```bash
cargo test --workspace
```

Expected: 301 tests passing, 0 failures across 9 crates.

### Step 4: Verify formatting

```bash
cargo fmt --all -- --check
```

Expected: Clean, no diff.

### Step 5: Verify the desktop frontend builds

```bash
cd apps/desktop && npm install && npm run build
```

Expected: TypeScript compiles, Vite builds successfully.

### Step 6: Determine the next task

1. Open `BUILD_MANIFEST.md`
2. Find the next `[ ]` item whose dependencies are satisfied
3. Read the subsystem document for that item in `docs/implementation/`
4. Inspect the existing source code
5. Form an implementation plan
6. Implement the smallest coherent change
7. Run `cargo test --workspace` after each increment

### Step 7: After implementing

1. Update `BUILD_MANIFEST.md` — mark completed items `[x]`
2. Run `cargo fmt --all`
3. Run `cargo check --workspace`
4. Run `cargo test --workspace`
5. If tests pass, the change is verified

## Important constraints

- Do NOT touch `tiny-mite-llama-cpp` unless specifically tasked
- Do NOT reintroduce the llama.cpp ABI problem
- Keep implementations provider-independent
- All intelligence components must work without an LLM call
- Preserve the offline-first architecture
- Never store secrets in source code
- Never execute model-generated shell commands directly

## Quick reference

| Command | Purpose |
|---------|---------|
| `cargo check --workspace` | Verify compilation |
| `cargo test --workspace` | Run all tests |
| `cargo fmt --all -- --check` | Verify formatting |
| `cargo clippy --workspace` | Lint check |
| `cd apps/desktop && npm run build` | Build desktop UI |
| `cargo run --example smoke_test` | Run llama.cpp smoke test (requires GGUF) |
| `./scripts/package_linux.sh 0.1.0` | Create Linux package |