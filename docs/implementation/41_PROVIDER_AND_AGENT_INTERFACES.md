# 41 — Provider and Agent Interfaces

## Provider

Conceptual interface:

```text
identity()
capabilities()
discover_models()
health()
load(model, options)
unload(session)
generate(session, request)
stream(session, request, sink)
cancel(request_id)
tokenize(session, text)
```

Optional capabilities:

```text
embeddings
reranking
vision
audio
grammar
structured_output
tool_calling
```

Provider-specific options must never leak into the core orchestration layer.

## Agent

Agent input:

```text
task_id
role
goal
constraints
context_refs
capability_refs
resource_budget
deadline
```

Agent output:

```text
status
artifacts
proposed_actions
evidence
lessons
next_state
```

Agents propose actions. The Tool Gateway decides whether those actions are authorized.
