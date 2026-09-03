use crate::protocol::{CallToolResult, ToolDefinition};
use crate::tool::{SimpleTool, Tool};
use serde_json::{json, Value};
use std::sync::Arc;

const SYNONYM_MAP: &[(&[&str], &[&str])] = &[
    (
        &[
            "write", "save", "create", "edit", "modify", "update", "append", "make",
        ],
        &["fs_write_file"],
    ),
    (
        &[
            "read", "view", "show", "cat", "open", "inspect", "contents", "fetch",
        ],
        &["fs_read_file"],
    ),
    (
        &[
            "list",
            "ls",
            "dir",
            "directory",
            "folder",
            "files",
            "browse",
        ],
        &["fs_list_dir"],
    ),
    (
        &[
            "search", "find", "grep", "lookup", "pattern", "query", "scan",
        ],
        &["fs_search_text"],
    ),
    (
        &[
            "git",
            "commit",
            "status",
            "branch",
            "staged",
            "uncommitted",
            "vcs",
        ],
        &["git_status", "git_diff"],
    ),
    (
        &["diff", "patch", "changes", "unstaged", "review"],
        &["git_diff"],
    ),
    (
        &[
            "run", "execute", "shell", "terminal", "bash", "cmd", "command", "process",
        ],
        &["system_run_command"],
    ),
    (
        &[
            "system", "cpu", "memory", "ram", "os", "hardware", "info", "specs", "host",
        ],
        &["system_info"],
    ),
    (
        &[
            "dex",
            "swap",
            "gravity",
            "liquidity",
            "omni",
            "token",
            "crypto",
            "price",
            "pool",
        ],
        &[
            "gravity_simulate_swap",
            "gravity_get_market_price",
            "gravity_get_liquidity_pools",
        ],
    ),
];

/// Creates the meta-tool `intermcp_search_tools` which enables LLMs to dynamically
/// search and pull only relevant tools just-in-time, saving 85% of prompt token bloat.
pub fn create_tool_discovery_tool(all_tool_defs: Vec<ToolDefinition>) -> Box<dyn Tool> {
    let defs = Arc::new(all_tool_defs);

    Box::new(SimpleTool::new(
        "intermcp_search_tools",
        "Dynamically discover and inspect available tools on this system by keyword/intent query (reduces prompt context token bloat)",
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords or capability needed (e.g. 'git', 'save file', 'shell command', 'system hardware')"
                }
            },
            "required": ["query"]
        }),
        move |args: Value| {
            let tools_ref = Arc::clone(&defs);
            async move {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                if query.is_empty() {
                    return Ok(CallToolResult::error("Query parameter cannot be empty"));
                }

                let query_terms: Vec<&str> = query.split_whitespace().collect();

                let mut scored_tools = Vec::new();

                for tool in tools_ref.iter() {
                    let name_lower = tool.name.to_lowercase();
                    let desc_lower = tool.description.to_lowercase();

                    let mut score: u32 = 0;

                    for term in &query_terms {
                        if name_lower == *term {
                            score += 10;
                        } else if name_lower.contains(term) {
                            score += 5;
                        }
                        if desc_lower.contains(term) {
                            score += 3;
                        }

                        // Check semantic intent synonyms
                        for (syn_words, target_tools) in SYNONYM_MAP {
                            if syn_words.contains(term) && target_tools.contains(&tool.name.as_str()) {
                                score += 8;
                            }
                        }
                    }

                    if score > 0 {
                        scored_tools.push((score, tool));
                    }
                }

                // Sort by relevance score descending
                scored_tools.sort_by_key(|b| std::cmp::Reverse(b.0));

                let matched_tools: Vec<Value> = scored_tools
                    .into_iter()
                    .map(|(score, tool)| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "relevanceScore": score,
                            "inputSchema": tool.input_schema
                        })
                    })
                    .collect();

                let result = json!({
                    "query": query,
                    "matchedCount": matched_tools.len(),
                    "tools": matched_tools,
                    "tip": "You can now directly invoke any of these matched tools via standard tools/call"
                });

                Ok(CallToolResult::text(serde_json::to_string_pretty(&result).unwrap_or_default()))
            }
        },
    ))
}
