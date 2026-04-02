use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};
use crate::utils::claude_cache_control as cache_control;
use crate::utils::oauth2_refresh;

/// Claude Code channel (Anthropic Messages API with OAuth).
pub struct ClaudeCodeChannel;

const DEFAULT_CLAUDECODE_VERSION: &str = "2.1.89";
const DEFAULT_CLAUDECODE_ENTRYPOINT: &str = "cli";
const DEFAULT_CLAUDECODE_USER_TYPE: &str = "external";
const BILLING_HASH_SALT: &str = "59cf53e54c78";
const BILLING_CCH_HEX_LEN: usize = 5;
const BILLING_VERSION_HASH_LEN: usize = 3;
const BILLING_VERSION_CHAR_OFFSETS: [usize; 3] = [4, 7, 20];
const CLAUDECODE_SESSION_NAMESPACE: uuid::Uuid =
    uuid::uuid!("f348ca5a-091f-5e75-aec7-c6d7c1b8c3d6");

// ---------------------------------------------------------------------------
// Default-value helpers
// ---------------------------------------------------------------------------

fn default_claudecode_base_url() -> String {
    "https://api.anthropic.com".to_string()
}

fn default_claudecode_device_id() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("device_id entropy should be available");
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for ClaudeCodeSettings {
    fn default() -> Self {
        Self {
            base_url: default_claudecode_base_url(),
            user_agent: None,
            max_retries_on_429: None,
            enable_magic_cache: false,
            cache_breakpoints: Vec::new(),
        }
    }
}

impl ChannelSettings for ClaudeCodeSettings {
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

// ---------------------------------------------------------------------------
// Credential
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeCredential {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at_ms: u64,
    #[serde(default = "default_claudecode_device_id")]
    pub device_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
}

impl Default for ClaudeCodeCredential {
    fn default() -> Self {
        Self {
            access_token: String::new(),
            refresh_token: String::new(),
            expires_at_ms: 0,
            device_id: default_claudecode_device_id(),
            account_uuid: None,
            subscription_type: None,
            rate_limit_tier: None,
            cookie: None,
            user_email: None,
        }
    }
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
            if let Some(account_uuid) = update.get("account_uuid").and_then(|v| v.as_str()) {
                self.account_uuid = Some(account_uuid.to_string());
            }
            if let Some(device_id) = update.get("device_id").and_then(|v| v.as_str()) {
                self.device_id = device_id.to_string();
            }
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Body-mutation helpers
// ---------------------------------------------------------------------------

/// Build the `metadata.user_id` JSON string that Claude Code sends.
fn build_metadata_user_id(credential: &ClaudeCodeCredential, session_id: &str) -> String {
    // The value is itself a JSON-encoded string
    serde_json::json!({
        "device_id": credential.device_id.as_str(),
        "account_uuid": credential.account_uuid.as_deref().unwrap_or(""),
        "session_id": session_id,
    })
    .to_string()
}

/// Build the billing attribution text injected as the first system element.
fn build_attribution(user_message: &str) -> String {
    let cch = truncated_sha256_hex(user_message, BILLING_CCH_HEX_LEN);
    let version_hash_input = format!(
        "{}{}{}",
        DEFAULT_CLAUDECODE_VERSION,
        BILLING_HASH_SALT,
        sampled_message_chars(user_message)
    );
    let version_hash = truncated_sha256_hex(&version_hash_input, BILLING_VERSION_HASH_LEN);

    format!(
        "x-anthropic-billing-header: cc_version={}.{}; cc_entrypoint={}; cch={};",
        DEFAULT_CLAUDECODE_VERSION, version_hash, DEFAULT_CLAUDECODE_ENTRYPOINT, cch
    )
}

fn request_session_id(request: &PreparedRequest, body: &Value) -> String {
    if let Some(session_id) = request
        .headers
        .get("x-claude-code-session-id")
        .or_else(|| request.headers.get("session_id"))
        .or_else(|| request.headers.get("x-client-request-id"))
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
    {
        return session_id.to_owned();
    }

    let session_seed = format!(
        "{}\n{}\n{}",
        system_fingerprint_text(body),
        first_message_fingerprint_text(body),
        request.path
    );
    Uuid::new_v5(&CLAUDECODE_SESSION_NAMESPACE, session_seed.as_bytes()).to_string()
}

fn truncated_sha256_hex(input: &str, hex_len: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let hash = hasher.finalize();
    let mut hex = String::with_capacity(hash.len() * 2);
    for byte in hash {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex.chars().take(hex_len).collect()
}

fn sampled_message_chars(user_message: &str) -> String {
    let chars: Vec<char> = user_message.chars().collect();
    BILLING_VERSION_CHAR_OFFSETS
        .iter()
        .map(|index| chars.get(*index).copied().unwrap_or('0'))
        .collect()
}

fn system_fingerprint_text(body: &Value) -> String {
    match body.get("system") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(text_from_content_block)
            .collect::<Vec<_>>()
            .join(""),
        Some(value) => serde_json::to_string(value).unwrap_or_default(),
        None => String::new(),
    }
}

fn text_from_content_block(block: &Value) -> Option<String> {
    if let Some(text) = block.as_str() {
        return Some(text.to_owned());
    }

    let block_type = block.get("type").and_then(Value::as_str)?;
    if block_type != "text" {
        return None;
    }

    block
        .get("text")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn first_message_fingerprint_text(body: &Value) -> String {
    let Some(message) = body
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| messages.first())
    else {
        return String::new();
    };

    serde_json::to_string(message).unwrap_or_default()
}

fn first_user_message_text(body: &Value) -> String {
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return String::new();
    };

