use super::*;

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
    Extract(MiddlewareTransformError),
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

impl<S> Service<TransformRequestPayload> for RequestProviderExtractService<S>
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

    fn call(&mut self, request: TransformRequestPayload) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let extracted = extract_provider_from_request_payload(request)
                .await
                .map_err(RequestProviderExtractServiceError::Extract)?;
            inner
                .call(extracted)
                .await
                .map_err(RequestProviderExtractServiceError::Inner)
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResponseProviderPrefixLayer {
    default_provider: String,
}

impl ResponseProviderPrefixLayer {
    pub fn new(default_provider: impl Into<String>) -> Self {
        Self {
            default_provider: default_provider.into(),
        }
    }
}

impl<S> Layer<S> for ResponseProviderPrefixLayer {
    type Service = ResponseProviderPrefixService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseProviderPrefixService {
            inner,
            default_provider: self.default_provider.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResponseProviderPrefixService<S> {
    inner: S,
    default_provider: String,
}

#[derive(Debug)]
pub enum ResponseProviderPrefixServiceError<E> {
    Prefix(MiddlewareTransformError),
    Inner(E),
}

impl<E: Display> Display for ResponseProviderPrefixServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prefix(err) => Display::fmt(err, f),
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for ResponseProviderPrefixServiceError<E> {}

impl<S> Service<ProviderScopedRequest> for ResponseProviderPrefixService<S>
where
    S: Service<ProviderScopedRequest, Response = TransformResponsePayload> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = TransformResponsePayload;
    type Error = ResponseProviderPrefixServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(ResponseProviderPrefixServiceError::Inner)
    }

    fn call(&mut self, request: ProviderScopedRequest) -> Self::Future {
        let provider = request
            .provider
            .clone()
            .unwrap_or_else(|| self.default_provider.clone());
        let fut = self.inner.call(request);
        Box::pin(async move {
            let response = fut
                .await
                .map_err(ResponseProviderPrefixServiceError::Inner)?;
            add_provider_prefix_to_response_payload(response, &provider)
                .await
                .map_err(ResponseProviderPrefixServiceError::Prefix)
        })
    }
}
