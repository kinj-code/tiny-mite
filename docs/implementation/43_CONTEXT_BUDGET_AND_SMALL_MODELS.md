# 43 — Context Budget and Small-Model Strategy

Tiny Mite must not send a 300-page documentation set to a 3B–9B model.

For every candidate context item track:

```text
token_cost
relevance
trust
freshness
task_impact
dependency_criticality
source_quality
```

A baseline score can be:

```text
0.35 relevance
+0.20 task impact
+0.15 trust
+0.10 freshness
+0.10 dependency criticality
+0.10 source quality
```

These weights are tunable and must be benchmarked.

Always reserve space for:

- system/security policy;
- current user constraints;
- task state;
- output/tool-call budget.

Prefer narrow decomposition:

```text
large task
 → small objective
 → relevant evidence
 → structured action
 → deterministic verification
 → next objective
```

The benchmark must compare the same model:

```text
raw model
vs
context-managed
vs
context + retrieval
vs
context + tools
vs
full Tiny Mite
```

Success-adjusted latency and task success matter more than raw tokens/sec.
