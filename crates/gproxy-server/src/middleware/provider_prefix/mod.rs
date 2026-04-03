mod capture;

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower::{Layer, Service};

use crate::middleware::classify::ClassifiedRequest;
use crate::middleware::error::MiddlewareError;

pub use capture::{add_provider_prefix, split_provider_prefixed_model};

/// A classified request enriched with the extracted provider prefix.
pub struct ProviderScopedRequest {
    pub request: ClassifiedRequest,
    pub provider: Option<String>,
}

/// Extract the provider prefix from a classified request's body, stripping it
/// from the model field.  Returns `ProviderScopedRequest` with the provider
/// set to `None` when the model has no prefix (single-provider setup).
pub fn extract_provider_from_classified(
    mut req: ClassifiedRequest,
) -> Result<ProviderScopedRequest, MiddlewareError> {
    let provider = capture::strip_provider_prefix_from_classified(&mut req)?;
    Ok(ProviderScopedRequest {
        request: req,
        provider,
    })
}

// ---------------------------------------------------------------------------
// Tower Layer / Service — request side
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestProviderExtractLayer;

impl RequestProviderExtractLayer {
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestProviderExtractLayer {
    type Service = RequestProviderExtractService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestProviderExtractService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct RequestProviderExtractService<S> {
    inner: S,
}

#[derive(Debug)]
pub enum RequestProviderExtractServiceError<E> {
    Extract(MiddlewareError),
    Inner(E),
}

impl<E: Display> Display for RequestProviderExtractServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Extract(err) => Display::fmt(err, f),
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for RequestProviderExtractServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<ClassifiedRequest> for RequestProviderExtractService<S>
where
    S: Service<ProviderScopedRequest> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = RequestProviderExtractServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(RequestProviderExtractServiceError::Inner)
    }

    fn call(&mut self, request: ClassifiedRequest) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let scoped = extract_provider_from_classified(request)
                .map_err(RequestProviderExtractServiceError::Extract)?;
            inner
                .call(scoped)
                .await
                .map_err(RequestProviderExtractServiceError::Inner)
        })
    }
}
