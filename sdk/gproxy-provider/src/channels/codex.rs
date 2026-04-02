use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::utils::oauth2_refresh;
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};

/// Codex CLI channel (OpenAI Responses API with OAuth).
pub struct CodexChannel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexSettings {
    #[serde(default = "default_codex_base_url")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,
}

fn default_codex_base_url() -> String {
    "https://api.openai.com".to_string()
}

impl ChannelSettings for CodexSettings {
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
pub struct CodexCredential {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default)]
    pub expires_at_ms: u64,
}

impl ChannelCredential for CodexCredential {
    fn apply_update(&mut self, update: &serde_json::Value) -> bool {
        if let Some(token) = update.get("access_token").and_then(|v| v.as_str()) {
            self.access_token = token.to_string();
            if let Some(exp) = update.get("expires_at_ms").and_then(|v| v.as_u64()) {
                self.expires_at_ms = exp;
            }
            if let Some(rt) = update.get("refresh_token").and_then(|v| v.as_str()) {
                self.refresh_token = Some(rt.to_string());
            }
            true
        } else {
            false
        }
    }
}

impl Channel for CodexChannel {
    const ID: &'static str = "codex";
    type Settings = CodexSettings;
    type Credential = CodexCredential;
    type Health = ModelCooldownHealth;

    fn dispatch_table(&self) -> DispatchTable {
        // Same as openai — native protocol is openai_response / openai_chat_completions
        let mut t = DispatchTable::new();
        let pass = |op: &str, proto: &str| {
            (RouteKey::new(op, proto), RouteImplementation::Passthrough)
        };
        let xform = |op: &str, proto: &str, dst_op: &str, dst_proto: &str| {
            (
                RouteKey::new(op, proto),
                RouteImplementation::TransformTo {
                    destination: RouteKey::new(dst_op, dst_proto),
                },
            )
        };

        let routes: Vec<(RouteKey, RouteImplementation)> = vec![
            // Model list/get
            pass("model_list", "openai"),
            xform("model_list", "claude", "model_list", "openai"),
            xform("model_list", "gemini", "model_list", "openai"),
            pass("model_get", "openai"),
            xform("model_get", "claude", "model_get", "openai"),
            xform("model_get", "gemini", "model_get", "openai"),

            // Count tokens
            pass("count_tokens", "openai"),
            xform("count_tokens", "claude", "count_tokens", "openai"),
            xform("count_tokens", "gemini", "count_tokens", "openai"),

            // Generate content (non-stream)
            pass("generate_content", "openai_response"),
            pass("generate_content", "openai_chat_completions"),
            xform("generate_content", "claude", "generate_content", "openai_response"),
            xform("generate_content", "gemini", "generate_content", "openai_response"),

            // Generate content (stream)
            pass("stream_generate_content", "openai_response"),
            pass("stream_generate_content", "openai_chat_completions"),
            xform("stream_generate_content", "claude", "stream_generate_content", "openai_response"),
            xform("stream_generate_content", "gemini", "stream_generate_content", "openai_response"),
            xform("stream_generate_content", "gemini_ndjson", "stream_generate_content", "openai_response"),

            // WebSocket
            pass("openai_response_websocket", "openai"),
            xform("gemini_live", "gemini", "stream_generate_content", "openai_response"),

            // Images
            pass("create_image", "openai"),
            pass("stream_create_image", "openai"),
            pass("create_image_edit", "openai"),
            pass("stream_create_image_edit", "openai"),

            // Embeddings
            pass("embeddings", "openai"),
            xform("embeddings", "gemini", "embeddings", "openai"),

            // Compact
            pass("compact", "openai"),
        ];

        for (key, imp) in routes {
            t.set(key, imp);
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
            .header("Authorization", format!("Bearer {}", credential.access_token))
            .header("Content-Type", "application/json")
            .header("originator", "codex_cli_rs");

        if let Some(ua) = settings.user_agent() {
            builder = builder.header("User-Agent", ua);
        }

        if let Some(account_id) = &credential.account_id {
            if !account_id.is_empty() {
                builder = builder.header("ChatGPT-Account-ID", account_id.as_str());
            }
        }

        // Forward caller-provided headers (x-codex-turn-state, x-codex-turn-metadata,
        // x-codex-beta-features, x-client-request-id, session_id, etc.)
        for (key, value) in request.headers.iter() {
            builder = builder.header(key, value);
        }

        builder
            .body(request.body.clone())
            .map_err(|e| UpstreamError::RequestBuild(e.to_string()))
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
            let refresh_token = match &credential.refresh_token {
                Some(rt) if !rt.is_empty() => rt.clone(),
                _ => return Ok(false),
            };
            let result = oauth2_refresh::refresh_oauth2_token(
                &client,
                "https://auth.openai.com/oauth/token",
                "",
                "",
                &refresh_token,
            )
            .await?;
            credential.access_token = result.access_token;
            credential.expires_at_ms = result.expires_at_ms;
            if let Some(rt) = result.refresh_token {
                credential.refresh_token = Some(rt);
            }
            Ok(true)
        }
    }
}

fn codex_dispatch_table() -> DispatchTable {
    CodexChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(CodexChannel::ID, codex_dispatch_table) }
