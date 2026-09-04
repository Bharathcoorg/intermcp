use intermcp::protocol::{CallToolResult, ContentItem, ResourceContent};
use intermcp::server::mask_secrets;
use intermcp::Server;
use serde_json::json;

#[test]
fn test_sensitive_env_keys_redaction() {
    std::env::set_var("TEST_JWT_SECRET", "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9");
    std::env::set_var("TEST_BEARER_AUTH", "bearer_token_secret_abcdef12345");
    std::env::set_var("TEST_CLIENT_SECRET_VAL", "client_secret_987654321_xyz");
    std::env::set_var(
        "TEST_MNEMONIC_KEY",
        "twelve word seed phrase mnemonic secret key phrase",
    );

    let sample_output = "Connected with token eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9 and auth bearer_token_secret_abcdef12345 and secret client_secret_987654321_xyz";
    let masked = mask_secrets(sample_output);

    assert!(!masked.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
    assert!(!masked.contains("bearer_token_secret_abcdef12345"));
    assert!(!masked.contains("client_secret_987654321_xyz"));
    assert!(masked.contains("[REDACTED_BY_INTERMCP]"));
}

#[tokio::test]
async fn test_mask_all_content_item_variants() {
    std::env::set_var(
        "TEST_DATABASE_DSN_SECRET",
        "postgres://admin:topsecretpass@db.lan/prod",
    );

    let mut server = Server::new("test-redact", "0.1.0");

    let text_tool = intermcp::tool::SimpleTool::new(
        "return_text",
        "return secret text",
        json!({ "type": "object" }),
        |_| async move {
            Ok(CallToolResult::text(
                "Result: postgres://admin:topsecretpass@db.lan/prod",
            ))
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

    let text_resp = server
        .handle_raw_message(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": { "name": "return_text", "arguments": {} }
            })
            .to_string(),
        )
        .await
        .unwrap();
    assert!(!text_resp.contains("topsecretpass"));

    let res_resp = server
        .handle_raw_message(
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "return_resource", "arguments": {} }
            })
            .to_string(),
        )
        .await
        .unwrap();
    assert!(!res_resp.contains("topsecretpass"));

    let img_resp = server
        .handle_raw_message(
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "return_image", "arguments": {} }
            })
            .to_string(),
        )
        .await
        .unwrap();
    assert!(!img_resp.contains("topsecretpass"));
}

#[test]
fn test_redact_for_log_corpus() {
    use intermcp::server::redact_for_log;

    // 1. Bearer token
    let bearer_input = "Received authorization token Bearer secret_bearer_token_123456 in request";
    let bearer_redacted = redact_for_log(bearer_input);
    assert!(
        !bearer_redacted.contains("secret_bearer_token_123456"),
        "Bearer token must be redacted, got: {}",
        bearer_redacted
    );
    assert!(bearer_redacted.contains("[REDACTED]"));

    // 2. JSON key + value
    let json_input = r#"{"api_key": "prod_live_secret_key_9876543210", "status": "ok"}"#;
    let json_redacted = redact_for_log(json_input);
    assert!(
        !json_redacted.contains("prod_live_secret_key_9876543210"),
        "JSON secret must be redacted, got: {}",
        json_redacted
    );
    assert!(json_redacted.contains("[REDACTED]"));

    // 3. Header line
    let header_input =
        "Authorization: Bearer super-secret-header-auth-token-12345\r\nHost: example.com";
    let header_redacted = redact_for_log(header_input);
    assert!(
        !header_redacted.contains("super-secret-header-auth-token-12345"),
        "Header token must be redacted, got: {}",
        header_redacted
    );
    assert!(header_redacted.contains("[REDACTED]"));

    // 4. Benign short string (does NOT redact)
    let benign_short = "Benign token: abc and Bearer 123";
    let benign_result = redact_for_log(benign_short);
    assert_eq!(
        benign_result, benign_short,
        "Benign short string must not be redacted"
    );
}

#[test]
fn test_tracing_subscriber_redaction_capture() {
    use intermcp::server::redact_for_log;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct CapturingWriter(Arc<Mutex<Vec<u8>>>);
    impl Write for CapturingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturingWriter {
        type Writer = CapturingWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer = CapturingWriter(Arc::clone(&buffer));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .finish();

    let bearer_secret = "Bearer secret_bearer_token_99887766";
    let api_key_secret = "api_key=sk_live_1234567890abcdef";

    tracing::subscriber::with_default(subscriber, || {
        // Trigger callsites representative of hub, server, and http_server
        tracing::info!(
            "Spawning upstream server '{}'...",
            redact_for_log(bearer_secret)
        );
        tracing::warn!(
            "Upstream '{}' stdout stream closed",
            redact_for_log(bearer_secret)
        );
        tracing::error!(
            "Failed to spawn upstream '{}': {}",
            redact_for_log("upstream-svc"),
            redact_for_log(api_key_secret)
        );
        tracing::warn!(
            "TLS handshake failed from 127.0.0.1:41240: {}",
            redact_for_log(bearer_secret)
        );
    });

    let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    assert!(
        !output.contains("secret_bearer_token_99887766"),
        "Bearer secret must not appear in tracing logs, got: {}",
        output
    );
    assert!(
        !output.contains("sk_live_1234567890abcdef"),
        "API key secret must not appear in tracing logs, got: {}",
        output
    );
    assert!(
        output.contains("[REDACTED]"),
        "Redaction placeholder must appear in tracing logs, got: {}",
        output
    );
}
