<?php

declare(strict_types=1);

require_once __DIR__ . '/../php/src/Client.php';

use InterMcp\Client;

echo "🚀 Connecting PHP client to InterMCP Trust Runtime...\n";

$client = new Client();
$init = $client->start();

echo "✅ Protocol handshake successful! Server protocol: " . ($init['protocolVersion'] ?? 'unknown') . "\n";

$tools = $client->listTools();
echo "📦 Registered tools (" . count($tools) . "):\n";
foreach (array_slice($tools, 0, 5) as $tool) {
    echo "   • {$tool['name']}: " . substr($tool['description'] ?? '', 0, 50) . "...\n";
}

echo "\n🔍 Executing system_info tool:\n";
$result = $client->callTool('system_info', []);
echo json_encode($result, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n";

$client->close();
echo "\n🎉 Done!\n";
