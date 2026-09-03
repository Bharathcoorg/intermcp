# Writing Custom Tools, Resources & Prompts with InterMCP

InterMCP is designed to make writing tools fast, type-safe, and ergonomic in both Rust and TypeScript.

---

## 🦀 In Rust

Add `intermcp` to your `Cargo.toml`:
```toml
[dependencies]
intermcp = "0.1"
serde_json = "1.0"
tokio = { version = "1", features = ["full"] }
```

### Writing an Async Custom Tool:

```rust
use intermcp::{Server, SimpleTool, CallToolResult, Result};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    let mut server = Server::new("finance-tools", "1.0.0");

    // Define a custom currency converter tool
    server.add_tool(Box::new(SimpleTool::new(
        "convert_currency",
        "Converts an amount from one fiat currency to another",
        json!({
            "type": "object",
            "properties": {
                "amount": { "type": "number", "description": "The amount to convert" },
                "from": { "type": "string", "description": "Source currency (e.g. USD)" },
                "to": { "type": "string", "description": "Target currency (e.g. EUR)" }
            },
            "required": ["amount", "from", "to"]
        }),
        |args| async move {
            let amount = args["amount"].as_f64().unwrap_or(0.0);
            let from = args["from"].as_str().unwrap_or("USD");
            let to = args["to"].as_str().unwrap_or("EUR");

            let converted = amount * 0.92; // example rate
            Ok(CallToolResult::text(format!("{:.2} {} is {:.2} {}", amount, from, converted, to)))
        },
    )));

    // Run the high-speed stdio loop
    server.run_stdio().await
}
```

---

## 📦 In TypeScript / Node.js

Install the npm package:
```bash
npm install intermcp
```

Use the client SDK to orchestrate tools:
```typescript
import { InterMcpClient } from "intermcp";

async function main() {
  const client = new InterMcpClient();
  await client.start();

  // List available tools
  const tools = await client.listTools();
  console.log(`Found ${tools.length} available tools.`);

  // Execute a tool
  const result = await client.callTool("system_info", {});
  console.log("System Diagnostics:", result);

  client.stop();
}

main();
```
