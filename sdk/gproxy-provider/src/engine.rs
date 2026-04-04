use std::pin::Pin;
use std::sync::Arc;

use async_stream::try_stream;
use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::Instrument;

use crate::health::ModelCooldownHealth;
use crate::request::PreparedRequest;
use crate::response::UpstreamError;
use crate::store::{CredentialUpdate, ProviderStore, ProviderStoreBuilder};

fn is_stream_aggregation_route(
    src_operation: &str,
    dst_operation: &str,
    src_protocol: &str,
    dst_protocol: &str,
) -> bool {
    src_operation == "generate_content"
        && dst_operation == "stream_generate_content"
        && src_protocol == dst_protocol
}

fn aggregate_stream_body(protocol: &str, body: &[u8]) -> Result<Vec<u8>, UpstreamError> {
    let ndjson = match protocol {
        "openai_response" | "openai_chat_completions" | "claude" => {
            gproxy_protocol::stream::sse_to_ndjson_stream(&String::from_utf8_lossy(body))
        }
        "gemini" | "gemini_ndjson" => String::from_utf8_lossy(body).into_owned(),
        _ => {
            return Err(UpstreamError::Channel(format!(
                "no stream aggregation for protocol: {protocol}"
            )));
        }
    };

    let owned_chunks: Vec<Vec<u8>> = ndjson
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.as_bytes().to_vec())
        .collect();
    let chunk_refs: Vec<&[u8]> = owned_chunks.iter().map(Vec::as_slice).collect();
    crate::transform_dispatch::stream_to_nonstream(protocol, &chunk_refs)
}

/// Execution request passed to the engine.
pub struct ExecuteRequest {
    pub provider: String,
    pub operation: String,
    pub protocol: String,
    pub body: Vec<u8>,
    pub headers: http::HeaderMap,
    pub model: Option<String>,
}

/// Result of an engine execution.
pub struct ExecuteResult {
    pub status: u16,
    pub headers: http::HeaderMap,
    pub body: ExecuteBody,
    pub usage: Option<Usage>,
    pub meta: Option<UpstreamRequestMeta>,
    pub credential_updates: Vec<CredentialUpdate>,
}

pub type ExecuteBodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, UpstreamError>> + Send>>;

pub enum ExecuteBody {
    Full(Vec<u8>),
    Stream(ExecuteBodyStream),
}

/// Token usage extracted from upstream response.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub cache_creation_input_tokens_5min: Option<i64>,
    pub cache_creation_input_tokens_1h: Option<i64>,
}

/// Metadata about the upstream request for logging/storage.
#[derive(Debug, Clone)]
pub struct UpstreamRequestMeta {
    pub method: String,
    pub url: String,
    pub request_headers: Vec<(String, String)>,
    pub request_body: Option<Vec<u8>>,
    pub response_status: Option<u16>,
    pub response_headers: Vec<(String, String)>,
    pub model: Option<String>,
    pub latency_ms: u64,
    pub credential_index: Option<usize>,
}

/// The main SDK entry point. Consumes the current provider store snapshot and an HTTP client.
pub struct GproxyEngine {
    store: Arc<ProviderStore>,
    client: wreq::Client,
    spoof_client: Option<wreq::Client>,
    pub enable_usage: bool,
    pub enable_upstream_log: bool,
    pub enable_upstream_log_body: bool,
}

/// Serialized provider configuration for building an engine from JSON/DB data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub channel: String,
    pub settings_json: serde_json::Value,
    pub credentials: Vec<serde_json::Value>,
}

pub struct GproxyEngineBuilder {
    store: Option<Arc<ProviderStore>>,
    store_builder: ProviderStoreBuilder,
    client: Option<wreq::Client>,
    spoof_client: Option<wreq::Client>,
    enable_usage: bool,
    enable_upstream_log: bool,
    enable_upstream_log_body: bool,
}

impl GproxyEngineBuilder {
    pub fn new() -> Self {
        Self {
            store: None,
            store_builder: ProviderStoreBuilder::new(),
            client: None,
            spoof_client: None,
            enable_usage: true,
            enable_upstream_log: true,
            enable_upstream_log_body: true,
        }
    }

