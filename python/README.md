# InterMCP Python Client

Official Python Client for **InterMCP** — Ultra-Fast, Safe Model Context Protocol (MCP) Runtime.

## Installation

```bash
pip install intermcp
```

## Quickstart

```python
from intermcp import InterMcpClient

with InterMcpClient() as client:
    # Query registered tools
    tools = client.list_tools()
    print("Available tools:", [t["name"] for t in tools])

    # Call a tool with sub-microsecond latency
    info = client.call_tool("system_info", {})
    print(info)
```
