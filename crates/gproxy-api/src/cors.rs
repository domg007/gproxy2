use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use http::{HeaderValue, Method, Request, Response, StatusCode, header};
use tower::{Layer, Service};

/// CORS layer that allows all origins (`*`) by default, or a configured list.
///
/// Handles preflight `OPTIONS` requests and injects `Access-Control-*` headers
/// on every response.
#[derive(Debug, Clone)]
pub struct CorsLayer {
    allow_origin: AllowOrigin,
}

#[derive(Debug, Clone)]
enum AllowOrigin {
    Any,
    List(Vec<HeaderValue>),
}

impl CorsLayer {
    /// Allow all origins (`Access-Control-Allow-Origin: *`).
    pub fn permissive() -> Self {
        Self {
            allow_origin: AllowOrigin::Any,
        }
    }

    /// Allow only the given origins.
    pub fn with_origins(origins: Vec<String>) -> Self {
        let values = origins
            .into_iter()
            .filter_map(|o| HeaderValue::from_str(&o).ok())
            .collect();
        Self {
            allow_origin: AllowOrigin::List(values),
        }
    }
}

impl<S> Layer<S> for CorsLayer {
    type Service = CorsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CorsService {
            inner,
            allow_origin: self.allow_origin.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CorsService<S> {
    inner: S,
    allow_origin: AllowOrigin,
}

impl<S> Service<Request<Body>> for CorsService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let origin = req.headers().get(header::ORIGIN).cloned();
        let is_preflight = req.method() == Method::OPTIONS;
        let allow_origin = self.allow_origin.clone();

        // For preflight, return immediately without calling inner service.
        if is_preflight {
            return Box::pin(async move {
                let mut resp = Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .body(Body::empty())
                    .unwrap();
                inject_cors_headers(resp.headers_mut(), &allow_origin, origin.as_ref());
                Ok(resp)
            });
        }

        let mut inner = self.inner.clone();
        Box::pin(async move {
            let mut resp = inner.call(req).await?;
            inject_cors_headers(resp.headers_mut(), &allow_origin, origin.as_ref());
            Ok(resp)
        })
    }
}

fn inject_cors_headers(
    headers: &mut http::HeaderMap,
    allow_origin: &AllowOrigin,
    request_origin: Option<&HeaderValue>,
) {
    // Access-Control-Allow-Origin
    match allow_origin {
        AllowOrigin::Any => {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
        }
        AllowOrigin::List(origins) => {
            if let Some(req_origin) = request_origin
                && origins.iter().any(|o| o == req_origin)
            {
                headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, req_origin.clone());
                // Vary so caches key on Origin
                headers.append(header::VARY, HeaderValue::from_static("Origin"));
            }
        }
    }

    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("*"),
    );
}
