"""
InterMCP Python Client
High-Performance Model Context Protocol Client for Python AI Agents.
"""

import json
import os
import subprocess
import sys
import threading
from typing import Any, Dict, List, Optional


class InterMcpClient:
    """Python client for connecting to the native InterMCP Trust Runtime."""

    def __init__(self, binary_path: Optional[str] = None):
        if binary_path is None:
            is_win = sys.platform == "win32"
            exe_name = "intermcp.exe" if is_win else "intermcp"
            candidates = [
                os.environ.get("INTERMCP_BIN"),
                os.path.join(os.path.expanduser("~"), ".intermcp", "bin", exe_name),
                os.path.join(os.path.dirname(__file__), "..", "..", "target", "release", exe_name),
                exe_name,
            ]
            self.binary_path = next((p for p in candidates if p and os.path.exists(p)), exe_name)
        else:
            self.binary_path = binary_path

        self.proc: Optional[subprocess.Popen] = None
        self.request_id = 0
        self._lock = threading.Lock()

    def start(self) -> Dict[str, Any]:
        """Start the native InterMCP process and perform MCP 2024-11-05 handshake."""
        self.proc = subprocess.Popen(
            [self.binary_path, "serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            encoding="utf-8",
        )

        return self.request("initialize", {
            "protocolVersion": "2024-11-05",
            "clientInfo": {"name": "intermcp-python-sdk", "version": "0.2.0"},
            "capabilities": {}
        })

    def request(self, method: str, params: Optional[Dict[str, Any]] = None) -> Any:
        """Send a standard JSON-RPC 2.0 message and return result.

        Note on Threading / Concurrency (Finding 17):
        Stdio-based JSON-RPC requires strict serial request-response framing across
        standard input/output pipes. `self._lock` serializes requests across multiple threads
        to prevent interleaving. For high-concurrency async workloads, use the async client
        or HTTP/SSE transport.
        """
        with self._lock:
            if not self.proc or not self.proc.stdin or not self.proc.stdout:
                raise RuntimeError("InterMCP client is not running. Call .start() first.")

            self.request_id += 1
            msg = json.dumps({
                "jsonrpc": "2.0",
                "id": self.request_id,
                "method": method,
                "params": params or {},
            }) + "\n"

            self.proc.stdin.write(msg)
            self.proc.stdin.flush()

            line = self.proc.stdout.readline()
            if not line:
                err = self.proc.stderr.read() if self.proc.stderr else "Unknown error"
                raise RuntimeError(f"InterMCP engine process exited: {err}")

            payload = json.loads(line)
            if "error" in payload:
                raise RuntimeError(f"MCP Error {payload['error'].get('code')}: {payload['error'].get('message')}")

            return payload.get("result")

    def list_tools(self) -> List[Dict[str, Any]]:
        """List all registered tools."""
        res = self.request("tools/list")
        return res.get("tools", [])

    def call_tool(self, name: str, arguments: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Execute a tool with SafeFS, secret redaction, and signed receipts."""
        return self.request("tools/call", {
            "name": name,
            "arguments": arguments or {}
        })

    def list_resources(self) -> List[Dict[str, Any]]:
        """List all resources."""
        res = self.request("resources/list")
        return res.get("resources", [])

    def read_resource(self, uri: str) -> Dict[str, Any]:
        """Read resource content."""
        return self.request("resources/read", {"uri": uri})

    def list_prompts(self) -> List[Dict[str, Any]]:
        """List reusable prompt templates."""
        res = self.request("prompts/list")
        return res.get("prompts", [])

    def get_prompt(self, name: str, arguments: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Retrieve a specific prompt."""
        return self.request("prompts/get", {
            "name": name,
            "arguments": arguments or {}
        })

    def stop(self):
        """Cleanly terminate the native runtime."""
        with self._lock:
            if self.proc:
                try:
                    self.proc.terminate()
                    self.proc.wait(timeout=2)
                except Exception:
                    self.proc.kill()
                self.proc = None

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.stop()
