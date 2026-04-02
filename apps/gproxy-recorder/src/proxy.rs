use std::io::Read as _;
use std::sync::Arc;
use std::time::Instant;

use bytes::BytesMut;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;
use tracing::{error, info, warn};

use crate::har::HarStreamEvent;
use crate::mitm::MitmAcceptor;
use crate::recorder::Recorder;

/// Connect to an upstream host, optionally through a SOCKS5 proxy.
async fn connect_upstream(
    host: &str,
    port: u16,
    upstream_proxy: &Option<String>,
) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
    let addr = format!("{}:{}", host, port);
    if let Some(proxy_url) = upstream_proxy {
        let proxy_addr = proxy_url.strip_prefix("socks5://").unwrap_or(proxy_url);
        let stream = tokio_socks::tcp::Socks5Stream::connect(proxy_addr, addr.as_str()).await?;
        Ok(stream.into_inner())
    } else {
        Ok(TcpStream::connect(&addr).await?)
    }
}

/// Start the HTTP forward proxy listener.
pub async fn run_http_proxy(
    listen_addr: String,
    mitm: Arc<MitmAcceptor>,
    recorder: Arc<Recorder>,
    upstream_proxy: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(&listen_addr).await?;
    let upstream_proxy = Arc::new(upstream_proxy);
    info!("HTTP proxy listening on {}", listen_addr);

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let mitm = Arc::clone(&mitm);
        let recorder = Arc::clone(&recorder);
        let upstream_proxy = Arc::clone(&upstream_proxy);

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, mitm, recorder, upstream_proxy).await {
                warn!("Connection from {} error: {}", peer_addr, e);
            }
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    mitm: Arc<MitmAcceptor>,
    recorder: Arc<Recorder>,
    upstream_proxy: Arc<Option<String>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mitm2 = Arc::clone(&mitm);
    let recorder2 = Arc::clone(&recorder);
    let upstream_proxy2 = Arc::clone(&upstream_proxy);

    let service = service_fn(move |req: Request<Incoming>| {
        let mitm = Arc::clone(&mitm2);
        let recorder = Arc::clone(&recorder2);
        let upstream_proxy = Arc::clone(&upstream_proxy2);
        async move { handle_request(req, mitm, recorder, upstream_proxy).await }
    });

    http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades()
        .await?;

    Ok(())
}

async fn handle_request(
    req: Request<Incoming>,
    mitm: Arc<MitmAcceptor>,
    recorder: Arc<Recorder>,
    upstream_proxy: Arc<Option<String>>,
) -> Result<Response<Full<bytes::Bytes>>, hyper::Error> {
    if req.method() == Method::CONNECT {
        match handle_connect(req, mitm, recorder, upstream_proxy).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                error!("CONNECT error: {}", e);
                Ok(Response::builder()
                    .status(502)
                    .body(Full::new(bytes::Bytes::from(format!("CONNECT error: {e}"))))
                    .unwrap())
            }
        }
    } else {
        match handle_plain_http(req, recorder, upstream_proxy).await {
            Ok(resp) => Ok(resp),
            Err(e) => {
                error!("HTTP forward error: {}", e);
                Ok(Response::builder()
                    .status(502)
                    .body(Full::new(bytes::Bytes::from(format!("Proxy error: {e}"))))
                    .unwrap())
            }
        }
    }
}

async fn handle_connect(
    req: Request<Incoming>,
    mitm: Arc<MitmAcceptor>,
    recorder: Arc<Recorder>,
    upstream_proxy: Arc<Option<String>>,
) -> Result<Response<Full<bytes::Bytes>>, Box<dyn std::error::Error + Send + Sync>> {
    let target = req
        .uri()
        .authority()
        .map(|a| a.to_string())
        .unwrap_or_else(|| req.uri().to_string());

    let (host, port) = parse_host_port(&target, 443);

    // Respond 200 to the client to establish the tunnel
    tokio::spawn(async move {
        // Wait for the upgrade
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let io = TokioIo::new(upgraded);

                if let Err(e) =
                    handle_connect_tunnel(io, &host, port, mitm, recorder, upstream_proxy).await
                {
                    warn!("Tunnel error for {}:{}: {}", host, port, e);
                }
            }
            Err(e) => {
                error!("Upgrade error: {}", e);
            }
        }
    });

    Ok(Response::builder()
        .status(200)
        .body(Full::new(bytes::Bytes::new()))
        .unwrap())
}