    pub fn provider_store(mut self, store: Arc<ProviderStore>) -> Self {
        self.store = Some(store);
        self
    }

    pub fn add_provider<C: crate::Channel>(
        mut self,
        name: impl Into<String>,
        channel: C,
        settings: C::Settings,
        credentials: Vec<(C::Credential, C::Health)>,
    ) -> Self {
        self.store_builder = self
            .store_builder
            .add_provider(name, channel, settings, credentials);
        self
    }

    /// Set the HTTP client.
    pub fn http_client(mut self, client: wreq::Client) -> Self {
        self.client = Some(client);
        self
    }

    /// Set the spoof HTTP client (browser-impersonating TLS fingerprint).
    pub fn spoof_client(mut self, client: wreq::Client) -> Self {
        self.spoof_client = Some(client);
        self
    }

    /// Build HTTP clients from proxy and impersonate config.
    ///
    /// Constructs both the normal client (with optional proxy) and the
    /// spoof client (with browser TLS impersonation + optional proxy).
    pub fn configure_clients(self, proxy: Option<&str>, emulation: Option<&str>) -> Self {
        let mut client_builder = wreq::Client::builder();
        if let Some(proxy_url) = proxy
            && let Ok(p) = wreq::Proxy::all(proxy_url)
        {
            client_builder = client_builder.proxy(p);
        }
        let client = client_builder.build().unwrap_or_default();

        let emu = parse_emulation(emulation.unwrap_or("chrome_136"));
        let mut spoof_builder = wreq::Client::builder().emulation(emu);
        if let Some(proxy_url) = proxy
            && let Ok(p) = wreq::Proxy::all(proxy_url)
        {
            spoof_builder = spoof_builder.proxy(p);
        }
        let spoof = spoof_builder.build().unwrap_or_default();

        self.http_client(client).spoof_client(spoof)
    }

    /// Control whether usage is extracted from responses (default: true).
    pub fn enable_usage(mut self, enabled: bool) -> Self {
        self.enable_usage = enabled;
        self
    }

    /// Control whether upstream request metadata is collected (default: true).
    pub fn enable_upstream_log(mut self, enabled: bool) -> Self {
        self.enable_upstream_log = enabled;
        self
    }

    /// Control whether upstream log includes request/response body (default: true).
    pub fn enable_upstream_log_body(mut self, enabled: bool) -> Self {
        self.enable_upstream_log_body = enabled;
        self
    }

    pub fn build(self) -> GproxyEngine {
        GproxyEngine {
            store: self
                .store
                .unwrap_or_else(|| Arc::new(self.store_builder.build())),
            client: self.client.unwrap_or_default(),
            spoof_client: self.spoof_client,
            enable_usage: self.enable_usage,
            enable_upstream_log: self.enable_upstream_log,
            enable_upstream_log_body: self.enable_upstream_log_body,
        }
    }

    /// Add a provider from serialized JSON config.
    ///
    /// Dispatches by `channel` string to the concrete channel type.
    /// Returns an error if the channel ID is unknown or JSON is invalid.
    pub fn add_provider_json(self, config: ProviderConfig) -> Result<Self, UpstreamError> {
        macro_rules! add {
            ($self:expr, $ch:expr, $cfg:expr) => {{
                let settings = serde_json::from_value($cfg.settings_json).map_err(|e| {
                    UpstreamError::Channel(format!("invalid settings for '{}': {e}", $cfg.name))
                })?;
                let creds: Vec<_> = $cfg
                    .credentials
                    .into_iter()
                    .filter_map(|c| {
                        serde_json::from_value(c)
                            .ok()
                            .map(|c| (c, ModelCooldownHealth::default()))
                    })
                    .collect();
                Ok($self.add_provider(&$cfg.name, $ch, settings, creds))
            }};
        }

        use crate::channels::*;

        match config.channel.as_str() {
            "openai" => add!(self, openai::OpenAiChannel, config),
            "anthropic" => add!(self, anthropic::AnthropicChannel, config),
            "claudecode" => add!(self, claudecode::ClaudeCodeChannel, config),
            "codex" => add!(self, codex::CodexChannel, config),
            "vertex" => add!(self, vertex::VertexChannel, config),
            "vertexexpress" => add!(self, vertexexpress::VertexExpressChannel, config),
            "aistudio" => add!(self, aistudio::AiStudioChannel, config),
            "geminicli" => add!(self, geminicli::GeminiCliChannel, config),
            "antigravity" => add!(self, antigravity::AntigravityChannel, config),
            "nvidia" => add!(self, nvidia::NvidiaChannel, config),
            "deepseek" => add!(self, deepseek::DeepSeekChannel, config),
            "groq" => add!(self, groq::GroqChannel, config),
            "openrouter" => add!(self, openrouter::OpenRouterChannel, config),
            "custom" => add!(self, custom::CustomChannel, config),
            _ => Err(UpstreamError::Channel(format!(
                "unknown channel: {}",
                config.channel
            ))),
        }
    }
}

