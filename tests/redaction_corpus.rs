use intermcp::protocol::{CallToolResult, ContentItem, ResourceContent};
use intermcp::server::mask_secrets;
use intermcp::Server;
use serde_json::json;

#[test]
fn test_sensitive_env_keys_redaction() {
    std::env::set_var("TEST_JWT_SECRET", "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9");
    std::env::set_var("TEST_BEARER_AUTH", "bearer_token_secret_abcdef12345");
    std::env::set_var("TEST_CLIENT_SECRET_VAL", "client_secret_987654321_xyz");
    std::env::set_var("TEST_MNEMONIC_KEY", "twelve word seed phrase mnemonic secret key phrase");

    let sample_output = "Connected with token eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9 and auth bearer_token_secret_abcdef12345 and secret client_secret_987654321_xyz";
    let masked = mask_secrets(sample_output);

    assert!(!masked.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
    assert!(!masked.contains("bearer_token_secret_abcdef12345"));
    assert!(!masked.contains("client_secret_987654321_xyz"));
    assert!(masked.contains("[REDACTED_BY_INTERMCP]"));
}

#[tokio::test]
async fn test_mask_all_content_item_variants() {
    std::env::set_var("TEST_DATABASE_DSN_SECRET", "postgres://admin:topsecretpass@db.lan/prod");

    let mut server = Server::new("test-redact", "0.1.0");

    let text_tool = intermcp::tool::SimpleTool::new(
        "return_text",
        "return secret text",
        json!({ "type": "object" }),
        |_| async move {
            Ok(CallToolResult::text("Result: postgres://admin:topsecretpass@db.lan/prod"))
        },
    );

    let resource_tool = intermcp::tool::SimpleTool::new(
        "return_resource",
        "return secret resource",
        json!({ "type": "object" }),
        |_| async move {
            Ok(CallToolResult {
                content: vec![ContentItem::Resource {
                    resource: ResourceContent {
                        uri: "dsn://postgres://admin:topsecretpass@db.lan/prod".to_string(),
                        mime_type: Some("text/plain".to_string()),
                        text: "Host: postgres://admin:topsecretpass@db.lan/prod".to_string(),
                    },
                }],
                is_error: false,
            })
        },
    );

    let image_tool = intermcp::tool::SimpleTool::new(
        "return_image",
        "return secret image data",
        json!({ "type": "object" }),
        |_| async move {
            Ok(CallToolResult {
                content: vec![ContentItem::Image {
                    data: "BASE64_postgres://admin:topsecretpass@db.lan/prod_DATA".to_string(),
                    mime_type: "image/png".to_string(),
                }],
                is_error: false,
            })
        },
    );

    server.add_tool(Box::new(text_tool));
    server.add_tool(Box::new(resource_tool));
    server.add_tool(Box::new(image_tool));

    let text_resp = server.handle_raw_message(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "return_text", "arguments": {} }
    }).to_string()).await.unwrap();
    assert!(!text_resp.contains("topsecretpass"));

    let res_resp = server.handle_raw_message(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "return_resource", "arguments": {} }
    }).to_string()).await.unwrap();
    assert!(!res_resp.contains("topsecretpass"));

    let img_resp = server.handle_raw_message(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": { "name": "return_image", "arguments": {} }
    }).to_string()).await.unwrap();
    assert!(!img_resp.contains("topsecretpass"));
}
