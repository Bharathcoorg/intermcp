use intermcp::protocol::{CallToolResult, JsonRpcRequest};
use intermcp::receipts::{
    canonicalize_json, hash_canonical_json, verify_receipt_chain_file, ReceiptBook, ReceiptStatus,
};
use intermcp::tool::SimpleTool;
use intermcp::Server;
use serde_json::json;
use tempfile::NamedTempFile;

#[test]
fn test_rfc_8785_jcs_canonicalization_ordering() {
    // Keys in arbitrary order with arbitrary whitespace
    let val1 = json!({
        "z_key": 1,
        "a_key": "hello",
        "nested": {
            "b": true,
            "a": null
        }
    });

    let val2 = json!({
        "nested": {
            "a": null,
            "b": true
        },
        "a_key": "hello",
        "z_key": 1
    });

    let canonical1 = canonicalize_json(&val1).unwrap();
    let canonical2 = canonicalize_json(&val2).unwrap();

    // Canonical representation must be byte-for-byte identical
    assert_eq!(canonical1, canonical2);

    // Hash must match
    let hash1 = hash_canonical_json(&val1).unwrap();
    let hash2 = hash_canonical_json(&val2).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn test_rfc_8785_f64_shortest_round_trip() {
    let f64_payload = json!({
        "a_float": 0.1,
        "tiny": 1e-100,
        "huge": 1.0e308,
        "zero": 0.0,
    });

    let canonical = canonicalize_json(&f64_payload).unwrap();
    let canonical_str = String::from_utf8(canonical).unwrap();

    // Verify shortest round-trip formatting per RFC 8785 section 3.2.2.3
    assert!(canonical_str.contains("\"a_float\":0.1"));
    assert!(canonical_str.contains("\"tiny\":1e-100"));
    assert!(canonical_str.contains("\"huge\":1e+308"));
    assert!(canonical_str.contains("\"zero\":0"));

    // Deterministic hash
    let hash1 = hash_canonical_json(&f64_payload).unwrap();
    let hash2 = hash_canonical_json(&f64_payload).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn test_rfc_8785_integer_boundary_round_trip() {
    let int_payload = json!({
        "max_u64": u64::MAX,
        "max_i64": i64::MAX,
        "min_i64": i64::MIN,
    });
    let canonical = canonicalize_json(&int_payload).unwrap();
    let canonical_str = String::from_utf8(canonical).unwrap();
    assert!(canonical_str.contains(&format!("\"max_u64\":{}", u64::MAX)));
    assert!(canonical_str.contains(&format!("\"max_i64\":{}", i64::MAX)));
    assert!(canonical_str.contains(&format!("\"min_i64\":{}", i64::MIN)));

    let hash1 = hash_canonical_json(&int_payload).unwrap();
    let hash2 = hash_canonical_json(&int_payload).unwrap();
    assert_eq!(hash1, hash2);
}

#[test]
fn test_signed_receipt_chain_generation_and_verification() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path();
    let key = b"adversarial-testing-key-secret-999";

    let book = ReceiptBook::new(path, key, "node-prod-01").unwrap();

    let input = json!({"path": "src/main.rs"});
    let output = json!({"status": "ok", "lines": 42});

    // Record receipt 1
    let r1 = book
        .record_execution(
            "sess-1",
            "fs_read",
            "schema_hash_1",
            &input,
            &output,
            250,
            ReceiptStatus::Success,
        )
        .unwrap();

    assert_eq!(r1.receipt.sequence, 1);
    assert_eq!(
        r1.receipt.prev_receipt_hash,
        "0000000000000000000000000000000000000000000000000000000000000000"
    );

    // Record receipt 2
    let r2 = book
        .record_execution(
            "sess-1",
            "system_info",
            "schema_hash_2",
            &json!({}),
            &json!({"os": "windows"}),
            120,
            ReceiptStatus::Success,
        )
        .unwrap();

    assert_eq!(r2.receipt.sequence, 2);
    assert_eq!(r2.receipt.prev_receipt_hash, r1.receipt_hash);

    // Verify entire chain with key
    let summary = verify_receipt_chain_file(path, Some(key)).unwrap();
    assert_eq!(summary.count, 2);
    assert_eq!(summary.last_hash, r2.receipt_hash);

    // Verify with invalid key fails
    let err = verify_receipt_chain_file(path, Some(b"wrong-key"))
        .expect_err("Wrong key must fail verification");
    assert!(err
        .to_string()
        .contains("cryptographic signature verification failed"));
}

#[test]
fn test_signed_receipt_tamper_detection() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path();
    let key = b"my-signing-key";

    let book = ReceiptBook::new(path, key, "node-1").unwrap();
    book.record_execution(
        "s1",
        "tool_a",
        "h1",
        &json!({}),
        &json!({}),
        50,
        ReceiptStatus::Success,
    )
    .unwrap();
    book.record_execution(
        "s1",
        "tool_b",
        "h2",
        &json!({}),
        &json!({}),
        60,
        ReceiptStatus::Success,
    )
    .unwrap();

    // Read content and alter 1 byte in the input hash of receipt 1
    let content = std::fs::read_to_string(path).unwrap();
    let tampered = content.replacen("tool_a", "tool_x", 1);
    std::fs::write(path, tampered).unwrap();

    let err = verify_receipt_chain_file(path, Some(key))
        .expect_err("Tampered receipt must fail verification");
    assert!(err.to_string().contains("verification failed"));
}

#[tokio::test]
async fn test_server_with_signed_receipts_integration() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path();
    let key = b"server-integration-key";

    let book = ReceiptBook::new(path, key, "test-server-node").unwrap();
    let mut server = Server::new("test-receipts-server", "1.0.0");

    server.add_tool(Box::new(SimpleTool::new(
        "compute_sum",
        "Adds two numbers",
        json!({
            "type": "object",
            "properties": {
                "a": { "type": "number" },
                "b": { "type": "number" }
            }
        }),
        |args| async move {
            let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(CallToolResult::text(format!("{}", a + b)))
        },
    )));

    server = server.with_receipt_book(book);

    let req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(100)),
        method: "tools/call".to_string(),
        params: Some(json!({
            "name": "compute_sum",
            "arguments": { "a": 10, "b": 25 }
        })),
    };

    let resp = server.handle_request(req).await;
    assert!(resp.unwrap().result.is_some());

    // Verify receipt was written to file
    let summary = verify_receipt_chain_file(path, Some(key)).unwrap();
    assert_eq!(summary.count, 1);
    assert!(!summary.last_hash.is_empty());

    // Verify session ID matches server session ID and does not use hardcoded phantom session-1
    let raw_receipt_content = std::fs::read_to_string(path).unwrap();
    assert!(raw_receipt_content.contains(server.session_id()));
    assert!(!raw_receipt_content.contains("\"session_id\":\"session-1\""));
}
