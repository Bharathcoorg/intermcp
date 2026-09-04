/**
 * InterMCP Node.js Client Example
 * Connects to the native InterMCP binary over standard stdio JSON-RPC 2024-11-05 protocol.
 */

const { InterMcpClient } = require("../index");

async function main() {
  console.log("🚀 Connecting Node.js client to InterMCP Trust Runtime...");

  const client = new InterMcpClient();
  await client.start();

  console.log("✅ Protocol handshake successful!");

  const tools = await client.listTools();
  console.log(`📦 Registered tools (${tools.length}):`);
  for (const tool of tools.slice(0, 5)) {
    const desc = (tool.description || "").slice(0, 50);
    console.log(`   • ${tool.name}: ${desc}...`);
  }

  console.log("\n🔍 Executing system_info tool:");
  const result = await client.callTool("system_info", {});
  console.log(JSON.stringify(result, null, 2));

  client.stop();
  console.log("\n🎉 Done!");
}

main().catch((err) => {
  console.error("Error running Node.js client:", err);
  process.exit(1);
});
