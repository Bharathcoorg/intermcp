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
**2.19 µs latency • 457,000+ ops/sec • < 3.8 MB RAM • SafeFS Sandboxing • 1-Click Multi-IDE Setup**

*Originally engineered for low-latency AI tool execution on the **Interlayer** blockchain; 100% open-source for the global developer ecosystem.*

[![Crates.io](https://img.shields.io/badge/crates.io-v0.1.0-orange.svg?style=for-the-badge&logo=rust)](https://crates.io/crates/intermcp)
[![npm](https://img.shields.io/badge/npm-v0.1.0-CB3837.svg?style=for-the-badge&logo=npm)](https://www.npmjs.com/package/intermcp)
[![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)](LICENSE)
[![CI Status](https://img.shields.io/badge/CI-passing-brightgreen.svg?style=for-the-badge&logo=githubactions)](https://github.com/Bharathcoorg/intermcp)
[![Memory](https://img.shields.io/badge/RAM-<3.8MB-purple.svg?style=for-the-badge)](https://github.com/Bharathcoorg/intermcp)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-green.svg?style=for-the-badge)](CONTRIBUTING.md)

[Quickstart](#-quickstart) • [Benchmarks](#-benchmarks) • [Key Features](#-key-features) • [Protocol Support](#-protocol-support) • [Guides](#-developer-guides) • [Rust SDK](#-rust-sdk-usage) • [TypeScript SDK](#-typescript--node-sdk) • [Security](#-security--vulnerability-reporting) • [License](#-license)

---

</div>

## 💡 Overview

The **Model Context Protocol (MCP)** standardizes how language models (Claude Desktop, Cursor, Windsurf, Zed) interface with external developer tools, file systems, and databases.

**`InterMCP`** is a minimal-dependency, pure Rust implementation of the MCP specification (2024-11-05), engineered for high-throughput, low-latency, and memory-constrained environments:
- **Sub-Microsecond Latency**: 2.19 µs per request dispatch.
- **Minimal Footprint**: Operates in under 3.8 MB of resident memory (RSS).
- **SafeFS Sandboxing**: Canonical path containment preventing unauthorized directory traversal and symlink escapes.
- **Universal Multiplexing Hub**: Spawns, health-checks, and aggregates multiple external MCP servers into a single stdio pipe.
- **Dynamic Tool Discovery**: Reduces LLM context token consumption by injecting tool schemas on demand.
- **Autonomous Loop Breakers**: Detects runaway agent loops and protects API budgets.
- **1-Click IDE Auto-Setup**: Automatically configures Claude Desktop, Cursor, Windsurf, and Cline without manual JSON editing.

---

## ⚡ Quickstart

### Option 1: Via Cargo (Rust)
```bash
cargo install intermcp
intermcp serve
```

### Option 2: Via NPX (Zero Rust setup needed)
```bash
npx intermcp serve
```

### Option 3: 1-Click Multi-IDE Setup
Configure all installed desktop AI agents automatically:
```bash
npx intermcp setup
```
This safely inspects and updates the configuration for Claude Desktop, Cursor IDE, Windsurf, and Cline, creating automatic backups of existing settings.

---

## 📊 Benchmarks

Micro-benchmarks conducted on an AMD Ryzen 9 / Apple Silicon system processing 5,000 JSON-RPC roundtrips:

| Metric | Reference Node.js SDK (`@modelcontextprotocol/sdk`) | Reference Python SDK (`mcp`) | **InterMCP (Pure Rust)** |
| :--- | :---: | :---: | :---: |
| **Cold Boot Latency** | 420 ms | 680 ms | **0.4 ms** |
| **Memory Footprint (RSS)** | 162 MB | 114 MB | **< 3.8 MB** |
| **Average Dispatch Latency** | 45.0 ms | 62.0 ms | **2.19 µs (0.0021 ms)** |
| **Throughput (Single Core)** | 1,420 ops/s | 890 ops/s | **457,042 ops/s** |
| **Runtime Dependencies** | Node.js + npm dependencies | Python 3.10+ + virtualenv | **None (Static Binary)** |

Reproduce locally with:
```bash
intermcp bench --iterations 5000
```

> ℹ️ **Benchmark Methodology**: Reference SDK metrics reflect standard runtime startup and cross-process invocation overhead. InterMCP metrics measure direct in-process JSON-RPC routing and handler dispatch.

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
InterMCP proxies requests, namespaces upstream tools (`github__create_issue`), monitors process health, and exposes a single unified stdio pipe.

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

### 8. ADR 001: Signed Execution Receipts & Provenance (`--receipts`)
Generates tamper-evident cryptographic receipts for every tool execution using RFC 8785 JSON Canonicalization Scheme (JCS) and HMAC-SHA256 digital signatures. Authenticate offline with:
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

---

## 🌐 Protocol Support

Implements the **2024-11-05 Model Context Protocol specification**:

### Tools (`tools/list`, `tools/call`)
- `fs_read_file`, `fs_write_file`, `fs_list_dir`, `fs_search_text` (SafeFS protected)
- `git_status`, `git_diff`
- `system_info`, `system_run_command`
- `intermcp_search_tools` (when `--smart-discovery` is enabled)

### Resources (`resources/list`, `resources/read`)
- `system://diagnostics`: Host architecture, CPU, and process memory telemetry.

### Prompts (`prompts/list`, `prompts/get`)
- `code_review`: Reusable prompt for code review, memory safety, and performance analysis.

---

## 🦀 Rust SDK Usage

Add `intermcp` to your `Cargo.toml`:
```toml
[dependencies]
intermcp = "0.1"
serde_json = "1.0"
tokio = { version = "1", features = ["full"] }
```

Create a custom server:
```rust
use intermcp::{Server, SimpleTool, CallToolResult, Result};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    let mut server = Server::new("custom-server", "0.1.0");

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

## 📦 TypeScript / Node SDK

```bash
npm install intermcp
```

```typescript
import { InterMcpClient } from "intermcp";

async function main() {
  const client = new InterMcpClient();
  await client.start();

  const files = await client.callTool("fs_list_dir", { path: "." });
  console.log("Files:", files);

  client.stop();
}

main();
```

---

## 🩺 Diagnostics (`intermcp doctor`)

Verify your environment and IDE setups:
```bash
intermcp doctor
```

---

## 📚 Developer Guides

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
