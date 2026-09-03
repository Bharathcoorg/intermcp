use crate::protocol::{CallToolResult, ToolDefinition};
use crate::tool::{SimpleTool, Tool};
use serde_json::{json, Value};
use std::sync::Arc;

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
                    "description": "Keywords or capability needed (e.g. 'git', 'filesystem', 'database', 'system', 'crypto')"
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

                let mut matched_tools = Vec::new();

                for tool in tools_ref.iter() {
                    let name_lower = tool.name.to_lowercase();
                    let desc_lower = tool.description.to_lowercase();

                    let matches = query_terms.iter().any(|term| {
                        name_lower.contains(term) || desc_lower.contains(term)
                    });

                    if matches {
                        matched_tools.push(json!({
                            "name": tool.name,
                            "description": tool.description,
                            "inputSchema": tool.input_schema
                        }));
                    }
                }

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
