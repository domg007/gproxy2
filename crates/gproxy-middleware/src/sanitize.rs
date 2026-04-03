use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tower::{Layer, Service};

use crate::classify::ClassifiedRequest;

/// Headers removed by the sanitize middleware.
const AUTH_HEADERS: &[&str] = &["authorization", "x-api-key", "x-goog-api-key"];

/// Query parameters that carry credentials and should be stripped.
const AUTH_QUERY_KEYS: &[&str] = &["key"];

/// Strip authentication headers and sensitive query parameters from a
/// classified request so downstream middleware and business logic never see
/// raw credentials.
pub fn sanitize_request(req: &mut ClassifiedRequest) {
    strip_auth_headers(req);
    strip_auth_query_params(req);
}

fn strip_auth_headers(req: &mut ClassifiedRequest) {
    for name in AUTH_HEADERS {
        req.request.headers_mut().remove(*name);
    }
}

fn strip_auth_query_params(req: &mut ClassifiedRequest) {
    let uri = req.request.uri();
    let Some(query) = uri.query() else {
        return;
    };
    let filtered: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            let param_key = pair.split('=').next().unwrap_or("");
            !AUTH_QUERY_KEYS.contains(&param_key)
        })
        .collect();
    let new_pq = if filtered.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), filtered.join("&"))
    };
    if let Ok(new_uri) = new_pq.parse() {
        *req.request.uri_mut() = new_uri;
    }
}

// ---------------------------------------------------------------------------
// Tower Layer / Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestSanitizeLayer;

impl RequestSanitizeLayer {
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestSanitizeLayer {
    type Service = RequestSanitizeService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestSanitizeService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct RequestSanitizeService<S> {
    inner: S,
}

#[derive(Debug)]
pub enum RequestSanitizeServiceError<E> {
    Inner(E),
}

impl<E: Display> Display for RequestSanitizeServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for RequestSanitizeServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<ClassifiedRequest> for RequestSanitizeService<S>
where
    S: Service<ClassifiedRequest> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = RequestSanitizeServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(RequestSanitizeServiceError::Inner)
    }

    fn call(&mut self, mut request: ClassifiedRequest) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            sanitize_request(&mut request);
            inner
                .call(request)
                .await
                .map_err(RequestSanitizeServiceError::Inner)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::classify_request_payload;
    use bytes::Bytes;
    use http::Request;

    fn make_classified(uri: &str, headers: &[(&str, &str)]) -> ClassifiedRequest {
        let mut builder = Request::builder().method(http::Method::POST).uri(uri);
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        let req = builder.body(Bytes::from(r#"{"model":"gpt-4o"}"#)).unwrap();
        classify_request_payload(req).unwrap()
    }

    #[test]
    fn strips_authorization_header() {
        let mut classified =
            make_classified("/v1/responses", &[("authorization", "Bearer sk-test-123")]);
        sanitize_request(&mut classified);
        assert!(classified.request.headers().get("authorization").is_none());
    }

    #[test]
    fn strips_x_api_key() {
        let mut classified = make_classified(
            "/v1/messages",
            &[
                ("x-api-key", "sk-ant-key"),
                ("anthropic-version", "2023-06-01"),
            ],
        );
        sanitize_request(&mut classified);
        assert!(classified.request.headers().get("x-api-key").is_none());
    }

    #[test]
    fn strips_key_query_param_preserves_others() {
        let mut classified = make_classified(
            "/v1beta/models/gemini-pro:generateContent?key=AIza-secret&alt=sse",
            &[],
        );
        sanitize_request(&mut classified);
        assert_eq!(classified.request.uri().query(), Some("alt=sse"),);
    }

    #[test]
    fn strips_key_only_query() {
        let mut classified = make_classified(
            "/v1beta/models/gemini-pro:generateContent?key=AIza-secret",
            &[],
        );
        sanitize_request(&mut classified);
        assert!(classified.request.uri().query().is_none());
    }

    #[test]
    fn preserves_non_auth_headers() {
        let mut classified = make_classified(
            "/v1/responses",
            &[
                ("authorization", "Bearer x"),
                ("content-type", "application/json"),
            ],
        );
        sanitize_request(&mut classified);
        assert!(classified.request.headers().get("authorization").is_none());
        assert!(classified.request.headers().get("content-type").is_some());
    }
}
