use intermcp::protocol::{CallToolResult, JsonRpcResponse};
use intermcp::vault_lock::TimeLockedVault;
use intermcp::Server;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn test_unprotected_tool_executes_immediately() {
    let vault = TimeLockedVault::new(vec!["dangerous_tool".to_string()], 5);
    assert!(!vault.is_protected("safe_tool"));
    let res = vault.check_or_wait("safe_tool", &json!({})).await.unwrap();
    assert!(res);
}

#[tokio::test]
async fn test_protected_tool_approval() {
    let vault = TimeLockedVault::new(vec!["system_run_command".to_string()], 5);
    let vault_clone = vault.clone();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let pending = vault_clone.list_pending();
        if let Some(first) = pending.first() {
            vault_clone.approve(&first.id);
        }
    });

    let res = vault
        .check_or_wait("system_run_command", &json!({"command": "git push"}))
        .await
        .unwrap();
    assert!(res);
}

#[tokio::test]
async fn test_protected_tool_rejection() {
    let vault = TimeLockedVault::new(vec!["rm_tool".to_string()], 5);
    let vault_clone = vault.clone();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let pending = vault_clone.list_pending();
        if let Some(first) = pending.first() {
            vault_clone.reject(&first.id);
        }
    });

    let res = vault.check_or_wait("rm_tool", &json!({})).await.unwrap();
    assert!(!res);
}

#[tokio::test]
async fn test_server_with_time_locked_vault_integration() {
    let vault = TimeLockedVault::new(vec!["protected_call".to_string()], 1);
    let mut server = Server::new("test-vault", "0.1.0").with_time_locked_vault(vault);

    let dummy_tool = intermcp::tool::SimpleTool::new(
        "protected_call",
        "protected action",
        json!({ "type": "object" }),
        |_| async move { Ok(CallToolResult::text("executed")) },
    );
    server.add_tool(Box::new(dummy_tool));

    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "protected_call", "arguments": {} }
    })
    .to_string();

    // No approval sent, will timeout after 1 second
    let resp_str = server.handle_raw_message(&call_req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    let res = resp.result.unwrap();
    let is_error = res
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(is_error);
    let text = res.get("content").unwrap().as_array().unwrap()[0]
        .get("text")
        .unwrap()
        .as_str()
        .unwrap();
    assert!(text.contains("Time-Locked Vault"));
}
