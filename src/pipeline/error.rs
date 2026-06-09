//! Pipeline error → HTTP response mapping (handler boundary).

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::channel::ChannelError;

/// Errors surfaced by the request pipeline, each with a fixed HTTP status.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("unknown route: {0}")]
    UnknownRoute(String),
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    #[error("unsupported path")]
    UnsupportedPath,
    #[error("no available members")]
    NoMembers,
    #[error("no available credentials")]
    NoCredentials,
    #[error("unknown channel: {0}")]
    UnknownChannel(String),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error("all upstream attempts failed")]
    AllAttemptsFailed,
    #[error("upstream transport error: {0}")]
    Transport(String),
}

impl PipelineError {
    pub fn status(&self) -> StatusCode {
        use PipelineError::*;
        match self {
            Unauthorized => StatusCode::UNAUTHORIZED,
            UnknownRoute(_) | UnknownProvider(_) | UnsupportedPath => StatusCode::NOT_FOUND,
            NoMembers | NoCredentials => StatusCode::SERVICE_UNAVAILABLE,
            UnknownChannel(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Channel(_) | AllAttemptsFailed | Transport(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

impl IntoResponse for PipelineError {
    fn into_response(self) -> Response {
        let status = self.status();
        // Variants whose Display embeds upstream/internal detail (URLs, transport
        // causes, possibly proxy info) are collapsed to a generic client message
        // and the real cause is logged server-side (CWE-209). The remaining
        // variants reveal nothing sensitive (client-supplied names / fixed text).
        let message = match &self {
            PipelineError::Channel(_)
            | PipelineError::Transport(_)
            | PipelineError::AllAttemptsFailed => {
                tracing::warn!(error = %self, "upstream request failed");
                "upstream request failed".to_string()
            }
            other => other.to_string(),
        };
        let body = json!({ "error": { "message": message, "type": "gproxy_error" } });
        (status, Json(body)).into_response()
    }
}
