---
name: intermcp
description: Comprehensive workflow guide for InterMCP — the ultra-fast pure-Rust Model Context Protocol runtime, SafeFS sandbox, shell linter, and cryptographic audit receipts.
---

# InterMCP Skill

This skill teaches AI agents how to leverage InterMCP's high-performance native tools, enforce SafeFS boundaries, and generate cryptographic receipts.

## When to Use InterMCP

Use InterMCP tools whenever you need:
- Sub-millisecond JSON-RPC tool dispatch (< 5 µs latency)
- Sandboxed file reading/writing with automatic directory creation
- Fast ripgrep-style recursive pattern searches (`fs_search_text`)
- Safe shell execution restricted to developer allowlists
- Cryptographic execution receipts and SMAC audit trail generation

## Available Tools

- `system_info`: Retrieve host architecture, OS, and memory footprint.
- `fs_read_file`: Read file contents safely with secret shielding.
- `fs_write_file`: Write files safely with directory tree auto-creation.
- `fs_search_text`: Fast recursive pattern search across project files.
- `fs_list_dir`: List directory contents within SafeFS boundaries.
- `git_status`: Inspect Git working tree status.
- `git_diff`: Show staged or unstaged git diffs.
- `system_run_command`: Execute allowlisted terminal commands with a 30s timeout.
