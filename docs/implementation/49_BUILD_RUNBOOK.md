# 49 — Build Runbook

## Fresh agent startup

1. Open the TinyMite folder as the workspace.
2. Read `AGENTS.md`.
3. Read `BUILD_MANIFEST.md`.
4. Read `docs/00_MASTER_SPECIFICATION.md`.
5. Select the next dependency-safe task.
6. Read only relevant documentation.
7. Inspect existing code.
8. Implement.
9. Compile/typecheck.
10. Test.
11. Security-test where applicable.
12. Update documentation and manifest.

## Never

- paste the entire documentation into a prompt;
- assume a task is complete because code exists;
- silently change architecture;
- silently send local data to a cloud provider;
- grant an agent permissions from model output;
- execute raw model-generated shell commands without policy enforcement.

## Completion report

Every coding session ends with:

```text
Completed
Changed files
Tests
Verification evidence
Security notes
Performance notes
Blockers
Next task
```