async fn handle_connect_tunnel(
    client_stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    host: &str,
    port: u16,
    mitm: Arc<MitmAcceptor>,
    recorder: Arc<Recorder>,
    upstream_proxy: Arc<Option<String>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Convert the upgraded client stream into a TcpStream-like pair via a local
    // TCP loopback so we can hand the server-side to the MITM TLS acceptor.
    let loopback_listener = TcpListener::bind("127.0.0.1:0").await?;
    let loopback_addr = loopback_listener.local_addr()?;

    let host_owned = host.to_string();

    // Spawn a task that copies data between the upgraded client connection and
    // the loopback client socket.
    let copy_handle = tokio::spawn(async move {
        if let Ok(mut loopback_client) = TcpStream::connect(loopback_addr).await {
            let (mut cr, mut cw) = tokio::io::split(client_stream);
            let (mut lr, mut lw) = loopback_client.split();
            let _ = tokio::join!(
                tokio::io::copy(&mut cr, &mut lw),
                tokio::io::copy(&mut lr, &mut cw),
            );
        }
    });

    // Accept on the loopback to get a real TcpStream for MITM
    let (loopback_server, _) = loopback_listener.accept().await?;

    // MITM TLS handshake with the client (through loopback)
    let tls_stream = mitm
        .accept(loopback_server, &host_owned)
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e })?;

    // Now serve HTTP/1 over the decrypted TLS connection
    let host_for_service = host_owned.clone();
    let port_for_service = port;
    let recorder_for_service = Arc::clone(&recorder);
    let upstream_proxy_for_service = Arc::clone(&upstream_proxy);

    let service = service_fn(move |req: Request<Incoming>| {
        let host = host_for_service.clone();
        let recorder = Arc::clone(&recorder_for_service);
        let upstream_proxy = Arc::clone(&upstream_proxy_for_service);
        async move {
            handle_tunneled_request(req, &host, port_for_service, recorder, upstream_proxy).await
        }
    });

    let result: Result<(), hyper::Error> = http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(TokioIo::new(tls_stream), service)
        .await;

    if let Err(e) = result {
        // Client disconnect is normal
        if !e.is_incomplete_message() {
            warn!("MITM HTTP serve error for {}: {}", host_owned, e);
        }
    }

    copy_handle.abort();
    Ok(())
}

async fn handle_tunneled_request(
    req: Request<Incoming>,
    host: &str,
    port: u16,
    recorder: Arc<Recorder>,
    upstream_proxy: Arc<Option<String>>,
) -> Result<Response<Full<bytes::Bytes>>, hyper::Error> {
    let method = req.method().to_string();
    let path = req
        .uri()
        .path_and_query()
        .map(|pq| pq.to_string())
        .unwrap_or_else(|| "/".to_string());
    let url = format!("https://{}:{}{}", host, port, path);
    let http_version = format!("{:?}", req.version());

    let start = Instant::now();
    let started = chrono::Utc::now();

    // Collect request headers
    let request_headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let request_content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Collect request body
    let (parts, body) = req.into_parts();
    let request_body_bytes = match http_body_util::BodyExt::collect(body).await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => bytes::Bytes::new(),
    };
    let request_body = if request_body_bytes.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&request_body_bytes).to_string())
    };

    // Connect to the real upstream server
    match connect_and_forward_tls(
        host,
        port,
        &method,
        &path,
        &parts.headers,
        &request_body_bytes,
        &upstream_proxy,
    )
    .await
    {
        Ok((
            status,
            status_text,
            response_headers,
            response_body,
            streaming_events,
            response_content_type,
            response_content_encoding,
        )) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;

            let decoded_body = decompress_body(&response_body, &response_content_encoding);
            let response_headers_for_client = response_headers.clone();
            recorder.record_entry(
                &method,
                &url,
                &http_version,
                request_headers,
                request_body,
                request_content_type,
                status,
                &status_text,
                response_headers,
                Some(String::from_utf8_lossy(&decoded_body).to_string()),
                response_content_type,
                streaming_events,
                started,
                elapsed,
            );

            let mut resp_builder = Response::builder().status(status);
            for (key, value) in &response_headers_for_client {
                resp_builder = resp_builder.header(key.as_str(), value.as_str());
            }
            let resp = resp_builder
                .body(Full::new(bytes::Bytes::from(response_body)))
                .unwrap();

            Ok(resp)
        }
        Err(e) => {
            error!("Upstream connection error: {}", e);
            Ok(Response::builder()
                .status(502)
                .body(Full::new(bytes::Bytes::from(format!(
                    "Upstream error: {e}"
                ))))
                .unwrap())
        }
    }
}