impl Default for GproxyEngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GproxyEngine {
    pub fn builder() -> GproxyEngineBuilder {
        GproxyEngineBuilder::new()
    }

    pub fn store(&self) -> &Arc<ProviderStore> {
        &self.store
    }

    /// Create a new engine with different HTTP clients but the same provider store.
    ///
    /// Use this when proxy or spoof settings change without needing to
    /// rebuild providers/credentials.
    pub fn with_new_clients(&self, proxy: Option<&str>, emulation: Option<&str>) -> GproxyEngine {
        let builder = GproxyEngineBuilder::new()
            .provider_store(self.store.clone())
            .configure_clients(proxy, emulation)
            .enable_usage(self.enable_usage)
            .enable_upstream_log(self.enable_upstream_log)
            .enable_upstream_log_body(self.enable_upstream_log_body);
        builder.build()
    }

    /// Rebuild engine with new settings but same provider store.
    ///
    /// Use when global config changes (proxy, spoof, usage/log flags).
    pub fn with_settings(
        &self,
        proxy: Option<&str>,
        emulation: Option<&str>,
        enable_usage: bool,
        enable_upstream_log: bool,
        enable_upstream_log_body: bool,
    ) -> GproxyEngine {
        let builder = GproxyEngineBuilder::new()
            .provider_store(self.store.clone())
            .configure_clients(proxy, emulation)
            .enable_usage(enable_usage)
            .enable_upstream_log(enable_upstream_log)
            .enable_upstream_log_body(enable_upstream_log_body);
        builder.build()
    }

