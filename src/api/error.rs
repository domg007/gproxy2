//! Admin API error → JSON. Mirrors the gateway error shape; never leaks secrets.

use http::StatusCode;

/// Admin-API error taxonomy. Each variant maps to a fixed status and a
/// public-safe message; `Internal` collapses to a generic message and logs the
/// real cause (CWE-209).
#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    Forbidden(String),
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl ApiError {
    /// HTTP status for this error.
    pub fn status(&self) -> StatusCode {
        match self {
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Public-safe message. `Internal` never reveals its cause — that is logged
    /// server-side instead.
    pub fn message(&self) -> String {
        match self {
            ApiError::Unauthorized => "unauthorized".to_string(),
            ApiError::Forbidden(m) => m.clone(),
            ApiError::BadRequest(m) | ApiError::NotFound(m) | ApiError::Conflict(m) => m.clone(),
            ApiError::Internal(cause) => {
                tracing::error!(error = %cause, "admin api internal error");
                "internal error".to_string()
            }
        }
    }

    /// Short machine-readable type tag for the JSON body.  Cross-target so
    /// both the native `IntoResponse` and the edge dispatcher share one mapping.
    pub fn type_str(&self) -> &'static str {
        match self {
            ApiError::Unauthorized => "unauthorized",
            ApiError::Forbidden(_) => "forbidden",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::NotFound(_) => "not_found",
            ApiError::Conflict(_) => "conflict",
            ApiError::Internal(_) => "internal",
        }
    }

    /// Cross-target render of the error envelope `{"error":{"message","type"}}`.
    /// The native `IntoResponse` delegates here; the edge dispatcher uses it
    /// directly (no axum).
    pub fn to_parts(&self) -> (http::StatusCode, Vec<u8>) {
        let body = serde_json::json!({
            "error": { "message": self.message(), "type": self.type_str() }
        });
        (self.status(), serde_json::to_vec(&body).unwrap_or_default())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, bytes) = self.to_parts();
        let body = axum::body::Body::from(bytes);
        let mut resp = axum::response::Response::new(body);
        *resp.status_mut() = status;
        resp.headers_mut().insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        resp
    }
}
