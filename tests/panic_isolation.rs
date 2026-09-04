use intermcp::protocol::{CallToolResult, JsonRpcResponse};
use intermcp::Server;
use serde_json::json;

#[tokio::test]
async fn test_tool_panic_isolation() {
    let mut server = Server::new("test-panic-isolation", "0.1.0");

    let panicking_tool = intermcp::tool::SimpleTool::new(
        "crash_tool",
        "A buggy tool that deliberately panics",
        json!({ "type": "object" }),
        |_| async move {
            panic!("FATAL_TOOL_CRASH_UNWIND_TEST");
            #[allow(unreachable_code)]
            Ok(CallToolResult::text("unreachable"))
        },
    );

    server.add_tool(Box::new(panicking_tool));

    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 999,
        "method": "tools/call",
        "params": {
            "name": "crash_tool",
            "arguments": {}
        }
    })
    .to_string();

    let resp_str = server.handle_raw_message(&call_req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();

    assert!(resp.error.is_some());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32603);
    assert_eq!(err.message, "internal tool panic");

    // Verify server remains alive and responds to subsequent requests
    let ping_req = json!({
        "jsonrpc": "2.0",
        "id": 1000,
        "method": "ping"
    })
    .to_string();

    let ping_resp_str = server.handle_raw_message(&ping_req).await.unwrap();
    let ping_resp: JsonRpcResponse = serde_json::from_str(&ping_resp_str).unwrap();
    assert!(ping_resp.result.is_some());
}
