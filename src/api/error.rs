//! Admin API error → JSON. Mirrors the gateway error shape; never leaks secrets.

use http::StatusCode;

/// Admin-API error taxonomy. Each variant maps to a fixed status and a
/// public-safe message; `Internal` collapses to a generic message and logs the
/// real cause (CWE-209).
#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
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
            ApiError::BadRequest(m) | ApiError::NotFound(m) | ApiError::Conflict(m) => m.clone(),
            ApiError::Internal(cause) => {
                tracing::error!(error = %cause, "admin api internal error");
                "internal error".to_string()
            }
        }
    }

    /// Short machine-readable type tag for the JSON body. Only the native
    /// `IntoResponse` consumes it; on wasm the admin HTTP layer is absent.
    #[cfg(not(target_arch = "wasm32"))]
    fn type_tag(&self) -> &'static str {
        match self {
            ApiError::Unauthorized => "unauthorized",
            ApiError::BadRequest(_) => "bad_request",
            ApiError::NotFound(_) => "not_found",
            ApiError::Conflict(_) => "conflict",
            ApiError::Internal(_) => "internal",
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status();
        let body = serde_json::json!({
            "error": { "message": self.message(), "type": self.type_tag() }
        });
        (status, axum::Json(body)).into_response()
    }
}
