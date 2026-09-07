use parking_lot::RwLock;
use std::collections::HashMap;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadBuf};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tracing::info;

use std::path::PathBuf;

use crate::server::Server;

pub struct HttpServerConfig {
    pub addr: String,
    pub auth_token: Option<String>,
    pub cors_origin: Option<String>,
    pub max_conns: Option<usize>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

pub fn bind_addr_is_public(addr: &str) -> bool {
    if let Ok(sa) = addr.parse::<std::net::SocketAddr>() {
        return !sa.ip().is_loopback();
    }

    let host = if let Some(stripped) = addr.strip_prefix('[') {
        stripped.split(']').next().unwrap_or(stripped)
    } else if let Some((h, _)) = addr.rsplit_once(':') {
        h
    } else {
        addr
    };

    match host.parse::<IpAddr>() {
        Ok(ip) => !ip.is_loopback(),
        Err(_) => true,
    }
}

impl HttpServerConfig {
    pub fn bind_addr_is_public(&self) -> bool {
        bind_addr_is_public(&self.addr)
    }
}

enum MaybeTlsStream {
    Plain(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>),
}

impl tokio::io::AsyncRead for MaybeTlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for MaybeTlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Self::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

fn load_tls_acceptor(
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
) -> Result<tokio_rustls::TlsAcceptor, Box<dyn std::error::Error>> {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer};

    let certs: Vec<CertificateDer<'static>> =
        CertificateDer::pem_file_iter(cert_path)?.collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        return Err("No certificates found in TLS certificate file".into());
    }

    let key = PrivateKeyDer::from_pem_file(key_path)?;

    let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(certs, key)?;

    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(server_config)))
}

use governor::clock::DefaultClock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;

type IpRateLimiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;
static IP_RATE_LIMITER: OnceLock<IpRateLimiter> = OnceLock::new();

fn check_ip_rate_limit(ip: IpAddr) -> bool {
    let limiter = IP_RATE_LIMITER
        .get_or_init(|| RateLimiter::keyed(Quota::per_minute(NonZeroU32::new(60).unwrap())));
    limiter.check_key(&ip).is_ok()
}

type SseSender = tokio::sync::mpsc::Sender<String>;

pub(crate) struct SseState {
    pub sessions: HashMap<String, SseSender>,
    pub ip_connections: HashMap<IpAddr, (Instant, u32)>,
}

impl SseState {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            ip_connections: HashMap::new(),
        }
    }

    /// Refuse 4th SSE from same IP within 60s
    pub fn check_and_record_sse_ip(&mut self, ip: IpAddr) -> bool {
        let now = Instant::now();
        // Evict expired entries if table grows
        if self.ip_connections.len() > 500 {
            self.ip_connections
                .retain(|_, (ts, _)| now.duration_since(*ts) < Duration::from_secs(60));
        }

        let entry = self.ip_connections.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) > Duration::from_secs(60) {
            *entry = (now, 1);
            true
        } else {
            entry.1 += 1;
            entry.1 <= 3
        }
    }
}

static SSE_SESSIONS: OnceLock<RwLock<SseState>> = OnceLock::new();

fn get_sse_sessions() -> &'static RwLock<SseState> {
    SSE_SESSIONS.get_or_init(|| RwLock::new(SseState::new()))
}

fn generate_session_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut s = String::with_capacity(32);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Validate that a session ID is exactly 32 lowercase hex characters.
fn is_valid_session_id(sid: &str) -> bool {
    sid.len() == 32 && sid.chars().all(|c| c.is_ascii_hexdigit())
}

const MAX_SSE_SESSIONS: usize = 1024;

