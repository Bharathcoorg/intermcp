# InterMCP Security Boundaries

This document defines the intended trust boundaries for the InterMCP runtime. It is
an engineering contract for future changes, not a claim that any single layer is a
complete sandbox.

## Request path

MCP client -> protocol validation -> policy -> guardrails -> taint/approval -> tool -> result redaction -> receipt/recording -> client

Each layer has a distinct responsibility:

- Protocol validates JSON-RPC framing, method parameters, and negotiated MCP version.
- Policy decides whether a requested operation is permitted by configured rules.
- Guardrails limit repeated calls, bounded recent-pattern repetition/cycles, resource consumption, and runaway agent behavior.
- Taint tracks structured provenance/confidentiality labels and blocks unsafe flows to privileged sinks.
- Approval vault introduces an explicit human decision for configured high-risk actions.
- Tool sandbox applies the concrete filesystem/process/network restrictions of the operation.
- Redaction removes known secrets before tool results are returned to the model/client.
- Receipts/recording provide provenance and debugging evidence; they are not an authorization mechanism.

## Filesystem boundary

Policy and SafeFS checks must be performed before every filesystem operation.
Path validation must not be treated as a substitute for safe file opening semantics.
Future privileged filesystem operations should prefer directory/file handles and
no-follow semantics where the platform permits them, reducing TOCTOU exposure.

Policy path deny rules support dependency-free wildcard matching. Configuration using
patterns such as **/*.pem is intended to match nested files.

## Shell boundary

Command-pattern filtering is defense-in-depth. It must not be described as a
complete shell sandbox. Privileged command execution should run with a constrained
environment, explicit working directory, controlled inherited descriptors, bounded
output, and platform process/resource restrictions.

## Approval boundary

Pending approval records must contain only safe-to-display arguments. The original
argument object is not a UI/audit payload. Approval records expose a SHA-256
argument fingerprint so a supervisor can correlate the approval with the exact
request without displaying credentials.

## WASM boundary

InterMCP's current WASM component is an inspector/validator. It does not execute
WASM bytecode. Its memory and execution configuration must not be interpreted as
proof of runtime isolation.

If execution is added later, it must use an actual WASM runtime with enforced
memory, fuel/time, import capabilities, cancellation, and host-call restrictions.

## Receipts

Receipt hash chains provide tamper evidence for modifications inside the chain.
They do not independently prove that the file has not been truncated at its tail.
Long-lived audit deployments should maintain an external checkpoint or other
out-of-band durable sequence/hash reference.

## Remote HTTP/SSE

Public HTTP binds require TLS. Bearer authentication, connection limits, IP rate
limits, request-size limits, and SSE session limits are defense-in-depth controls.
The transport should remain fail-closed when mandatory security configuration is
missing.

## SDK contract

All language SDKs should converge on the same lifecycle and semantics:
startup,
initialize/version negotiation, request timeouts, process failure handling,
tool/resource/prompt operations, cancellation where supported, and deterministic
shutdown. The bundled Node, Python, Go, and PHP clients now expose the same core tool/resource/prompt lifecycle and bound blocking stdio response reads.

## Release discipline

Security or protocol changes should be committed independently and reviewed as
feature-sized changes. This branch intentionally does not publish packages or
create a release.
