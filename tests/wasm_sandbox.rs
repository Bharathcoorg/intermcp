use intermcp::protocol::JsonRpcResponse;
use intermcp::wasm::{WasmModuleValidator, WasmSandboxConfig, WasmTool};
use intermcp::Server;
use serde_json::json;

#[tokio::test]
async fn test_wasm_tool_sandbox() {
    // Valid standard minimal WASM v1 module header: "\0asm\1\0\0\0"
    let valid_wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    let config = WasmSandboxConfig::default();

    // 1. Inspect valid WASM
    let metadata = WasmModuleValidator::inspect(&valid_wasm, &config).unwrap();
    assert_eq!(metadata.version, 1);
    assert!(metadata.is_valid);

    // 2. Reject corrupt WASM
    let corrupt_wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x02, 0x00, 0x00, 0x00]; // version 2
    assert!(WasmModuleValidator::inspect(&corrupt_wasm, &config).is_err());

    let not_wasm = b"NOT_WASM_FILE_DATA";
    assert!(WasmModuleValidator::inspect(not_wasm, &config).is_err());

    // 3. Register WasmTool into InterMCP Server
    let wasm_tool = WasmTool::new(
        "wasm_evaluator",
        "Deterministic sandboxed WebAssembly execution",
        json!({
            "type": "object",
            "properties": { "expression": { "type": "string" } }
        }),
        valid_wasm,
        config,
    )
    .unwrap();

    let mut server = Server::new("test-wasm", "1.0.0");
    server.add_tool(Box::new(wasm_tool));

    // 4. Invoke WasmTool via JSON-RPC protocol
    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "wasm_evaluator",
            "arguments": { "expression": "2 + 2" }
        }
    })
    .to_string();

    let resp_str = server.handle_raw_message(&call_req).await.unwrap();
    let resp: JsonRpcResponse = serde_json::from_str(&resp_str).unwrap();
    assert!(resp.error.is_none());
    let result_obj = resp.result.unwrap();
    assert_eq!(result_obj["isError"], false);
    let content_text = result_obj["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("executed_in_wasm_sandbox"));
    assert!(content_text.contains("zero_host_filesystem_and_network_access"));
}
