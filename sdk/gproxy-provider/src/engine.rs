use std::sync::Arc;

use tracing::Instrument;

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
    pub body: Vec<u8>,
    pub usage: Option<Usage>,
    pub meta: Option<UpstreamRequestMeta>,
    pub credential_updates: Vec<CredentialUpdate>,
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
    pub enable_usage: bool,
    pub enable_upstream_log: bool,
}

pub struct GproxyEngineBuilder {
    store: Option<Arc<ProviderStore>>,
    store_builder: ProviderStoreBuilder,
    client: Option<wreq::Client>,
    enable_usage: bool,
    enable_upstream_log: bool,
}

impl GproxyEngineBuilder {
    pub fn new() -> Self {
        Self {
            store: None,
            store_builder: ProviderStoreBuilder::new(),
            client: None,
            enable_usage: true,
            enable_upstream_log: true,
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

    pub fn build(self) -> GproxyEngine {
        GproxyEngine {
            store: self
                .store
                .unwrap_or_else(|| Arc::new(self.store_builder.build())),
            client: self.client.unwrap_or_default(),
            enable_usage: self.enable_usage,
            enable_upstream_log: self.enable_upstream_log,
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

    /// Execute a request against a named provider.
    pub async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteResult, UpstreamError> {
        let span = tracing::info_span!(
            "engine.execute",
            provider = %request.provider,
            operation = %request.operation,
            protocol = %request.protocol,
            model = request.model.as_deref().unwrap_or(""),
        );
        self.execute_inner(request).instrument(span).await
    }

    async fn execute_inner(&self, request: ExecuteRequest) -> Result<ExecuteResult, UpstreamError> {        let provider = self.store.get_runtime(&request.provider).ok_or_else(|| {
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
                    body,
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

        let prepared = PreparedRequest {
            method: http::Method::POST,
            path: format!("/{}", dst_op),
            model: request.model.clone(),
            body,
            headers: request.headers,
        };
        let prepared = provider.finalize_request(prepared)?;
        let affinity_hint = crate::affinity::cache_affinity_hint_for_request(&dst_proto, &prepared);

        let provider_result = provider
            .execute(prepared.clone(), affinity_hint, &self.client)
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
            body: response_body,
            usage,
            meta,
            credential_updates,
        })
    }
}
