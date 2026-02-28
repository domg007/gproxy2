use gproxy_middleware::TransformResponse;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wreq::Response as WreqResponse;
use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::ChannelCredential;

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

pub async fn tracked_send_request(
    client: &WreqClient,
    method: WreqMethod,
    url: &str,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
) -> Result<(WreqResponse, UpstreamRequestMeta), wreq::Error> {
    let method_name = method.as_str().to_string();
    let mut req = client.request(method, url);
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