async fn connect_and_forward_tls(
    host: &str,
    port: u16,
    method: &str,
    path: &str,
    headers: &hyper::HeaderMap,
    body: &[u8],
    upstream_proxy: &Option<String>,
) -> Result<
    (
        u16,
        String,
        Vec<(String, String)>,
        Vec<u8>,
        Option<Vec<HarStreamEvent>>,
        Option<String>,
        Option<String>,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let tcp_stream = connect_upstream(host, port, upstream_proxy).await?;

    // TLS connect to upstream
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(tls_config));
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())?;
    let tls_stream = connector.connect(server_name, tcp_stream).await?;

    // Manually write HTTP/1.1 request and read response
    let (mut reader, mut writer) = tokio::io::split(tls_stream);

    // Build raw HTTP request
    let mut raw_request = format!("{} {} HTTP/1.1\r\n", method, path);
    let mut has_host = false;
    for (key, value) in headers.iter() {
        let key_str = key.as_str();
        if key_str.eq_ignore_ascii_case("host") {
            has_host = true;
        }
        raw_request.push_str(&format!(
            "{}: {}\r\n",
            key_str,
            value.to_str().unwrap_or("")
        ));
    }
    if !has_host {
        raw_request.push_str(&format!("Host: {}\r\n", host));
    }
    if !body.is_empty() && !headers.contains_key("content-length") {
        raw_request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    raw_request.push_str("\r\n");

    writer.write_all(raw_request.as_bytes()).await?;
    if !body.is_empty() {
        writer.write_all(body).await?;
    }
    writer.flush().await?;

    // Read response headers
    let mut response_buf = BytesMut::with_capacity(8192);
    let mut header_end = None;

    loop {
        let mut tmp = [0u8; 4096];
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        response_buf.extend_from_slice(&tmp[..n]);

        if let Some(pos) = find_header_end(&response_buf) {
            header_end = Some(pos);
            break;
        }
    }

    let header_end = header_end.ok_or("Failed to read response headers")?;
    let header_str = String::from_utf8_lossy(&response_buf[..header_end]).to_string();

    // Parse status line and headers
    let mut lines = header_str.split("\r\n");
    let status_line = lines.next().unwrap_or("HTTP/1.1 502 Bad Gateway");
    let mut status_parts = status_line.splitn(3, ' ');
    let _http_ver = status_parts.next().unwrap_or("HTTP/1.1");
    let status: u16 = status_parts.next().unwrap_or("502").parse().unwrap_or(502);
    let status_text = status_parts.next().unwrap_or("").to_string();

    let mut response_headers: Vec<(String, String)> = Vec::new();
    let mut content_type: Option<String> = None;
    let mut content_length: Option<usize> = None;
    let mut content_encoding: Option<String> = None;
    let mut is_chunked = false;

    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if key.eq_ignore_ascii_case("content-type") {
                content_type = Some(value.clone());
            }
            if key.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().ok();
            }
            if key.eq_ignore_ascii_case("content-encoding") {
                content_encoding = Some(value.clone());
            }
            if key.eq_ignore_ascii_case("transfer-encoding")
                && value.to_lowercase().contains("chunked")
            {
                is_chunked = true;
            }
            response_headers.push((key, value));
        }
    }

    let is_streaming = content_type
        .as_deref()
        .is_some_and(is_streaming_content_type);

    // If response is compressed, don't try to record per-chunk streaming events
    // (compressed chunks are not individually decodable). We'll split after decompression.
    let is_compressed = content_encoding.as_ref().is_some_and(|e| {
        let l = e.to_lowercase();
        l.contains("gzip") || l.contains("deflate") || l.contains("br")
    });
    let record_chunks = is_streaming && !is_compressed;

    // Body data starts after header_end + 4 bytes (\r\n\r\n)
    let body_start = header_end + 4;
    let initial_body = if body_start < response_buf.len() {
        response_buf[body_start..].to_vec()
    } else {
        Vec::new()
    };

    let (response_body, mut streaming_events) = if is_chunked {
        read_chunked_body(&mut reader, &initial_body, record_chunks).await?
    } else if let Some(cl) = content_length {
        read_fixed_body(&mut reader, &initial_body, cl, record_chunks).await?
    } else {
        read_until_eof(&mut reader, &initial_body, record_chunks).await?
    };

    // For compressed streaming responses: decompress full body, then split into SSE events
    if is_streaming && is_compressed {
        let decoded = decompress_body(&response_body, &content_encoding);
        let text = String::from_utf8_lossy(&decoded);
        streaming_events = Some(split_sse_events(&text));
    }

    Ok((
        status,
        status_text,
        response_headers,
        response_body,
        streaming_events,
        content_type,
        content_encoding,
    ))
}

