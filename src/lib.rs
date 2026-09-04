pub mod auto_config;
pub mod cache;
pub mod config;
pub mod discovery;
pub mod error;
pub mod guardrails;
pub mod http_server;
pub mod hub;
pub mod manifest;
pub mod prompt;
pub mod protocol;
pub mod reaper;
pub mod receipts;
pub mod record;
pub mod resource;
pub mod sandbox;
pub mod server;
pub mod smac;
pub mod tool;
pub mod tools;
pub mod vault_lock;

pub use auto_config::{configure_all_ides, configure_all_ides as auto_configure_all_ides, SetupResult};
pub use cache::ToolCache;
pub use config::PolicyConfig;
pub use discovery::create_tool_discovery_tool;
pub use error::FastMcpError;
pub use guardrails::GuardrailPolicy;
pub use http_server::{run_http_server, HttpServerConfig};
pub use hub::{load_hub_tools, load_hub_tools_with_firewall, HubConfig, PinnedToolContract, SupplyChainFirewall, UpstreamServerConfig};
pub use manifest::{load_manifest_tools, DeclarativeTool, ManifestConfig, ManifestTool};
pub use prompt::{Prompt, SimplePrompt};
pub use protocol::{CallToolResult, ContentItem, JsonRpcRequest, JsonRpcResponse, ToolDefinition};
pub use receipts::{canonicalize_json, hash_canonical_json, verify_receipt_chain_file, ExecutionReceipt, ReceiptBook, ReceiptStatus, SignedReceiptRecord, VerificationSummary};
pub use record::{FrameDirection, ReplaySummary, SessionFrame, SessionRecorder, SessionReplayer};
pub use resource::{Resource, SimpleResource};
pub use sandbox::SandboxPolicy;
pub use server::{mask_secrets, Server};
pub use smac::{verify_smac_log, SmacEntry, SmacLogger};
pub use tool::{SimpleTool, Tool};
pub use vault_lock::{PendingActionSummary, TimeLockedVault};

pub type Result<T> = std::result::Result<T, FastMcpError>;

/// Instantiate standard InterMCP server with universal tools, resources, and prompts
pub fn create_default_server() -> Server {
    let mut server = Server::new("intermcp", env!("CARGO_PKG_VERSION"));
    server.add_tools(tools::universal_toolset(None));
    server.add_resource(resource::create_system_resource());
    server.add_prompt(prompt::create_code_review_prompt());
    server
}

/// Instantiate an InterMCP server with SafeFS sandboxing and optional caching
pub fn create_sandboxed_server(
    sandbox: SandboxPolicy,
    cache_ttl: Option<std::time::Duration>,
) -> Server {
    let mut server = Server::new("intermcp-sandboxed", env!("CARGO_PKG_VERSION"));
    if let Some(ttl) = cache_ttl {
        server = server.with_cache(ttl);
    }
    server.add_tools(tools::universal_toolset(Some(sandbox)));
    server.add_resource(resource::create_system_resource());
    server.add_prompt(prompt::create_code_review_prompt());
    server
}

/// Instantiate an InterMCP server with dynamic semantic tool discovery enabled (saves 85% prompt tokens)
pub fn create_smart_discovery_server() -> Server {
    let mut server = create_default_server();
    let defs = server.list_tool_definitions();
    let discovery_tool = discovery::create_tool_discovery_tool(defs);
    server.add_tool(discovery_tool);
    server
}

