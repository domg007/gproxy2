use http::StatusCode;

use crate::openai::create_response::response::OpenAiCreateResponseResponse;
use crate::openai::create_response::stream::{
    OpenAiCreateResponseSseData, OpenAiCreateResponseSseStreamBody,
};
use crate::openai::create_response::types::{OpenAiApiError, OpenAiApiErrorResponse};
use crate::openai::types::OpenAiResponseHeaders;
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiCreateResponseSseStreamBody> for OpenAiCreateResponseResponse {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseSseStreamBody) -> Result<Self, TransformError> {
        let mut latest_response = None;
        let mut stream_error = None::<(String, String, Option<String>)>;

        for event in value.events {
            match event.data {
                OpenAiCreateResponseSseData::Done(_) => break,
                OpenAiCreateResponseSseData::Event(event) => match event {
                    crate::openai::create_response::stream::ResponseStreamEvent::Created {
                        response,
                        ..
                    }
                    | crate::openai::create_response::stream::ResponseStreamEvent::Queued {
                        response,
                        ..
                    }
                    | crate::openai::create_response::stream::ResponseStreamEvent::InProgress {
                        response,
                        ..
                    }
                    | crate::openai::create_response::stream::ResponseStreamEvent::Completed {
                        response,
                        ..
                    }
                    | crate::openai::create_response::stream::ResponseStreamEvent::Incomplete {
                        response,
                        ..
                    }
                    | crate::openai::create_response::stream::ResponseStreamEvent::Failed {
                        response,
                        ..
                    } => latest_response = Some(response),
                    crate::openai::create_response::stream::ResponseStreamEvent::Error {
                        code,
                        message,
                        param,
                        ..
                    } => stream_error = Some((code, message, param)),
                    _ => {}
                },
            }
        }

        if let Some(body) = latest_response {
            Ok(OpenAiCreateResponseResponse::Success {
                stats_code: StatusCode::OK,
                headers: OpenAiResponseHeaders::default(),
                body,
            })
        } else if let Some((code, message, param)) = stream_error {
            Ok(OpenAiCreateResponseResponse::Error {
                stats_code: StatusCode::BAD_REQUEST,
                headers: OpenAiResponseHeaders::default(),
                body: OpenAiApiErrorResponse {
                    error: OpenAiApiError {
                        message,
                        type_: "stream_error".to_string(),
                        param,
                        code: Some(code),
                    },
                },
            })
        } else {
            Err(TransformError::not_implemented(
                "cannot convert OpenAI response SSE stream body without response snapshots",
            ))
        }
    }
}