pub async fn run_http_server(
    server: Arc<Server>,
    config: HttpServerConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if bind_addr_is_public(&config.addr) && (config.tls_cert.is_none() || config.tls_key.is_none())
    {
        return Err(format!(
            "Insecure public bind '{}' rejected: HTTP server requires TLS configuration (tls_cert and tls_key) when binding to public addresses (0.0.0.0 or ::)",
            config.addr
        ).into());
    }

    if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key) {
        if !cert.exists() {
            return Err(format!("TLS certificate file not found: {}", cert.display()).into());
        }
        if !key.exists() {
            return Err(format!("TLS key file not found: {}", key.display()).into());
        }
    }

    let tls_acceptor = match (&config.tls_cert, &config.tls_key) {
        (Some(cert), Some(key)) => Some(load_tls_acceptor(cert, key)?),
        _ => None,
    };

    let listener = TcpListener::bind(&config.addr).await?;
    let scheme = if tls_acceptor.is_some() {
        "https"
    } else {
        "http"
    };
    info!(
        "🌐 InterMCP HTTP/SSE Server running at {}://{}",
        scheme, config.addr
    );

    let auth_token = config.auth_token;
    let cors_origin = config.cors_origin.map(|o| o.replace(['\r', '\n'], ""));
    let max_conns = config.max_conns.unwrap_or(512);
    let connection_semaphore = Arc::new(Semaphore::new(max_conns));

    loop {
        let (raw_socket, peer_addr) = listener.accept().await?;
        let permit = match connection_semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!(
                    "Connection rejected from {}: max connections limit reached ({})",
                    crate::server::redact_for_log(&peer_addr.to_string()),
                    max_conns
                );
                let mut socket = raw_socket;
                let resp = "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: 28\r\nConnection: close\r\n\r\nMax connections limit reached";
                let _ = socket.write_all(resp.as_bytes()).await;
                continue;
            }
        };

        if !check_ip_rate_limit(peer_addr.ip()) {
            tracing::warn!(
                "Connection rejected from {}: IP rate limit exceeded (60/m)",
                crate::server::redact_for_log(&peer_addr.to_string())
            );
            let mut socket = raw_socket;
            let resp = "HTTP/1.1 429 Too Many Requests\r\nContent-Type: text/plain\r\nContent-Length: 26\r\nConnection: close\r\n\r\nRate limit exceeded (60/m)";
            let _ = socket.write_all(resp.as_bytes()).await;
            continue;
        }

        let maybe_acceptor = tls_acceptor.clone();
        let server_ref = Arc::clone(&server);
        let token_ref = auth_token.clone();
        let cors_ref = cors_origin.clone();

        tokio::spawn(async move {
            let _permit = permit;
            let mut socket: MaybeTlsStream = if let Some(acceptor) = maybe_acceptor {
                match acceptor.accept(raw_socket).await {
                    Ok(tls_stream) => MaybeTlsStream::Tls(Box::new(tls_stream)),
                    Err(e) => {
                        tracing::warn!(
                            "TLS handshake failed from {}: {}",
                            crate::server::redact_for_log(&peer_addr.to_string()),
                            crate::server::redact_for_log(&e.to_string())
                        );
                        return;
                    }
                }
            } else {
                MaybeTlsStream::Plain(raw_socket)
            };

            let handle_conn = async {
                let mut buffer = Vec::new();
                let mut temp_buf = [0u8; 4096];
                let header_end;

                loop {
                    let n = match socket.read(&mut temp_buf).await {
                        Ok(n) if n > 0 => n,
                        _ => return,
                    };
                    buffer.extend_from_slice(&temp_buf[..n]);

                    if buffer.len() > 32 * 1024 {
                        tracing::warn!(
                            "Request headers exceed 32KB limit from {}",
                            crate::server::redact_for_log(&peer_addr.to_string())
                        );
                        let resp = "HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 27\r\nConnection: close\r\n\r\nHeaders exceed 32KB limit";
                        let _ = socket.write_all(resp.as_bytes()).await;
                        return;
                    }

                    if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = pos + 4;
                        break;
                    }
                }

                let headers_str = String::from_utf8_lossy(&buffer[..header_end]).to_string();
                let lines: Vec<&str> = headers_str.split("\r\n").collect();
                if lines.is_empty() {
                    return;
                }

                let first_line = lines[0];
                let parts: Vec<&str> = first_line.split_whitespace().collect();
                if parts.len() < 3 {
                    tracing::warn!(
                        "Malformed HTTP request line from {}: '{}'",
                        crate::server::redact_for_log(&peer_addr.to_string()),
                        crate::server::redact_for_log(first_line)
                    );
                    return;
                }

                let method = parts[0];
                let raw_path = parts[1];
                let http_version = parts[2];
                let path = raw_path.split('?').next().unwrap_or(raw_path);

                let mut has_host = false;
                let mut is_sse_accept = false;
                let mut authorization_header = None;
                let mut content_type_header = None;
                let mut origin_header = None;
                let mut content_length: Option<usize> = None;
                let mut bad_content_length = false;
                let mut has_transfer_encoding_chunked = false;

                for line in &lines[1..] {
                    let lower = line.to_lowercase();
                    if lower.starts_with("host:") {
                        has_host = true;
                    } else if lower.starts_with("accept:") && lower.contains("text/event-stream") {
                        is_sse_accept = true;
                    } else if lower.starts_with("authorization: bearer ") {
                        authorization_header = Some(line[22..].trim().to_string());
                    } else if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
                        has_transfer_encoding_chunked = true;
                    } else if lower.starts_with("content-type:") {
                        content_type_header = Some(line[13..].trim().to_string());
                    } else if lower.starts_with("origin:") {
                        origin_header = Some(line[7..].trim().to_string());
                    } else if lower.starts_with("content-length:") {
                        let val_str = line[15..].trim();
                        if let Ok(len) = val_str.parse::<usize>() {
                            if let Some(existing) = content_length {
                                if existing != len {
                                    bad_content_length = true;
                                }
                            } else {
                                content_length = Some(len);
                            }
                        } else {
                            bad_content_length = true;
                        }
                    }
                }

                if bad_content_length {
                    tracing::warn!(
                        "Conflicting or invalid Content-Length header from {}",
                        crate::server::redact_for_log(&peer_addr.to_string())
                    );
                    let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 30\r\nConnection: close\r\n\r\nConflicting Content-Length";
                    let _ = socket.write_all(resp.as_bytes()).await;
                    return;
                }

                // Finding 4 / AUDIT-04: Reject Transfer-Encoding requests with 400 Bad Request
                if has_transfer_encoding_chunked {
                    tracing::warn!(
                        "Transfer-Encoding chunked rejected from {}",
                        crate::server::redact_for_log(&peer_addr.to_string())
                    );
                    let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 38\r\nConnection: close\r\n\r\nTransfer-Encoding is not supported";
                    let _ = socket.write_all(resp.as_bytes()).await;
                    return;
                }

                let content_length = content_length.unwrap_or(0);

                if http_version == "HTTP/1.1" && !has_host {
                    tracing::warn!(
                        "Missing Host header in HTTP/1.1 request from {}",
                        crate::server::redact_for_log(&peer_addr.to_string())
                    );
                    let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 20\r\nConnection: close\r\n\r\nMissing Host header";
                    let _ = socket.write_all(resp.as_bytes()).await;
                    return;
                }

                if let Some(expected_token) = &token_ref {
                    let authorized = if let Some(token) = &authorization_header {
                        token.as_bytes().ct_eq(expected_token.as_bytes()).into()
                    } else {
                        false
                    };

                    let is_public_get = method == "GET" && path == "/health";
                    if !authorized && !is_public_get {
                        tracing::warn!(
                            "Unauthorized HTTP request to {} from {}",
                            crate::server::redact_for_log(path),
                            crate::server::redact_for_log(&peer_addr.to_string())
                        );
                        let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Type: text/plain\r\nContent-Length: 26\r\nConnection: close\r\n\r\nInvalid or missing Bearer";
                        let _ = socket.write_all(resp.as_bytes()).await;
                        return;
                    }
                }

                if content_length > 10 * 1024 * 1024 {
                    tracing::warn!(
                        "Payload exceeds 10MB limit ({} bytes) from {}",
                        content_length,
                        crate::server::redact_for_log(&peer_addr.to_string())
                    );
                    let resp = "HTTP/1.1 413 Payload Too Large\r\nContent-Length: 20\r\nConnection: close\r\n\r\nPayload exceeds 10MB";
                    let _ = socket.write_all(resp.as_bytes()).await;
                    return;
                }

                let body_read_res = tokio::time::timeout(Duration::from_secs(30), async {
                    while buffer.len() - header_end < content_length {
                        let n = match socket.read(&mut temp_buf).await {
                            Ok(n) if n > 0 => n,
                            _ => return Err(()),
                        };
                        buffer.extend_from_slice(&temp_buf[..n]);
                    }
                    Ok(())
                })
                .await;

                match body_read_res {
                    Ok(Ok(())) => {}
                    Ok(Err(())) => {
                        tracing::warn!(
                            "Incomplete request payload from {}",
                            crate::server::redact_for_log(&peer_addr.to_string())
                        );
                        let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 28\r\nConnection: close\r\n\r\nIncomplete request payload";
                        let _ = socket.write_all(resp.as_bytes()).await;
                        return;
                    }
                    Err(_) => {
                        tracing::warn!(
                            "Request body read timed out after 30s from {}",
                            crate::server::redact_for_log(&peer_addr.to_string())
                        );
                        let resp = "HTTP/1.1 408 Request Timeout\r\nContent-Length: 15\r\nConnection: close\r\n\r\nRequest Timeout";
                        let _ = socket.write_all(resp.as_bytes()).await;
                        return;
                    }
                }

                let cors_header = match &cors_ref {
                    Some(origin) => {
                        let clean_origin = origin.replace(['\r', '\n'], "");
                        format!("Access-Control-Allow-Origin: {}\r\n", clean_origin)
                    }
                    None => String::new(),
                };

                if method == "OPTIONS" {
                    let response = format!(
                        "HTTP/1.1 204 No Content\r\n{}Access-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nConnection: close\r\n\r\n",
                        cors_header
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    return;
                }

                if method == "GET" && path == "/" {
                    let dashboard_html = render_dashboard_html(&server_ref);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n{}Content-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        cors_header,
                        dashboard_html.len(),
                        dashboard_html
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                } else if method == "GET" && path == "/health" {
                    let status = concat!(
                        "{\"status\":\"healthy\",\"server\":\"intermcp\",\"version\":\"",
                        env!("CARGO_PKG_VERSION"),
                        "\"}"
                    );
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        cors_header,
                        status.len(),
                        status
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                } else if method == "GET" && path == "/sse" && is_sse_accept {
                    let (refusal, session_id, sse_rx) = {
                        let mut sse_guard = get_sse_sessions().write();
                        if sse_guard.sessions.len() >= MAX_SSE_SESSIONS {
                            (Some((503, "Maximum SSE sessions reached")), None, None)
                        } else if !sse_guard.check_and_record_sse_ip(peer_addr.ip()) {
                            (Some((429, "Too many SSE sessions for this IP")), None, None)
                        } else {
                            let sid = generate_session_id();
                            let (tx, rx) = tokio::sync::mpsc::channel(64);
                            sse_guard.sessions.insert(sid.clone(), tx);
                            (None, Some(sid), Some(rx))
                        }
                    };

                    if let Some((code, msg)) = refusal {
                        if code == 503 {
                            tracing::warn!(
                                "Maximum SSE sessions reached ({}) when connection attempted from {}",
                                MAX_SSE_SESSIONS,
                                crate::server::redact_for_log(&peer_addr.to_string())
                            );
                            let resp = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 31\r\nConnection: close\r\n\r\nMaximum SSE sessions reached";
                            let _ = socket.write_all(resp.as_bytes()).await;
                        } else {
                            tracing::warn!(
                                "Per-IP SSE connection cap reached: refusing 4th connection within 60s from {}",
                                crate::server::redact_for_log(&peer_addr.to_string())
                            );
                            let resp = format!(
                                "HTTP/1.1 429 Too Many Requests\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                                cors_header,
                                msg.len(),
                                msg
                            );
                            let _ = socket.write_all(resp.as_bytes()).await;
                        }
                        return;
                    }

                    let session_id = session_id.unwrap();
                    let mut rx = sse_rx.unwrap();

                    let sse_init = format!(
                        "HTTP/1.1 200 OK\r\n{}Content-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n",
                        cors_header
                    );
                    if socket.write_all(sse_init.as_bytes()).await.is_err() {
                        get_sse_sessions().write().sessions.remove(&session_id);
                        return;
                    }

                    // Emit official MCP 2024-11-05 endpoint event
                    let endpoint_event = format!(
                        "event: endpoint\ndata: /message?sessionId={}\n\n",
                        session_id
                    );
                    if socket.write_all(endpoint_event.as_bytes()).await.is_err() {
                        get_sse_sessions().write().sessions.remove(&session_id);
                        return;
                    }
                    let _ = socket.flush().await;

                    let mut ping_interval = tokio::time::interval(Duration::from_secs(15));
                    loop {
                        tokio::select! {
                            msg = rx.recv() => {
                                match msg {
                                    Some(payload) => {
                                        // Finding 2: Safe SSE serialization preventing framing injection
                                        let sanitized = payload.replace("\r\n", "\n").replace('\r', "\n");
                                        let mut data_lines = String::new();
                                        for line in sanitized.split('\n') {
                                            data_lines.push_str("data: ");
                                            data_lines.push_str(line);
                                            data_lines.push('\n');
                                        }
                                        let event = format!("event: message\n{}\n", data_lines);
                                        if socket.write_all(event.as_bytes()).await.is_err() {
                                            break;
                                        }
                                        let _ = socket.flush().await;
                                    }
                                    None => break,
                                }
                            }
                            _ = ping_interval.tick() => {
                                let ping_msg = ": ping\n\n";
                                if socket.write_all(ping_msg.as_bytes()).await.is_err() {
                                    break;
                                }
                                let _ = socket.flush().await;
                            }
                        }
                    }
                    get_sse_sessions().write().sessions.remove(&session_id);
                } else if method == "POST"
                    && (path == "/message" || raw_path.starts_with("/message?"))
                {
                    let body_slice = &buffer[header_end..header_end + content_length];
                    let body_str = String::from_utf8_lossy(body_slice);

                    // If sessionId is present in query string, validate format and lookup
                    let raw_session_id = raw_path
                        .split("sessionId=")
                        .nth(1)
                        .and_then(|s| s.split('&').next());

                    if let Some(raw_sid) = raw_session_id {
                        if is_valid_session_id(raw_sid) {
                            let sse_tx = get_sse_sessions().read().sessions.get(raw_sid).cloned();
                            if let Some(tx) = sse_tx {
                                if let Some(resp_json) =
                                    server_ref.handle_raw_message(&body_str).await
                                {
                                    let _ = tx.send(resp_json).await;
                                }
                                let response = format!(
                                    "HTTP/1.1 202 Accepted\r\n{}Content-Type: text/plain\r\nContent-Length: 8\r\nConnection: close\r\n\r\nAccepted",
                                    cors_header
                                );
                                let _ = socket.write_all(response.as_bytes()).await;
                                let _ = socket.flush().await;
                                return;
                            }
                        }
                        tracing::warn!(
                            "SSE message dispatch failed: session ID not found '{}'",
                            crate::server::redact_for_log(raw_sid)
                        );
                        let response = format!(
                            "HTTP/1.1 404 Not Found\r\n{}Content-Type: text/plain\r\nContent-Length: 20\r\nConnection: close\r\n\r\nSession ID not found",
                            cors_header
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.flush().await;
                        return;
                    }

                    if let Some(resp_json) = server_ref.handle_raw_message(&body_str).await {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            cors_header,
                            resp_json.len(),
                            resp_json
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.flush().await;
                    } else {
                        let response = format!(
                            "HTTP/1.1 204 No Content\r\n{}Connection: close\r\n\r\n",
                            cors_header
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.flush().await;
                    }
                } else if method == "POST" && (path == "/mcp" || path == "/") {
                    let body_slice = &buffer[header_end..header_end + content_length];
                    let body_str = String::from_utf8_lossy(body_slice);

                    let resp_body = match server_ref.handle_raw_message(&body_str).await {
                        Some(json_str) => json_str,
                        None => "{\"jsonrpc\":\"2.0\",\"result\":null}".to_string(),
                    };

                    let response = format!(
                        "HTTP/1.1 200 OK\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        cors_header,
                        resp_body.len(),
                        resp_body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                } else if method == "GET" && path == "/api/pending" {
                    if let Some(expected_token) = &token_ref {
                        let authorized = if let Some(token) = &authorization_header {
                            token.as_bytes().ct_eq(expected_token.as_bytes()).into()
                        } else {
                            false
                        };
                        if !authorized {
                            tracing::warn!(
                                "Unauthorized /api/pending request from {}",
                                crate::server::redact_for_log(&peer_addr.to_string())
                            );
                            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Type: text/plain\r\nContent-Length: 26\r\nConnection: close\r\n\r\nInvalid or missing Bearer";
                            let _ = socket.write_all(resp.as_bytes()).await;
                            return;
                        }
                    }

                    let json_str = match server_ref.vault_lock() {
                        Some(v) => {
                            let pending = v.list_pending();
                            serde_json::to_string(&pending)
                                .unwrap_or_else(|_| "{\"pending\":[]}".into())
                        }
                        None => "{\"pending\":[]}".into(),
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n{}Content-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        cors_header,
                        json_str.len(),
                        json_str
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                } else if path.starts_with("/api/approve/") || path.starts_with("/approve/") {
                    if let Some(expected_token) = &token_ref {
                        let authorized = if let Some(token) = &authorization_header {
                            token.as_bytes().ct_eq(expected_token.as_bytes()).into()
                        } else {
                            false
                        };
                        if !authorized {
                            tracing::warn!(
                                "Unauthorized approve request on {} from {}",
                                crate::server::redact_for_log(path),
                                crate::server::redact_for_log(&peer_addr.to_string())
                            );
                            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Type: text/plain\r\nContent-Length: 26\r\nConnection: close\r\n\r\nInvalid or missing Bearer";
                            let _ = socket.write_all(resp.as_bytes()).await;
                            return;
                        }
                    }

                    if method != "POST" {
                        tracing::warn!(
                            "Method {} not allowed on {}",
                            crate::server::redact_for_log(method),
                            crate::server::redact_for_log(path)
                        );
                        let resp = format!(
                            "HTTP/1.1 405 Method Not Allowed\r\n{}Allow: POST\r\nContent-Type: text/plain\r\nContent-Length: 18\r\nConnection: close\r\n\r\nMethod Not Allowed",
                            cors_header
                        );
                        let _ = socket.write_all(resp.as_bytes()).await;
                        return;
                    }

                    if let Some(expected_origin) = &cors_ref {
                        if let Some(req_origin) = &origin_header {
                            if req_origin != expected_origin {
                                tracing::warn!(
                                    "Origin mismatch on {}: received '{}', expected '{}'",
                                    crate::server::redact_for_log(path),
                                    crate::server::redact_for_log(req_origin),
                                    crate::server::redact_for_log(expected_origin)
                                );
                                let resp = format!(
                                    "HTTP/1.1 403 Forbidden\r\n{}Content-Type: text/plain\r\nContent-Length: 16\r\nConnection: close\r\n\r\nOrigin Forbidden",
                                    cors_header
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                return;
                            }
                        }
                    }

                    if content_length > 0 {
                        let is_json = content_type_header
                            .as_deref()
                            .map(|ct| ct.to_lowercase().starts_with("application/json"))
                            .unwrap_or(false);
                        if !is_json {
                            tracing::warn!(
                                "Invalid Content-Type on {}: expected application/json or empty body",
                                crate::server::redact_for_log(path)
                            );
                            let resp = format!(
                                "HTTP/1.1 415 Unsupported Media Type\r\n{}Content-Type: text/plain\r\nContent-Length: 32\r\nConnection: close\r\n\r\nContent-Type must be application/json",
                                cors_header
                            );
                            let _ = socket.write_all(resp.as_bytes()).await;
                            return;
                        }
                    }

                    let id = path.rsplit('/').next().unwrap_or("");
                    let approved = server_ref
                        .vault_lock()
                        .map(|v| v.approve(id))
                        .unwrap_or(false);
                    if !approved {
                        tracing::warn!(
                            "Approval action failed or expired for id '{}'",
                            crate::server::redact_for_log(id)
                        );
                    }
                    let status = if approved {
                        "{\"success\":true,\"action\":\"approved\"}"
                    } else {
                        "{\"success\":false,\"error\":\"not found or expired\"}"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        cors_header,
                        status.len(),
                        status
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                } else if path.starts_with("/api/reject/") || path.starts_with("/reject/") {
                    if let Some(expected_token) = &token_ref {
                        let authorized = if let Some(token) = &authorization_header {
                            token.as_bytes().ct_eq(expected_token.as_bytes()).into()
                        } else {
                            false
                        };
                        if !authorized {
                            tracing::warn!(
                                "Unauthorized reject request on {} from {}",
                                crate::server::redact_for_log(path),
                                crate::server::redact_for_log(&peer_addr.to_string())
                            );
                            let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Type: text/plain\r\nContent-Length: 26\r\nConnection: close\r\n\r\nInvalid or missing Bearer";
                            let _ = socket.write_all(resp.as_bytes()).await;
                            return;
                        }
                    }

                    if method != "POST" {
                        tracing::warn!(
                            "Method {} not allowed on {}",
                            crate::server::redact_for_log(method),
                            crate::server::redact_for_log(path)
                        );
                        let resp = format!(
                            "HTTP/1.1 405 Method Not Allowed\r\n{}Allow: POST\r\nContent-Type: text/plain\r\nContent-Length: 18\r\nConnection: close\r\n\r\nMethod Not Allowed",
                            cors_header
                        );
                        let _ = socket.write_all(resp.as_bytes()).await;
                        return;
                    }

                    if let Some(expected_origin) = &cors_ref {
                        if let Some(req_origin) = &origin_header {
                            if req_origin != expected_origin {
                                tracing::warn!(
                                    "Origin mismatch on {}: received '{}', expected '{}'",
                                    crate::server::redact_for_log(path),
                                    crate::server::redact_for_log(req_origin),
                                    crate::server::redact_for_log(expected_origin)
                                );
                                let resp = format!(
                                    "HTTP/1.1 403 Forbidden\r\n{}Content-Type: text/plain\r\nContent-Length: 16\r\nConnection: close\r\n\r\nOrigin Forbidden",
                                    cors_header
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                return;
                            }
                        }
                    }

                    if content_length > 0 {
                        let is_json = content_type_header
                            .as_deref()
                            .map(|ct| ct.to_lowercase().starts_with("application/json"))
                            .unwrap_or(false);
                        if !is_json {
                            tracing::warn!(
                                "Invalid Content-Type on {}: expected application/json or empty body",
                                crate::server::redact_for_log(path)
                            );
                            let resp = format!(
                                "HTTP/1.1 415 Unsupported Media Type\r\n{}Content-Type: text/plain\r\nContent-Length: 32\r\nConnection: close\r\n\r\nContent-Type must be application/json",
                                cors_header
                            );
                            let _ = socket.write_all(resp.as_bytes()).await;
                            return;
                        }
                    }

                    let id = path.rsplit('/').next().unwrap_or("");
                    let rejected = server_ref
                        .vault_lock()
                        .map(|v| v.reject(id))
                        .unwrap_or(false);
                    if !rejected {
                        tracing::warn!(
                            "Reject action failed or expired for id '{}'",
                            crate::server::redact_for_log(id)
                        );
                    }
                    let status = if rejected {
                        "{\"success\":true,\"action\":\"rejected\"}"
                    } else {
                        "{\"success\":false,\"error\":\"not found or expired\"}"
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        cors_header,
                        status.len(),
                        status
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                } else {
                    tracing::warn!(
                        "HTTP 404 for {} {} from {}",
                        crate::server::redact_for_log(method),
                        crate::server::redact_for_log(raw_path),
                        crate::server::redact_for_log(&peer_addr.to_string())
                    );
                    let not_found = "404 Not Found";
                    let response = format!(
                        "HTTP/1.1 404 Not Found\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        cors_header,
                        not_found.len(),
                        not_found
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                }
            };

            let _ = tokio::time::timeout(Duration::from_secs(3600), handle_conn).await;
        });
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '`' => out.push_str("&#x60;"),
            '$' => out.push_str("&#36;"),
            _ => out.push(c),
        }
    }
    out
}

fn render_dashboard_html(server: &Server) -> String {
    let tool_count = server.tool_count();
    let resource_count = server.resource_count();
    let prompt_count = server.prompt_count();
    let cache_info = match server.cache_stats() {
        Some((hits, misses, entries)) => {
            format!("{} hits / {} misses ({} active)", hits, misses, entries)
        }
        None => "Disabled".to_string(),
    };

    let pending_items = server
        .vault_lock()
        .map(|v| v.list_pending())
        .unwrap_or_default();

    let mut pending_rows = String::new();
    if pending_items.is_empty() {
        pending_rows.push_str("<tr><td colspan='4' style='color: #8b949e; text-align: center;'>No pending tool approvals at this time.</td></tr>");
    } else {
        for p in pending_items {
            let args_str = serde_json::to_string(&p.arguments).unwrap_or_default();
            let safe_tool = html_escape(&p.tool);
            let safe_args = html_escape(&args_str);
            let safe_id = html_escape(&p.id);
            pending_rows.push_str(&format!(
                "<tr><td class='tool-name'>{}</td><td><code>{}</code></td><td>{}s left</td><td><button onclick=\"fetch('/api/approve/{}', {{method:'POST'}}).then(()=>location.reload())\" style=\"background:#238636;color:#fff;border:none;padding:4px 8px;border-radius:4px;cursor:pointer;\">Approve</button> <button onclick=\"fetch('/api/reject/{}', {{method:'POST'}}).then(()=>location.reload())\" style=\"background:#da3633;color:#fff;border:none;padding:4px 8px;border-radius:4px;cursor:pointer;\">Veto</button></td></tr>",
                safe_tool, safe_args, p.remaining_secs, safe_id, safe_id
            ));
        }
    }

    let tools = server.list_tool_definitions();
    let mut tool_rows = String::new();
    for t in tools {
        let safe_name = html_escape(&t.name);
        let safe_desc = html_escape(&t.description);
        tool_rows.push_str(&format!(
            "<tr><td class='tool-name'>{}</td><td class='tool-desc'>{}</td></tr>",
            safe_name, safe_desc
        ));
    }

    let version = env!("CARGO_PKG_VERSION");
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>InterMCP Flight Recorder</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #0d1117; color: #c9d1d9; margin: 0; padding: 24px; }}
.header {{ display: flex; align-items: center; justify-content: space-between; border-bottom: 1px solid #30363d; padding-bottom: 16px; margin-bottom: 24px; }}
.title {{ font-size: 24px; font-weight: 700; color: #58a6ff; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; margin-bottom: 32px; }}
.card {{ background: #161b22; border: 1px solid #30363d; border-radius: 6px; padding: 16px; }}
.card-label {{ font-size: 12px; color: #8b949e; text-transform: uppercase; letter-spacing: 0.5px; }}
.card-val {{ font-size: 28px; font-weight: 700; color: #f0f6fc; margin-top: 8px; }}
table {{ width: 100%; border-collapse: collapse; background: #161b22; border: 1px solid #30363d; border-radius: 6px; margin-bottom: 24px; }}
th, td {{ padding: 12px 16px; text-align: left; border-bottom: 1px solid #21262d; }}
th {{ background: #21262d; color: #8b949e; font-size: 12px; text-transform: uppercase; }}
.tool-name {{ font-family: monospace; color: #79c0ff; font-weight: 600; }}
.tool-desc {{ color: #8b949e; font-size: 13px; }}
code {{ font-family: monospace; color: #e6edf3; background: #21262d; padding: 2px 4px; border-radius: 4px; }}
</style>
</head>
<body>
<div class="header">
<div class="title">InterMCP Live Dashboard</div>
<div>v{version}</div>
</div>
<div class="grid">
<div class="card"><div class="card-label">Registered Tools</div><div class="card-val">{tool_count}</div></div>
<div class="card"><div class="card-label">Active Resources</div><div class="card-val">{resource_count}</div></div>
<div class="card"><div class="card-label">Prompts Available</div><div class="card-val">{prompt_count}</div></div>
<div class="card"><div class="card-label">Micro-Cache</div><div class="card-val" style="font-size: 16px; margin-top: 14px;">{cache_info}</div></div>
</div>

<h3>Time-Locked Supervisor Approvals</h3>
<table>
<thead><tr><th>Tool</th><th>Action Payload</th><th>Time Remaining</th><th>Supervisor Decision</th></tr></thead>
<tbody>
{pending_rows}
</tbody>
</table>

<h3>Active Protocol Tools</h3>
<table>
<thead><tr><th>Tool Identifier</th><th>Description</th></tr></thead>
<tbody>
{tool_rows}
</tbody>
</table>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape_all_dangerous_characters() {
        let input = "<script>alert(`hello ${user}` & 'win');</script>\"";
        let escaped = html_escape(input);
        assert!(!escaped.contains('`'));
        assert!(!escaped.contains('$'));
        assert!(!escaped.contains('<'));
        assert!(!escaped.contains('>'));
        assert!(escaped.contains("&#x60;"));
        assert!(escaped.contains("&#36;"));
        assert!(escaped.contains("&lt;"));
        assert!(escaped.contains("&gt;"));
        assert!(escaped.contains("&quot;"));
        assert!(escaped.contains("&#39;"));
    }
}
