use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

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
        let output = Command::new(binary).args(&resolved_args).output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                if out.status.success() {
                    Ok(CallToolResult::text(stdout))
                } else {
                    Ok(CallToolResult::error(format!(
                        "Command exited with status: {}\n{}",
                        out.status, stderr
                    )))
                }
            }
            Err(e) => Ok(CallToolResult::error(format!(
                "Failed to execute '{}': {}",
                binary, e
            ))),
        }
    }
}

/// Load declarative tools from a JSON manifest file
pub fn load_manifest_tools(path: &Path) -> Result<Vec<Box<dyn Tool>>, FastMcpError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| FastMcpError::ToolExecution(format!("Failed to read manifest file: {}", e)))?;

    let manifest: ManifestConfig =
        serde_json::from_str(&content).map_err(FastMcpError::Serialization)?;

    let tools = manifest
        .tools
        .into_iter()
        .map(|t| Box::new(DeclarativeTool::new(t)) as Box<dyn Tool>)
        .collect();

    Ok(tools)
}
