# Tiny Mite — Threat Model v1.0

## Overview

This document describes the security boundaries, attack surfaces, threat actors, and defenses for the Tiny Mite Intelligence OS. Tiny Mite is an offline-first, CPU-first intelligence runtime designed to run small (3B–9B) local language models through intelligent orchestration.

## Trust Boundaries

### Boundary 1: Model Output → Intelligence Pipeline
- **Threat**: Malicious or misleading model output corrupting planning, verification, or tool execution
- **Attack vectors**: Prompt injection, adversarial prompts, hallucinated tool calls
- **Defenses**: VerificationEngine, structured output validation, capability-based permissions, audit logging

### Boundary 2: External Content → Context Engine
- **Threat**: Retrieved documents, tool outputs, or imported memories containing prompt injection payloads
- **Attack vectors**: Data poisoning through retrieval, indirect prompt injection through web content, memory poisoning
- **Defenses**: Content sensitivity classification, authority levels, injection pattern detection, provenance tracking

### Boundary 3: Plugin / MCP → Core
- **Threat**: Untrusted third-party plugins gaining unauthorized access to system capabilities
- **Attack vectors**: Plugin privilege escalation, malicious MCP servers, capability token theft
- **Defenses**: CapabilityToken scoping, sandbox isolation, explicit permission grants, ToolGateway authorization

### Boundary 4: Tool Execution → System
- **Threat**: Model-generated shell commands or file operations causing system damage
- **Attack vectors**: Arbitrary command execution, filesystem escape, network exfiltration
- **Defenses**: Sandbox path restrictions, dry-run mode, shell execution denied by default, user approval gateways

## Threat Actors

| Actor | Capability | Motivation |
|-------|-----------|------------|
| Malicious user input | Injection payloads | Manipulate model behavior |
| Poisoned retrieval content | Embedded instructions | Indirect prompt injection |
| Compromised MCP server | Full system access if unguarded | Data theft, RCE |
| Buggy model output | Hallucinated tool calls | Accidental data loss |

## Defense-in-Depth

1. **Capability layer**: CapabilityToken — no implicit permissions
2. **Policy layer**: SecurityPolicy + AccessPolicy — explicit allow/deny per resource
3. **Gateway layer**: ToolGateway — every tool call authorized before execution
4. **Sandbox layer**: SandboxConfig — path restrictions, network deny by default
5. **Audit layer**: AuditLog — every decision recorded for forensics
6. **Validation layer**: OutputValidator — injection detection, size limits
7. **Memory layer**: MemoryPoisoningDefense — provenance tracking, age limits

## Risk Categories

| Risk Level | Requires Approval | Examples |
|-----------|-------------------|----------|
| None | No | Read-only file access within project |
| Low | No | Search, list files |
| Medium | No | Git status, code compilation |
| High | Yes | Write files, shell commands |
| Critical | Yes | Deploy, network access, system modification |

## Security Properties

- All model output treated as untrusted
- All tool output validated before re-ingestion
- All retrieved content classified by sensitivity
- All secrets zeroed on drop, never serialized
- All capabilities explicitly granted, never inherited
- All operations audited with correlation IDs
- Default deny for network and filesystem access

## Version History

- v1.0 — Initial threat model covering Phase 0–5 architecture