    /// Connect to an upstream WebSocket endpoint for a provider.
    ///
    /// Returns `Connected` for passthrough (same protocol), `NeedsProtocolBridge`
    /// when the dispatch table maps to a different WS operation, or an error
    /// (e.g. 426, no WS support) that the caller can use to fall back to HTTP.
    pub async fn connect_upstream_ws(
        &self,
        provider_name: &str,
        operation: &str,
        protocol: &str,
        path: &str,
        model: Option<&str>,
    ) -> Result<WsConnectionResult, UpstreamError> {
        let span =
            tracing::info_span!("engine.connect_upstream_ws", provider = provider_name, path);
        async {
            let provider = self.store.get_runtime(provider_name).ok_or_else(|| {
                UpstreamError::Channel(format!("unknown provider: {provider_name}"))
            })?;

            // Check dispatch table to determine WS routing strategy
            let route_key = crate::dispatch::RouteKey::new(operation, protocol);
            let (ws_path, ws_model, src_protocol, dst_protocol) =
                match provider.dispatch_table().resolve(&route_key) {
                    Some(crate::dispatch::RouteImplementation::Passthrough) => {
                        (path.to_string(), model, protocol.to_string(), protocol.to_string())
                    }
                    Some(crate::dispatch::RouteImplementation::TransformTo { destination }) => {
                        // Check if destination is also a WS operation
                        let dst_op = &destination.operation;
                        let dst_proto = &destination.protocol;
                        let (target_path, target_model) = ws_path_for_operation(dst_op, dst_proto, model);
                        match target_path {
                            Some(p) => (p, target_model, protocol.to_string(), dst_proto.clone()),
                            None => {
                                return Err(UpstreamError::Channel(
                                    "upstream does not support native WebSocket for this operation; use HTTP fallback".into(),
                                ));
                            }
                        }
                    }
                    _ => {
                        return Err(UpstreamError::Channel(
                            "upstream does not support native WebSocket for this operation; use HTTP fallback".into(),
                        ));
                    }
                };

            // Get auth candidates for all credentials
            let auth_candidates = provider.prepare_ws_auth(&ws_path, ws_model)?;

            let mut last_error = None;
            for (idx, (auth_url, auth_headers)) in auth_candidates.into_iter().enumerate() {
                // Convert URL scheme to wss/ws
                let ws_url = auth_url
                    .replace("https://", "wss://")
                    .replace("http://", "ws://");

                // Append model query param if not already in the URL
                let ws_url = if let Some(m) = ws_model
                    && !ws_url.contains("model=")
                {
                    let sep = if ws_url.contains('?') { "&" } else { "?" };
                    format!("{ws_url}{sep}model={m}")
                } else {
                    ws_url
                };

                tracing::info!(url = %ws_url, credential = idx, "connecting upstream websocket");

                // Build WS request with channel-specific auth headers
                let mut ws_builder = wreq::websocket(&ws_url);
                for (name, value) in auth_headers.iter() {
                    if name != http::header::CONTENT_TYPE {
                        ws_builder =
                            ws_builder.header(name.as_str(), value.to_str().unwrap_or(""));
                    }
                }

                let response = match ws_builder.send().await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(credential = idx, error = %e, "ws handshake failed, trying next credential");
                        last_error = Some(UpstreamError::Http(format!("ws handshake failed: {e}")));
                        continue;
                    }
                };

                let status = response.status().as_u16();
                if status == 426 {
                    return Err(UpstreamError::Channel(
                        "upstream requires HTTP (426 Upgrade Required)".into(),
                    ));
                }
                if status == 401 || status == 403 {
                    tracing::warn!(credential = idx, status, "ws auth rejected, trying next credential");
                    last_error = Some(UpstreamError::Channel(format!(
                        "ws auth rejected (HTTP {status})"
                    )));
                    continue;
                }

                let ws = match response.into_websocket().await {
                    Ok(ws) => ws,
                    Err(e) => {
                        tracing::warn!(credential = idx, error = %e, "ws upgrade failed, trying next credential");
                        last_error =
                            Some(UpstreamError::Http(format!("ws upgrade failed: {e}")));
                        continue;
                    }
                };

                tracing::info!(credential = idx, "upstream websocket connected");
                let upstream = UpstreamWebSocket { inner: ws };
                let meta = WsUpstreamMeta {
                    url: ws_url,
                    request_headers: auth_headers
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect(),
                    response_status: status,
                    credential_index: idx,
                };

                return if src_protocol == dst_protocol {
                    Ok(WsConnectionResult::Connected(upstream, meta))
                } else {
                    Ok(WsConnectionResult::NeedsProtocolBridge {
                        upstream,
                        src_protocol,
                        dst_protocol,
                        meta,
                    })
                };
            }

