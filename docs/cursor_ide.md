# Setting Up InterMCP with Cursor IDE

Cursor natively supports the Model Context Protocol (MCP), enabling Cursor's AI Composer and Agent to use local tools with sub-millisecond execution speeds.

## ⚡ 1-Click Global Setup (Fastest)

Configure Cursor globally for all projects in 1 second:

```bash
# Using installed binary
intermcp setup

# Or via NPX
npx intermcp setup
```

This automatically discovers `~/.cursor/mcp.json`, backs up existing settings, and registers InterMCP with zero manual work.

---

## 📁 Manual Configuration

### Project-Level Configuration

In the root of your project directory, create a `.cursor` folder with an `mcp.json` file:

```bash
mkdir -p .cursor
touch .cursor/mcp.json
```

Add the following configuration:

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

---

## 2. Global Configuration

To make InterMCP available across all projects in Cursor:
* **macOS / Linux:** `~/.cursor/mcp.json`
* **Windows:** `%USERPROFILE%\.cursor\mcp.json`

Add the same JSON configuration above.

---

## 3. Testing in Cursor

1. Open Cursor Settings -> **Features** -> **MCP**.
2. Verify that `intermcp` is listed with a green status indicator.
3. In Cursor Composer (`Cmd+I` or `Ctrl+I`), enable **Agent Mode**.
4. Cursor can now search files via `fs_search_text`, inspect git changes with `git_diff`, and dispatch tool calls with sub-microsecond latency.
