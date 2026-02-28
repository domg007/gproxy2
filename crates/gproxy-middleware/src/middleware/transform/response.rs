use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower::{Layer, Service};

use super::engine::transform_response_payload;
use super::error::MiddlewareTransformError;
use super::message::{TransformRequestPayload, TransformResponsePayload, TransformRoute};

#[derive(Debug, Clone, Copy)]
pub struct ResponseTransformLayer {
    route: TransformRoute,
}

impl ResponseTransformLayer {
    pub const fn new(route: TransformRoute) -> Self {
        Self { route }
    }
}

impl<S> Layer<S> for ResponseTransformLayer {
    type Service = ResponseTransformService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseTransformService {
            inner,
            route: self.route,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResponseTransformService<S> {
    inner: S,
    route: TransformRoute,
}

#[derive(Debug)]
pub enum ResponseTransformServiceError<E> {
    Transform(MiddlewareTransformError),
    Inner(E),
}

impl<E: Display> Display for ResponseTransformServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transform(err) => Display::fmt(err, f),
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for ResponseTransformServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<TransformRequestPayload> for ResponseTransformService<S>
where
    S: Service<TransformRequestPayload, Response = TransformResponsePayload> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = TransformResponsePayload;
    type Error = ResponseTransformServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(ResponseTransformServiceError::Inner)
    }

    fn call(&mut self, request: TransformRequestPayload) -> Self::Future {
        let route = self.route;
        let fut = self.inner.call(request);
        Box::pin(async move {
            let response = fut.await.map_err(ResponseTransformServiceError::Inner)?;
            transform_response_payload(response, route)
                .await
                .map_err(ResponseTransformServiceError::Transform)
        })
    }
}
