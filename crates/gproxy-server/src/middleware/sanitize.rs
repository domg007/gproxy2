use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/// Auth headers to remove.
const AUTH_HEADERS: &[&str] = &["authorization", "x-api-key", "x-goog-api-key"];

/// Sensitive query parameters to remove.
const AUTH_QUERY_KEYS: &[&str] = &["key"];

/// Axum middleware: strip auth headers and sensitive query params.
pub async fn sanitize_middleware(mut request: Request, next: Next) -> Response {
    for name in AUTH_HEADERS {
        request.headers_mut().remove(*name);
    }
    strip_auth_query_params(&mut request);
    next.run(request).await
}

fn strip_auth_query_params(request: &mut Request) {
    let uri = request.uri();
    let Some(query) = uri.query() else {
        return;
    };
    let filtered: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            let key = pair.split('=').next().unwrap_or("");
            !AUTH_QUERY_KEYS.contains(&key)
        })
        .collect();
    let new_pq = if filtered.is_empty() {
        uri.path().to_string()
    } else {
        format!("{}?{}", uri.path(), filtered.join("&"))
    };
    if let Ok(new_uri) = new_pq.parse() {
        *request.uri_mut() = new_uri;
    }
}
