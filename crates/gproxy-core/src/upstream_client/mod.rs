use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use wreq::{Client, Method, Proxy};

use gproxy_common::GlobalConfig;
use gproxy_provider_core::provider::{UpstreamFailure, UpstreamTransportErrorKind};
use gproxy_provider_core::{
    Headers, HttpMethod, UpstreamBody, UpstreamHttpRequest, UpstreamHttpResponse,
};

pub trait UpstreamClient: Send + Sync {
    fn send<'a>(
        &'a self,
        req: UpstreamHttpRequest,
    ) -> Pin<Box<dyn Future<Output = Result<UpstreamHttpResponse, UpstreamFailure>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
pub struct UpstreamClientConfig {
    pub proxy: Option<String>,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub stream_idle_timeout: Duration,
}

impl UpstreamClientConfig {
    pub fn from_global(global: &GlobalConfig) -> Self {
        Self {
            proxy: global.proxy.clone(),
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(86400),
            stream_idle_timeout: Duration::from_secs(30),
        }
    }
}

impl Default for UpstreamClientConfig {
    fn default() -> Self {
        Self {
            proxy: None,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(86400),
            stream_idle_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone)]
pub struct WreqUpstreamClient {
    config: UpstreamClientConfig,
    proxy_resolver: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    clients: Arc<Mutex<HashMap<Option<String>, Client>>>,
}

impl WreqUpstreamClient {
    pub fn new(config: UpstreamClientConfig) -> Result<Self, wreq::Error> {
        let proxy = normalize_proxy(config.proxy.clone());
        Self::new_with_proxy_resolver(config, move || proxy.clone())
    }

    pub fn new_with_proxy_resolver<F>(
        config: UpstreamClientConfig,
        proxy_resolver: F,
    ) -> Result<Self, wreq::Error>
    where
        F: Fn() -> Option<String> + Send + Sync + 'static,
    {
        let resolver: Arc<dyn Fn() -> Option<String> + Send + Sync> = Arc::new(proxy_resolver);
        let initial_proxy = normalize_proxy(resolver());
        let initial_client = build_client(&config, initial_proxy.as_deref())?;
        let mut clients = HashMap::new();
        clients.insert(initial_proxy, initial_client);
        Ok(Self {
            config,
            proxy_resolver: resolver,
            clients: Arc::new(Mutex::new(clients)),
        })
    }

    fn current_proxy(&self) -> Option<String> {
        normalize_proxy((self.proxy_resolver)())
    }

    fn client_for_proxy(&self, proxy: Option<String>) -> Result<Client, UpstreamFailure> {
        let mut guard = self
            .clients
            .lock()
            .map_err(|_| UpstreamFailure::Transport {
                kind: UpstreamTransportErrorKind::Other,
                message: "upstream client cache lock failed".to_string(),
            })?;
        if let Some(client) = guard.get(&proxy) {
            return Ok(client.clone());
        }
        let client = build_client(&self.config, proxy.as_deref()).map_err(map_wreq_error)?;
        guard.insert(proxy, client.clone());
        Ok(client)
    }
}

fn normalize_proxy(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn build_client(config: &UpstreamClientConfig, proxy: Option<&str>) -> Result<Client, wreq::Error> {
    let mut builder = Client::builder()
        .connect_timeout(config.connect_timeout)
        .timeout(config.request_timeout)
        .read_timeout(config.stream_idle_timeout);

    if let Some(proxy) = proxy {
        builder = builder.proxy(Proxy::all(proxy)?);
    }

    builder.build()
}

impl UpstreamClient for WreqUpstreamClient {
    fn send<'a>(
        &'a self,
        req: UpstreamHttpRequest,
    ) -> Pin<Box<dyn Future<Output = Result<UpstreamHttpResponse, UpstreamFailure>> + Send + 'a>>
    {
        Box::pin(async move {
            let client = self.client_for_proxy(self.current_proxy())?;
            if req.url.starts_with("local://") {
                let body = req.body.unwrap_or_default();
                return Ok(UpstreamHttpResponse {
                    status: 200,
                    headers: req.headers,
                    body: UpstreamBody::Bytes(body),
                });
            }
            let method = http_method_to_wreq(req.method);
            let mut builder = client.request(method, &req.url);

            for (k, v) in &req.headers {
                builder = builder.header(k, v);
            }

            if let Some(body) = req.body {
                builder = builder.body(body);
            }

            let resp = builder.send().await.map_err(map_wreq_error)?;
            convert_response(resp, req.is_stream, self.config.stream_idle_timeout).await
        })
    }
}

fn http_method_to_wreq(method: HttpMethod) -> Method {
    match method {
        HttpMethod::Get => Method::GET,
        HttpMethod::Post => Method::POST,
        HttpMethod::Put => Method::PUT,
        HttpMethod::Patch => Method::PATCH,
        HttpMethod::Delete => Method::DELETE,
    }
}

async fn convert_response(
    resp: wreq::Response,
    want_stream: bool,
    stream_idle_timeout: Duration,
) -> Result<UpstreamHttpResponse, UpstreamFailure> {
    let status = resp.status().as_u16();
    let headers = headers_from_wreq(resp.headers());

    let is_success = (200..300).contains(&status);
    if !is_success || !want_stream {
        let body = resp.bytes().await.map_err(map_wreq_error)?;
        return Ok(UpstreamHttpResponse {
            status,
            headers,
            body: UpstreamBody::Bytes(body),
        });
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(16);
    tokio::spawn(async move {
        let mut stream = resp.bytes_stream();
        loop {
            let next = tokio::time::timeout(stream_idle_timeout, stream.next()).await;
            let item = match next {
                Ok(item) => item,
                Err(_) => break,
            };
            let Some(item) = item else {
                break;
            };
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(_) => break,
            };
            if tx.send(chunk).await.is_err() {
                break;
            }
        }
    });

    Ok(UpstreamHttpResponse {
        status,
        headers,
        body: UpstreamBody::Stream(rx),
    })
}

fn headers_from_wreq(map: &wreq::header::HeaderMap) -> Headers {
    let mut out = Vec::new();
    for (k, v) in map {
        if let Ok(s) = v.to_str() {
            out.push((k.as_str().to_string(), s.to_string()));
        }
    }
    out
}

fn map_wreq_error(err: wreq::Error) -> UpstreamFailure {
    let kind = classify_wreq_error(&err);
    UpstreamFailure::Transport {
        kind,
        message: err.to_string(),
    }
}

fn classify_wreq_error(err: &wreq::Error) -> UpstreamTransportErrorKind {
    let message = err.to_string().to_ascii_lowercase();
    if err.is_timeout() {
        if message.contains("read") || message.contains("idle") {
            return UpstreamTransportErrorKind::ReadTimeout;
        }
        return UpstreamTransportErrorKind::Timeout;
    }
    if err.is_connect() {
        if message.contains("dns") || message.contains("resolve") {
            return UpstreamTransportErrorKind::Dns;
        }
        if message.contains("tls") || message.contains("ssl") {
            return UpstreamTransportErrorKind::Tls;
        }
        return UpstreamTransportErrorKind::Connect;
    }
    if err.is_connection_reset() {
        return UpstreamTransportErrorKind::Connect;
    }
    if message.contains("tls") || message.contains("ssl") {
        return UpstreamTransportErrorKind::Tls;
    }
    UpstreamTransportErrorKind::Other
}
