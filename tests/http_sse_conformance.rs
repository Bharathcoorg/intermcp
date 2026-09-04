use intermcp::http_server::{run_http_server, HttpServerConfig};
use intermcp::Server;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn test_http_post_root_requires_auth_when_configured() {
    let server = Arc::new(Server::new("test-auth-root", "0.1.0"));
    let addr = "127.0.0.1:41240";

    tokio::spawn(async move {
        let _ = run_http_server(
            server,
            HttpServerConfig {
                addr: addr.to_string(),
                auth_token: Some("super-secret-mcp-key".to_string()),
                cors_origin: None,
                max_conns: Some(512),
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Attacker attempts POST / without auth or with invalid token
    if let Ok(mut stream) = TcpStream::connect(addr).await {
        let req = "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 53\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{}}";
        let _ = stream.write_all(req.as_bytes()).await;

        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(
                resp.contains("401 Unauthorized"),
                "Unauthenticated POST / must be blocked with 401 Unauthorized, got: {}",
                resp
            );
        }
    }

    // Legitimate client sends POST / with valid Bearer token
    if let Ok(mut stream) = TcpStream::connect(addr).await {
        let req = "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer super-secret-mcp-key\r\nContent-Type: application/json\r\nContent-Length: 53\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{}}";
        let _ = stream.write_all(req.as_bytes()).await;

        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(
                resp.contains("200 OK"),
                "Authenticated POST / must succeed with 200 OK, got: {}",
                resp
            );
        }
    }
}

#[tokio::test]
async fn test_mcp_sse_handshake_and_message_dispatch() {
    let server = Arc::new(Server::new("test-mcp-sse", "0.1.0"));
    let addr = "127.0.0.1:41241";

    tokio::spawn(async move {
        let _ = run_http_server(
            server,
            HttpServerConfig {
                addr: addr.to_string(),
                auth_token: None,
                cors_origin: None,
                max_conns: Some(512),
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Step 1: Connect via GET /sse
    let mut sse_stream = TcpStream::connect(addr).await.expect("Failed to connect to SSE");
    let get_req = "GET /sse HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n\r\n";
    sse_stream.write_all(get_req.as_bytes()).await.unwrap();

    let mut buf = [0u8; 2048];
    let n = sse_stream.read(&mut buf).await.unwrap();
    let sse_init = String::from_utf8_lossy(&buf[..n]);

    assert!(sse_init.contains("200 OK"), "Must return 200 OK for SSE");
    assert!(sse_init.contains("Content-Type: text/event-stream"));
    assert!(sse_init.contains("event: endpoint"));
    assert!(sse_init.contains("data: /message?sessionId="));

    // Extract endpoint path from SSE data
    let endpoint_line = sse_init
        .lines()
        .find(|l| l.starts_with("data: /message?sessionId="))
        .expect("Must have data: /message?sessionId=");
    let post_path = endpoint_line.trim_start_matches("data: ").trim();

    // Step 2: In a separate client connection, send POST /message?sessionId=...
    let mut post_stream = TcpStream::connect(addr).await.expect("Failed to connect for POST");
    let json_rpc = "{\"jsonrpc\":\"2.0\",\"id\":42,\"method\":\"tools/list\",\"params\":{}}";
    let post_req = format!(
        "POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        post_path,
        json_rpc.len(),
        json_rpc
    );
    post_stream.write_all(post_req.as_bytes()).await.unwrap();

    let mut post_buf = [0u8; 1024];
    let pn = post_stream.read(&mut post_buf).await.unwrap();
    let post_resp = String::from_utf8_lossy(&post_buf[..pn]);
    assert!(post_resp.contains("202 Accepted"), "POST /message must return 202 Accepted, got: {}", post_resp);

    // Step 3: Verify the SSE stream receives event: message with the JSON-RPC response!
    let sn = sse_stream.read(&mut buf).await.unwrap();
    let sse_event = String::from_utf8_lossy(&buf[..sn]);
    assert!(sse_event.contains("event: message"), "SSE must receive event: message, got: {}", sse_event);
    assert!(sse_event.contains("\"id\":42") || sse_event.contains("\"id\": 42"), "SSE payload must contain JSON-RPC response");
}
