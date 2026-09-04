use intermcp::protocol::{CallToolResult, JsonRpcResponse};
use intermcp::Server;
use serde_json::{json, Value};

#[tokio::test]
async fn test_jsonrpc_2_0_validation() {
    let server = Server::new("test-proto", "0.1.0");

    // Invalid jsonrpc version
    let req = json!({
        "jsonrpc": "1.0",
        "id": 1,
        "method": "ping"
    })
    .to_string();

    let resp_str = server.handle_raw_message(&req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp.error.unwrap().code, -32600);
}

#[tokio::test]
async fn test_batch_jsonrpc_requests() {
    let server = Server::new("test-proto", "0.1.0");

    let batch_req = json!([
        { "jsonrpc": "2.0", "id": 1, "method": "ping" },
        { "jsonrpc": "2.0", "id": 2, "method": "ping" },
        { "jsonrpc": "2.0", "id": 3, "method": "ping" }
    ])
    .to_string();

    let resp_str = server.handle_raw_message(&batch_req).await.unwrap();
    let responses: Vec<JsonRpcResponse> = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0].id, 1);
    assert_eq!(responses[1].id, 2);
    assert_eq!(responses[2].id, 3);
}

#[tokio::test]
async fn test_notifications_initialized() {
    let server = Server::new("test-proto", "0.1.0");

    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    })
    .to_string();

    let resp = server.handle_raw_message(&notif).await;
    assert!(resp.is_none(), "Notifications must not produce a response");
}

#[tokio::test]
async fn test_logging_set_level() {
    let server = Server::new("test-proto", "0.1.0");

    let req = json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "logging/setLevel",
        "params": { "level": "debug" }
    })
    .to_string();

    let resp_str = server.handle_raw_message(&req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert!(resp.result.is_some());
}

#[tokio::test]
async fn test_completion_complete() {
    let mut server = Server::new("test-proto", "0.1.0");
    let dummy_tool = intermcp::tool::SimpleTool::new(
        "search_records",
        "search the database for matching records",
        json!({ "type": "object" }),
        |_| async move { Ok(CallToolResult::text("ok")) },
    );
    server.add_tool(Box::new(dummy_tool));

    let req = json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "completion/complete",
        "params": {
            "ref": { "type": "ref/prompt", "name": "search" },
            "argument": { "name": "query", "value": "search" }
        }
    })
    .to_string();

    let resp_str = server.handle_raw_message(&req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    let res = resp.result.unwrap();
    let completions = res
        .get("completion")
        .unwrap()
        .get("values")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(completions.contains(&Value::String("search_records".to_string())));
}
