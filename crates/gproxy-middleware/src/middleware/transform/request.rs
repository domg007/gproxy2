use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower::{Layer, Service};

use super::engine::transform_request_payload;
use super::error::MiddlewareTransformError;
use super::message::{TransformRequestPayload, TransformRoute};

#[derive(Debug, Clone, Copy)]
pub struct RequestTransformLayer {
    route: TransformRoute,
}

impl RequestTransformLayer {
    pub const fn new(route: TransformRoute) -> Self {
        Self { route }
    }
}

impl<S> Layer<S> for RequestTransformLayer {
    type Service = RequestTransformService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestTransformService {
            inner,
            route: self.route,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestTransformService<S> {
    inner: S,
    route: TransformRoute,
}

#[derive(Debug)]
pub enum RequestTransformServiceError<E> {
    Transform(MiddlewareTransformError),
    Inner(E),
}

impl<E: Display> Display for RequestTransformServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transform(err) => Display::fmt(err, f),
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for RequestTransformServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<TransformRequestPayload> for RequestTransformService<S>
where
    S: Service<TransformRequestPayload> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = RequestTransformServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(RequestTransformServiceError::Inner)
    }

    fn call(&mut self, request: TransformRequestPayload) -> Self::Future {
        let route = self.route;
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let transformed = transform_request_payload(request, route)
                .await
                .map_err(RequestTransformServiceError::Transform)?;
            inner
                .call(transformed)
                .await
                .map_err(RequestTransformServiceError::Inner)
        })
    }
}
