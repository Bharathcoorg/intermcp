use intermcp::protocol::{CallToolResult, ContentItem, JsonRpcResponse};
use intermcp::taint::TaintTracker;
use intermcp::tools::create_shell_exec_tool;
use intermcp::vault_lock::TimeLockedVault;
use intermcp::Server;
use serde_json::json;

#[tokio::test]
async fn test_guardrail_taint_vault_composition() {
    let vault = TimeLockedVault::new(vec!["system_run_command".to_string()], 60);
    let mut server = Server::new("test-composition-node", "0.1.0")
        .with_guardrails(100, 2) // Threshold: 2 consecutive calls before loop breaker
        .with_taint_tracker(TaintTracker::new())
        .with_time_locked_vault(vault);
    server.add_tool(create_shell_exec_tool());

    let tainted_call = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "system_run_command",
            "arguments": {
                "command": "git status",
                "_taint": "untrusted"
            }
        }
    })
    .to_string();

    // Call 1: Taint tracker intercepts the untrusted payload before the slow vault approval
    let resp1_str = server.handle_raw_message(&tainted_call).await.unwrap();
    let resp1: JsonRpcResponse = serde_json::from_str(&resp1_str).unwrap();
    let res1: CallToolResult = serde_json::from_value(resp1.result.unwrap()).unwrap();
    assert!(res1.is_error);
    if let ContentItem::Text { text } = &res1.content[0] {
        assert!(
            text.contains("Taint Flow Violation: Untrusted data cannot flow into privileged sink"),
            "Expected taint violation, got: {}",
            text
        );
    } else {
        panic!("Expected text content");
    }

    // Vault should NOT have any pending items because taint vetoed fast
    let vault_ref = server.vault_lock().unwrap();
    assert_eq!(vault_ref.list_pending().len(), 0);

    // Call 2: Second call still intercepted by taint
    let resp2_str = server.handle_raw_message(&tainted_call).await.unwrap();
    let resp2: JsonRpcResponse = serde_json::from_str(&resp2_str).unwrap();
    let res2: CallToolResult = serde_json::from_value(resp2.result.unwrap()).unwrap();
    assert!(res2.is_error);

    // Call 3: Exceeds loop_detection_threshold (2). Guardrail loop breaker triggers FIRST (fastest check)!
    let resp3_str = server.handle_raw_message(&tainted_call).await.unwrap();
    let resp3: JsonRpcResponse = serde_json::from_str(&resp3_str).unwrap();
    let res3: CallToolResult = serde_json::from_value(resp3.result.unwrap()).unwrap();
    assert!(res3.is_error);
    if let ContentItem::Text { text } = &res3.content[0] {
        assert!(
            text.contains("InterMCP Loop Breaker: Infinite loop detected"),
            "Expected loop breaker error, got: {}",
            text
        );
    } else {
        panic!("Expected text content");
    }
}
