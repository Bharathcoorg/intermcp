#!/usr/bin/env node

/**
 * InterMCP Node.js CLI Runner
 * Spawns the native intermcp binary or builds it automatically if not compiled.
 */

const { spawn } = require("child_process");
const path = require("path");
const fs = require("fs");

const isWin = process.platform === "win32";
const binaryName = isWin ? "intermcp.exe" : "intermcp";

// Candidates for binary location
const candidates = [
  path.join(__dirname, "..", "target", "release", binaryName),
  path.join(__dirname, "..", "target", "debug", binaryName),
  path.join(__dirname, binaryName),
];

let targetBin = candidates.find(p => fs.existsSync(p));

if (!targetBin) {
  // If not compiled yet, invoke via cargo run directly
  const cargoArgs = ["run", "--manifest-path", path.join(__dirname, "..", "Cargo.toml"), "--", ...process.argv.slice(2)];
  const proc = spawn("cargo", cargoArgs, { stdio: "inherit" });
  proc.on("exit", (code) => process.exit(code || 0));
} else {
  const proc = spawn(targetBin, process.argv.slice(2), { stdio: "inherit" });
  proc.on("exit", (code) => process.exit(code || 0));
}
