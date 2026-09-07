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
    let test_tool = intermcp::tool::SimpleTool::new(
        "search_records",
        "search the database for matching records",
        json!({ "type": "object" }),
        |_| async move { Ok(CallToolResult::text("ok")) },
    );
    server.add_tool(Box::new(test_tool));

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

#[tokio::test]
async fn test_protocol_version_negotiation() {
    let server = Server::new("test-proto", "0.1.0");

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-10-07",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1.0" }
        }
    })
    .to_string();

    let resp_str = server.handle_raw_message(&req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    let res = resp.result.unwrap();
    assert_eq!(res["protocolVersion"], "2024-10-07");

    let req2 = json!({
        "jsonrpc": "2.0",
        "id": "str-id-42",
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1.0" }
        }
    })
    .to_string();

    let resp_str2 = server.handle_raw_message(&req2).await.unwrap();
    let resp2: JsonRpcResponse = serde_json::from_str(&resp_str2).unwrap();
    assert_eq!(resp2.id, "str-id-42");
    assert_eq!(resp2.result.unwrap()["protocolVersion"], "2024-11-05");
}

#[tokio::test]
async fn test_invalid_id_type_rejection() {
    let server = Server::new("test-proto", "0.1.0");

    let req = json!({
        "jsonrpc": "2.0",
        "id": { "invalid": true },
        "method": "ping"
    })
    .to_string();

    let resp_str = server.handle_raw_message(&req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp.error.unwrap().code, -32600);
}

#[tokio::test]
async fn test_payload_exceeding_10mib_rejected() {
    let server = Server::new("test-proto", "0.1.0");

    let huge_padding = "a".repeat(11 * 1024 * 1024);
    let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":"{huge_padding}"}}"#);

    let resp_str = server.handle_raw_message(&req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp.error.unwrap().code, -32600);
}

#[tokio::test]
async fn test_tool_call_cancellation() {
    let mut server = Server::new("test-proto", "0.1.0");
    let slow_tool = intermcp::tool::SimpleTool::new(
        "slow_op",
        "slow operation",
        json!({ "type": "object" }),
        |_| async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok(CallToolResult::text("done"))
        },
    );
    server.add_tool(Box::new(slow_tool));

    let server_arc = std::sync::Arc::new(server);
    let server_clone = std::sync::Arc::clone(&server_arc);

    let handle = tokio::spawn(async move {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 999,
            "method": "tools/call",
            "params": {
                "name": "slow_op",
                "arguments": {}
            }
        })
        .to_string();
        server_clone.handle_raw_message(&req).await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let cancel_notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": {
            "requestId": 999
        }
    })
    .to_string();
    server_arc.handle_raw_message(&cancel_notif).await;

    let resp_str = handle.await.unwrap().unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp.id, 999);
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32000);
    assert!(err.message.contains("cancelled"));
}

#[tokio::test]
async fn test_cancellation_with_number_request_id_42() {
    let mut server = Server::new("test-proto", "0.1.0");
    let slow_tool = intermcp::tool::SimpleTool::new(
        "slow_op_42",
        "slow operation",
        json!({ "type": "object" }),
        |_| async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok(CallToolResult::text("done"))
        },
    );
    server.add_tool(Box::new(slow_tool));

    let server_arc = std::sync::Arc::new(server);
    let server_clone = std::sync::Arc::clone(&server_arc);

    let handle = tokio::spawn(async move {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "tools/call",
            "params": {
                "name": "slow_op_42",
                "arguments": {}
            }
        })
        .to_string();
        server_clone.handle_raw_message(&req).await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let cancel_notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": {
            "requestId": 42
        }
    })
    .to_string();
    server_arc.handle_raw_message(&cancel_notif).await;

    let resp_str = handle.await.unwrap().unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp.id, 42);
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32000);
    assert!(err.message.contains("cancelled"));
}
