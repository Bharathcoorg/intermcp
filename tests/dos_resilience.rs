use intermcp::http_server::{run_http_server, HttpServerConfig};
use intermcp::Server;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn test_http_missing_host_header_rejected() {
    let server = Arc::new(Server::new("test-dos", "0.1.0"));
    let addr = "127.0.0.1:41234";

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

    tokio::time::sleep(Duration::from_millis(100)).await;

    if let Ok(mut stream) = TcpStream::connect(addr).await {
        let req = "POST /mcp HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}";
        let _ = stream.write_all(req.as_bytes()).await;

        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(resp.contains("400 Bad Request"));
            assert!(resp.contains("Missing Host header"));
        }
    }
}

#[tokio::test]
async fn test_http_unauthorized_before_body() {
    let server = Arc::new(Server::new("test-dos-auth", "0.1.0"));
    let addr = "127.0.0.1:41235";

    tokio::spawn(async move {
        let _ = run_http_server(
            server,
            HttpServerConfig {
                addr: addr.to_string(),
                auth_token: Some("secret-token-xyz".to_string()),
                cors_origin: None,
                max_conns: Some(512),
                tls_cert: None,
                tls_key: None,
            },
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    if let Ok(mut stream) = TcpStream::connect(addr).await {
        // Request with missing or invalid token
        let req = "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer wrong-token\r\nContent-Length: 1000\r\n\r\n";
        let _ = stream.write_all(req.as_bytes()).await;

        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(resp.contains("401 Unauthorized"));
        }
    }
}