/// Instantiate an InterMCP server with an optional plugin (e.g. "gravity" for Omni-VM)
pub fn create_server_with_plugin(plugin: &str) -> Server {
    let mut server = create_default_server();
    server.add_tools(tools::plugin_toolset(plugin));
    server
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[tokio::test]
    async fn test_mcp_initialize() {
        let server = create_default_server();
        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "claude-desktop",
                    "version": "0.1.0"
                }
            }
        });

        let resp_str = server.handle_raw_message(&init_req.to_string()).await;
        assert!(resp_str.is_some());
        let resp: JsonRpcResponse = serde_json::from_str(&resp_str.unwrap()).unwrap();
        assert_eq!(resp.id, json!(1));
        assert!(resp.error.is_none());
        assert!(resp.result.is_some());
    }

    #[tokio::test]
    async fn test_mcp_smart_discovery_tool() {
        let server = create_smart_discovery_server();

        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "tools/call",
            "params": {
                "name": "intermcp_search_tools",
                "arguments": { "query": "git" }
            }
        })
        .to_string();

        let resp_str = server.handle_raw_message(&call_req).await.unwrap();
        let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
        let tool_res: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert!(!tool_res.is_error);
        if let ContentItem::Text { text } = &tool_res.content[0] {
            assert!(text.contains("git_status"));
            assert!(text.contains("git_diff"));
        } else {
            panic!("Expected text output");
        }
    }

    #[tokio::test]
    async fn test_mcp_loop_breaker_guardrail() {
        let server = create_default_server().with_guardrails(100, 3); // loop threshold: 3 calls

        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 50,
            "method": "tools/call",
            "params": {
                "name": "system_info",
                "arguments": {}
            }
        })
        .to_string();

        // Call 1, 2, 3 should succeed
        let _ = server.handle_raw_message(&call_req).await;
        let _ = server.handle_raw_message(&call_req).await;
        let _ = server.handle_raw_message(&call_req).await;

        // Call 4: Loop Breaker must trigger!
        let resp4_str = server.handle_raw_message(&call_req).await.unwrap();
        let resp4: JsonRpcResponse = serde_json::from_str(&resp4_str).unwrap();
        let tool_res: CallToolResult = serde_json::from_value(resp4.result.unwrap()).unwrap();

        assert!(tool_res.is_error);
        if let ContentItem::Text { text } = &tool_res.content[0] {
            assert!(text.contains("InterMCP Loop Breaker: Infinite loop detected"));
        } else {
            panic!("Expected loop breaker error text");
        }
    }

    #[tokio::test]
    async fn test_mcp_tool_caching() {
        let server = create_default_server().with_cache(Duration::from_secs(60));

        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "system_info",
                "arguments": {}
            }
        })
        .to_string();

        let resp1 = server.handle_raw_message(&call_req).await;
        assert!(resp1.is_some());

        let resp2 = server.handle_raw_message(&call_req).await;
        assert!(resp2.is_some());

        let (hits, misses, count) = server.cache_stats().unwrap();
        assert_eq!(misses, 1);
        assert_eq!(hits, 1);
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_mcp_safefs_sandbox_violation() {
        let sandbox = SandboxPolicy::new(vec![std::path::PathBuf::from("./src")]);
        let server = create_sandboxed_server(sandbox, None);

        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {
                "name": "fs_read_file",
                "arguments": { "path": "../../Cargo.toml" }
            }
        })
        .to_string();

        let resp_str = server.handle_raw_message(&call_req).await.unwrap();
        let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
        let tool_res: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();

        assert!(tool_res.is_error);
        if let ContentItem::Text { text } = &tool_res.content[0] {
            assert!(text.contains("SafeFS Security Violation"));
        } else {
            panic!("Expected SafeFS violation text");
        }
    }

    #[tokio::test]
    async fn test_mcp_resources() {
        let server = create_default_server();

        let list_req = json!({
            "jsonrpc": "2.0",
            "id": "res-1",
            "method": "resources/list",
            "params": {}
        });
        let resp_str = server
            .handle_raw_message(&list_req.to_string())
            .await
            .unwrap();
        let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
        let list_res: protocol::ListResourcesResult =
            serde_json::from_value(resp.result.unwrap()).unwrap();
        assert!(list_res
            .resources
            .iter()
            .any(|r| r.uri == "system://diagnostics"));
    }

    #[tokio::test]
    async fn test_safefs_nested_uncreated_path() {
        let sandbox = SandboxPolicy::new(vec![std::path::PathBuf::from("./src")]);

        // Legitimate nested uncreated file inside ./src must be allowed
        let valid_path = std::path::Path::new("./src/uncreated_sub/nested/file.rs");
        assert!(sandbox.validate_path(valid_path).is_ok());

        // Path traversal using ../ outside allowed root must be blocked
        let traversal_path = std::path::Path::new("./src/uncreated_sub/../../Cargo.toml");
        assert!(sandbox.validate_path(traversal_path).is_err());
    }

    #[tokio::test]
    async fn test_cache_safety_on_mutations() {
        // Filesystem and git tools must NEVER be cached to prevent serving stale data to agents
        let server = create_default_server().with_cache(Duration::from_secs(60));

        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "tools/call",
            "params": {
                "name": "git_status",
                "arguments": {}
            }
        })
        .to_string();

        let _ = server.handle_raw_message(&call_req).await;
        let _ = server.handle_raw_message(&call_req).await;

        let (hits, _misses, _count) = server.cache_stats().unwrap();
        // git_status is not cacheable, so hits must remain 0!
        assert_eq!(hits, 0);
    }

    #[tokio::test]
    async fn test_safefs_secret_shield() {
        let sandbox = SandboxPolicy::new(vec![std::path::PathBuf::from(".")]);
        // .env file inside allowed root must still be blocked by Secret Shield
        let env_path = std::path::Path::new("./.env");
        let res = sandbox.validate_path(env_path);
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("Secret Shield"));

        // id_rsa private key must be blocked
        let rsa_path = std::path::Path::new("./id_rsa");
        assert!(sandbox.validate_path(rsa_path).is_err());
    }

    #[tokio::test]
    async fn test_safeshell_destructive_command_blocked() {
        let server = create_default_server();
        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 101,
            "method": "tools/call",
            "params": {
                "name": "system_run_command",
                "arguments": { "command": "rm -rf /" }
            }
        })
        .to_string();

        let resp_str = server.handle_raw_message(&call_req).await.unwrap();
        let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
        let tool_res: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert!(tool_res.is_error);
        if let ContentItem::Text { text } = &tool_res.content[0] {
            assert!(text.contains("Safe-Shell Violation"));
        } else {
            panic!("Expected Safe-Shell violation text");
        }
    }

    #[tokio::test]
    async fn test_semantic_discovery_synonyms() {
        let server = create_smart_discovery_server();

        // Query with semantic intent synonym "save code" should discover fs_write_file
        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 102,
            "method": "tools/call",
            "params": {
                "name": "intermcp_search_tools",
                "arguments": { "query": "save code" }
            }
        })
        .to_string();

        let resp_str = server.handle_raw_message(&call_req).await.unwrap();
        let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
        let tool_res: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert!(!tool_res.is_error);
        if let ContentItem::Text { text } = &tool_res.content[0] {
            assert!(text.contains("fs_write_file"));
        } else {
            panic!("Expected text output with fs_write_file");
        }
    }

    #[tokio::test]
    async fn test_token_budget_sentinel() {
        // Create server with strict 10-token budget (~40 chars)
        let server = create_default_server().with_token_budget(10);

        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 103,
            "method": "tools/call",
            "params": {
                "name": "system_info",
                "arguments": {}
            }
        })
        .to_string();

        let resp_str = server.handle_raw_message(&call_req).await.unwrap();
        let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
        let tool_res: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        // Because system_info output is > 100 chars, it must trigger the budget sentinel
        assert!(tool_res.is_error);
        if let ContentItem::Text { text } = &tool_res.content[0] {
            assert!(text.contains("InterMCP Budget Sentinel"));
        } else {
            panic!("Expected budget sentinel error");
        }
    }

    #[tokio::test]
    async fn test_secret_vault_redaction() {
        std::env::set_var("TEST_INTERMCP_SECRET_TOKEN", "super_secret_api_key_12345");
        let server = create_default_server();

        // If a command echos the secret token, it must be intercepted and masked
        let call_req = json!({
            "jsonrpc": "2.0",
            "id": 104,
            "method": "tools/call",
            "params": {
                "name": "system_run_command",
                "arguments": { "command": "echo super_secret_api_key_12345" }
            }
        })
        .to_string();

        let resp_str = server.handle_raw_message(&call_req).await.unwrap();
        let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
        let tool_res: CallToolResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        if let ContentItem::Text { text } = &tool_res.content[0] {
            assert!(!text.contains("super_secret_api_key_12345"));
            assert!(text.contains("[REDACTED_BY_INTERMCP]"));
        } else {
            panic!("Expected redacted text output");
        }
    }

    #[test]
    fn test_cache_capacity_eviction() {
        let cache = ToolCache::new(Duration::from_secs(300));
        // Insert 1005 items; cache capacity must stay bounded at 1000 items
        for i in 0..1005 {
            let key = format!("val_{}", i);
            cache.set("tool", &json!({ "i": key }), json!({ "res": i }), None);
        }
        let (_, _, count) = cache.stats();
        assert!(count <= 1000);
    }

    #[tokio::test]
    async fn test_guardrail_consecutive_loop_reset() {
        // Threshold: 3 consecutive calls
        let server = create_default_server().with_guardrails(100, 3);

        let call_a = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "system_info", "arguments": {} }
        })
        .to_string();

        let call_b = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "git_status", "arguments": {} }
        })
        .to_string();

        // Alternating calls A, B, A, B should NOT trigger loop breaker
        for _ in 0..5 {
            let _ = server.handle_raw_message(&call_a).await;
            let _ = server.handle_raw_message(&call_b).await;
        }

        // 4 consecutive calls to A MUST trigger loop breaker on call 4
        let _ = server.handle_raw_message(&call_a).await;
        let _ = server.handle_raw_message(&call_a).await;
        let _ = server.handle_raw_message(&call_a).await;
        let resp4_str = server.handle_raw_message(&call_a).await.unwrap();
        let resp4: JsonRpcResponse = serde_json::from_str(&resp4_str).unwrap();
        let tool_res: CallToolResult = serde_json::from_value(resp4.result.unwrap()).unwrap();
        assert!(tool_res.is_error);
        if let ContentItem::Text { text } = &tool_res.content[0] {
            assert!(text.contains("InterMCP Loop Breaker"));
        }
    }

    #[test]
    fn test_hub_config_with_env() {
        let json_data = json!({
            "servers": [
                {
                    "name": "github",
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-github"],
                    "env": {
                        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_12345"
                    }
                }
            ]
        });

        let hub_cfg: HubConfig = serde_json::from_value(json_data).unwrap();
        assert_eq!(
            hub_cfg.servers[0]
                .env
                .get("GITHUB_PERSONAL_ACCESS_TOKEN")
                .unwrap(),
            "ghp_12345"
        );
    }
}
