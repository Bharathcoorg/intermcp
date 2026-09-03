# Setting Up InterMCP with Claude Desktop

Connect InterMCP to Anthropic's Claude Desktop application in under 60 seconds.

---

## 1. Prerequisites

- **Claude Desktop** installed: [Download Claude Desktop](https://claude.ai/download)
- **InterMCP** installed:
  ```bash
  # Via Cargo (Rust)
  cargo install intermcp

  # Or via NPX (Zero compilation needed)
  npx intermcp serve
  ```

---

## 2. Locate Your Configuration File

Depending on your operating system:

| Platform | Configuration File Path |
| :--- | :--- |
| **macOS** | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| **Windows** | `%APPDATA%\Claude\claude_desktop_config.json` |
| **Linux** | `~/.config/Claude/claude_desktop_config.json` |

> 💡 **Tip**: You can run `intermcp doctor` in your terminal to automatically detect whether this file exists on your system!

---

## 3. Configuration Snippet

Open `claude_desktop_config.json` in your favorite editor and add the `intermcp` server under `mcpServers`:

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

If you installed via npm/npx without Rust:
```json
{
  "mcpServers": {
    "intermcp": {
      "command": "npx",
      "args": ["-y", "intermcp", "serve"]
    }
  }
}
```

---

## 4. Restart and Verify

1. Completely close and restart Claude Desktop.
2. In any chat window, click the **Attach / Tools icon (Hammer/Plug)** in the lower right corner.
3. You will see all 8 universal developer tools available:
   - `fs_read_file`
   - `fs_write_file`
   - `fs_list_dir`
   - `fs_search_text`
   - `git_status`
   - `git_diff`
   - `system_info`
   - `system_run_command`

### Example Prompt to Test:
> *"Claude, use `git_status` to see my uncommitted changes, and check `system_info` to report my CPU architecture."*
