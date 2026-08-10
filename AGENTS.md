# Tiny Mite — Coding Agent Instructions

You are an implementation agent working inside the Tiny Mite repository.

## 1. Non-negotiable rule

The repository documentation is the engineering source of truth. Do not replace the documented architecture with a simpler architecture merely because the simpler implementation is easier.

## 2. Startup procedure

Before changing code:

1. Read `README.md`.
2. Read `docs/00_MASTER_SPECIFICATION.md`.
3. Read `BUILD_MANIFEST.md`.
4. Identify the next unfinished task whose dependencies are satisfied.
5. Read only the subsystem documents relevant to that task.
6. Inspect the existing implementation and tests.
7. Form an implementation plan.
8. Implement the smallest coherent change that satisfies the specification.

## 3. Context discipline

Do NOT load the entire documentation set into the model context. Retrieve the documents relevant to the current task. Prefer targeted search and summaries.

## 4. Implementation rules

- Do not invent public APIs without checking the architecture.
- Do not duplicate functionality already provided by a core service.
- Do not introduce a dependency without evaluating its footprint, license, maintenance, security, and offline behavior.
- Prefer Rust for privileged/core/runtime functionality.
- Prefer TypeScript/React for desktop presentation.
- Keep inference providers behind a common interface.
- Keep agents behind a common runtime contract.
- Keep tools behind a permission boundary.
- Use structured events rather than tightly coupling modules.
- Use typed schemas for inter-module messages.
- Keep blocking CPU work off UI threads.
- Never store secrets in source code.
- Never execute model-generated shell commands directly without authorization and sandbox policy.
- Never treat model output as trusted input.

## 5. Quality gate

A task is not complete until:

- code compiles;
- relevant unit tests pass;
- integration tests pass where applicable;
- security checks pass;
- acceptance criteria are satisfied;
- errors are handled;
- observability is present;
- documentation is updated when behavior or interfaces changed.

## 6. Failure behavior

If a requirement is genuinely ambiguous:

1. inspect all relevant documents;
2. inspect existing interfaces;
3. check ADRs;
4. choose the least surprising safe behavior only if the ambiguity is minor;
5. otherwise create/update an ADR and stop the affected task for human review.

Do not silently make architecture-changing assumptions.

## 7. Progress tracking

After a verified task, update `BUILD_MANIFEST.md`.

Use:

- `[ ]` not started
- `[~]` in progress
- `[x]` verified complete
- `[!]` blocked
- `[?]` requires design decision

Never mark work complete merely because code was written.

## 8. Testing philosophy

Tests should verify behavior, not implementation trivia. Every important subsystem needs unit, integration, failure, security, and performance coverage appropriate to its risk.

## 9. Autonomous execution

You may continue through dependency-safe tasks without asking for permission after every small operation. Stop for:

- destructive operations;
- credential access;
- external data transmission;
- irreversible migrations;
- security-policy changes;
- architecture conflicts;
- unexplained test failures;
- actions outside the documented project scope.

## 10. Final response after a work session

Report:

- tasks completed;
- files changed;
- tests run;
- performance observations;
- security observations;
- remaining blockers;
- next recommended task.
