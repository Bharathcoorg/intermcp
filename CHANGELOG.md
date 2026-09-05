# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - 2026-09-05

### Security & Hardening Pass
- **CRITICAL**: Hardened shell linter against path-qualified executable bypasses (e.g. `./git`, `/bin/git`, `C:\Windows\git.exe`) and inline environment variable assignment prefixes (`FOO=bar git`) in `validate_shell_command` (`src/tools/system.rs`).
- **CRITICAL**: Replaced predictable sequential `AtomicU64` approval IDs with 128-bit cryptographically secure random tokens generated via `rand::thread_rng()` (`src/vault_lock.rs`).
- **CRITICAL**: Enforced `POST`-only requests on HTTP approval/rejection endpoints (`/api/approve/`, `/api/reject/`), returning RFC-compliant `405 Method Not Allowed` for `GET` requests to prevent 1-click CSRF attacks (`src/http_server.rs`).
- **CRITICAL**: Piped upstream external MCP process `stderr` and sanitized output through `redact_for_log` in an asynchronous background reader task instead of raw `Stdio::inherit()` (`src/hub.rs`).
- **CRITICAL**: Guarded session flight trace replay against executing destructive mutations (`fs_write_file`, `system_run_command`, mutating git actions) on the host machine; added `--allow-mutations` CLI flag to explicitly authorize mutations (`src/record.rs`, `src/main.rs`).
- **MEDIUM**: Integrated `split_chained_commands` in `Server::handle_request` to ensure all chained subcommands (`&&`, `||`, `;`, `|`) are individually evaluated against `engine.check_shell` (`src/server.rs`).
- **MEDIUM**: Propagated active `SandboxPolicy` into `search_dir` so custom sensitive file rules are strictly enforced during `fs_search_text` pattern scans (`src/tools/fs.rs`).
- **MEDIUM**: Adopted RFC 8785 JSON Canonicalization Scheme (`canonicalize_json`) in `SmacLogger::hash_value` to guarantee deterministic hashes across varying key ordering (`src/smac.rs`).
- **LOW**: Replaced panic-inducing `.unwrap()` on `config.as_object_mut()` with safe matches returning explicit error results in multi-IDE configurator (`src/auto_config.rs`).
- **LOW**: Added `options.binaryPath` support and automatic debug binary detection to Node.js / TypeScript SDK client (`index.js`).
- **DOCS**: Calibrated benchmark descriptions to specify ping routing latency and ops/sec; updated cryptographic terminology to "HMAC-authenticated receipts" (`README.md`).

## [0.2.0] - 2026-09-04

