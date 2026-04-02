use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::channel::{Channel, ChannelSettings};
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::request::PreparedRequest;
use crate::response::{UpstreamError, UpstreamResponse};
use crate::retry::retry_with_credentials_max;

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

// === Type-erased provider ===

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

trait AnyProvider: Send + Sync {
    fn dispatch_table(&self) -> &DispatchTable;

    fn handle_local(
        &self,
        operation: &str,
        protocol: &str,
        body: &[u8],
    ) -> Option<Result<Vec<u8>, UpstreamError>>;

    fn normalize_response(&self, body: Vec<u8>) -> Vec<u8>;

    fn execute<'a>(
        &'a self,
        request: PreparedRequest,
        client: &'a wreq::Client,
    ) -> BoxFuture<'a, Result<UpstreamResponse, UpstreamError>>;
}

struct ProviderInstance<C: Channel> {
    channel: C,
    settings: C::Settings,
    credentials: std::sync::Mutex<Vec<(C::Credential, C::Health)>>,
    dispatch_table: DispatchTable,
}

impl<C: Channel> AnyProvider for ProviderInstance<C> {
    fn dispatch_table(&self) -> &DispatchTable {
        &self.dispatch_table
    }

    fn handle_local(
        &self,
        operation: &str,
        protocol: &str,
        body: &[u8],
    ) -> Option<Result<Vec<u8>, UpstreamError>> {
        self.channel.handle_local(operation, protocol, body)
    }

    fn normalize_response(&self, body: Vec<u8>) -> Vec<u8> {
        self.channel.normalize_response(body)
    }

    fn execute<'a>(
        &'a self,
        request: PreparedRequest,
        client: &'a wreq::Client,
    ) -> BoxFuture<'a, Result<UpstreamResponse, UpstreamError>> {
        Box::pin(async move {
            let max_retries = self.settings.max_retries_on_429();
            let mut creds = self.credentials.lock().unwrap().clone();

            let result = retry_with_credentials_max(
                &self.channel,
                &mut creds,
                &self.settings,
                &request,
                max_retries,
                client,
                |req| crate::http_client::send_request(client, req),
            )
            .await;

            // Write back credential and health state changes
            {
                let mut guard = self.credentials.lock().unwrap();
                for (i, (cred, health)) in creds.into_iter().enumerate() {
                    if let Some((stored_cred, stored_health)) = guard.get_mut(i) {
                        *stored_cred = cred;
                        *stored_health = health;
                    }
                }
            }

            result
        })
    }
}

// === Engine ===

/// The main SDK entry point. Holds provider instances and an HTTP client.
pub struct GproxyEngine {
    providers: HashMap<String, Arc<dyn AnyProvider>>,
    client: wreq::Client,
    pub enable_usage: bool,
    pub enable_upstream_log: bool,
}

pub struct GproxyEngineBuilder {
    providers: HashMap<String, Arc<dyn AnyProvider>>,
    client: Option<wreq::Client>,
    enable_usage: bool,
    enable_upstream_log: bool,
}

impl GproxyEngineBuilder {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            client: None,
            enable_usage: true,
            enable_upstream_log: true,
        }
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

    /// Add a provider instance.
    pub fn add_provider<C: Channel>(
        mut self,
        name: impl Into<String>,
        channel: C,
        settings: C::Settings,
        credentials: Vec<(C::Credential, C::Health)>,
    ) -> Self {
        let instance = Arc::new(ProviderInstance {
            dispatch_table: channel.dispatch_table(),
            channel,
            settings,
            credentials: std::sync::Mutex::new(credentials),
        });
        self.providers.insert(name.into(), instance);
        self
    }

    pub fn build(self) -> GproxyEngine {
        GproxyEngine {
            providers: self.providers,
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

    /// Execute a request against a named provider.
    pub async fn execute(&self, request: ExecuteRequest) -> Result<ExecuteResult, UpstreamError> {
        let provider = self.providers.get(&request.provider).ok_or_else(|| {
            UpstreamError::Channel(format!("unknown provider: {}", request.provider))
        })?;

        let start = std::time::Instant::now();

        // Dispatch table lookup
        let src_key = RouteKey::new(&request.operation, &request.protocol);
        let route = provider
            .dispatch_table()
            .resolve(&src_key)
            .ok_or_else(|| {
                UpstreamError::Channel(format!(
                    "unsupported route: ({}, {})",
                    request.operation, request.protocol
                ))
            })?
            .clone();

        let (dst_op, dst_proto, needs_transform) = match &route {
            RouteImplementation::Passthrough => {
                (request.operation.clone(), request.protocol.clone(), false)
            }
            RouteImplementation::TransformTo { destination } => (
                destination.operation.clone(),
                destination.protocol.clone(),
                true,
            ),
            RouteImplementation::Local => {
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
                });
            }
            RouteImplementation::Unsupported => {
                return Err(UpstreamError::Channel(format!(
                    "unsupported: ({}, {})",
                    request.operation, request.protocol
                )));
            }
        };

        // Transform request if needed
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

        let prepared = PreparedRequest {
            method: http::Method::POST,
            path: format!("/{}", dst_op),
            model: request.model.clone(),
            body,
            headers: request.headers,
        };

        let response = provider.execute(prepared, &self.client).await?;

        // 1. Normalize upstream response (channel-specific fixups)
        let normalized_body = provider.normalize_response(response.body);

        // 2. Extract usage from normalized upstream body (before protocol transform)
        let usage = if self.enable_usage {
            crate::usage::extract_usage(&dst_proto, &normalized_body)
        } else {
            None
        };

        // 3. Transform response if needed (cross-protocol)
        let response_body = if needs_transform {
            crate::transform_dispatch::transform_response(
                &request.operation,
                &request.protocol,
                &dst_op,
                &dst_proto,
                normalized_body,
            )?
        } else {
            normalized_body
        };

        let latency_ms = start.elapsed().as_millis() as u64;

        let meta = if self.enable_upstream_log {
            Some(UpstreamRequestMeta {
                method: "POST".to_string(),
                url: String::new(), // TODO: fill from prepared request
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
        })
    }
}
