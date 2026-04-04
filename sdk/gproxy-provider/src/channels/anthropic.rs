use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};
use crate::utils::claude_cache_control as cache_control;

/// Anthropic Claude API channel.
pub struct AnthropicChannel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicSettings {
    #[serde(default = "default_anthropic_base_url")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,
    /// Enable magic string -> cache_control conversion (e.g. <|CACHE_5M|> in text)
    #[serde(default)]
    pub enable_magic_cache: bool,
    /// Cache breakpoint rules
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache_breakpoints: Vec<cache_control::CacheBreakpointRule>,
}

fn default_anthropic_base_url() -> String {
    "https://api.anthropic.com".to_string()
}

impl ChannelSettings for AnthropicSettings {
    fn base_url(&self) -> &str {
        &self.base_url
    }
    fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }
    fn max_retries_on_429(&self) -> u32 {
        self.max_retries_on_429.unwrap_or(3)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnthropicCredential {
    pub api_key: String,
}

impl ChannelCredential for AnthropicCredential {}

impl Channel for AnthropicChannel {
    const ID: &'static str = "anthropic";
    type Settings = AnthropicSettings;
    type Credential = AnthropicCredential;
    type Health = ModelCooldownHealth;

    fn dispatch_table(&self) -> DispatchTable {
        let mut t = DispatchTable::new();
        let pass =
            |op: &str, proto: &str| (RouteKey::new(op, proto), RouteImplementation::Passthrough);
        let xform = |op: &str, proto: &str, dst_op: &str, dst_proto: &str| {
            (
                RouteKey::new(op, proto),
                RouteImplementation::TransformTo {
                    destination: RouteKey::new(dst_op, dst_proto),
                },
            )
        };

        let routes = vec![
            // Model list/get
            pass("model_list", "claude"),
            xform("model_list", "openai", "model_list", "claude"),
            xform("model_list", "gemini", "model_list", "claude"),
            pass("model_get", "claude"),
            xform("model_get", "openai", "model_get", "claude"),
            xform("model_get", "gemini", "model_get", "claude"),
            // Count tokens
            pass("count_tokens", "claude"),
            xform("count_tokens", "openai", "count_tokens", "claude"),
            xform("count_tokens", "gemini", "count_tokens", "claude"),
            // Generate content (non-stream)
            pass("generate_content", "claude"),
            xform(
                "generate_content",
                "openai_chat_completions",
                "generate_content",
                "claude",
            ),
            xform(
                "generate_content",
                "openai_response",
                "generate_content",
                "claude",
            ),
            xform("generate_content", "gemini", "generate_content", "claude"),
            // Generate content (stream)
            pass("stream_generate_content", "claude"),
            xform(
                "stream_generate_content",
                "openai_chat_completions",
                "stream_generate_content",
                "claude",
            ),
            xform(
                "stream_generate_content",
                "openai_response",
                "stream_generate_content",
                "claude",
            ),
            xform(
                "stream_generate_content",
                "gemini",
                "stream_generate_content",
                "claude",
            ),
            xform(
                "stream_generate_content",
                "gemini_ndjson",
                "stream_generate_content",
                "claude",
            ),
            // Live API
            xform("gemini_live", "gemini", "stream_generate_content", "claude"),
            // WebSocket → stream
            xform(
                "openai_response_websocket",
                "openai",
                "stream_generate_content",
                "claude",
            ),
            // Compact → generate
            xform("compact", "openai", "generate_content", "claude"),
            // Files API
            pass("file_upload", "claude"),
            pass("file_list", "claude"),
            pass("file_download", "claude"),
            pass("file_get", "claude"),
            pass("file_delete", "claude"),
        ];

        for (key, implementation) in routes {
            t.set(key, implementation);
        }
        t
    }

    fn prepare_request(
        &self,
        credential: &Self::Credential,
        settings: &Self::Settings,
        request: &PreparedRequest,
    ) -> Result<http::Request<Vec<u8>>, UpstreamError> {
        let url = format!("{}{}", settings.base_url(), request.path);
        let mut builder = http::Request::builder()
            .method(request.method.clone())
            .uri(&url)
            .header("x-api-key", &credential.api_key)
            .header("anthropic-version", "2023-06-01");

        // File operations: don't force Content-Type to application/json
        // (multipart upload carries its own Content-Type via request.headers).
        if !crate::engine::is_file_operation_path(&request.path) {
            builder = builder.header("Content-Type", "application/json");
        }

        if let Some(ua) = settings.user_agent() {
            builder = builder.header("User-Agent", ua);
        }

        for (key, value) in request.headers.iter() {
            builder = builder.header(key, value);
        }

        builder
            .body(request.body.clone())
            .map_err(|e| UpstreamError::RequestBuild(e.to_string()))
    }

    fn finalize_request(
        &self,
        settings: &Self::Settings,
        mut request: PreparedRequest,
    ) -> Result<PreparedRequest, UpstreamError> {
        // File operations: inject beta header, skip JSON body normalization.
        if crate::engine::is_file_operation_path(&request.path) {
            request.headers.insert(
                "anthropic-beta",
                http::HeaderValue::from_static("files-api-2025-04-14"),
            );
            return Ok(request);
        }

        if !settings.enable_magic_cache && settings.cache_breakpoints.is_empty() {
            return Ok(request);
        }

        let mut body_json: Value = serde_json::from_slice(&request.body)
            .map_err(|e| UpstreamError::RequestBuild(e.to_string()))?;
        if settings.enable_magic_cache {
            cache_control::apply_magic_string_cache_control_triggers(&mut body_json);
        }
        if !settings.cache_breakpoints.is_empty() {
            cache_control::ensure_cache_breakpoint_rules(
                &mut body_json,
                &settings.cache_breakpoints,
            );
        }
        request.body = serde_json::to_vec(&body_json)
            .map_err(|e| UpstreamError::RequestBuild(e.to_string()))?;
        Ok(request)
    }

    fn classify_response(
        &self,
        status: u16,
        headers: &http::HeaderMap,
        _body: &[u8],
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
                ResponseClassification::RateLimited {
                    retry_after_ms: retry_after,
                }
            }
            529 => ResponseClassification::TransientError,
            500..=599 => ResponseClassification::TransientError,
            _ => ResponseClassification::PermanentError,
        }
    }

    fn count_strategy(&self) -> CountStrategy {
        CountStrategy::UpstreamApi
    }
}

fn anthropic_dispatch_table() -> DispatchTable {
    AnthropicChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(AnthropicChannel::ID, anthropic_dispatch_table) }
