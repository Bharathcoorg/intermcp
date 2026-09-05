"""
InterMCP Python Client Example
Connects to the native InterMCP binary over standard stdio JSON-RPC 2024-11-05 protocol.
No external dependencies required (uses standard library).
"""

import json
import os
import subprocess
import sys

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")


class InterMcpPythonClient:
    def __init__(self, binary_path=None):
        if binary_path is None:
            # Look in standard build or PATH locations
            is_win = sys.platform == "win32"
            exe_name = "intermcp.exe" if is_win else "intermcp"
            candidates = [
                os.path.join(os.path.dirname(__file__), "..", "target", "release", exe_name),
                os.path.join(os.path.expanduser("~"), ".intermcp", "bin", exe_name),
                exe_name,
            ]
            self.binary_path = next((p for p in candidates if os.path.exists(p)), exe_name)
        else:
            self.binary_path = binary_path

        self.proc = None
        self.request_id = 0

    def start(self):
        """Start the InterMCP process and complete initial protocol handshake."""
        self.proc = subprocess.Popen(
            [self.binary_path, "serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )

        # MCP 2024-11-05 Handshake
        init_res = self.send_request("initialize", {
            "protocolVersion": "2024-11-05",
            "clientInfo": {"name": "intermcp-python-client", "version": "0.2.1"},
            "capabilities": {}
        })
        return init_res

    def send_request(self, method: str, params: dict = None):
        """Send a JSON-RPC 2.0 request and wait for the response."""
        self.request_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params or {},
        }
        raw_msg = json.dumps(payload) + "\n"
        self.proc.stdin.write(raw_msg)
        self.proc.stdin.flush()

        line = self.proc.stdout.readline()
        if not line:
            err = self.proc.stderr.read()
            raise RuntimeError(f"InterMCP process terminated unexpectedly: {err}")

        response = json.loads(line)
        if "error" in response:
            raise RuntimeError(f"MCP Error: {response['error']}")
        return response.get("result")

    def list_tools(self):
        """Query all available tools from InterMCP."""
        res = self.send_request("tools/list")
        return res.get("tools", [])

    def call_tool(self, name: str, arguments: dict = None):
        """Execute a tool with SafeFS sandboxing and HMAC receipts."""
        return self.send_request("tools/call", {
            "name": name,
            "arguments": arguments or {}
        })

    def close(self):
        """Cleanly terminate the process."""
        if self.proc:
            self.proc.terminate()
            self.proc.wait()


if __name__ == "__main__":
    print("🚀 Connecting Python client to InterMCP...")
    client = InterMcpPythonClient()
    init_info = client.start()
    print(f"✅ Handshake successful! Protocol: {init_info.get('protocolVersion')}")

    tools = client.list_tools()
    print(f"📦 Registered tools ({len(tools)}):")
    for t in tools[:5]:
        print(f"   • {t['name']}: {t.get('description', '')[:60]}...")

    print("\n🔍 Executing system_info tool:")
    result = client.call_tool("system_info", {})
    print(json.dumps(result, indent=2))

    client.close()
    print("\n🎉 Done!")