fn is_streaming_content_type(ct: &str) -> bool {
    let ct_lower = ct.to_lowercase();
    ct_lower.contains("text/event-stream")
        || ct_lower.contains("application/x-ndjson")
        || ct_lower.contains("application/stream+json")
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

async fn read_fixed_body(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    initial: &[u8],
    content_length: usize,
    is_streaming: bool,
) -> Result<(Vec<u8>, Option<Vec<HarStreamEvent>>), Box<dyn std::error::Error + Send + Sync>> {
    let stream_start = Instant::now();
    let mut body = Vec::with_capacity(content_length);
    let mut events = if is_streaming { Some(Vec::new()) } else { None };

    body.extend_from_slice(initial);
    if is_streaming {
        record_chunk_events(events.as_mut().unwrap(), initial, stream_start);
    }

    while body.len() < content_length {
        let mut tmp = [0u8; 8192];
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        let chunk = &tmp[..n];
        body.extend_from_slice(chunk);
        if is_streaming {
            record_chunk_events(events.as_mut().unwrap(), chunk, stream_start);
        }
    }

    Ok((body, events))
}

async fn read_chunked_body(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    initial: &[u8],
    is_streaming: bool,
) -> Result<(Vec<u8>, Option<Vec<HarStreamEvent>>), Box<dyn std::error::Error + Send + Sync>> {
    let stream_start = Instant::now();
    let mut events = if is_streaming { Some(Vec::new()) } else { None };

    // Accumulate raw chunked data, then decode
    let mut raw = Vec::from(initial);

    loop {
        let mut tmp = [0u8; 8192];
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&tmp[..n]);

        // Check if we have the terminating 0\r\n\r\n
        if raw.windows(5).any(|w| w == b"0\r\n\r\n") {
            break;
        }
    }

    let decoded = decode_chunked(&raw, is_streaming, &mut events, stream_start);

    Ok((decoded, events))
}