            Err(last_error.unwrap_or(UpstreamError::AllCredentialsExhausted))
        }
        .instrument(span)
        .await
    }

    /// Start an OAuth flow for a provider.
    pub async fn oauth_start(
        &self,
        provider_name: &str,
        params: std::collections::HashMap<String, String>,
    ) -> Result<Option<crate::channel::OAuthFlow>, UpstreamError> {
        let span = tracing::info_span!("engine.oauth_start", provider = provider_name);
        async {
            self.store
                .oauth_start(provider_name, &self.client, params)
                .await
        }
        .instrument(span)
        .await
    }

    /// Finish an OAuth flow (exchange code for tokens).
    pub async fn oauth_finish(
        &self,
        provider_name: &str,
        params: std::collections::HashMap<String, String>,
    ) -> Result<Option<crate::store::OAuthFinishResult>, UpstreamError> {
        let span = tracing::info_span!("engine.oauth_finish", provider = provider_name);
        async {
            self.store
                .oauth_finish(provider_name, &self.client, params)
                .await
        }
        .instrument(span)
        .await
    }

    /// Query upstream quota/usage for a provider credential.
    pub async fn query_quota(
        &self,
        provider_name: &str,
    ) -> Result<Option<crate::response::UpstreamResponse>, UpstreamError> {
        let span = tracing::info_span!("engine.query_quota", provider = provider_name);
        async {
            let provider = self.store.get_runtime(provider_name).ok_or_else(|| {
                UpstreamError::Channel(format!("unknown provider: {provider_name}"))
            })?;
            let Some(http_request) = provider.prepare_quota_request()? else {
                return Ok(None);
            };
            let response = crate::http_client::send_request(&self.client, http_request).await?;
            Ok(Some(response))
        }
        .instrument(span)
        .await
    }

    /// Execute a request against a named provider.
    pub async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteResult, UpstreamError> {
        let span = tracing::info_span!(
            "engine.execute",
            provider = %request.provider,
            operation = %request.operation,
            protocol = %request.protocol,
            model = request.model.as_deref().unwrap_or(""),
        );
        if request.operation.starts_with("stream_") {
            self.execute_stream_inner(request).instrument(span).await
        } else {
            self.execute_inner(request).instrument(span).await
        }
    }

    async fn execute_inner(&self, request: ExecuteRequest) -> Result<ExecuteResult, UpstreamError> {
        let provider = self.store.get_runtime(&request.provider).ok_or_else(|| {
            tracing::warn!(provider = %request.provider, "unknown provider");
            UpstreamError::Channel(format!("unknown provider: {}", request.provider))
        })?;

        let start = std::time::Instant::now();

        // Dispatch table lookup
        let src_key = crate::dispatch::RouteKey::new(&request.operation, &request.protocol);
        let route = provider
            .dispatch_table()
            .resolve(&src_key)
            .ok_or_else(|| {
                tracing::warn!(operation = %request.operation, protocol = %request.protocol, "route not found");
                UpstreamError::Channel(format!(
                    "unsupported route: ({}, {})",
                    request.operation, request.protocol
                ))
            })?
            .clone();

        let (dst_op, dst_proto, needs_transform) = match &route {
            crate::dispatch::RouteImplementation::Passthrough => {
                (request.operation.clone(), request.protocol.clone(), false)
            }
            crate::dispatch::RouteImplementation::TransformTo { destination } => (
                destination.operation.clone(),
                destination.protocol.clone(),
                true,
            ),
            crate::dispatch::RouteImplementation::Local => {
                let body = provider
                    .handle_local(&request.operation, &request.protocol, &request.body)
                    .unwrap_or_else(|| {
                        Err(UpstreamError::Channel("local route not implemented".into()))
                    })?;
                return Ok(ExecuteResult {
                    status: 200,
                    headers: http::HeaderMap::new(),
                    body: ExecuteBody::Full(body),
                    usage: None,
                    meta: None,
                    credential_updates: Vec::new(),
                });
            }
            crate::dispatch::RouteImplementation::Unsupported => {
                return Err(UpstreamError::Channel(format!(
                    "unsupported: ({}, {})",
                    request.operation, request.protocol
                )));
            }
        };

        let force_stream_aggregation =
            is_stream_aggregation_route(&request.operation, &dst_op, &request.protocol, &dst_proto);

        // Transform request if needed
        let body = if needs_transform {
            tracing::debug!(dst_op = %dst_op, dst_proto = %dst_proto, "transforming request");
            crate::transform_dispatch::transform_request(
                &request.operation,
                &request.protocol,
                if force_stream_aggregation {
                    &request.operation
                } else {
                    &dst_op
                },
                &dst_proto,
                request.body,
            )?
        } else {
            request.body
        };

        let method = operation_http_method(&dst_op);
        let mut body = body;
        let path = build_operation_path(&dst_op, &mut body);

        let prepared = PreparedRequest {
            method,
            path,
            model: request.model.clone(),
            body,
            headers: request.headers,
        };
        let prepared = provider.finalize_request(prepared)?;
        let affinity_hint = crate::affinity::cache_affinity_hint_for_request(&dst_proto, &prepared);

        let provider_result = provider
            .execute(
                prepared.clone(),
                affinity_hint,
                &self.client,
                self.spoof_client.as_ref(),
            )
            .await?;
        let response = provider_result.response;
        let credential_updates = provider_result.credential_updates;

        // 1. Normalize upstream response (channel-specific fixups)
        let normalized_body = provider.normalize_response(&prepared, response.body);
        let response_transform_dst_op = if force_stream_aggregation {
            request.operation.as_str()
        } else {
            dst_op.as_str()
        };
        let normalized_nonstream_body =
            if force_stream_aggregation && (200..=299).contains(&response.status) {
                aggregate_stream_body(&dst_proto, &normalized_body)?
            } else {
                normalized_body
            };

        // 2. Extract usage from normalized upstream body (before protocol transform)
        let usage = if self.enable_usage {
            crate::usage::extract_usage(&dst_proto, &normalized_nonstream_body)
        } else {
            None
        };

        // 3. Transform response if needed (cross-protocol)
        let response_body = if needs_transform {
            tracing::debug!("transforming response");
            crate::transform_dispatch::transform_response(
                &request.operation,
                &request.protocol,
                response_transform_dst_op,
                &dst_proto,
                normalized_nonstream_body,
            )?
        } else {
            normalized_nonstream_body
        };

        let latency_ms = start.elapsed().as_millis() as u64;

        let meta = if self.enable_upstream_log {
            Some(UpstreamRequestMeta {
                method: "POST".to_string(),
                url: String::new(),
                request_headers: Vec::new(),
                request_body: None,
                response_status: Some(response.status),
                response_headers: response
                    .headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect(),
                model: request.model,
                latency_ms,
                credential_index: None,
            })
        } else {
            None
        };

        Ok(ExecuteResult {
            status: response.status,
            headers: response.headers,
            body: ExecuteBody::Full(response_body),
            usage,
            meta,
            credential_updates,
        })
    }

    async fn execute_stream_inner(
        &self,
        request: ExecuteRequest,
    ) -> Result<ExecuteResult, UpstreamError> {
        let provider = self.store.get_runtime(&request.provider).ok_or_else(|| {
            tracing::warn!(provider = %request.provider, "unknown provider");
            UpstreamError::Channel(format!("unknown provider: {}", request.provider))
        })?;

        let start = std::time::Instant::now();

        let src_key = crate::dispatch::RouteKey::new(&request.operation, &request.protocol);
        let route = provider
            .dispatch_table()
            .resolve(&src_key)
            .ok_or_else(|| {
                tracing::warn!(operation = %request.operation, protocol = %request.protocol, "route not found");
                UpstreamError::Channel(format!(
                    "unsupported route: ({}, {})",
                    request.operation, request.protocol
                ))
            })?
            .clone();

        let (dst_op, dst_proto, needs_transform) = match &route {
            crate::dispatch::RouteImplementation::Passthrough => {
                (request.operation.clone(), request.protocol.clone(), false)
            }
            crate::dispatch::RouteImplementation::TransformTo { destination } => (
                destination.operation.clone(),
                destination.protocol.clone(),
                true,
            ),
            crate::dispatch::RouteImplementation::Local => {
                let body = provider
                    .handle_local(&request.operation, &request.protocol, &request.body)
                    .unwrap_or_else(|| {
                        Err(UpstreamError::Channel("local route not implemented".into()))
                    })?;
                return Ok(ExecuteResult {
                    status: 200,
                    headers: http::HeaderMap::new(),
                    body: ExecuteBody::Full(body),
                    usage: None,
                    meta: None,
                    credential_updates: Vec::new(),
                });
            }
            crate::dispatch::RouteImplementation::Unsupported => {
                return Err(UpstreamError::Channel(format!(
                    "unsupported: ({}, {})",
                    request.operation, request.protocol
                )));
            }
        };

        let body = if needs_transform {
            crate::transform_dispatch::transform_request(
                &request.operation,
                &request.protocol,
                &dst_op,
                &dst_proto,
                request.body,
            )?
        } else {
            request.body
        };

        let method = operation_http_method(&dst_op);
        let mut body = body;
        let path = build_operation_path(&dst_op, &mut body);

        let prepared = PreparedRequest {
            method,
            path,
            model: request.model.clone(),
            body,
            headers: request.headers,
        };
        let prepared = provider.finalize_request(prepared)?;
        let affinity_hint = crate::affinity::cache_affinity_hint_for_request(&dst_proto, &prepared);

        let provider_result = provider
            .execute_stream(
                prepared.clone(),
                affinity_hint,
                &self.client,
                self.spoof_client.as_ref(),
            )
            .await?;
        let response = provider_result.response;
        let credential_updates = provider_result.credential_updates;

        let meta = if self.enable_upstream_log {
            Some(UpstreamRequestMeta {
                method: "POST".to_string(),
                url: String::new(),
                request_headers: Vec::new(),
                request_body: None,
                response_status: Some(response.status),
                response_headers: response
                    .headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect(),
                model: request.model.clone(),
                latency_ms: start.elapsed().as_millis() as u64,
                credential_index: None,
            })
        } else {
            None
        };

        let body = if needs_transform {
            let transformer = crate::transform_dispatch::create_stream_response_transformer(
                &request.operation,
                &request.protocol,
                &dst_op,
                &dst_proto,
                Some({
                    let store = self.store.clone();
                    let provider_name = request.provider.clone();
                    let prepared = prepared.clone();
                    Arc::new(move |body: Vec<u8>| {
                        store
                            .get_runtime(&provider_name)
                            .map(|runtime| runtime.normalize_response(&prepared, body.clone()))
                            .unwrap_or(body)
                    })
                }),
            )?;

            let mut upstream = response.body;
            let stream = try_stream! {
                let mut transformer = transformer;
                while let Some(chunk) = upstream.next().await {
                    let chunk = chunk?;
                    let out = transformer.push_chunk(&chunk)?;
                    if !out.is_empty() {
                        yield Bytes::from(out);
                    }
                }

                let tail = transformer.finish()?;
                if !tail.is_empty() {
                    yield Bytes::from(tail);
                }
            };
            ExecuteBody::Stream(Box::pin(stream))
        } else {
            ExecuteBody::Stream(response.body)
        };

        Ok(ExecuteResult {
            status: response.status,
            headers: response.headers,
            body,
            usage: None,
            meta,
            credential_updates,
        })
    }
}

