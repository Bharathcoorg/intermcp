#!/usr/bin/env node

/**
 * InterMCP Node.js CLI Runner
 * Spawns the high-performance native intermcp binary or provides effortless 1-click guidance.
 */

const { spawn, execSync } = require("child_process");
const path = require("path");
const fs = require("fs");
const os = require("os");

const isWin = process.platform === "win32";
const binaryName = isWin ? "intermcp.exe" : "intermcp";

// Look for precompiled or installed binaries in priority order
const candidates = [
  process.env.INTERMCP_BIN,
  path.join(__dirname, "..", "target", "release", binaryName),
  path.join(__dirname, binaryName),
  path.join(os.homedir(), ".intermcp", "bin", binaryName),
  path.join(os.homedir(), ".cargo", "bin", binaryName),
].filter(Boolean);

let targetBin = candidates.find(p => fs.existsSync(p));

// If not found in standard paths, check system PATH
if (!targetBin) {
  try {
    const whichCmd = isWin ? `where ${binaryName}` : `which ${binaryName}`;
    const stdout = execSync(whichCmd, { stdio: ["ignore", "pipe", "ignore"] }).toString().trim();
    const firstLine = stdout.split(/\r?\n/)[0];
    if (firstLine && fs.existsSync(firstLine)) {
      targetBin = firstLine;
    }
  } catch (_) {
    // Binary not currently on PATH
  }
}

if (targetBin) {
  const proc = spawn(targetBin, process.argv.slice(2), { stdio: "inherit" });
  proc.on("exit", (code) => process.exit(code || 0));
} else {
  // Check if cargo is available to build or run from local source
  let hasCargo = false;
  try {
    execSync("cargo --version", { stdio: "ignore" });
    hasCargo = true;
  } catch (_) {
    hasCargo = false;
  }

  const manifestPath = path.join(__dirname, "..", "Cargo.toml");
  if (hasCargo && fs.existsSync(manifestPath)) {
    const cargoArgs = [
      "run",
      "--release",
      "--manifest-path",
      manifestPath,
      "--",
      ...process.argv.slice(2),
    ];
    const proc = spawn("cargo", cargoArgs, { stdio: "inherit" });
    proc.on("exit", (code) => process.exit(code || 0));
  } else {
    console.error(`
\x1b[36m⚡ InterMCP — High-Performance Model Context Protocol Runtime\x1b[0m
============================================================
The native InterMCP binary was not found on your system.

\x1b[32m🚀 1-Click Installation Options:\x1b[0m

  \x1b[1mOption 1: Shell 1-Click Installer (Zero Setup)\x1b[0m
  ${isWin
    ? "  PowerShell:\n  \x1b[33mirm https://raw.githubusercontent.com/Bharathcoorg/intermcp/main/install.ps1 | iex\x1b[0m"
    : "  macOS / Linux / WSL:\n  \x1b[33mcurl -fsSL https://raw.githubusercontent.com/Bharathcoorg/intermcp/main/install.sh | sh\x1b[0m"}

  \x1b[1mOption 2: Install via Cargo (Rust)\x1b[0m
  \x1b[33mcargo install intermcp\x1b[0m

  \x1b[1mOption 3: Download Precompiled Standalone Binary\x1b[0m
  Visit: \x1b[34mhttps://github.com/Bharathcoorg/intermcp/releases/latest\x1b[0m

Once installed, simply run:
  \x1b[32mintermcp setup\x1b[0m
to automatically configure Claude, Cursor, Antigravity IDE, Kilo Code, and VS Code!
`);
    process.exit(1);
  }
}
