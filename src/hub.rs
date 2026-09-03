use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::error::FastMcpError;
use crate::protocol::{CallToolResult, JsonRpcResponse, ToolDefinition};
use crate::tool::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubConfig {
    pub servers: Vec<UpstreamServerConfig>,
}

#[allow(dead_code)]
struct UpstreamProcess {
    name: String,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    _child: Child,
    request_id: u64,
}

impl UpstreamProcess {
    async fn spawn(config: &UpstreamServerConfig) -> Result<Self, FastMcpError> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = cmd.spawn().map_err(|e| {
            FastMcpError::ToolExecution(format!(
                "Failed to spawn upstream MCP server '{}': {}",
                config.name, e
            ))
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            FastMcpError::ToolExecution("Failed to acquire upstream stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            FastMcpError::ToolExecution("Failed to acquire upstream stdout".into())
        })?;
        let reader = BufReader::new(stdout);

        let mut proc = Self {
            name: config.name.clone(),
            stdin,
            reader,
            _child: child,
            request_id: 0,
        };

        // Initialize upstream
        let init_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": { "name": "intermcp-hub", "version": "0.1.0" }
            }
        });

        let _ = proc.send_raw(&init_req.to_string()).await?;
        Ok(proc)
    }

    async fn send_raw(&mut self, json_str: &str) -> Result<String, FastMcpError> {
        self.stdin
            .write_all(json_str.as_bytes())
            .await
            .map_err(FastMcpError::Io)?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(FastMcpError::Io)?;
        self.stdin.flush().await.map_err(FastMcpError::Io)?;

        let read_future = async {
            loop {
                let mut line = String::new();
                let bytes = self
                    .reader
                    .read_line(&mut line)
                    .await
                    .map_err(FastMcpError::Io)?;
                if bytes == 0 {
                    return Err(FastMcpError::ToolExecution(format!(
                        "Upstream MCP process '{}' terminated or closed stdout stream unexpectedly",
                        self.name
                    )));
                }
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    return Ok(line);
                }
            }
        };

        match tokio::time::timeout(std::time::Duration::from_secs(30), read_future).await {
            Ok(res) => res,
            Err(_) => Err(FastMcpError::ToolExecution(format!(
                "Upstream MCP process '{}' timed out after 30 seconds",
                self.name
            ))),
        }
    }

    async fn list_tools(&mut self) -> Result<Vec<ToolDefinition>, FastMcpError> {
        self.request_id += 1;
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "tools/list",
            "params": {}
        });

        let resp_str = self.send_raw(&req.to_string()).await?;
        let resp: JsonRpcResponse =
            serde_json::from_str(&resp_str).map_err(FastMcpError::Serialization)?;

        if let Some(res) = resp.result {
            let list_res: crate::protocol::ListToolsResult =
                serde_json::from_value(res).map_err(FastMcpError::Serialization)?;
            Ok(list_res.tools)
        } else {
            Ok(Vec::new())
        }
    }

    async fn call_tool(
        &mut self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<CallToolResult, FastMcpError> {
        self.request_id += 1;
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": args
            }
        });

        let resp_str = self.send_raw(&req.to_string()).await?;
        let resp: JsonRpcResponse =
            serde_json::from_str(&resp_str).map_err(FastMcpError::Serialization)?;

        if let Some(res) = resp.result {
            let tool_res: CallToolResult =
                serde_json::from_value(res).map_err(FastMcpError::Serialization)?;
            Ok(tool_res)
        } else if let Some(err) = resp.error {
            Ok(CallToolResult::error(format!(
                "Upstream error [{}]: {}",
                err.code, err.message
            )))
        } else {
            Ok(CallToolResult::error("Empty upstream response"))
        }
    }
}

/// Upstream proxy tool that routes calls to a child MCP server
#[allow(dead_code)]
pub struct ProxiedTool {
    upstream_name: String,
    original_tool_name: String,
    prefixed_name: String,
    description: String,
    input_schema: serde_json::Value,
    proc: Arc<Mutex<UpstreamProcess>>,
}

#[async_trait::async_trait]
impl Tool for ProxiedTool {
    fn name(&self) -> &str {
        &self.prefixed_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<CallToolResult, FastMcpError> {
        let mut proc_guard = self.proc.lock().await;
        proc_guard
            .call_tool(&self.original_tool_name, arguments)
            .await
    }
}

/// Creates tools multiplexed from multiple external MCP servers
pub async fn load_hub_tools(config: HubConfig) -> Result<Vec<Box<dyn Tool>>, FastMcpError> {
    let mut all_tools: Vec<Box<dyn Tool>> = Vec::new();

    for srv_cfg in config.servers {
        info!(
            "Spawning and aggregating upstream MCP server '{}'...",
            srv_cfg.name
        );
        match UpstreamProcess::spawn(&srv_cfg).await {
            Ok(mut proc) => match proc.list_tools().await {
                Ok(tools) => {
                    info!("Discovered {} tools from '{}'", tools.len(), srv_cfg.name);
                    let shared_proc = Arc::new(Mutex::new(proc));
                    for tool in tools {
                        let prefixed = format!("{}__{}", srv_cfg.name, tool.name);
                        all_tools.push(Box::new(ProxiedTool {
                            upstream_name: srv_cfg.name.clone(),
                            original_tool_name: tool.name,
                            prefixed_name: prefixed,
                            description: tool.description,
                            input_schema: tool.input_schema,
                            proc: Arc::clone(&shared_proc),
                        }));
                    }
                }
                Err(e) => {
                    error!("Failed to list tools from '{}': {}", srv_cfg.name, e);
                }
            },
            Err(e) => {
                error!(
                    "Failed to initialize upstream server '{}': {}",
                    srv_cfg.name, e
                );
            }
        }
    }

    Ok(all_tools)
}
