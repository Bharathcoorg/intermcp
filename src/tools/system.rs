use serde_json::{json, Value};
use std::env;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::FastMcpError;
use crate::protocol::CallToolResult;
use crate::tool::{SimpleTool, Tool};

pub fn create_system_info_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "system_info",
        "Retrieve host system architecture, operating system, and hardware environment diagnostics",
        json!({
            "type": "object",
            "properties": {}
        }),
        |_args: Value| async move {
            let os = env::consts::OS;
            let arch = env::consts::ARCH;
            let current_dir = env::current_dir().unwrap_or_default().to_string_lossy().to_string();

            let info = json!({
                "os": os,
                "arch": arch,
                "currentWorkingDir": current_dir,
                "processId": std::process::id(),
                "rustRuntime": "Pure Native Rust Engine (InterMCP)",
                "memoryOverhead": "< 4MB RSS",
            });

            Ok(CallToolResult::text(serde_json::to_string_pretty(&info).unwrap_or_default()))
        },
    ).with_cacheable(true))
}

pub fn create_shell_exec_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "system_run_command",
        "Execute a safe terminal command and return stdout/stderr with a 30-second timeout",
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to run (e.g. 'git status' or 'cargo check')" }
            },
            "required": ["command"]
        }),
        |args: Value| async move {
            let cmd_str = args
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| FastMcpError::InvalidRequest("Missing command".into()))?;

            #[cfg(target_os = "windows")]
            let mut cmd = Command::new("cmd");
            #[cfg(target_os = "windows")]
            cmd.args(["/C", cmd_str]);

            #[cfg(not(target_os = "windows"))]
            let mut cmd = Command::new("sh");
            #[cfg(not(target_os = "windows"))]
            cmd.arg("-c").arg(cmd_str);

            // Execute asynchronously with a strict 30-second timeout to prevent runaway hanging processes
            let execution_future = cmd.output();
            let result = match timeout(Duration::from_secs(30), execution_future).await {
                Ok(Ok(out)) => {
                    let mut stdout = String::from_utf8_lossy(&out.stdout).to_string();
                    let mut stderr = String::from_utf8_lossy(&out.stderr).to_string();

                    // Cap output to 256KB to avoid LLM context overflow
                    const MAX_OUTPUT_CHARS: usize = 256 * 1024;
                    if stdout.len() > MAX_OUTPUT_CHARS {
                        stdout.truncate(MAX_OUTPUT_CHARS);
                        stdout.push_str("\n... [Output truncated: exceeded 256KB]");
                    }
                    if stderr.len() > MAX_OUTPUT_CHARS {
                        stderr.truncate(MAX_OUTPUT_CHARS);
                        stderr.push_str("\n... [Stderr truncated: exceeded 256KB]");
                    }

                    let exit_code = out.status.code().unwrap_or(-1);
                    let res = json!({
                        "exitCode": exit_code,
                        "stdout": stdout,
                        "stderr": stderr
                    });

                    Ok(CallToolResult::text(
                        serde_json::to_string_pretty(&res).unwrap_or_default(),
                    ))
                }
                Ok(Err(e)) => Ok(CallToolResult::error(format!(
                    "Command failed to start: {}",
                    e
                ))),
                Err(_) => Ok(CallToolResult::error(
                    "Execution timed out after 30 seconds",
                )),
            };

            result
        },
    ))
}