### Security & Hardening
- **Fix A / MEDIUM**: Prevented SafeFS hardlink bypasses by implementing `HardlinkExt` and rejecting hardlink targets and ancestors in `SandboxPolicy::validate_path` and `src/tools/fs.rs`.
- **Fix B / MEDIUM**: Eliminated ADR 001 receipt partial-write corruption hazards by writing lines to same-directory temporary file, calling `sync_all`, performing atomic rename, and updating hash/sequence only upon success (`src/receipts.rs`).
- **Fix C / MEDIUM**: Hardened POSIX child process isolation against terminal signal race conditions by ignoring `SIGTTOU`, `SIGTTIN`, and `SIGTSTP` after `setpgid` in `pre_exec` (`src/reaper.rs`).
- **Fix D / MEDIUM**: Added graceful fallback on Windows Job Object creation or assignment failure by logging warnings and falling back to `kill_on_drop` process reaping (`src/reaper.rs`).
- **Fix E / LOW**: Completed dashboard HTML escaping by adding backtick (&#x60;) and dollar-sign (&#36;) entity escaping (`src/http_server.rs`).
- **Fix F / LOW**: Included `fs_search_text` in `PolicyEngine` filesystem evaluation to enforce denied pattern checks across pattern searches (`src/server.rs`).
- **Pass 2 - Finding 1 / MEDIUM**: Fixed PolicyEngine tool-name check typo by matching `fs_list_dir` and `fs_list_directory` with default path handling for omitted parameters, ensuring directory listing policies are strictly enforced (`src/server.rs`).
- **Pass 2 - Finding 2 / LOW**: Fixed ADR 001 receipt `session_id` inconsistency by attributing successful executions to the instance `&self.session_id` rather than phantom `"session-1"` (`src/server.rs`).
- **Pass 2 - Finding 3 / LOW**: Hardened `Dashboard` subcommand by adding `--token`, `--tls-cert`, and `--tls-key` CLI options, passing authentication token to HTTP config, and rejecting public binds without token or TLS (`src/main.rs`).
- **Pass 2 - Finding 4 / MEDIUM**: Verified Go SDK handshake deadlock prevention by acquiring mutex for subprocess setup only and releasing before `c.Request("initialize", ...)` (`go/intermcp/client.go`).
- **F-08 / CRITICAL**: Integrated declarative `PolicyEngine` into `Server::handle_request` and CLI `--config` loader, enforcing path rules, shell execution allowlists, sliding-window rate limits, and output byte ceilings (`src/server.rs`, `src/main.rs`).
- **F-01 / HIGH**: Fixed SSE endpoint routing hijack by restricting SSE dispatch strictly to `method == "GET" && path == "/sse" && is_sse_accept` (`src/http_server.rs`).
- **F-02 / HIGH**: Implemented standard multi-line SSE event serialization (`data: ` prefix per line) preventing SSE event framing injection via unescaped payload newlines (`src/http_server.rs`).
- **F-03 / HIGH**: Eliminated Reflected XSS vulnerability in live dashboard HTML rendering via context-aware entity escaping (`html_escape`) on tool names, descriptions, payloads, and IDs (`src/http_server.rs`).
- **F-06 / HIGH**: Fixed task orphanage on client cancellation: `tokio::select!` cancellation now explicitly invokes `task.abort()` and wraps inner futures with `CancellationToken` (`src/server.rs`).
- **F-07 / HIGH**: Eliminated hardcoded fallback HMAC key (`"intermcp-default-secret-key"`): omitted `--signing-key` now generates a 256-bit cryptographically secure ephemeral key via `rand::rngs::OsRng` (`src/main.rs`).
- **F-15 / HIGH**: Fixed response truncation on large JSON payloads in PHP SDK by assembling multi-chunk lines until terminal newline (`php/src/Client.php`).
- **F-04 / MEDIUM**: Protected against HTTP request smuggling (RFC 7230 §3.3.3) by rejecting all `Transfer-Encoding` requests with `HTTP 400 Bad Request` (`src/http_server.rs`).
- **F-09 / MEDIUM**: Fixed WASM memory limit bypass by decoding binary LEB128 page count from Section 5 payload instead of hardcoding `declared_memory = Some(1)` (`src/wasm.rs`).
- **F-10 / MEDIUM**: Rejected malformed or truncated WASM binaries where section lengths exceed remaining payload bytes (`src/wasm.rs`).
- **F-11 / MEDIUM**: Replaced non-atomic quarantine file writes with hidden temporary file write + atomic `std::fs::rename` (`src/hub.rs`).
- **F-12 / MEDIUM**: Hardened upstream environment scrubber by blocking `AUTH`, `CREDENTIAL`, `PRIVATE`, `CERT`, `PASSPHRASE`, `MNEMONIC`, `SEED_PHRASE`, and scanning for high-entropy hex/base64 strings (`src/hub.rs`).
- **F-13 / MEDIUM**: Prevented upstream tool shadowing and namespace prefix collisions by disallowing leading/trailing underscores and `__` in upstream and tool names (`src/hub.rs`).
- **F-14 / MEDIUM**: Resolved mutex deadlock hazard during Go SDK initialization by releasing `c.mu` prior to executing the `initialize` handshake (`go/intermcp/client.go`).
- **F-05 / LOW**: Bounded JSON-RPC 2.0 batch size to 100 items, rejecting larger arrays with `-32600 Invalid Request: batch too large` (`src/server.rs`).
- **F-16 / LOW**: Handled asynchronous `stdin.write` callback errors in Node.js SDK to reject pending requests immediately on pipe failure (`index.js`).
- **F-17 / LOW**: Documented stdio request serialization architecture in Python SDK client (`python/intermcp/client.py`).
- **Fix A / MEDIUM**: Replaced fragile Windows retry loop in `atomic_write_json` with unique tempfile write and single atomic rename (`src/auto_config.rs`).
- **Fix B / LOW**: Added pre-existing server inspection and warning before modifying `"intermcp"` configuration in IDE settings files (`src/auto_config.rs`).
- **Fix C / LOW**: Hardened backup creation against symlink TOCTOU races by verifying ancestor directory and destination file symlink metadata (`src/auto_config.rs`).
- **AUDIT-01 / HIGH**: Enforced 32-character hexadecimal format validation for SSE session IDs and returned `404 Not Found` for invalid or missing sessions (`src/http_server.rs`).
- **AUDIT-02 / HIGH**: Added periodic pruning to the HTTP IP rate limiter state map to prevent slow memory exhaustion (`src/http_server.rs`).
- **AUDIT-03 / MEDIUM**: Bounded maximum active concurrent SSE sessions (`MAX_SSE_SESSIONS = 1024`) with `503 Service Unavailable` on capacity (`src/http_server.rs`).
- **AUDIT-06 / MEDIUM**: Enforced clean process group reaping in hub upstream supervisor by calling `guard.kill_group()` before respawn loops (`src/hub.rs`).
- **AUDIT-08 / LOW**: Added `CONIN$` and `CONOUT$` to Windows reserved NTFS device names (`src/sandbox.rs`).
- **AUDIT-10 / LOW**: Generated unique session identifiers per server instance for ADR 001 execution receipts (`src/server.rs`).
- **AUDIT-13 / HIGH**: Clarified and documented data-flow taint tracking as a cooperative advisory contract (`src/taint.rs`).
- **AUDIT-17 / HIGH**: Converted PHP SDK `proc_open()` from shell string execution with `escapeshellcmd()` to array form to prevent argument injection (`php/src/Client.php`).
- **C-1..C-7, H-1..H-10, M-1..M-7**: Preserved and verified all prior security mechanisms across SafeFS, Windows Job Objects, Unix process groups, Aho-Corasick secret redaction, and JCS HMAC signing.

### Added
- Native in-process TLS termination via `rustls` v0.23, `tokio-rustls`, and `rustls-pki-types` with `--tls-cert` and `--tls-key` CLI options.
- Multi-OS GitHub Actions CI workflow covering Windows, macOS, and Linux with clippy, formatting, testing, and `cargo audit` gating (`.github/workflows/ci.yml`).
- Official Go module (`go/intermcp`) and PHP 8.1+ Composer package (`php/`).
- Python SDK wheel and sdist packaging (`python/pyproject.toml`).
- Comprehensive security model documentation (`docs/SECURITY_MODEL.md`).

### Changed
- Refactored CLI documentation and README claims to accurately reflect verified benchmark dispatch latency, WASM inspector scope, and loopback TLS exemptions.
- Removed deprecated `rustls-pemfile` dependency in favor of modern `rustls-pki-types` 1.x.
