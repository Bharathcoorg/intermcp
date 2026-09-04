use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

use crate::error::FastMcpError;
use crate::protocol::CallToolResult;
use crate::tool::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestParam {
    #[serde(rename = "type")]
    pub param_type: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestTool {
    pub name: String,
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub params: HashMap<String, ManifestParam>,
    #[serde(default)]
    pub cache_ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestConfig {
    pub tools: Vec<ManifestTool>,
}

pub struct DeclarativeTool {
    tool: ManifestTool,
    schema: Value,
}

impl DeclarativeTool {
    pub fn new(tool: ManifestTool) -> Self {
        let mut properties = json!({});
        let mut required = Vec::new();

        for (param_name, param_def) in &tool.params {
            properties[param_name] = json!({
                "type": param_def.param_type,
                "description": param_def.description
            });
            if param_def.required {
                required.push(param_name.clone());
            }
        }

        let schema = json!({
            "type": "object",
            "properties": properties,
            "required": required
        });

        Self { tool, schema }
    }
}

#[async_trait::async_trait]
impl Tool for DeclarativeTool {
    fn name(&self) -> &str {
        &self.tool.name
    }

    fn description(&self) -> &str {
        &self.tool.description
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, arguments: Value) -> Result<CallToolResult, FastMcpError> {
        // Resolve arguments safely without raw shell interpolation
        let mut resolved_args: Vec<String> = if !self.tool.args.is_empty() {
            self.tool.args.clone()
        } else {
            // Split command string into binary and initial arguments
            let parts: Vec<&str> = self.tool.command.split_whitespace().collect();
            if parts.len() > 1 {
                parts[1..].iter().map(|s| s.to_string()).collect()
            } else {
                Vec::new()
            }
        };

        let binary = if !self.tool.args.is_empty() {
            &self.tool.command
        } else {
            self.tool
                .command
                .split_whitespace()
                .next()
                .unwrap_or(&self.tool.command)
        };

        if let Some(obj) = arguments.as_object() {
            for arg in &mut resolved_args {
                for (k, v) in obj {
                    let placeholder = format!("{{{}}}", k);
                    let val_str = match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    *arg = arg.replace(&placeholder, &val_str);
                }
            }
        }

        // Direct process execution without shell interpreter prevents command injection
        let mut cmd = Command::new(binary);
        cmd.args(&resolved_args);
        crate::tools::system::apply_isolated_environment(&mut cmd);
        crate::reaper::configure_child_isolation(&mut cmd);

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Ok(CallToolResult::error(format!(
                    "Failed to execute '{}': {}",
                    binary, e
                )))
            }
        };

        let mut guard_opt = crate::reaper::ChildIsolationGuard::new(&child).ok();
        let mut stdout_handle = child.stdout.take();
        let mut stderr_handle = child.stderr.take();
        let mut stdout_bytes = Vec::new();
        let mut stderr_bytes = Vec::new();

        let timeout_duration = Duration::from_secs(30);

        tokio::select! {
            status_res = async {
                use tokio::io::AsyncReadExt;
                let out_fut = async {
                    if let Some(ref mut h) = stdout_handle {
                        let _ = h.read_to_end(&mut stdout_bytes).await;
                    }
                };
                let err_fut = async {
                    if let Some(ref mut h) = stderr_handle {
                        let _ = h.read_to_end(&mut stderr_bytes).await;
                    }
                };
                let ((), (), st) = tokio::join!(out_fut, err_fut, child.wait());
                st
            } => {
                match status_res {
                    Ok(status) => {
                        if let Some(ref mut g) = guard_opt {
                            g.disarm();
                        }
                        let mut stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
                        let mut stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

                        const MAX_OUTPUT_CHARS: usize = 256 * 1024;
                        if stdout.len() > MAX_OUTPUT_CHARS {
                            stdout.truncate(MAX_OUTPUT_CHARS);
                            stdout.push_str("\n... [Output truncated: exceeded 256KB]");
                        }
                        if stderr.len() > MAX_OUTPUT_CHARS {
                            stderr.truncate(MAX_OUTPUT_CHARS);
                            stderr.push_str("\n... [Stderr truncated: exceeded 256KB]");
                        }

                        if status.success() {
                            Ok(CallToolResult::text(stdout))
                        } else {
                            Ok(CallToolResult::error(format!(
                                "Command exited with status: {}\n{}",
                                status, stderr
                            )))
                        }
                    }
                    Err(e) => Ok(CallToolResult::error(format!(
                        "Failed to wait for '{}': {}",
                        binary, e
                    ))),
                }
            }
            _ = tokio::time::sleep(timeout_duration) => {
                let _ = child.kill().await;
                Ok(CallToolResult::error(format!(
                    "Command '{}' timed out after 30 seconds",
                    binary
                )))
            }
        }
    }
}

/// Load declarative tools from a JSON manifest file
pub fn load_manifest_tools(path: &Path) -> Result<Vec<Box<dyn Tool>>, FastMcpError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| FastMcpError::ToolExecution(format!("Failed to read manifest file: {}", e)))?;

    let manifest: ManifestConfig =
        serde_json::from_str(&content).map_err(FastMcpError::Serialization)?;

    for tool in &manifest.tools {
        if tool.name.trim().is_empty() {
            return Err(FastMcpError::InvalidRequest(
                "Manifest tool 'name' cannot be empty".into(),
            ));
        }
        if tool.command.trim().is_empty() {
            return Err(FastMcpError::InvalidRequest(format!(
                "Manifest tool '{}' has an empty command",
                tool.name
            )));
        }
    }

    let tools = manifest
        .tools
        .into_iter()
        .map(|t| Box::new(DeclarativeTool::new(t)) as Box<dyn Tool>)
        .collect();

    Ok(tools)
}
