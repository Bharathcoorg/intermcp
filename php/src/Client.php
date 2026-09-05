<?php

declare(strict_types=1);

namespace InterMcp;

use RuntimeException;

class Client
{
    private string $binaryPath;
    /** @var resource|null */
    private $process = null;
    /** @var array<int, resource> */
    private array $pipes = [];
    private int $requestId = 0;

    public function __construct(?string $binaryPath = null)
    {
        $this->binaryPath = $binaryPath ?? $this->discoverBinary();
    }

    private function discoverBinary(): string
    {
        $isWindows = strtoupper(substr(PHP_OS, 0, 3)) === 'WIN';
        $exeName = $isWindows ? 'intermcp.exe' : 'intermcp';

        $envBin = getenv('INTERMCP_BIN');
        if ($envBin && file_exists($envBin)) {
            return $envBin;
        }

        $home = getenv('HOME') ?: (getenv('USERPROFILE') ?: '');
        $candidates = [
            $home . DIRECTORY_SEPARATOR . '.intermcp' . DIRECTORY_SEPARATOR . 'bin' . DIRECTORY_SEPARATOR . $exeName,
            dirname(__DIR__, 2) . DIRECTORY_SEPARATOR . 'target' . DIRECTORY_SEPARATOR . 'release' . DIRECTORY_SEPARATOR . $exeName,
            $exeName,
        ];

        foreach ($candidates as $candidate) {
            if (file_exists($candidate)) {
                return $candidate;
            }
        }

        return $exeName;
    }

    public function start(): array
    {
        $descriptorSpec = [
            0 => ['pipe', 'r'], // stdin
            1 => ['pipe', 'w'], // stdout
            2 => ['pipe', 'w'], // stderr
        ];

        // AUDIT-17: Use array form to avoid shell interpretation of binary path
        $this->process = proc_open(
            [$this->binaryPath, 'serve'],
            $descriptorSpec,
            $this->pipes
        );

        if (!is_resource($this->process)) {
            throw new RuntimeException("Failed to spawn InterMCP process: {$this->binaryPath}");
        }

        // Complete MCP 2024-11-05 Handshake
        $initParams = [
            'protocolVersion' => '2024-11-05',
            'clientInfo' => [
                'name' => 'intermcp-php-sdk',
                'version' => '0.2.1',
            ],
            'capabilities' => (object)[],
        ];

        return $this->request('initialize', $initParams);
    }

    public function request(string $method, array $params = []): array
    {
        if (!is_resource($this->process) || !isset($this->pipes[0], $this->pipes[1])) {
            throw new RuntimeException('InterMCP client is not running. Call ->start() first.');
        }

        $this->requestId++;
        $payload = [
            'jsonrpc' => '2.0',
            'id' => $this->requestId,
            'method' => $method,
            'params' => (object)$params,
        ];

        $rawMsg = json_encode($payload, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE) . "\n";
        fwrite($this->pipes[0], $rawMsg);
        fflush($this->pipes[0]);

        $line = '';
        while (($chunk = fgets($this->pipes[1])) !== false) {
            $line .= $chunk;
            if (substr($chunk, -1) === "\n") {
                break;
            }
        }
        if ($line === '') {
            $err = stream_get_contents($this->pipes[2]) ?: 'Unknown termination';
            throw new RuntimeException("InterMCP engine exited unexpectedly: {$err}");
        }

        $response = json_decode($line, true);
        if (isset($response['error'])) {
            $code = $response['error']['code'] ?? 0;
            $msg = $response['error']['message'] ?? 'Unknown error';
            throw new RuntimeException("MCP Error ({$code}): {$msg}");
        }

        return $response['result'] ?? [];
    }

    public function listTools(): array
    {
        $res = $this->request('tools/list');
        return $res['tools'] ?? [];
    }

    public function callTool(string $name, array $arguments = []): array
    {
        return $this->request('tools/call', [
            'name' => $name,
            'arguments' => (object)$arguments,
        ]);
    }

    public function listResources(): array
    {
        $res = $this->request('resources/list');
        return $res['resources'] ?? [];
    }

    public function readResource(string $uri): array
    {
        return $this->request('resources/read', ['uri' => $uri]);
    }

    public function close(): void
    {
        if (isset($this->pipes[0]) && is_resource($this->pipes[0])) {
            fclose($this->pipes[0]);
        }
        if (isset($this->pipes[1]) && is_resource($this->pipes[1])) {
            fclose($this->pipes[1]);
        }
        if (isset($this->pipes[2]) && is_resource($this->pipes[2])) {
            fclose($this->pipes[2]);
        }
        if (is_resource($this->process)) {
            proc_terminate($this->process);
            proc_close($this->process);
            $this->process = null;
        }
    }

    public function __destruct()
    {
        $this->close();
    }
}
