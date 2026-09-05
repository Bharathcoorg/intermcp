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
                tls_cert: None,
                tls_key: None,
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
                tls_cert: None,
                tls_key: None,
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Step 1: Connect via GET /sse
    let mut sse_stream = TcpStream::connect(addr)
        .await
        .expect("Failed to connect to SSE");
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
    let mut post_stream = TcpStream::connect(addr)
        .await
        .expect("Failed to connect for POST");
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
    assert!(
        post_resp.contains("202 Accepted"),
        "POST /message must return 202 Accepted, got: {}",
        post_resp
    );

    // Step 3: Verify the SSE stream receives event: message with the JSON-RPC response!
    let sn = sse_stream.read(&mut buf).await.unwrap();
    let sse_event = String::from_utf8_lossy(&buf[..sn]);
    assert!(
        sse_event.contains("event: message"),
        "SSE must receive event: message, got: {}",
        sse_event
    );
    assert!(
        sse_event.contains("\"id\":42") || sse_event.contains("\"id\": 42"),
        "SSE payload must contain JSON-RPC response"
    );
}

#[tokio::test]
async fn test_http_approve_reject_requires_auth_when_configured() {
    let server = Arc::new(Server::new("test-auth-approve", "0.1.0"));
    let addr = "127.0.0.1:41242";

    tokio::spawn(async move {
        let _ = run_http_server(
            server,
            HttpServerConfig {
                addr: addr.to_string(),
                auth_token: Some("secure-token-vault".to_string()),
                cors_origin: None,
                max_conns: Some(512),
                tls_cert: None,
                tls_key: None,
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    // Unauthenticated GET /api/approve/any-id must be 401
    if let Ok(mut stream) = TcpStream::connect(addr).await {
        let req = "GET /api/approve/req-123 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let _ = stream.write_all(req.as_bytes()).await;
        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(
                resp.contains("401 Unauthorized"),
                "Expected 401, got: {}",
                resp
            );
        }
    }

    // Unauthenticated POST /api/reject/any-id must be 401
    if let Ok(mut stream) = TcpStream::connect(addr).await {
        let req =
            "POST /api/reject/req-123 HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n";
        let _ = stream.write_all(req.as_bytes()).await;
        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(
                resp.contains("401 Unauthorized"),
                "Expected 401, got: {}",
                resp
            );
        }
    }

    // Authenticated POST /api/approve/any-id succeeds with 200 (even if id is not found in vault)
    if let Ok(mut stream) = TcpStream::connect(addr).await {
        let req = "POST /api/approve/req-123 HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer secure-token-vault\r\nContent-Length: 0\r\n\r\n";
        let _ = stream.write_all(req.as_bytes()).await;
        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(
                resp.contains("200 OK"),
                "Expected 200 OK for authenticated approve, got: {}",
                resp
            );
        }
    }

    // Authenticated GET /api/approve/any-id must be rejected with 405 Method Not Allowed (CSRF protection)
    if let Ok(mut stream) = TcpStream::connect(addr).await {
        let req = "GET /api/approve/req-123 HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer secure-token-vault\r\n\r\n";
        let _ = stream.write_all(req.as_bytes()).await;
        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(
                resp.contains("405 Method Not Allowed"),
                "Expected 405 Method Not Allowed for GET approve, got: {}",
                resp
            );
        }
    }
}

#[tokio::test]
async fn test_run_http_server_rejects_public_bind_without_tls() {
    let server = Arc::new(Server::new("test-public-bind-reject", "0.1.0"));
    let config = HttpServerConfig {
        addr: "0.0.0.0:41243".to_string(),
        auth_token: Some("test-token-123".to_string()),
        cors_origin: None,
        max_conns: None,
        tls_cert: None,
        tls_key: None,
    };
    let result = run_http_server(server, config).await;
    assert!(
        result.is_err(),
        "run_http_server with 0.0.0.0 and no TLS must return Err"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Insecure public bind") || err.contains("TLS configuration"),
        "Unexpected error: {}",
        err
    );
}

