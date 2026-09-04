# Setting Up InterMCP with Google Antigravity IDE

Google Antigravity IDE natively supports the Model Context Protocol (MCP) through global customizations and per-workspace agent configurations.

---

## ⚡ 1-Click Auto-Setup (Fastest)

InterMCP includes automated detection and configuration for Antigravity IDE:

```bash
# Using the installed binary
intermcp setup

# Or via NPX without local compilation
npx intermcp setup
```

This scans for your Antigravity configuration at `~/.gemini/config/mcp_config.json`, creates a byte-verified backup (`.json.bak`), and atomically merges the `intermcp` runtime under `"mcpServers"`.

---

## 📁 Manual Configuration

If you prefer manual configuration, locate your Antigravity global configuration file:

* **Windows:** `%USERPROFILE%\.gemini\config\mcp_config.json`
* **macOS / Linux:** `~/.gemini/config/mcp_config.json`

Add the `intermcp` server entry:

```json
{
  "mcpServers": {
    "intermcp": {
      "command": "intermcp",
      "args": ["serve"]
    }
  }
}
```

If you installed via standalone binary at `~/.intermcp/bin`:
```json
{
  "mcpServers": {
    "intermcp": {
      "command": "C:\\Users\\<USER>\\.intermcp\\bin\\intermcp.exe",
      "args": ["serve"]
    }
  }
}
```

---

## 🔍 Verification

1. Restart Antigravity IDE.
2. In the AI chat pane or agent session, test tool discovery or run `@intermcp system_info`.
3. InterMCP will respond with sub-microsecond latency, SafeFS sandboxing, and signed execution receipts.
