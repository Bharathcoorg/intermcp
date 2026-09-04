# InterMCP Security Model & Threat Architecture

This document describes the security architecture, trust boundaries, guarantees, and known limitations of the **InterMCP** runtime.

---

## 1. What Is Protected

InterMCP enforces multiple defense-in-depth layers around Model Context Protocol (MCP) tool execution:

- **SafeFS Path Containment**:
  - Filesystem access through `fs_*` tools is restricted to declared allowed root directories.
  - Mitigates path traversal via canonicalization (`dunce::canonicalize`), symlink escape checks, and TOCTOU swap detection.
  - Explicitly blocks NTFS reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`), 8.3 short names (`~`), and verbatim UNC prefixes (`\\?\`).
  - Secret shield intercepts access attempts to known credential files (`.env`, `id_rsa`, `.pem`, `credentials.json`, `.npmrc`).

- **Secret Masking & Log Redaction**:
  - Tool outputs are scanned using an Aho-Corasick automaton against sensitive environment variables (`API_KEY`, `SECRET`, `TOKEN`, etc.) and replaced with `[REDACTED_BY_INTERMCP]`.
  - Structured tracing and debug logging use `redact_for_log` to mask Bearer tokens, `Authorization:` headers, and API keys.

- **Data-Flow Taint Tracking**:
  - Enforces confidentiality labels (`Public`, `Internal`, `Confidential`, `Untrusted`) across tool inputs and outputs.
  - Prevents untrusted external data (e.g. from web search or community MCP tools) from flowing directly into privileged sinks (like `system_run_command`) without human supervisor approval.

- **Cryptographic Execution Receipts (ADR 001)**:
  - Generates tamper-evident execution receipts serialized via RFC 8785 JSON Canonicalization Scheme (JCS) and signed with HMAC-SHA256 digital signatures.
  - Provides verifiable cryptographic provenance and offline chain verification (`intermcp verify-receipts`).

- **Upstream Supply-Chain Firewall (ADR 002)**:
  - Fingerprints external upstream tool descriptions and input schemas using SHA-256.
  - Detects dynamic schema mutations and prompt-injection drift at runtime, immediately quarantining suspicious upstream servers.

- **Time-Locked Supervisor Vault**:
  - High-risk operations (e.g. destructive commands, publishing actions) can be placed in an approval queue (`--time-lock`) requiring explicit human authorization before execution.

---

## 2. What Is NOT Protected

Developers must understand the boundaries of the runtime:

- **WASM Module Inspector (Not Bytecode Sandbox)**:
  - `src/wasm.rs` implements `WasmInspector`, which validates WebAssembly v1 headers, versions, declared memory limits, and exports.
  - It does **not** execute WebAssembly bytecode in an isolated VM (e.g. via Wasmtime or Wasmer).
- **HTTP Loopback Plaintext**:
  - Loopback addresses (`127.0.0.1`, `::1`) permit plaintext HTTP for local development. Public binds (`0.0.0.0`, `::`) require TLS termination via native rustls (`--tls-cert` and `--tls-key`).
- **Safe-Shell Execution Model**:
  - `system_run_command` is allowlist- and heuristic-linter-based; it is **not** a deny-by-default or hardware-isolated container sandbox.
- **Host Process Memory Access**:
  - In-process native Rust tools share the address space of the InterMCP binary.

---

## 3. Trust Boundaries

- **Stdio Transport**:
  - Assumed to run as a child process of a trusted parent process (such as Claude Desktop, Cursor, or an authorized local IDE). Communication over stdio is unauthenticated under the parent-child trust model.
- **HTTP / SSE Remote Transport**:
  - External network boundary. Mandatory Bearer token authentication (`--token`) is enforced by default (`default-deny`).
  - Public interface binds (`0.0.0.0` or `::`) require TLS termination configuration (`tls_cert` and `tls_key`) via native rustls in-process TLS.
- **Upstream MCP Servers (Hub)**:
  - Child processes spawned by `intermcp hub` are treated as untrusted supply-chain dependencies. Environment variables are scrubbed, schemas are pinned, and tool names are quarantined to dedicated namespaces.

---

## 4. Known Limitations

- **RFC 8785 JCS Dependency**:
  - Deterministic JSON canonicalization relies on `serde_jcs = "0.1"`.
- **Operating-System Process Groups**:
  - Process cleanup on Windows uses `ProcessJobGroup` (Windows Job Objects with `kill_on_drop` semantics). On Unix platforms, standard POSIX process groups are used.
- **Shell Linter Heuristics**:
  - The shell command analyzer uses tokenization and pattern interception. Extremely obfuscated shell payloads (such as multi-stage nested variable evaluation) should be restricted by using strict binary allowlists (`--allow-bin`) or the Time-Locked Vault.

---

## 5. Vulnerability Reporting

If you discover a security vulnerability within InterMCP, please review our reporting instructions in [SECURITY.md](../SECURITY.md).