const TEST_CERT_PEM: &str = "\
-----BEGIN CERTIFICATE-----\n\
MIIBSDCB7qADAgECAhRIsIfJvIqcw4Q5Qux5SivMNSK2cjAKBggqhkjOPQQDAjAU\n\
MRIwEAYDVQQDDAkxMjcuMC4wLjEwHhcNMjYwOTAzMTQ1NDQwWhcNMjcwOTA0MTQ1\n\
NDQwWjAUMRIwEAYDVQQDDAkxMjcuMC4wLjEwWTATBgcqhkjOPQIBBggqhkjOPQMB\n\
BwNCAAQ8YhHIIlUNv9jqMPoshAHO9L3VHT6znDS1NNGP4cQhBgv0glH2rwuZ8vVj\n\
dWvwrUx9iicXl7ZTGPLeh7MObi/Iox4wHDAaBgNVHREEEzARgglsb2NhbGhvc3SH\n\
BH8AAAEwCgYIKoZIzj0EAwIDSQAwRgIhAKi2HNesjOX1OfEStICy8JFk4AEbx2Di\n\
gttchk6FUjr1AiEA/Q0R/kyM36oEQpvLkfJ2Y3j9gu1KfsbDNIg++2AsZLo=\n\
-----END CERTIFICATE-----\n";

const TEST_KEY_PEM: &str = "\
-----BEGIN PRIVATE KEY-----\n\
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgPWsJV0OlgyEliAVh\n\
cI1TFahL/CuY7FnHGitHSQ0Qy1qhRANCAAQ8YhHIIlUNv9jqMPoshAHO9L3VHT6z\n\
nDS1NNGP4cQhBgv0glH2rwuZ8vVjdWvwrUx9iicXl7ZTGPLeh7MObi/I\n\
-----END PRIVATE KEY-----\n";

