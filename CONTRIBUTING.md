# Contributing to InterMCP 🚀

First off, thank you for considering contributing to **InterMCP**! We welcome contributions of all kinds: new universal developer tools, protocol improvements, performance optimizations, documentation guides, and bug fixes.

---

## 🛠️ Local Development Setup

### Prerequisites:
- **Rust toolchain** (1.75+): [https://rustup.rs](https://rustup.rs)
- **Node.js** (v18+): Optional, only if working on the npm wrapper package.

### Step 1: Clone and Build
```bash
git clone https://github.com/Bharathcoorg/intermcp.git
cd intermcp
cargo build
```

### Step 2: Run Tests & Linter
```bash
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

---

## 🧩 How to Add a New Tool in 5 Minutes

All tools in InterMCP implement the `Tool` trait or use the `SimpleTool` constructor.

1. Create a new file in `src/tools/my_tool.rs` (or add to an existing module):

```rust
use serde_json::{json, Value};
use crate::protocol::CallToolResult;
use crate::tool::{SimpleTool, Tool};

pub fn create_my_custom_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "my_tool_name",
        "A clear, 1-sentence description of what this tool accomplishes",
        json!({
            "type": "object",
            "properties": {
                "input_param": {
                    "type": "string",
                    "description": "Explanation of this parameter"
                }
            },
            "required": ["input_param"]
        }),
        |args: Value| async move {
            let param = args["input_param"].as_str().unwrap_or("");
            let result = format!("Processed: {}", param);
            Ok(CallToolResult::text(result))
        },
    ))
}
```

2. Register the tool in `src/tools/mod.rs` inside `universal_toolset()`.

3. Test your tool instantly in the terminal:
```bash
cargo run -- test-tool my_tool_name --args '{"input_param":"hello"}'
```

4. Add a unit test in `src/lib.rs`.

---

## 📋 Pull Request Guidelines

1. **Keep it Universal**: Default tools should solve headaches for general software engineers (Filesystem, Git, Shell, Diagnostics, Net, SQLite, etc.). Domain-specific extensions should be placed in `src/tools/` as optional plugins.
2. **Pure Native & Lightweight**: Avoid adding heavy external C/C++ dependencies or large crates that blow up binary size.
3. **No stdout Pollution**: The Model Context Protocol communicates via newline-delimited JSON-RPC over `stdout`. **Never use `println!` or log to `stdout` in server code.** Always use `tracing::info!`, `tracing::error!`, or `eprintln!` which write to `stderr`.
4. **Performance**: Tool executions should be non-blocking (`async`) and minimize heap allocations.

---

## 💬 Community & Questions

- **Issues**: If you find a bug or want to request a feature, please [open an issue](https://github.com/Bharathcoorg/intermcp/issues).
- **Security Disclosures**: Please send confidential vulnerability reports to `bharathbr0x@gmail.com`.
- **Maintainer**: Bharath B R ([@Bharathcoorg](https://github.com/Bharathcoorg)).
