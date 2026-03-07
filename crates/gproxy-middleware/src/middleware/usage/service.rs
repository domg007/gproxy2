use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub struct ResponseUsageExtractLayer;

impl ResponseUsageExtractLayer {
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for ResponseUsageExtractLayer {
    type Service = ResponseUsageExtractService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseUsageExtractService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct ResponseUsageExtractService<S> {
    inner: S,
}

#[derive(Debug)]
pub enum ResponseUsageExtractServiceError<E> {
    Inner(E),
}

impl<E: Display> Display for ResponseUsageExtractServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for ResponseUsageExtractServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<TransformRequestPayload> for ResponseUsageExtractService<S>
where
    S: Service<TransformRequestPayload, Response = TransformResponsePayload> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = UsageExtractedResponse;
    type Error = ResponseUsageExtractServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(ResponseUsageExtractServiceError::Inner)
    }

    fn call(&mut self, request: TransformRequestPayload) -> Self::Future {
        let fut = self.inner.call(request);
        Box::pin(async move {
            let response = fut.await.map_err(ResponseUsageExtractServiceError::Inner)?;
            Ok(attach_usage_extractor(response))
        })
    }
}