#[tokio::test]
async fn test_run_http_server_accepts_public_bind_with_tls() {
    let temp_dir = tempfile::tempdir().unwrap();
    let cert_path = temp_dir.path().join("test_cert.pem");
    let key_path = temp_dir.path().join("test_key.pem");
    std::fs::write(&cert_path, TEST_CERT_PEM).unwrap();
    std::fs::write(&key_path, TEST_KEY_PEM).unwrap();

    let server = Arc::new(Server::new("test-tls-server", "0.1.0"));
    let config = HttpServerConfig {
        addr: "127.0.0.1:41249".to_string(),
        auth_token: Some("test-token-123".to_string()),
        cors_origin: None,
        max_conns: None,
        tls_cert: Some(cert_path),
        tls_key: Some(key_path),
    };

    let server_handle = tokio::spawn(async move {
        let _ = run_http_server(server, config).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Connect to 127.0.0.1:41249 - TCP connect succeeds with TLS enabled
    let conn = TcpStream::connect("127.0.0.1:41249").await;
    assert!(
        conn.is_ok(),
        "run_http_server with valid TLS cert+key must succeed and accept connections"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_http_get_root_auth_enforcement() {
    // 1. With auth_token set: GET / returns 401 Unauthorized
    let server1 = Arc::new(Server::new("test-auth-root-get", "0.1.0"));
    let addr1 = "127.0.0.1:41251";
    let handle1 = tokio::spawn(async move {
        let _ = run_http_server(
            server1,
            HttpServerConfig {
                addr: addr1.to_string(),
                auth_token: Some("super-secret-token".to_string()),
                cors_origin: None,
                max_conns: None,
                tls_cert: None,
                tls_key: None,
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let mut stream1 = TcpStream::connect(addr1).await.unwrap();
    let req1 = "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream1.write_all(req1.as_bytes()).await.unwrap();

    let mut buf1 = [0u8; 1024];
    let n1 = stream1.read(&mut buf1).await.unwrap();
    let resp1 = String::from_utf8_lossy(&buf1[..n1]);
    assert!(
        resp1.contains("401 Unauthorized"),
        "GET / with auth_token configured must return 401, got: {}",
        resp1
    );
    handle1.abort();

    // 2. With auth_token None: GET / returns 200 OK
    let server2 = Arc::new(Server::new("test-noauth-root-get", "0.1.0"));
    let addr2 = "127.0.0.1:41252";
    let handle2 = tokio::spawn(async move {
        let _ = run_http_server(
            server2,
            HttpServerConfig {
                addr: addr2.to_string(),
                auth_token: None,
                cors_origin: None,
                max_conns: None,
                tls_cert: None,
                tls_key: None,
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let mut stream2 = TcpStream::connect(addr2).await.unwrap();
    let req2 = "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream2.write_all(req2.as_bytes()).await.unwrap();

    let mut buf2 = [0u8; 1024];
    let n2 = stream2.read(&mut buf2).await.unwrap();
    let resp2 = String::from_utf8_lossy(&buf2[..n2]);
    assert!(
        resp2.contains("200 OK"),
        "GET / with auth_token None must return 200 OK, got: {}",
        resp2
    );
    handle2.abort();
}

#[test]
fn test_bind_addr_is_public_classification() {
    use intermcp::http_server::bind_addr_is_public;

    // Loopback IPv4 addresses (127.0.0.0/8) are not public
    assert!(!bind_addr_is_public("127.0.0.1:8080"));
    assert!(!bind_addr_is_public("127.0.0.2:8080"));
    assert!(!bind_addr_is_public("127.1.2.3:9000"));

    // Loopback IPv6 address is not public
    assert!(!bind_addr_is_public("[::1]:8080"));

    // Public wildcard addresses are public
    assert!(bind_addr_is_public("0.0.0.0:8080"));
    assert!(bind_addr_is_public("[::]:8080"));

    // Non-loopback IP addresses are public
    assert!(bind_addr_is_public("192.168.1.100:8080"));
    assert!(bind_addr_is_public("10.0.0.1:8080"));
    assert!(bind_addr_is_public("1.1.1.1:8080"));

    // Unparseable / DNS hostnames are treated as potentially public
    assert!(bind_addr_is_public("example.com:8080"));
    assert!(bind_addr_is_public("custom-dns-host:9090"));
}

#[tokio::test]
async fn test_http_sse_missing_session_returns_404() {
    let port = 19188;
    let addr = format!("127.0.0.1:{}", port);
    let server = Arc::new(intermcp::create_default_server());

    let handle = tokio::spawn(async move {
        let _ = run_http_server(
            server,
            HttpServerConfig {
                addr: addr.clone(),
                auth_token: None,
                cors_origin: None,
                max_conns: Some(512),
                tls_cert: None,
                tls_key: None,
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .expect("Failed to connect");
    let json_rpc = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{}}";
    let post_req = format!(
        "POST /message?sessionId=non_existent_session_id HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        json_rpc.len(),
        json_rpc
    );
    stream.write_all(post_req.as_bytes()).await.unwrap();

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await.unwrap();
    let resp = String::from_utf8_lossy(&buf[..n]);
    assert!(
        resp.contains("404 Not Found"),
        "Unknown sessionId must return 404 Not Found, got: {}",
        resp
    );

    handle.abort();
}

#[tokio::test]
async fn test_http_conflicting_content_length_returns_400() {
    let port = 19189;
    let addr = format!("127.0.0.1:{}", port);
    let server = Arc::new(intermcp::create_default_server());

    let handle = tokio::spawn(async move {
        let _ = run_http_server(
            server,
            HttpServerConfig {
                addr: addr.clone(),
                auth_token: None,
                cors_origin: None,
                max_conns: Some(512),
                tls_cert: None,
                tls_key: None,
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .expect("Failed to connect");
    let post_req = "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 10\r\nContent-Length: 20\r\n\r\n0123456789";
    stream.write_all(post_req.as_bytes()).await.unwrap();

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await.unwrap();
    let resp = String::from_utf8_lossy(&buf[..n]);
    assert!(
        resp.contains("400 Bad Request"),
        "Conflicting Content-Length must return 400 Bad Request, got: {}",
        resp
    );

    handle.abort();
}