fn parse_emulation(name: &str) -> wreq_util::Emulation {
    match name {
        "chrome_136" => wreq_util::Emulation::Chrome136,
        "chrome_135" => wreq_util::Emulation::Chrome135,
        "chrome_134" => wreq_util::Emulation::Chrome134,
        "chrome_133" => wreq_util::Emulation::Chrome133,
        "chrome_132" => wreq_util::Emulation::Chrome132,
        "chrome_131" => wreq_util::Emulation::Chrome131,
        "chrome_127" => wreq_util::Emulation::Chrome127,
        "safari_18" => wreq_util::Emulation::Safari18,
        "safari_18.2" => wreq_util::Emulation::Safari18_2,
        "safari_18.3" => wreq_util::Emulation::Safari18_3,
        "safari_18.5" => wreq_util::Emulation::Safari18_5,
        _ => wreq_util::Emulation::Chrome136,
    }
}

/// Metadata about the upstream WebSocket connection for logging.
#[derive(Debug, Clone)]
pub struct WsUpstreamMeta {
    /// The upstream WebSocket URL connected to.
    pub url: String,
    /// Request headers sent during the handshake.
    pub request_headers: Vec<(String, String)>,
    /// HTTP status code from the handshake response.
    pub response_status: u16,
    /// Index of the credential used.
    pub credential_index: usize,
}

