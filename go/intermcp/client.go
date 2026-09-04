package intermcp

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sync"
	"sync/atomic"
)

// ToolDefinition represents an MCP tool definition returned by tools/list
type ToolDefinition struct {
	Name        string          `json:"name"`
	Description string          `json:"description,omitempty"`
	InputSchema json.RawMessage `json:"inputSchema,omitempty"`
}

// CallResult represents the output from a tools/call invocation
type CallResult struct {
	Content []struct {
		Type string `json:"type"`
		Text string `json:"text,omitempty"`
	} `json:"content"`
	IsError bool `json:"isError,omitempty"`
}

type jsonRpcRequest struct {
	JsonRpc string      `json:"jsonrpc"`
	ID      uint64      `json:"id"`
	Method  string      `json:"method"`
	Params  interface{} `json:"params,omitempty"`
}

type jsonRpcResponse struct {
	JsonRpc string          `json:"jsonrpc"`
	ID      uint64          `json:"id"`
	Result  json.RawMessage `json:"result,omitempty"`
	Error   *struct {
		Code    int    `json:"code"`
		Message string `json:"message"`
	} `json:"error,omitempty"`
}

// Client manages communication with the native InterMCP Trust Runtime over stdio
type Client struct {
	binaryPath string
	cmd        *exec.Cmd
	stdin      io.WriteCloser
	scanner    *bufio.Scanner
	reqCounter uint64
	mu         sync.Mutex
}

// NewClient creates a new InterMCP Go client. If binaryPath is empty, it discovers the binary automatically.
func NewClient(binaryPath string) *Client {
	if binaryPath == "" {
		binaryPath = discoverBinary()
	}
	return &Client{
		binaryPath: binaryPath,
	}
}

func discoverBinary() string {
	exeName := "intermcp"
	if runtime.GOOS == "windows" {
		exeName = "intermcp.exe"
	}

	if envBin := os.Getenv("INTERMCP_BIN"); envBin != "" {
		if _, err := os.Stat(envBin); err == nil {
			return envBin
		}
	}

	home, _ := os.UserHomeDir()
	candidates := []string{
		filepath.Join(home, ".intermcp", "bin", exeName),
		filepath.Join("..", "target", "release", exeName),
		filepath.Join(".", "target", "release", exeName),
		exeName,
	}

	for _, c := range candidates {
		if _, err := os.Stat(c); err == nil {
			return c
		}
	}

	return exeName
}

// Start launches the native InterMCP process and completes the protocol handshake
func (c *Client) Start() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	c.cmd = exec.Command(c.binaryPath, "serve")
	stdin, err := c.cmd.StdinPipe()
	if err != nil {
		return fmt.Errorf("failed to create stdin pipe: %w", err)
	}

	stdout, err := c.cmd.StdoutPipe()
	if err != nil {
		return fmt.Errorf("failed to create stdout pipe: %w", err)
	}

	c.stdin = stdin
	c.scanner = bufio.NewScanner(stdout)

	if err := c.cmd.Start(); err != nil {
		return fmt.Errorf("failed to start intermcp binary '%s': %w", c.binaryPath, err)
	}

	// MCP 2024-11-05 Handshake
	initParams := map[string]interface{}{
		"protocolVersion": "2024-11-05",
		"clientInfo": map[string]string{
			"name":    "intermcp-go-sdk",
			"version": "0.1.0",
		},
		"capabilities": map[string]interface{}{},
	}

	_, err = c.sendRequestLocked("initialize", initParams)
	if err != nil {
		c.Close()
		return fmt.Errorf("handshake failed: %w", err)
	}

	return nil
}

func (c *Client) sendRequestLocked(method string, params interface{}) (json.RawMessage, error) {
	id := atomic.AddUint64(&c.reqCounter, 1)
	req := jsonRpcRequest{
		JsonRpc: "2.0",
		ID:      id,
		Method:  method,
		Params:  params,
	}

	data, err := json.Marshal(req)
	if err != nil {
		return nil, err
	}
	data = append(data, '\n')

	if _, err := c.stdin.Write(data); err != nil {
		return nil, fmt.Errorf("failed to write to intermcp: %w", err)
	}

	if !c.scanner.Scan() {
		if err := c.scanner.Err(); err != nil {
			return nil, fmt.Errorf("read error: %w", err)
		}
		return nil, fmt.Errorf("intermcp process closed output pipe unexpectedly")
	}

	var resp jsonRpcResponse
	if err := json.Unmarshal(c.scanner.Bytes(), &resp); err != nil {
		return nil, fmt.Errorf("invalid json-rpc response: %w", err)
	}

	if resp.Error != nil {
		return nil, fmt.Errorf("mcp error (%d): %s", resp.Error.Code, resp.Error.Message)
	}

	return resp.Result, nil
}

// Request sends a generic JSON-RPC request to the InterMCP server
func (c *Client) Request(method string, params interface{}) (json.RawMessage, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.sendRequestLocked(method, params)
}

// ListTools returns all registered MCP tools
func (c *Client) ListTools() ([]ToolDefinition, error) {
	raw, err := c.Request("tools/list", map[string]interface{}{})
	if err != nil {
		return nil, err
	}

	var res struct {
		Tools []ToolDefinition `json:"tools"`
	}
	if err := json.Unmarshal(raw, &res); err != nil {
		return nil, err
	}
	return res.Tools, nil
}

// CallTool executes an MCP tool with SafeFS, secret redaction, and signed receipts
func (c *Client) CallTool(name string, args map[string]interface{}) (*CallResult, error) {
	params := map[string]interface{}{
		"name":      name,
		"arguments": args,
	}

	raw, err := c.Request("tools/call", params)
	if err != nil {
		return nil, err
	}

	var res CallResult
	if err := json.Unmarshal(raw, &res); err != nil {
		return nil, err
	}
	return &res, nil
}

// Close gracefully terminates the InterMCP subprocess
func (c *Client) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.stdin != nil {
		c.stdin.Close()
	}
	if c.cmd != nil && c.cmd.Process != nil {
		return c.cmd.Process.Kill()
	}
	return nil
}
