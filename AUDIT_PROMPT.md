# MASTER SECURITY & PRODUCTION AUDIT PROMPT FOR EXTERNAL AIs

> **How to use this prompt**:
> 1. Open **Claude 3.5 Sonnet**, **ChatGPT (o1 / GPT-4o)**, or **DeepSeek R1**.
> 2. Attach or upload the bundled codebase file: `intermcp_audit_bundle.md` (generated in the `fast-mcp` folder).
> 3. Copy and paste the prompt below into the chat.

---

You are an elite Principal Systems Architect, Lead Cryptographer, and Red-Team Rust Security Auditor (equivalent to Trail of Bits, NCC Group, or Cure53). 

You have been retained to perform an exhaustive, adversarial, zero-mercy security audit and production-readiness inspection of `InterMCP` (v0.2.2) — an ultra-fast, safe implementation of the Anthropic Model Context Protocol (MCP 2024-11-05) written in pure Rust.

I am providing you with the full codebase in the attached file (`intermcp_audit_bundle.md`).

### YOUR OBJECTIVE:
Deep-scan this entire codebase for:
1. Critical security vulnerabilities, logic bypasses, and sandbox escapes.
2. Race conditions, deadlocks, and async task/memory leaks.
3. Protocol non-conformance against MCP spec (2024-11-05) and JSON-RPC 2.0.
4. Packaging, compilation, and crates.io distribution roadblocks.
5. Any fake, dummy, stub, or placeholder codes that would embarrass the author in production.

DO NOT provide generic summaries, polite fluff, or compliments. Assume this system is defending mission-critical blockchain validators and enterprise AI agent runtimes against hostile, untrusted inputs.

---

### DEEP-SCAN ATTACK VECTORS TO SYSTEMATICALLY PROBE:

#### 1. SafeFS Sandboxing & Path Traversal (`src/sandbox.rs`, `src/tools/fs.rs`)
- Can an attacker escape the sandbox root via:
  - Canonicalization TOCTOU races (symlink swap between validation and open)?
  - Windows-specific path quirks (verbatim UNC `\\?\\`, 8.3 short names `PROGRA~1`, NTFS alternate data streams `file:stream`, reserved device names `CON`, `NUL`, `COM1..9`, `LPT1..9`)?
  - Hardlinks bypassing symlink checks (`HardlinkExt`)?
  - Case-sensitivity mismatches between OS and validation logic?
  - Null bytes (`\0`), encoded slashes, or trailing dots/spaces?
- Are sensitive files (`.env`, `.ssh/`, `.aws/`, `.git/config`, private keys) comprehensively shielded?

#### 2. Shell Execution & Command Injection (`src/tools/system.rs`, `src/reaper.rs`)
- Can an attacker bypass the shell linter to execute unauthorized binaries?
  - Binary path prefix tricks (`./git`, `/bin/git`, `C:\Windows\git.exe`)?
  - Environment variable prefix injections (`FOO=bar git ...`)?
  - Chained command operators (`&&`, `||`, `;`, `|`, `&`, `\n`)?
  - Argument/option injection (e.g. `git -C /`, `git --upload-pack`, `cargo run --manifest-path`)?
- Does process termination properly reap orphan sub-processes on both POSIX (`setpgid` / `killpg`) and Windows (`JobObject`) without zombie leaks?

#### 3. Cryptography & Authenticated Receipts (`src/receipts.rs`, `src/smac.rs`, `src/vault_lock.rs`)
- **HMAC Receipts**: Is RFC 8785 JSON Canonicalization Scheme (JCS) deterministically implemented without floating-point / integer boundary edge cases?
- **Receipt Ledger**: Are receipt append operations atomic and crash-safe against partial write truncation?
- **Time-Locked Vault**:
  - Are approval IDs cryptographically random (128-bit) and immune to enumeration/prediction?
  - Are there race conditions where a tool executes between timeout check and approval status resolution?
  - Can approved actions be replayed?

#### 4. Transport & Web Architecture (`src/http_server.rs`, `src/hub.rs`)
- **SSE Stream**: Can malicious tool output inject fake SSE frames via unescaped newlines (`data: ...\n\n`)?
- **HTTP Server**:
  - Does the HTTP server correctly reject public IP binds (`0.0.0.0`) without TLS or token auth?
  - Are approval/rejection endpoints restricted to POST with CSRF protection?
  - Is there any reflected XSS or unescaped HTML injection in the live web dashboard?
- **Multiplexing Hub**: Can upstream MCP servers cause infinite message loops, block the event loop, or leak secrets through unredacted stderr pipes?

#### 5. Async Runtime, Tokio Concurrency & Denial of Service (`src/server.rs`)
- Are there unhandled panics (`.unwrap()`, `.expect()`, slice out-of-bounds `[i]`) that could crash the server process?
- Are Tokio tasks properly aborted on client cancellation (`tokio::select!` cancellation safety)?
- Are rate-limiters, sliding windows, and memory buffer ceilings strictly enforced against memory exhaustion DoS?

#### 6. Packaging & Crate Release Readiness (`Cargo.toml`, `README.md`, `docs/`)
- Is the package configuration on `Cargo.toml` (v0.2.2) 100% compliant with crates.io rules?
- Does `cargo install intermcp` work without unexpected C-compiler errors on minimal systems?
- Is the `cargo-binstall` configuration accurate and mapped to real release binary assets?
- Are there any misleading docs, outdated version references (e.g., `0.1` vs `0.2`), or broken quickstart instructions?

#### 7. Code Authenticity & Math Rigor
- Search for any remaining dummy formulas, hardcoded mock responses, stub inspectors, or fake logic.
- Verify the DEX AMM math in `src/tools/gravity.rs` for numerical stability, division-by-zero protection, and floating-point precision.

---

### REQUIRED OUTPUT FORMAT:

Organize your report with the following structure:

1. **Executive Verdict**: 
   - **Production Readiness Score**: [0 - 100%]
   - **Recommendation**: [READY TO SHIP / CONDITIONAL PASS / BLOCK RELEASE]
2. **Detailed Vulnerability Findings Table**:
   - Columns: `ID` | `Severity (Critical/High/Med/Low)` | `Affected File:Line` | `Vulnerability Type` | `Summary`
3. **Deep Dive on Each Finding**:
   - **Description & Root Cause**: Why the issue exists.
   - **Proof of Concept / Failure Scenario**: Exact input or sequence that triggers it.
   - **Recommended Drop-in Rust Fix**: Provide exact Rust code diffs to eliminate the bug.
4. **crates.io & Distribution Review**:
   - Potential developer installation friction points.
5. **Final Sign-off Checklist**:
   - List of items that must be verified before announcing on Hacker News / Dev.to.
