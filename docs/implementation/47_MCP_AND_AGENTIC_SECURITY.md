# 47 — MCP and Agentic Security

MCP is an interoperability layer, not Tiny Mite's security boundary.

The 2026-07-28 MCP specification introduced a stateless protocol core, multi-round-trip requests, cacheable list results, authorization hardening, and an extensions framework. Tiny Mite should pin a supported version and translate MCP capabilities into its own Tool contract. citeturn0search8

Every MCP tool must still pass Tiny Mite's permission engine.

OWASP's agentic security guidance identifies reasoning, memory, tools, identity, human oversight, and multi-agent interaction as important attack surfaces. citeturn0search3

Required controls:

```text
prompt injection
 → untrusted-data labeling + external authorization

memory poisoning
 → provenance + trust + confirmation

tool abuse
 → capability policy + sandbox

excessive autonomy
 → graduated autonomy + approvals

data leakage
 → local-first + redaction + provider policy

infinite loops
 → step/time/tool budgets
```

No agent can grant itself a capability. No retrieved document can alter policy. No tool output can authorize another tool.
