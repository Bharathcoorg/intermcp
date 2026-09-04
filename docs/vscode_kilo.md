# Setting Up InterMCP with VS Code, Kilo Code & Codex

InterMCP seamlessly integrates with **VS Code** native MCP, **Kilo Code** extension, **OpenAI Codex / GitHub Copilot**, and **Cline / Roo Code**.

---

## ⚡ 1-Click Auto-Setup (Recommended)

To automatically configure VS Code and all installed AI coding extensions simultaneously:

```bash
# Using installed binary
intermcp setup

# Or via NPX
npx intermcp setup
```

This scans and merges configuration into:
* **VS Code / Codex**: `Code/User/mcp.json`
* **Kilo Code**: `Code/User/globalStorage/kilo.kilo-code/settings/mcp_settings.json`
* **Cline**: `Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json`
* **Roo Code**: `Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/cline_mcp_settings.json`

---

## 📁 Manual Configuration Paths

### 1. VS Code Native MCP & Codex
* **Windows:** `%APPDATA%\Code\User\mcp.json`
* **macOS:** `~/Library/Application Support/Code/User/mcp.json`
* **Linux:** `~/.config/Code/User/mcp.json`
* **Workspace Level:** `.vscode/mcp.json`

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

### 2. Kilo Code Extension
* **Windows:** `%APPDATA%\Code\User\globalStorage\kilo.kilo-code\settings\mcp_settings.json`
* **macOS:** `~/Library/Application Support/Code/User/globalStorage/kilo.kilo-code/settings/mcp_settings.json`
* **Linux:** `~/.config/Code/User/globalStorage/kilo.kilo-code/settings/mcp_settings.json`

Add the same `mcpServers` object shown above.

---

## 🔒 Security & Performance Features Active

Once connected, your VS Code AI coding agents benefit from:
* **SafeFS**: Prevents agents from traversing outside the workspace or reading `.env` credentials.
* **Safe-Shell Linter**: Intercepts destructive terminal commands (`rm -rf`, raw disk writes).
* **ADR 001 Receipts**: Signs every tool execution with HMAC-SHA256 for non-repudiation.
