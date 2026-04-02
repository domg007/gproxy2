use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::utils::claude_cache_control as cache_control;
use crate::utils::oauth2_refresh;
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};

/// Claude Code channel (Anthropic Messages API with OAuth).
pub struct ClaudeCodeChannel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeCodeSettings {
    #[serde(default = "default_claudecode_base_url")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,
    #[serde(default)]
    pub enable_magic_cache: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache_breakpoints: Vec<cache_control::CacheBreakpointRule>,
}

fn default_claudecode_base_url() -> String {
    "https://api.anthropic.com".to_string()
}

impl ChannelSettings for ClaudeCodeSettings {
    fn base_url(&self) -> &str { &self.base_url }
    fn user_agent(&self) -> Option<&str> { self.user_agent.as_deref() }
    fn max_retries_on_429(&self) -> u32 { self.max_retries_on_429.unwrap_or(3) }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeCodeCredential {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
}

impl ChannelCredential for ClaudeCodeCredential {
    fn apply_update(&mut self, update: &serde_json::Value) -> bool {
        if let Some(token) = update.get("access_token").and_then(|v| v.as_str()) {
            self.access_token = token.to_string();
            if let Some(exp) = update.get("expires_at_ms").and_then(|v| v.as_u64()) {
                self.expires_at_ms = exp;
            }
            if let Some(rt) = update.get("refresh_token").and_then(|v| v.as_str()) {
                self.refresh_token = rt.to_string();
            }
            true
        } else {
            false
        }
    }
}

impl Channel for ClaudeCodeChannel {
    const ID: &'static str = "claudecode";
    type Settings = ClaudeCodeSettings;
    type Credential = ClaudeCodeCredential;
    type Health = ModelCooldownHealth;

    fn dispatch_table(&self) -> DispatchTable {
        // Same dispatch table as anthropic — native protocol is "claude"
        let mut t = DispatchTable::new();
        let pass = |op: &str, proto: &str| {
            (RouteKey::new(op, proto), RouteImplementation::Passthrough)
        };
        let xform = |op: &str, proto: &str, dst_op: &str, dst_proto: &str| {
            (RouteKey::new(op, proto), RouteImplementation::TransformTo {
                destination: RouteKey::new(dst_op, dst_proto),
            })
        };

        let routes = vec![
            pass("model_list", "claude"),
            xform("model_list", "openai", "model_list", "claude"),
            xform("model_list", "gemini", "model_list", "claude"),
            pass("model_get", "claude"),
            xform("model_get", "openai", "model_get", "claude"),
            xform("model_get", "gemini", "model_get", "claude"),
            pass("count_tokens", "claude"),
            xform("count_tokens", "openai", "count_tokens", "claude"),
            xform("count_tokens", "gemini", "count_tokens", "claude"),
            pass("generate_content", "claude"),
            xform("generate_content", "openai_chat_completions", "generate_content", "claude"),
            xform("generate_content", "openai_response", "generate_content", "claude"),
            xform("generate_content", "gemini", "generate_content", "claude"),
            pass("stream_generate_content", "claude"),
            xform("stream_generate_content", "openai_chat_completions", "stream_generate_content", "claude"),
            xform("stream_generate_content", "openai_response", "stream_generate_content", "claude"),
            xform("stream_generate_content", "gemini", "stream_generate_content", "claude"),
            xform("stream_generate_content", "gemini_ndjson", "stream_generate_content", "claude"),
            xform("gemini_live", "gemini", "stream_generate_content", "claude"),
            xform("openai_response_websocket", "openai", "stream_generate_content", "claude"),
            xform("compact", "openai", "generate_content", "claude"),
        ];

        for (key, imp) in routes { t.set(key, imp); }
        t
    }

    fn prepare_request(
        &self,
        credential: &Self::Credential,
        settings: &Self::Settings,
        request: &PreparedRequest,
    ) -> Result<http::Request<Vec<u8>>, UpstreamError> {
        let body = if settings.enable_magic_cache || !settings.cache_breakpoints.is_empty() {
            let mut body_json: Value = serde_json::from_slice(&request.body)
                .map_err(|e| UpstreamError::RequestBuild(e.to_string()))?;
            if settings.enable_magic_cache {
                cache_control::apply_magic_string_cache_control_triggers(&mut body_json);
            }
            if !settings.cache_breakpoints.is_empty() {
                cache_control::ensure_cache_breakpoint_rules(&mut body_json, &settings.cache_breakpoints);
            }
            serde_json::to_vec(&body_json)
                .map_err(|e| UpstreamError::RequestBuild(e.to_string()))?
        } else {
            request.body.clone()
        };

        let url = format!("{}{}", settings.base_url(), request.path);
        let mut builder = http::Request::builder()
            .method(request.method.clone())
            .uri(&url)
            .header("Authorization", format!("Bearer {}", credential.access_token))
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("x-app", "cli")
            .header("Content-Type", "application/json");

        if let Some(ua) = settings.user_agent() {
            builder = builder.header("User-Agent", ua);
        }

        for (key, value) in request.headers.iter() {
            builder = builder.header(key, value);
        }

        builder.body(body).map_err(|e| UpstreamError::RequestBuild(e.to_string()))
    }

    fn classify_response(
        &self, status: u16, headers: &http::HeaderMap, _body: &[u8],
    ) -> ResponseClassification {
        match status {
            200..=299 => ResponseClassification::Success,
            401 | 403 => ResponseClassification::AuthDead,
            429 => {
                let retry_after = headers
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|secs| secs * 1000);
                ResponseClassification::RateLimited { retry_after_ms: retry_after }
            }
            529 => ResponseClassification::TransientError,
            500..=599 => ResponseClassification::TransientError,
            _ => ResponseClassification::PermanentError,
        }
    }

    fn count_strategy(&self) -> CountStrategy {
        CountStrategy::UpstreamApi
    }

    fn refresh_credential<'a>(
        &'a self,
        client: &'a wreq::Client,
        credential: &'a mut Self::Credential,
    ) -> impl std::future::Future<Output = Result<bool, UpstreamError>> + Send + 'a {
        let client = client.clone();
        async move {
            if credential.refresh_token.is_empty() {
                return Ok(false);
            }
            let result = oauth2_refresh::refresh_oauth2_token(
                &client,
                "https://console.anthropic.com/v1/oauth/token",
                "",
                "",
                &credential.refresh_token,
            ).await?;
            credential.access_token = result.access_token;
            credential.expires_at_ms = result.expires_at_ms;
            if let Some(rt) = result.refresh_token {
                credential.refresh_token = rt;
            }
            Ok(true)
        }
    }
}

fn claudecode_dispatch_table() -> DispatchTable { ClaudeCodeChannel.dispatch_table() }
inventory::submit! { ChannelRegistration::new(ClaudeCodeChannel::ID, claudecode_dispatch_table) }
