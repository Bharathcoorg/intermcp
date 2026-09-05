/** InterMCP Node.js / TypeScript SDK v0.2.1 */
const { spawn } = require("child_process");
const readline = require("readline");
const path = require("path");

class InterMcpClient {
  constructor(options = {}) {
    this.plugin = options.plugin || null;
    this.binaryPath = options.binaryPath || null;
    this.proc = null;
    this.rl = null;
    this.requestId = 0;
    this.pending = new Map();
  }

  async start() {
    const isWin = process.platform === "win32";
    const binaryName = isWin ? "intermcp.exe" : "intermcp";
    const defaultRelease = path.join(__dirname, "target", "release", binaryName);
    const defaultDebug = path.join(__dirname, "target", "debug", binaryName);

    let cmd = this.binaryPath;
    if (!cmd) {
      if (require("fs").existsSync(defaultRelease)) {
        cmd = defaultRelease;
      } else if (require("fs").existsSync(defaultDebug)) {
        cmd = defaultDebug;
      } else {
        cmd = "cargo";
      }
    }

    const args = cmd === "cargo"
      ? ["run", "--manifest-path", path.join(__dirname, "Cargo.toml"), "--", "serve", ...(this.plugin ? ["--plugin", this.plugin] : [])]
      : ["serve", ...(this.plugin ? ["--plugin", this.plugin] : [])];

    this.proc = spawn(cmd, args, {
      stdio: ["pipe", "pipe", "pipe"],
    });

    this.rl = readline.createInterface({
      input: this.proc.stdout,
      terminal: false,
    });

    this.rl.on("line", (line) => {
      try {
        const resp = JSON.parse(line);
        if (resp.id && this.pending.has(resp.id)) {
          const { resolve, reject } = this.pending.get(resp.id);
          this.pending.delete(resp.id);
          if (resp.error) {
            reject(new Error(`MCP Error ${resp.error.code}: ${resp.error.message}`));
          } else {
            resolve(resp.result);
          }
        }
      } catch (err) {
        console.error("InterMCP Client JSON parse error:", err);
      }
    });

    // Handshake
    await this.request("initialize", {
      protocolVersion: "2024-11-05",
      clientInfo: { name: "intermcp-node-client", version: "0.2.1" },
    });
  }

  request(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++this.requestId;
      // AUDIT-16: Add 30-second timeout to prevent memory leak and application hang
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`InterMCP request timeout after 30s: ${method}`));
      }, 30000);
      this.pending.set(id, {
        resolve: (val) => { clearTimeout(timer); resolve(val); },
        reject: (err) => { clearTimeout(timer); reject(err); },
      });
      const msg = JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n";
      this.proc.stdin.write(msg, (err) => {
        if (err) {
          clearTimeout(timer);
          this.pending.delete(id);
          reject(new Error(`Failed to write to InterMCP stdin: ${err.message}`));
        }
      });
    });
  }

  async listTools() {
    const res = await this.request("tools/list");
    return res.tools || [];
  }

  async callTool(name, args = {}) {
    return this.request("tools/call", { name, arguments: args });
  }

  async listResources() {
    const res = await this.request("resources/list");
    return res.resources || [];
  }

  async readResource(uri) {
    return this.request("resources/read", { uri });
  }

  async listPrompts() {
    const res = await this.request("prompts/list");
    return res.prompts || [];
  }

  async getPrompt(name, args = {}) {
    return this.request("prompts/get", { name, arguments: args });
  }

  stop() {
    if (this.proc) {
      this.proc.kill();
      this.proc = null;
    }
  }
}

module.exports = { InterMcpClient };