    let Some(message) = messages.iter().find(|message| {
        message
            .get("role")
            .and_then(Value::as_str)
            .is_some_and(|role| role == "user")
    }) else {
        return String::new();
    };

    let Some(content) = message.get("content") else {
        return String::new();
    };

    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(text_from_content_block)
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// Inject `metadata.user_id` into the body JSON.
fn inject_metadata_user_id(body: &mut Value, user_id_value: &str) {
    let metadata = body
        .as_object_mut()
        .expect("body must be an object")
        .entry("metadata")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(m) = metadata.as_object_mut() {
        m.insert(
            "user_id".to_string(),
            Value::String(user_id_value.to_string()),
        );
    }
}

/// Inject the billing attribution as the first element of the `system` array.
///
/// - If `system` is absent, create it as an array with the attribution text block.
/// - If `system` is a string, convert to an array: [attribution, original_text].
/// - If `system` is already an array, prepend the attribution text block.
fn inject_system_attribution(body: &mut Value, attribution: &str) {
    let attribution_block = serde_json::json!({
        "type": "text",
        "text": attribution,
    });

    let obj = body.as_object_mut().expect("body must be an object");

    match obj.get("system") {
        None => {
            obj.insert("system".to_string(), Value::Array(vec![attribution_block]));
        }
        Some(val) if val.is_string() => {
            let original_text = val.as_str().unwrap().to_string();
            let original_block = serde_json::json!({
                "type": "text",
                "text": original_text,
            });
            obj.insert(
                "system".to_string(),
                Value::Array(vec![attribution_block, original_block]),
            );
        }
        Some(val) if val.is_array() => {
            let arr = obj.get_mut("system").unwrap().as_array_mut().unwrap();
            arr.insert(0, attribution_block);
        }
        _ => {
            // system is some other type – overwrite with array
            obj.insert("system".to_string(), Value::Array(vec![attribution_block]));
        }
    }
}

// ---------------------------------------------------------------------------
// Channel implementation
// ---------------------------------------------------------------------------

impl Channel for ClaudeCodeChannel {
    const ID: &'static str = "claudecode";
    type Settings = ClaudeCodeSettings;
    type Credential = ClaudeCodeCredential;
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
            xform("gemini_live", "gemini", "stream_generate_content", "claude"),
            xform(
                "openai_response_websocket",
                "openai",
                "stream_generate_content",
                "claude",
            ),
            xform("compact", "openai", "generate_content", "claude"),
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
        // -- 1. Parse and mutate the body --------------------------------
        let (body, session_id) = {
            let mut body_json: Value = serde_json::from_slice(&request.body)
                .map_err(|e| UpstreamError::RequestBuild(e.to_string()))?;
            let session_id = request_session_id(request, &body_json);

            // Cache control transforms
            if settings.enable_magic_cache {
                cache_control::apply_magic_string_cache_control_triggers(&mut body_json);
            }
            if !settings.cache_breakpoints.is_empty() {
                cache_control::ensure_cache_breakpoint_rules(
                    &mut body_json,
                    &settings.cache_breakpoints,
                );
            }

            // Inject metadata.user_id
            let user_id_value = build_metadata_user_id(credential, &session_id);
            inject_metadata_user_id(&mut body_json, &user_id_value);

            // Inject billing attribution into system
            let attribution = build_attribution(&first_user_message_text(&body_json));
            inject_system_attribution(&mut body_json, &attribution);

            (
                serde_json::to_vec(&body_json)
                    .map_err(|e| UpstreamError::RequestBuild(e.to_string()))?,
                session_id,
            )
        };

        // -- 2. Build the User-Agent ------------------------------------
        let user_agent = match settings.user_agent() {
            Some(ua) => ua.to_string(),
            None => format!(
                "claude-cli/{} ({}, {})",
                DEFAULT_CLAUDECODE_VERSION,
                DEFAULT_CLAUDECODE_USER_TYPE,
                DEFAULT_CLAUDECODE_ENTRYPOINT
            ),
        };

        // -- 3. Fresh client request ID per request ---------------------
        let client_request_id = Uuid::now_v7().to_string();

        // -- 4. Assemble the HTTP request -------------------------------
        let url = format!("{}{}", settings.base_url(), request.path);
        let mut builder = http::Request::builder()
            .method(request.method.clone())
            .uri(&url)
            .header(
                "Authorization",
                format!("Bearer {}", credential.access_token),
            )
            .header("anthropic-version", "2023-06-01")
            .header("x-app", "cli")
            .header("User-Agent", &user_agent)
            .header("X-Claude-Code-Session-Id", &session_id)
            .header("x-client-request-id", &client_request_id)
            .header("Content-Type", "application/json");

        // Forward any additional headers from the prepared request
        for (key, value) in request.headers.iter() {
            builder = builder.header(key, value);
        }

        builder
            .body(body)
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
            )
            .await?;
            credential.access_token = result.access_token;
            credential.expires_at_ms = result.expires_at_ms;
            if let Some(rt) = result.refresh_token {
                credential.refresh_token = rt;
            }
            Ok(true)
        }
    }
}

fn claudecode_dispatch_table() -> DispatchTable {
    ClaudeCodeChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(ClaudeCodeChannel::ID, claudecode_dispatch_table) }
