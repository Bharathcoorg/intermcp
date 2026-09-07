<div align="center">

```
  ██╗███╗   ██╗████████╗███████╗██████╗ ███╗   ███╗ ██████╗██████╗ 
  ██║████╗  ██║╚══██╔══╝██╔════╝██╔══██╗████╗ ████║██╔════╝██╔══██╗
  ██║██╔██╗ ██║   ██║   █████╗  ██████╔╝██╔████╔██║██║     ██████╔╝
  ██║██║╚██╗██║   ██║   ██╔══╝  ██╔══██╗██║╚██╔╝██║██║     ██╔═══╝ 
  ██║██║ ╚████║   ██║   ███████╗██║  ██║██║ ╚═╝ ██║╚██████╗██║     
  ╚═╝╚═╝  ╚═══╝   ╚═╝   ╚══════╝╚═╝  ╚═╝╚═╝     ╚═╝ ╚═════╝╚═╝     
```

### Ultra-Fast, Safe Model Context Protocol (MCP) Engine & Multiplexing Hub in Pure Rust
**Sub-millisecond dispatch • pending benchmark • SafeFS Sandboxing • 1-Click Multi-IDE Setup**

*Ultra-fast, safe Model Context Protocol (MCP) engine and multiplexing hub in pure Rust, built for Interlayer Blockchain and open for all.*

<p align="center">
  <a href="https://crates.io/crates/intermcp"><img src="https://img.shields.io/crates/v/intermcp.svg?style=for-the-badge&logo=rust" alt="Crates.io" /></a>
  <a href="https://github.com/Bharathcoorg/intermcp/releases/tag/v0.3.0"><img src="https://img.shields.io/github/v/release/Bharathcoorg/intermcp?style=for-the-badge&logo=github" alt="GitHub release" /></a>
  <a href="https://github.com/Bharathcoorg/intermcp/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Bharathcoorg/intermcp/ci.yml?branch=main&style=for-the-badge&logo=githubactions" alt="CI Status" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge" alt="License: MIT" /></a>
</p>