fn decode_chunked(
    data: &[u8],
    is_streaming: bool,
    events: &mut Option<Vec<HarStreamEvent>>,
    stream_start: Instant,
) -> Vec<u8> {
    let mut body = Vec::new();
    let mut pos = 0;

    loop {
        let remaining = &data[pos..];
        let crlf = match remaining.windows(2).position(|w| w == b"\r\n") {
            Some(p) => p,
            None => break,
        };

        let size_str = String::from_utf8_lossy(&remaining[..crlf]);
        let size_str = size_str.trim();
        let chunk_size = match usize::from_str_radix(size_str, 16) {
            Ok(s) => s,
            Err(_) => break,
        };

        if chunk_size == 0 {
            break;
        }

        let chunk_start = pos + crlf + 2;
        let chunk_end = chunk_start + chunk_size;
        if chunk_end > data.len() {
            body.extend_from_slice(&data[chunk_start..]);
            break;
        }

        let chunk = &data[chunk_start..chunk_end];
        body.extend_from_slice(chunk);

        if is_streaming {
            record_chunk_events(events.as_mut().unwrap(), chunk, stream_start);
        }

        pos = chunk_end + 2;
        if pos > data.len() {
            break;
        }
    }

    body
}

async fn read_until_eof(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    initial: &[u8],
    is_streaming: bool,
) -> Result<(Vec<u8>, Option<Vec<HarStreamEvent>>), Box<dyn std::error::Error + Send + Sync>> {
    let stream_start = Instant::now();
    let mut body = Vec::from(initial);
    let mut events = if is_streaming { Some(Vec::new()) } else { None };

    if is_streaming && !initial.is_empty() {
        record_chunk_events(events.as_mut().unwrap(), initial, stream_start);
    }

    loop {
        let mut tmp = [0u8; 8192];
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        let chunk = &tmp[..n];
        body.extend_from_slice(chunk);
        if is_streaming {
            record_chunk_events(events.as_mut().unwrap(), chunk, stream_start);
        }
    }

    Ok((body, events))
}

fn record_chunk_events(events: &mut Vec<HarStreamEvent>, chunk: &[u8], stream_start: Instant) {
    let timestamp_ms = stream_start.elapsed().as_millis() as u64;
    let data = String::from_utf8_lossy(chunk).to_string();
    if !data.is_empty() {
        events.push(HarStreamEvent { timestamp_ms, data });
    }
}

