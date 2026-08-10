# 42 — Event and Task Schemas

Canonical event envelope:

```json
{
  "id": "evt_...",
  "version": 1,
  "type": "task.created",
  "timestamp": "RFC3339",
  "correlation_id": "task_...",
  "causation_id": null,
  "source": "request-gateway",
  "priority": "normal",
  "payload": {},
  "security": {
    "subject": "user",
    "scope": "project"
  }
}
```

## Task state machine

```text
NEW
 → CLASSIFYING
 → PLANNING
 → CONTEXT_PREPARING
 → EXECUTING
 → VERIFYING
 → REFLECTING
 → MEMORY_UPDATE
 → COMPLETE
```

Repair path:

```text
VERIFYING
 → REPAIRING
 → VERIFYING
```

Any active state may become `CANCELLED` or `BLOCKED`.

`COMPLETE` requires verification evidence for artifact-changing tasks.