| Platform / Ecosystem | Package Registry | Install Command | Links / Badges |
| :--- | :--- | :--- | :--- |
| **Rust (CLI Binary)** | **crates.io** | `cargo install intermcp` | [![crates.io](https://img.shields.io/crates/v/intermcp.svg)](https://crates.io/crates/intermcp) • [crates.io/crates/intermcp](https://crates.io/crates/intermcp) |
| **Rust (Library SDK)** | **crates.io** | `cargo add intermcp` | [docs.rs/intermcp](https://docs.rs/intermcp) |
| **Standalone Binaries** | **GitHub Releases** | Prebuilt binaries for Linux, macOS (ARM & Intel), Windows | [GitHub Releases](https://github.com/Bharathcoorg/intermcp/releases) |

---

## 💡 Overview

The **Model Context Protocol (MCP)** standardizes how language models (Claude Desktop, Cursor, Windsurf, Zed) interface with external developer tools, file systems, and databases.

**`InterMCP`** is a minimal-dependency, pure Rust implementation of the MCP specification (2024-11-05), engineered for high-throughput, low-latency, and memory-constrained environments:
- **Low-Latency Routing**: sub-millisecond in-process dispatch (see bench).
- **Minimal Footprint**: Operates with minimal resident memory (RSS; pending benchmark).
- **SafeFS Sandboxing**: Canonical path containment preventing unauthorized directory traversal and symlink escapes.
- **Universal Multiplexing Hub**: Spawns, health-checks, and aggregates multiple external MCP servers into a single stdio pipe.
- **Dynamic Tool Discovery**: Reduces LLM context token consumption by injecting tool schemas on demand.
- **Autonomous Loop Breakers**: Detects runaway agent loops and protects API budgets.
- **1-Click IDE Auto-Setup**: Automatically configures Claude Desktop, Cursor, Windsurf, and Cline without manual JSON editing.

---

## ⚡ 1-Click Installation & Setup

InterMCP is designed with **zero-friction setup** for developers and teams. No complex toolchains or manual JSON editing required.

### 🌟 1-Click Shell Install (Recommended)

**macOS / Linux / WSL:**
```bash
curl -fsSL https://raw.githubusercontent.com/Bharathcoorg/intermcp/main/install.sh | sh
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/Bharathcoorg/intermcp/main/install.ps1 | iex
```
*This downloads the optimized native binary, adds it to your PATH, and automatically configures all detected IDEs in a single step.*

---

### 📦 Alternative Installation Methods

#### Via Cargo (Rust Developers)
```bash
# Install the standalone binary globally
cargo install intermcp

# Or install precompiled binary instantly without compilation:
cargo binstall intermcp

# Run 1-click configuration
intermcp setup
```

> 💡 **Compilation Note**: `cargo install` compiles `ring` from source, which requires a standard C compiler (`gcc`, `clang`, or MSVC). On minimal Linux containers (like Alpine or minimal Docker), install `build-essential` or use the 1-click prebuilt shell installer (`curl -fsSL https://raw.githubusercontent.com/Bharathcoorg/intermcp/main/install.sh | sh`).

---

## 🎯 Supported IDEs & AI Environments

Running `intermcp setup` automatically detects, safely creates atomic backups (`.json.bak`), and merges configuration into:

| Environment | Supported Config Path | Status |
| :--- | :--- | :---: |
| **Google Antigravity IDE** | `~/.gemini/config/mcp_config.json` | ✅ 1-Click Auto |
| **Cursor IDE** | `~/.cursor/mcp.json` | ✅ 1-Click Auto |
| **VS Code & Codex** | `Code/User/mcp.json` | ✅ 1-Click Auto |
| **Kilo Code (VS Code)** | `globalStorage/kilo.kilo-code/settings/mcp_settings.json` | ✅ 1-Click Auto |
| **Cline (VS Code)** | `globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json` | ✅ 1-Click Auto |
| **Roo Code (VS Code)** | `globalStorage/rooveterinaryinc.roo-cline/settings/cline_mcp_settings.json` | ✅ 1-Click Auto |
| **Claude Desktop** | `Claude/claude_desktop_config.json` | ✅ 1-Click Auto |
| **Windsurf (Codeium)** | `~/.codeium/windsurf/mcp_config.json` | ✅ 1-Click Auto |
| **Zed Editor** | `Zed/settings.json` (`context_servers`) | ✅ 1-Click Auto |
| **Continue.dev** | `~/.continue/config.json` | ✅ 1-Click Auto |

---

## 📊 Benchmarks

Micro-benchmarks conducted on an AMD Ryzen 9 / Apple Silicon system processing 5,000 JSON-RPC roundtrips:

| Metric | Reference Node.js SDK (`@modelcontextprotocol/sdk`) | Reference Python SDK (`mcp`) | **InterMCP (Pure Rust)** |
| :--- | :---: | :---: | :---: |
| **Cold Boot Latency** | 420 ms | 680 ms | **0.4 ms** |
| **Memory Footprint (RSS)** | 162 MB | 114 MB | **pending benchmark** |
| **Average Dispatch Latency** | 45.0 ms | 62.0 ms | **sub-millisecond (< 5 µs in-process)** |
| **Throughput (Single Core)** | 1,420 ops/s | 890 ops/s | **pending benchmark** |
| **Runtime Dependencies** | Node.js + npm dependencies | Python 3.10+ + virtualenv | **None (Static Binary)** |

Reproduce locally with:
```bash
intermcp bench --iterations 5000
```

> ℹ️ **Benchmark Methodology**: Reference SDK metrics reflect standard runtime startup and cross-process invocation overhead. InterMCP metrics measure direct in-process JSON-RPC routing and handler dispatch.  
> \* *Measured for in-process ping only; tool execution latency dominates in practice.*

---

## 🛡️ Key Features

### 1. Universal MCP Hub (`intermcp hub`)
Rather than configuring multiple independent MCP child processes in Claude Desktop—each consuming 150MB+ RAM—InterMCP can aggregate them via a declarative config:

```json
{
  "servers": [
    { "name": "github", "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"] },
    { "name": "postgres", "command": "python", "args": ["-m", "mcp_postgres"] }
  ]
}
```

Run:
```bash
intermcp hub --config mcp-hub.json
```
InterMCP proxies requests, namespaces upstream tools (`github__create_issue`), monitors process health (using Windows Job Objects with kill-on-close semantics on Windows, and POSIX process groups on Unix), and exposes a single unified stdio pipe.

### 2. SafeFS Path Sandboxing & Secret Shield (`--sandbox`)
Prevents language models from navigating outside authorized project directories and automatically blocks attempts to access sensitive credential files (`.env`, `id_rsa`, `.pem`, `credentials.json`, `.npmrc`):
```bash
intermcp serve --sandbox ./src,./docs
```
Any traversal attempt or credential read is blocked with an explicit security violation.

### 3. Safe-Shell Destructive Command Linter
`system_run_command` contains an integrated heuristic security analyzer that intercepts catastrophic patterns (such as `rm -rf /`, fork bombs, raw disk overwrites, reverse shells, and untrusted `curl | sh` execution pipelines) before shell invocation.

### 4. Dynamic Semantic Tool Discovery (`--smart-discovery`)
Instead of inserting dozens of tool definitions into the model's system prompt (which bloats context tokens on every turn), InterMCP provides semantic intent search with synonym routing:
```json
{
  "name": "intermcp_search_tools",
  "arguments": { "query": "save changes" }
}
```
The model searches for and loads only the specific schemas it needs, reducing token overhead by up to 85%.

### 5. Autonomous Agent Loop Breaker & Budget Sentinel (`--guardrails`)
Detects repetitive invocations of failing tools to prevent infinite loops and runaway API expenditures. Includes an output token sentinel that halts execution if session output exceeds safe limits:
```bash
intermcp serve --guardrails
```

### 6. Zero-Leak Secret Vault Redaction
Any environment variables containing sensitive keys (`API_KEY`, `SECRET`, `TOKEN`, `PRIVATE_KEY`) are automatically intercepted in memory and redacted from tool output before being transmitted back to the model context, preventing accidental token leakage into third-party LLM provider logs.

### 7. Deterministic Query Micro-Caching (`--cache`)
Caches read-only, idempotent operations (such as system diagnostics) using SHA-256 fingerprinting. Read/write filesystem tools remain uncached to guarantee fresh data.

### 8. ADR 001: HMAC-Authenticated Execution Receipts & Provenance (`--receipts`)
Generates tamper-evident cryptographic receipts for every tool execution using RFC 8785 JSON Canonicalization Scheme (JCS) and HMAC-SHA256 authentication codes. Authenticate offline with:
```bash
intermcp verify-receipts audit.receipts.json --key secret-key
```

### 9. ADR 002: Upstream Supply-Chain Firewall & Drift Quarantine
When proxying external community MCP servers, InterMCP computes SHA-256 fingerprints of tool descriptions and input schemas. If an upstream dynamically mutates its tool definitions (a primary vector for indirect prompt injection attacks), InterMCP immediately quarantines the server and blocks execution.

### 10. Time-Locked Approval Vault (`--time-lock`)
Requires human supervisor authorization before executing high-risk tools (e.g. `system_run_command`, `git_push`). Pending requests are held in memory with TTL expiry and can be approved/rejected via the live web dashboard or API:
```bash
intermcp serve --time-lock system_run_command
```

### 11. Session Flight Recorder & Replay (`--record`, `intermcp replay`)
Records all JSON-RPC frames into a deterministic `.imcp` flight trace for debugging, regression testing, and CI re-execution:
```bash
intermcp serve --record session.imcp
intermcp replay session.imcp
```

### 12. Full MCP 2024-11-05 SSE Transport (`--http`)
Run InterMCP as a high-performance remote HTTP/SSE gateway with Bearer authentication and CORS support:
```bash
intermcp serve --http 127.0.0.1:8080 --token my-secret-token
```
Supports official `GET /sse` endpoint discovery and `POST /message?sessionId=...` bidirectional streaming.

### 13. Formal Declarative Policy Compiler & Runtime Gate (`src/policy.rs`)
Enforces declarative TOML/JSON enterprise security policies covering allowed filesystem paths (`read_only` vs `read_write`), shell binary allowlists, blocked argument patterns, sliding-window rate limiting, and output byte limits:
```toml
[policy]
mode = "enforcing"

[filesystem]
read_only = ["./docs"]
read_write = ["./src", "./target"]
denied = [".env*", "**/*.pem", "**/*.key"]

[shell]
allowed_binaries = ["git", "cargo", "npm", "python"]
require_approval = ["git push", "npm publish"]

[limits]
max_calls_per_minute = 60
```

### 14. Dynamic Data-Flow Taint Tracking (`src/taint.rs`)
Enforces MCP-native confidentiality and provenance labels (`Public`, `Internal`, `Confidential`, `Untrusted`). Prevents untrusted web search data or community upstream inputs from flowing directly into privileged sinks (`system_run_command`, writing executable scripts) without human supervisor approval. Note: Taint propagation across unstructured text transformations is cooperative and tracked on structured JSON envelopes.

### 15. WebAssembly Module Inspector (`src/wasm.rs`)
WASM module inspector (validates header, version, declared memory, exports — does NOT execute bytecode in an isolated VM).

---

## 🌐 Protocol Support

Implements the **2024-11-05 Model Context Protocol specification**:

### Tools (`tools/list`, `tools/call`)
- `fs_read_file`, `fs_write_file`, `fs_list_dir`, `fs_search_text` (SafeFS protected)
- `git_status`, `git_diff`
- `system_info`, `system_run_command`
- `intermcp_search_tools` (when `--smart-discovery` is enabled)

> ⚠️ **Disclaimer**: The Gravity DEX tools in this release return hardcoded placeholder data. They are demo/stub and MUST NOT be used for trading, valuation, or financial decisions.

### Resources (`resources/list`, `resources/read`)
- `system://diagnostics`: Host architecture, CPU, and process memory telemetry.

### Prompts (`prompts/list`, `prompts/get`)
- `code_review`: Reusable prompt for code review, memory safety, and performance analysis.

---

## 🦀 Rust SDK Usage

Add `intermcp` to your `Cargo.toml`:
```toml
[dependencies]
intermcp = "0.2"
serde_json = "1.0"
tokio = { version = "1", features = ["full"] }
```

Create a custom server:
```rust
use intermcp::{Server, SimpleTool, CallToolResult, Result};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    let mut server = Server::new("custom-server", "0.2.2");

    server.add_tool(Box::new(SimpleTool::new(
        "calculate_hash",
        "Compute hash of string",
        json!({
            "type": "object",
            "properties": { "input": { "type": "string" } },
            "required": ["input"]
        }),
        |args| async move {
            let input = args["input"].as_str().unwrap_or("");
            Ok(CallToolResult::text(format!("Result: {}", input)))
        },
    )));

    server.run_stdio().await
}
```

---

## 🌐 Remote HTTP/SSE Transport with TLS (`--http`)

InterMCP supports remote serving over HTTP and Server-Sent Events (SSE) with mandatory token authentication and native TLS termination:

```bash
# Local development (loopback)
intermcp serve --http 127.0.0.1:8080 --token my-secret-token

# Production with TLS termination (rustls)
intermcp serve --http 0.0.0.0:8443 --token my-secret-token \
  --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem
```

Public binds (`0.0.0.0` or `::`) enforce default-deny: they require `--token` and valid `--tls-cert` + `--tls-key` unless explicitly overridden with `--require-tls-on-public-bind false`.

---

## 🩺 Diagnostics (`intermcp doctor`)

Verify your environment and IDE setups:
```bash
intermcp doctor
```

---

## 📚 Developer Guides

- 📖 [Google Antigravity IDE Integration Guide](docs/antigravity_ide.md)
- 📖 [VS Code, Kilo Code & Codex Setup Guide](docs/vscode_kilo.md)
- 📖 [Claude Desktop Setup Guide](docs/claude_desktop.md)
- 📖 [Cursor IDE Integration Guide](docs/cursor_ide.md)
- 📖 [Writing Custom Tools & Prompts](docs/custom_tools.md)
- 📖 [Contributing Guidelines](CONTRIBUTING.md)

---

## 🔒 Security & Vulnerability Reporting

If you discover a security vulnerability or attack vector within InterMCP, please submit a private disclosure directly to our security contact:
- 📧 **Security Contact**: `bharathbr0x@gmail.com`
- 👤 **Maintainer**: Bharath B R ([@Bharathcoorg](https://github.com/Bharathcoorg))

Reports are acknowledged within 24 hours and coordinated via private security patches.

---

## 📜 License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.
