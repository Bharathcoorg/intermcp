use intermcp::protocol::{
    CallToolResult, ContentItem, GetPromptResult, JsonRpcResponse, PromptArgument, PromptMessage,
    ReadResourceResult, ResourceContent,
};
use intermcp::tool::SimpleTool;
use intermcp::Server;
use intermcp::{SimplePrompt, SimpleResource};
use serde_json::{json, Value};

fn build_conformance_server() -> Server {
    let mut server = Server::new("intermcp-conformance", "0.2.0");

    // Register a test tool
    let echo_tool = SimpleTool::new(
        "conformance_echo",
        "Echoes input text back",
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"]
        }),
        |args: Value| async move {
            let msg = args.get("message").and_then(|m| m.as_str()).unwrap_or("");
            Ok(CallToolResult::text(msg))
        },
    );
    server.add_tool(Box::new(echo_tool));

    // Register a test resource
    let res = SimpleResource::new(
        "test://data/manifest.txt",
        "Test Manifest",
        Some("Static test resource"),
        Some("text/plain"),
        || async move {
            Ok(ReadResourceResult {
                contents: vec![ResourceContent {
                    uri: "test://data/manifest.txt".into(),
                    mime_type: Some("text/plain".into()),
                    text: "test manifest content data".into(),
                }],
            })
        },
    );
    server.add_resource(Box::new(res));

    // Register a test prompt
    let prompt = SimplePrompt::new(
        "conformance_prompt",
        "Test prompt for MCP conformance",
        vec![PromptArgument {
            name: "topic".into(),
            description: Some("Topic for prompt".into()),
            required: true,
        }],
        |args| async move {
            let topic = args
                .get("topic")
                .and_then(|t| t.as_str())
                .unwrap_or("general");
            Ok(GetPromptResult {
                description: Some("Generated test prompt".into()),
                messages: vec![PromptMessage {
                    role: "user".into(),
                    content: ContentItem::Text {
                        text: format!("Analyze the topic: {}", topic),
                    },
                }],
            })
        },
    );
    server.add_prompt(Box::new(prompt));

    server
}

#[tokio::test]
async fn test_mcp_conformance_initialize_negotiation() {
    let server = build_conformance_server();

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "conformance-harness", "version": "1.0.0" }
        }
    })
    .to_string();

    let resp_str = server.handle_raw_message(&init_req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp.id, 1);
    assert!(resp.error.is_none());

    let result = resp.result.unwrap();
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["serverInfo"]["name"], "intermcp-conformance");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["capabilities"]["resources"].is_object());
    assert!(result["capabilities"]["prompts"].is_object());
}

#[tokio::test]
async fn test_mcp_conformance_notifications_fire_and_forget() {
    let server = build_conformance_server();

    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    })
    .to_string();

    let resp = server.handle_raw_message(&notif).await;
    assert!(
        resp.is_none(),
        "Notifications must never produce a JSON-RPC response"
    );
}

#[tokio::test]
async fn test_mcp_conformance_ping() {
    let server = build_conformance_server();

    let ping_req = json!({
        "jsonrpc": "2.0",
        "id": "ping-42",
        "method": "ping"
    })
    .to_string();

    let resp_str = server.handle_raw_message(&ping_req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp.id, "ping-42");
    assert_eq!(resp.result.unwrap(), json!({}));
}

#[tokio::test]
async fn test_mcp_conformance_tools_lifecycle() {
    let server = build_conformance_server();

    // 1. tools/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    })
    .to_string();

    let list_resp_str = server.handle_raw_message(&list_req).await.unwrap();
    let list_resp: JsonRpcResponse = serde_json::from_str(&list_resp_str).unwrap();
    let tools = list_resp.result.unwrap()["tools"]
        .as_array()
        .unwrap()
        .clone();
    assert!(tools.iter().any(|t| t["name"] == "conformance_echo"));

    // 2. tools/call valid
    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "conformance_echo",
            "arguments": { "message": "hello MCP" }
        }
    })
    .to_string();

    let call_resp_str = server.handle_raw_message(&call_req).await.unwrap();
    let call_resp: JsonRpcResponse = serde_json::from_str(&call_resp_str).unwrap();
    let result = call_resp.result.unwrap();
    assert_eq!(result["content"][0]["text"], "hello MCP");
    assert_eq!(result["isError"], false);

    // 3. tools/call unknown tool returns error
    let unknown_call = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "non_existent_tool",
            "arguments": {}
        }
    })
    .to_string();

    let unk_resp_str = server.handle_raw_message(&unknown_call).await.unwrap();
    let unk_resp: JsonRpcResponse = serde_json::from_str(&unk_resp_str).unwrap();
    assert_eq!(unk_resp.error.unwrap().code, -32602);
}

#[tokio::test]
async fn test_mcp_conformance_resources_lifecycle() {
    let server = build_conformance_server();

    // 1. resources/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "resources/list"
    })
    .to_string();

    let list_resp_str = server.handle_raw_message(&list_req).await.unwrap();
    let list_resp: JsonRpcResponse = serde_json::from_str(&list_resp_str).unwrap();
    let res_list = list_resp.result.unwrap()["resources"]
        .as_array()
        .unwrap()
        .clone();
    assert!(res_list
        .iter()
        .any(|r| r["uri"] == "test://data/manifest.txt"));

    // 2. resources/read valid
    let read_req = json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "resources/read",
        "params": { "uri": "test://data/manifest.txt" }
    })
    .to_string();

    let read_resp_str = server.handle_raw_message(&read_req).await.unwrap();
    let read_resp: JsonRpcResponse = serde_json::from_str(&read_resp_str).unwrap();
    let contents = read_resp.result.unwrap()["contents"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(contents[0]["text"], "test manifest content data");
}

#[tokio::test]
async fn test_mcp_conformance_prompts_lifecycle() {
    let server = build_conformance_server();

    // 1. prompts/list
    let list_req = json!({
        "jsonrpc": "2.0",
        "id": 20,
        "method": "prompts/list"
    })
    .to_string();

    let list_resp_str = server.handle_raw_message(&list_req).await.unwrap();
    let list_resp: JsonRpcResponse = serde_json::from_str(&list_resp_str).unwrap();
    let prompts = list_resp.result.unwrap()["prompts"]
        .as_array()
        .unwrap()
        .clone();
    assert!(prompts.iter().any(|p| p["name"] == "conformance_prompt"));

    // 2. prompts/get valid
    let get_req = json!({
        "jsonrpc": "2.0",
        "id": 21,
        "method": "prompts/get",
        "params": {
            "name": "conformance_prompt",
            "arguments": { "topic": "distributed consensus" }
        }
    })
    .to_string();

    let get_resp_str = server.handle_raw_message(&get_req).await.unwrap();
    let get_resp: JsonRpcResponse = serde_json::from_str(&get_resp_str).unwrap();
    let messages = get_resp.result.unwrap()["messages"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        messages[0]["content"]["text"],
        "Analyze the topic: distributed consensus"
    );
}

#[tokio::test]
async fn test_mcp_conformance_method_not_found_error() {
    let server = build_conformance_server();

    let bogus_req = json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "invalid/method/name"
    })
    .to_string();

    let resp_str = server.handle_raw_message(&bogus_req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp.error.unwrap().code, -32601);
}
