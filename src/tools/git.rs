use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::protocol::CallToolResult;
use crate::tool::{SimpleTool, Tool};

pub fn create_git_status_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "git_status",
        "Inspect the current Git working tree status (staged, modified, untracked files)",
        json!({
            "type": "object",
            "properties": {
                "repo_path": {
                    "type": "string",
                    "description": "Path to the git repository (defaults to current directory)"
                }
            }
        }),
        |args: Value| async move {
            let path = args
                .get("repo_path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let mut cmd = Command::new("git");
            cmd.args(["-C", path, "status", "--short", "--branch"]);

            match timeout(Duration::from_secs(15), cmd.output()).await {
                Ok(Ok(out)) => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    if text.trim().is_empty() {
                        Ok(CallToolResult::text(
                            "Git repository is clean. No uncommitted changes.",
                        ))
                    } else {
                        Ok(CallToolResult::text(text))
                    }
                }
                Ok(Err(e)) => Ok(CallToolResult::error(format!(
                    "Failed to run git status: {}",
                    e
                ))),
                Err(_) => Ok(CallToolResult::error(
                    "git status timed out after 15 seconds",
                )),
            }
        },
    ))
}

pub fn create_git_diff_tool() -> Box<dyn Tool> {
    Box::new(SimpleTool::new(
        "git_diff",
        "Show unstaged or staged git changes for code review or debugging",
        json!({
            "type": "object",
            "properties": {
                "repo_path": {
                    "type": "string",
                    "description": "Path to the git repository (defaults to current directory)"
                },
                "staged": {
                    "type": "boolean",
                    "description": "Show staged diff instead of working tree diff"
                }
            }
        }),
        |args: Value| async move {
            let path = args
                .get("repo_path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let staged = args
                .get("staged")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut cmd = Command::new("git");
            cmd.args(["-C", path, "diff"]);
            if staged {
                cmd.arg("--staged");
            }

            match timeout(Duration::from_secs(15), cmd.output()).await {
                Ok(Ok(out)) => {
                    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
                    if text.trim().is_empty() {
                        Ok(CallToolResult::text("No git diff detected."))
                    } else {
                        // Cap diff output to 256KB to avoid overflowing model context
                        const MAX_DIFF_CHARS: usize = 256 * 1024;
                        if text.len() > MAX_DIFF_CHARS {
                            text.truncate(MAX_DIFF_CHARS);
                            text.push_str("\n... [Diff truncated: exceeded 256KB]");
                        }
                        Ok(CallToolResult::text(text))
                    }
                }
                Ok(Err(e)) => Ok(CallToolResult::error(format!(
                    "Failed to run git diff: {}",
                    e
                ))),
                Err(_) => Ok(CallToolResult::error("git diff timed out after 15 seconds")),
            }
        },
    ))
}