/// Result of a WebSocket connection attempt.
pub enum WsConnectionResult {
    /// Direct passthrough — same protocol upstream and downstream.
    Connected(UpstreamWebSocket, WsUpstreamMeta),
    /// Cross-protocol bridge needed — upstream uses a different WS protocol.
    NeedsProtocolBridge {
        upstream: UpstreamWebSocket,
        src_protocol: String,
        dst_protocol: String,
        meta: WsUpstreamMeta,
    },
}

/// Determine HTTP method and base path for a given operation.
///
/// For most operations the engine historically used `POST /{op}`.
/// File and model endpoints require specific methods and real API paths.
/// Returns `(method, path)` where `path` may still need dynamic segments
/// (file_id, model_id, query params) appended by `build_operation_path`.
fn operation_http_method(operation: &str) -> http::Method {
    match operation {
        "file_list" | "file_download" | "file_get" | "model_list" | "model_get" => {
            http::Method::GET
        }
        "file_delete" => http::Method::DELETE,
        _ => http::Method::POST,
    }
}

/// Build the full upstream path for an operation, extracting path/query
/// parameters from the body JSON when needed.
///
/// For file operations the body carries a protocol-style JSON descriptor
/// with `path` and `query` sub-objects.  We extract what we need and
/// return the cleaned body (empty for GET/DELETE, original for POST).
fn build_operation_path(operation: &str, body: &mut Vec<u8>) -> String {
    match operation {
        "file_upload" => "/v1/files".to_string(),
        "file_list" => {
            let path = build_file_list_path(body);
            *body = Vec::new(); // GET has no body
            path
        }
        "file_download" => {
            let file_id = extract_path_param(body, "file_id");
            *body = Vec::new();
            format!("/v1/files/{}/content", file_id)
        }
        "file_get" => {
            let file_id = extract_path_param(body, "file_id");
            *body = Vec::new();
            format!("/v1/files/{}", file_id)
        }
        "file_delete" => {
            let file_id = extract_path_param(body, "file_id");
            *body = Vec::new();
            format!("/v1/files/{}", file_id)
        }
        _ => format!("/{}", operation),
    }
}

