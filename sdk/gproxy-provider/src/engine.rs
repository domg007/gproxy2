use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::channel::{Channel, ChannelSettings};
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
    pub meta: RequestMeta,
}

/// Token usage extracted from upstream response.
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
}

/// Metadata about the upstream request for logging.
#[derive(Debug, Clone)]
pub struct RequestMeta {
    pub model: Option<String>,
    pub latency_ms: u64,
    pub attempts: u32,
}

// === Type-erased provider ===

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

trait AnyProvider: Send + Sync {
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
}

impl<C: Channel> AnyProvider for ProviderInstance<C> {
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
                |req| crate::http_client::send_request(client, req),
            )
            .await;

            // Write back health state changes
            {
                let mut guard = self.credentials.lock().unwrap();
                for (i, (_, health)) in creds.into_iter().enumerate() {
                    if let Some((_, stored_health)) = guard.get_mut(i) {
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
}

pub struct GproxyEngineBuilder {
    providers: HashMap<String, Arc<dyn AnyProvider>>,
    client: Option<wreq::Client>,
}

impl GproxyEngineBuilder {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            client: None,
        }
    }

    /// Set the HTTP client.
    pub fn http_client(mut self, client: wreq::Client) -> Self {
        self.client = Some(client);
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
        let provider = self
            .providers
            .get(&request.provider)
            .ok_or_else(|| UpstreamError::Channel(format!("unknown provider: {}", request.provider)))?;

        let start = std::time::Instant::now();

        // TODO: dispatch table lookup + transform
        // For now, pass through directly

        let prepared = PreparedRequest {
            method: http::Method::POST,
            path: format!("/{}", request.operation),
            model: request.model.clone(),
            body: request.body,
            headers: request.headers,
        };

        let response = provider.execute(prepared, &self.client).await?;

        let latency_ms = start.elapsed().as_millis() as u64;

        Ok(ExecuteResult {
            status: response.status,
            headers: response.headers,
            body: response.body,
            usage: None, // TODO: channel.extract_usage()
            meta: RequestMeta {
                model: request.model,
                latency_ms,
                attempts: 1, // TODO: track from retry
            },
        })
    }
}
