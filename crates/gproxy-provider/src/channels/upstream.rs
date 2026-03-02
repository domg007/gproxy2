use gproxy_middleware::TransformResponse;
use http::Response as HttpResponse;
use http_body_util::BodyExt as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::task_local;
use wreq::RequestBuilder as WreqRequestBuilder;
use wreq::Response as WreqResponse;
use wreq::header::HeaderMap;
use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::ChannelCredential;

task_local! {
    static TRACKED_HTTP_EVENT_SINK: Arc<Mutex<Vec<TrackedHttpEvent>>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedHttpEvent {
    pub request_meta: UpstreamRequestMeta,
    pub response_status: Option<u16>,
    pub response_headers: Vec<(String, String)>,
    pub response_body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamCredentialUpdate {
    CodexTokenRefresh {
        credential_id: i64,
        access_token: String,
        refresh_token: String,
        expires_at_unix_ms: u64,
        user_email: Option<String>,
        id_token: Option<String>,
    },
    ClaudeCodeTokenRefresh {
        credential_id: i64,
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_at_unix_ms: Option<u64>,
        subscription_type: Option<String>,
        rate_limit_tier: Option<String>,
        user_email: Option<String>,
        cookie: Option<String>,
        enable_claude_1m_sonnet: Option<bool>,
        enable_claude_1m_opus: Option<bool>,
    },
    VertexTokenRefresh {
        credential_id: i64,
        access_token: String,
        expires_at_unix_ms: u64,
    },
    GeminiCliTokenRefresh {
        credential_id: i64,
        access_token: String,
        refresh_token: Option<String>,
        expires_at_unix_ms: u64,
        user_email: Option<String>,
    },
    AntigravityTokenRefresh {
        credential_id: i64,
        access_token: String,
        refresh_token: String,
        expires_at_unix_ms: u64,
        user_email: Option<String>,
    },
}

impl UpstreamCredentialUpdate {
    pub fn credential_id(&self) -> i64 {
        match self {
            Self::CodexTokenRefresh { credential_id, .. }
            | Self::ClaudeCodeTokenRefresh { credential_id, .. }
            | Self::VertexTokenRefresh { credential_id, .. }
            | Self::GeminiCliTokenRefresh { credential_id, .. }
            | Self::AntigravityTokenRefresh { credential_id, .. } => *credential_id,
        }
    }
}

#[derive(Debug)]
pub struct UpstreamResponse {
    pub credential_id: Option<i64>,
    pub attempts: usize,
    pub response: Option<WreqResponse>,
    pub local_response: Option<TransformResponse>,
    pub credential_update: Option<UpstreamCredentialUpdate>,
    pub request_meta: Option<UpstreamRequestMeta>,
}

impl UpstreamResponse {
    pub fn from_http(credential_id: i64, attempts: usize, response: WreqResponse) -> Self {
        Self {
            credential_id: Some(credential_id),
            attempts,
            response: Some(response),
            local_response: None,
            credential_update: None,
            request_meta: None,
        }
    }

    pub fn from_local(local_response: TransformResponse) -> Self {
        Self {
            credential_id: None,
            attempts: 0,
            response: None,
            local_response: Some(local_response),
            credential_update: None,
            request_meta: None,
        }
    }

    pub fn with_credential_update(mut self, update: Option<UpstreamCredentialUpdate>) -> Self {
        self.credential_update = update;
        self
    }

    pub fn with_request_meta(mut self, request_meta: UpstreamRequestMeta) -> Self {
        self.request_meta = Some(request_meta);
        self
    }

    pub async fn into_http_payload(self) -> Result<UpstreamOAuthResponse, UpstreamError> {
        let request_meta = self.request_meta.clone();
        if self.local_response.is_some() {
            return Err(UpstreamError::UnsupportedRequest);
        }
        let Some(response) = self.response else {
            return Err(UpstreamError::UpstreamRequest(
                "upstream returned empty response".to_string(),
            ));
        };

        let status_code = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_string(), value.to_string()))
            })
            .collect::<Vec<_>>();
        let body = response
            .bytes()
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?
            .to_vec();
        Ok(UpstreamOAuthResponse {
            status_code,
            headers,
            body,
            request_meta,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamRequestMeta {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl UpstreamRequestMeta {
    pub fn from_url(
        method: impl Into<String>,
        url: &str,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Self {
        Self {
            method: method.into(),
            url: url.to_string(),
            headers,
            body,
        }
    }
}

pub fn tracked_request_meta(
    method: impl Into<String>,
    url: &str,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
) -> UpstreamRequestMeta {
    UpstreamRequestMeta::from_url(method, url, headers, body)
}

fn push_tracked_http_event(event: TrackedHttpEvent) {
    let _ = TRACKED_HTTP_EVENT_SINK.try_with(|sink| {
        if let Ok(mut guard) = sink.lock() {
            guard.push(event);
        }
    });
}

fn response_headers_to_pairs(response: &WreqResponse) -> Vec<(String, String)> {
    response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

pub struct TrackedRequestBuilder {
    inner: WreqRequestBuilder,
    method: String,
    url: String,
    request_headers: Vec<(String, String)>,
    request_body: Option<Vec<u8>>,
}

impl TrackedRequestBuilder {
    pub fn header(self, name: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        let mut this = self;
        let name = name.as_ref().to_string();
        let value = value.as_ref().to_string();
        this.inner = this.inner.header(name.as_str(), value.as_str());
        this.request_headers.push((name, value));
        this
    }

    pub fn headers(self, headers: HeaderMap) -> Self {
        let mut this = self;
        for (name, value) in headers.iter() {
            if let Ok(value) = value.to_str() {
                this.request_headers
                    .push((name.as_str().to_string(), value.to_string()));
            }
        }
        this.inner = this.inner.headers(headers);
        this
    }

    pub fn bearer_auth(self, token: impl AsRef<str>) -> Self {
        let mut this = self;
        let token_value = format!("Bearer {}", token.as_ref());
        this.inner = this.inner.bearer_auth(token.as_ref());
        this.request_headers
            .push(("authorization".to_string(), token_value));
        this
    }

    pub fn body(self, body: impl Into<Vec<u8>>) -> Self {
        let mut this = self;
        let body = body.into();
        this.inner = this.inner.body(body.clone());
        this.request_body = Some(body);
        this
    }

    pub async fn send(self) -> Result<WreqResponse, wreq::Error> {
        let request_meta = tracked_request_meta(
            self.method,
            self.url.as_str(),
            self.request_headers,
            self.request_body,
        );
        match self.inner.send().await {
            Ok(response) => {
                let response_status = response.status().as_u16();
                let response_headers = response_headers_to_pairs(&response);
                if response.status().is_client_error() || response.status().is_server_error() {
                    let raw: HttpResponse<wreq::Body> = response.into();
                    let (parts, body) = raw.into_parts();
                    match body.collect().await {
                        Ok(collected) => {
                            let response_body = collected.to_bytes().to_vec();
                            push_tracked_http_event(TrackedHttpEvent {
                                request_meta,
                                response_status: Some(response_status),
                                response_headers,
                                response_body: Some(response_body.clone()),
                            });
                            Ok(WreqResponse::from(HttpResponse::from_parts(
                                parts,
                                response_body,
                            )))
                        }
                        Err(err) => {
                            push_tracked_http_event(TrackedHttpEvent {
                                request_meta,
                                response_status: Some(response_status),
                                response_headers,
                                response_body: None,
                            });
                            Err(err)
                        }
                    }
                } else {
                    push_tracked_http_event(TrackedHttpEvent {
                        request_meta,
                        response_status: Some(response_status),
                        response_headers,
                        response_body: None,
                    });
                    Ok(response)
                }
            }
            Err(err) => {
                let response_status = err.status().map(|value| value.as_u16());
                push_tracked_http_event(TrackedHttpEvent {
                    request_meta,
                    response_status,
                    response_headers: Vec::new(),
                    response_body: None,
                });
                Err(err)
            }
        }
    }
}

pub fn tracked_request(
    client: &WreqClient,
    method: WreqMethod,
    url: &str,
) -> TrackedRequestBuilder {
    TrackedRequestBuilder {
        inner: client.request(method.clone(), url),
        method: method.as_str().to_string(),
        url: url.to_string(),
        request_headers: Vec::new(),
        request_body: None,
    }
}

pub async fn capture_tracked_http_events<T>(
    fut: impl Future<Output = T>,
) -> (T, Vec<TrackedHttpEvent>) {
    let sink = Arc::new(Mutex::new(Vec::new()));
    let output = TRACKED_HTTP_EVENT_SINK.scope(sink.clone(), fut).await;
    let events = sink
        .lock()
        .ok()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or_default();
    (output, events)
}

pub async fn tracked_send_request(
    client: &WreqClient,
    method: WreqMethod,
    url: &str,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
) -> Result<(WreqResponse, UpstreamRequestMeta), wreq::Error> {
    let method_name = method.as_str().to_string();
    let mut req = tracked_request(client, method, url);
    for (name, value) in &headers {
        req = req.header(name.as_str(), value.as_str());
    }
    if let Some(body) = body.as_ref() {
        req = req.body(body.clone());
    }
    let response = req.send().await?;
    Ok((
        response,
        tracked_request_meta(method_name, url, headers, body),
    ))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamOAuthRequest {
    pub query: Option<String>,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamOAuthResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_meta: Option<UpstreamRequestMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamOAuthCredential {
    pub label: Option<String>,
    pub credential: ChannelCredential,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamOAuthCallbackResult {
    pub response: UpstreamOAuthResponse,
    pub credential: Option<UpstreamOAuthCredential>,
}

impl UpstreamOAuthCallbackResult {
    pub fn into_enveloped_response(self) -> UpstreamOAuthResponse {
        let upstream = serde_json::from_slice::<serde_json::Value>(&self.response.body)
            .unwrap_or_else(|_| {
                serde_json::Value::String(String::from_utf8_lossy(&self.response.body).to_string())
            });
        let body = serde_json::to_vec(&json!({
            "upstream": upstream,
            "credential": self.credential,
        }))
        .unwrap_or_default();

        let mut headers = self.response.headers;
        headers.retain(|(name, _)| !name.eq_ignore_ascii_case("content-type"));
        headers.push(("content-type".to_string(), "application/json".to_string()));

        UpstreamOAuthResponse {
            status_code: self.response.status_code,
            headers,
            body,
            request_meta: self.response.request_meta,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    #[error("provider {channel} has no eligible credential for model={model:?}")]
    NoEligibleCredential {
        channel: String,
        model: Option<String>,
    },
    #[error(
        "all eligible credentials exhausted for channel={channel}, attempts={attempts}, last_status={last_status:?}, last_error={last_error:?}"
    )]
    AllCredentialsExhausted {
        channel: String,
        attempts: usize,
        last_credential_id: Option<i64>,
        last_status: Option<u16>,
        last_error: Option<String>,
        last_request_meta: Option<Box<UpstreamRequestMeta>>,
    },
    #[error("unsupported request for upstream execution")]
    UnsupportedRequest,
    #[error("invalid provider base_url")]
    InvalidBaseUrl,
    #[error("upstream request failed: {0}")]
    UpstreamRequest(String),
    #[error("serialize request failed: {0}")]
    SerializeRequest(String),
}

impl UpstreamError {
    pub const fn http_status_code(&self) -> u16 {
        match self {
            Self::NoEligibleCredential { .. } => 409,
            Self::AllCredentialsExhausted { .. } => 503,
            Self::UnsupportedRequest => 501,
            Self::InvalidBaseUrl | Self::SerializeRequest(_) => 500,
            Self::UpstreamRequest(_) => 502,
        }
    }
}