/// Extract a path parameter from a JSON body like `{"path":{"file_id":"..."}}`
fn extract_path_param(body: &[u8], param: &str) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer(&format!("/path/{}", param))
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_default()
}

/// Build `/v1/files?after_id=...&before_id=...&limit=...` from the body JSON.
fn build_file_list_path(body: &[u8]) -> String {
    let mut base = "/v1/files".to_string();
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return base;
    };
    let query = v.get("query");
    let mut params = Vec::new();
    if let Some(q) = query {
        if let Some(s) = q.get("after_id").and_then(|v| v.as_str()) {
            params.push(format!("after_id={}", s));
        }
        if let Some(s) = q.get("before_id").and_then(|v| v.as_str()) {
            params.push(format!("before_id={}", s));
        }
        if let Some(n) = q.get("limit").and_then(|v| v.as_u64()) {
            params.push(format!("limit={}", n));
        }
    }
    if !params.is_empty() {
        base.push('?');
        base.push_str(&params.join("&"));
    }
    base
}

/// Returns true when the operation is one of the Files API endpoints.
pub fn is_file_operation(operation: &str) -> bool {
    matches!(
        operation,
        "file_upload" | "file_list" | "file_download" | "file_get" | "file_delete"
    )
}

/// Returns true when the prepared request path belongs to a Files API endpoint.
pub fn is_file_operation_path(path: &str) -> bool {
    path.starts_with("/v1/files")
}

/// Determine the WS path for a given destination operation.
/// Returns `None` if the destination is not a WS-capable operation.
fn ws_path_for_operation<'a>(
    operation: &str,
    _protocol: &str,
    model: Option<&'a str>,
) -> (Option<String>, Option<&'a str>) {
    match operation {
        "openai_response_websocket" => (Some("/v1/responses".to_string()), model),
        "gemini_live" => {
            let model_name = model.unwrap_or("unknown");
            let path = format!("/v1beta/models/{model_name}:streamGenerateContent");
            (Some(path), None) // model is in the path, not query
        }
        _ => (None, model),
    }
}

/// Wrapper around a wreq WebSocket connection to an upstream provider.
pub struct UpstreamWebSocket {
    inner: wreq::ws::WebSocket,
}

impl UpstreamWebSocket {
    /// Get a mutable reference to the inner wreq WebSocket.
    /// Use `futures_util::StreamExt` and `futures_util::SinkExt` for
    /// send/recv, or call `recv()` / `send()` directly.
    pub fn into_inner(self) -> wreq::ws::WebSocket {
        self.inner
    }

    /// Receive a message from the upstream WebSocket.
    pub async fn recv(&mut self) -> Option<Result<WsMessage, UpstreamError>> {
        self.inner
            .recv()
            .await
            .map(|r| r.map_err(|e| UpstreamError::Http(e.to_string())))
    }

    /// Send a message to the upstream WebSocket.
    pub async fn send(&mut self, msg: WsMessage) -> Result<(), UpstreamError> {
        self.inner
            .send(msg)
            .await
            .map_err(|e| UpstreamError::Http(e.to_string()))
    }
}

/// Re-export wreq WS message type.
pub use wreq::ws::message::Message as WsMessage;