async fn handle_plain_http(
    req: Request<Incoming>,
    recorder: Arc<Recorder>,
    upstream_proxy: Arc<Option<String>>,
) -> Result<Response<Full<bytes::Bytes>>, Box<dyn std::error::Error + Send + Sync>> {
    let method = req.method().to_string();
    let url = req.uri().to_string();
    let http_version = format!("{:?}", req.version());

    let start = Instant::now();
    let started = chrono::Utc::now();

    let request_headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let request_content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let uri = req.uri().clone();
    let host = uri.host().unwrap_or("localhost").to_string();
    let port = uri.port_u16().unwrap_or(80);

    let (parts, body) = req.into_parts();
    let request_body_bytes = match http_body_util::BodyExt::collect(body).await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => bytes::Bytes::new(),
    };
    let request_body = if request_body_bytes.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&request_body_bytes).to_string())
    };

    // Connect to upstream
    let tcp_stream = connect_upstream(&host, port, &upstream_proxy).await?;

    let path = uri
        .path_and_query()
        .map(|pq| pq.to_string())
        .unwrap_or_else(|| "/".to_string());

    let (mut reader, mut writer) = tcp_stream.into_split();

    // Build raw HTTP request
    let mut raw_request = format!("{} {} HTTP/1.1\r\n", method, path);
    for (key, value) in parts.headers.iter() {
        raw_request.push_str(&format!(
            "{}: {}\r\n",
            key.as_str(),
            value.to_str().unwrap_or("")
        ));
    }
    if !request_body_bytes.is_empty() && !parts.headers.contains_key("content-length") {
        raw_request.push_str(&format!("Content-Length: {}\r\n", request_body_bytes.len()));
    }
    raw_request.push_str("\r\n");

    writer.write_all(raw_request.as_bytes()).await?;
    if !request_body_bytes.is_empty() {
        writer.write_all(&request_body_bytes).await?;
    }
    writer.flush().await?;

    // Read response headers
    let mut response_buf = BytesMut::with_capacity(8192);
    let mut header_end_pos = None;

    loop {
        let mut tmp = [0u8; 4096];
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        response_buf.extend_from_slice(&tmp[..n]);

        if let Some(pos) = find_header_end(&response_buf) {
            header_end_pos = Some(pos);
            break;
        }
    }

    let header_end_pos = header_end_pos.ok_or("Failed to read response headers")?;
    let header_str = String::from_utf8_lossy(&response_buf[..header_end_pos]).to_string();

    let mut lines = header_str.split("\r\n");
    let status_line = lines.next().unwrap_or("HTTP/1.1 502 Bad Gateway");
    let mut status_parts = status_line.splitn(3, ' ');
    let _http_ver = status_parts.next().unwrap_or("HTTP/1.1");
    let status: u16 = status_parts.next().unwrap_or("502").parse().unwrap_or(502);
    let status_text = status_parts.next().unwrap_or("").to_string();

    let mut response_headers: Vec<(String, String)> = Vec::new();
    let mut content_type: Option<String> = None;
    let mut content_length: Option<usize> = None;
    let mut content_encoding: Option<String> = None;
    let mut is_chunked = false;

    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if key.eq_ignore_ascii_case("content-type") {
                content_type = Some(value.clone());
            }
            if key.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().ok();
            }
            if key.eq_ignore_ascii_case("content-encoding") {
                content_encoding = Some(value.clone());
            }
            if key.eq_ignore_ascii_case("transfer-encoding")
                && value.to_lowercase().contains("chunked")
            {
                is_chunked = true;
            }
            response_headers.push((key, value));
        }
    }

    let is_streaming = content_type
        .as_deref()
        .is_some_and(is_streaming_content_type);

    let body_start = header_end_pos + 4;
    let initial_body = if body_start < response_buf.len() {
        response_buf[body_start..].to_vec()
    } else {
        Vec::new()
    };

    let (response_body, streaming_events) = if is_chunked {
        read_chunked_body(&mut reader, &initial_body, is_streaming).await?
    } else if let Some(cl) = content_length {
        read_fixed_body(&mut reader, &initial_body, cl, is_streaming).await?
    } else {
        read_until_eof(&mut reader, &initial_body, is_streaming).await?
    };

    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    let decoded_body = decompress_body(&response_body, &content_encoding);
    let response_headers_for_client = response_headers.clone();
    recorder.record_entry(
        &method,
        &url,
        &http_version,
        request_headers,
        request_body,
        request_content_type,
        status,
        &status_text,
        response_headers,
        Some(String::from_utf8_lossy(&decoded_body).to_string()),
        content_type,
        streaming_events,
        started,
        elapsed,
    );

    let mut resp_builder = Response::builder().status(status);
    for (key, value) in &response_headers_for_client {
        resp_builder = resp_builder.header(key.as_str(), value.as_str());
    }
    Ok(resp_builder
        .body(Full::new(bytes::Bytes::from(response_body)))
        .unwrap())
}

fn parse_host_port(authority: &str, default_port: u16) -> (String, u16) {
    if let Some((host, port_str)) = authority.rsplit_once(':')
        && let Ok(port) = port_str.parse::<u16>()
    {
        return (host.to_string(), port);
    }
    (authority.to_string(), default_port)
}

fn split_sse_events(text: &str) -> Vec<HarStreamEvent> {
    let mut events = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.is_empty() && !current.is_empty() {
            events.push(HarStreamEvent {
                timestamp_ms: 0,
                data: current.trim().to_string(),
            });
            current.clear();
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        events.push(HarStreamEvent {
            timestamp_ms: 0,
            data: current.trim().to_string(),
        });
    }
    events
}

fn decompress_body(body: &[u8], encoding: &Option<String>) -> Vec<u8> {
    let enc = match encoding {
        Some(e) => e.to_lowercase(),
        None => return body.to_vec(),
    };
    if enc.contains("gzip") {
        let mut decoder = flate2::read::GzDecoder::new(body);
        let mut out = Vec::new();
        if decoder.read_to_end(&mut out).is_ok() {
            return out;
        }
    } else if enc.contains("deflate") {
        let mut decoder = flate2::read::DeflateDecoder::new(body);
        let mut out = Vec::new();
        if decoder.read_to_end(&mut out).is_ok() {
            return out;
        }
    }
    body.to_vec()
}
