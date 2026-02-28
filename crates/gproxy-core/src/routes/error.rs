use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct HttpError {
    pub status: StatusCode,
    pub message: String,
}

impl HttpError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

impl From<gproxy_admin::AdminApiError> for HttpError {
    fn from(value: gproxy_admin::AdminApiError) -> Self {
        use gproxy_admin::AdminApiError;
        match value {
            AdminApiError::Unauthorized => Self::new(StatusCode::UNAUTHORIZED, value.to_string()),
            AdminApiError::Forbidden => Self::new(StatusCode::FORBIDDEN, value.to_string()),
            AdminApiError::NotFound(_) => Self::new(StatusCode::NOT_FOUND, value.to_string()),
            AdminApiError::InvalidInput(_) => Self::new(StatusCode::BAD_REQUEST, value.to_string()),
            AdminApiError::Storage(_) | AdminApiError::Queue(_) => {
                Self::new(StatusCode::INTERNAL_SERVER_ERROR, value.to_string())
            }
        }
    }
}

impl From<gproxy_provider::UpstreamError> for HttpError {
    fn from(value: gproxy_provider::UpstreamError) -> Self {
        let status = StatusCode::from_u16(value.http_status_code())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        Self::new(status, value.to_string())
    }
}
