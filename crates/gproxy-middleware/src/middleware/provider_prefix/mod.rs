use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::StreamExt;
use gproxy_protocol::claude::create_message::stream::ClaudeCreateMessageStreamEvent;
use gproxy_protocol::claude::create_message::types::Model as ClaudeModel;
use gproxy_protocol::openai::create_chat_completions::stream::ChatCompletionChunk;
use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
use tower::{Layer, Service};

use crate::middleware::transform::engine::{decode_response_payload, encode_response_payload};
use crate::middleware::transform::error::MiddlewareTransformError;
use crate::middleware::transform::kinds::{OperationFamily, ProtocolKind};
use crate::middleware::transform::message::{
    TransformBodyStream, TransformRequestPayload, TransformResponse, TransformResponsePayload,
};

mod capture;
mod service;
mod stream;

use capture::{add_provider_prefix_to_response, strip_provider_prefix_from_request_json};
pub use service::*;
use stream::prefix_stream_response_body;

pub struct ProviderScopedRequest {
    pub request: TransformRequestPayload,
    pub provider: Option<String>,
}

pub async fn extract_provider_from_request_payload(
    input: TransformRequestPayload,
) -> Result<ProviderScopedRequest, MiddlewareTransformError> {
    if input.operation == OperationFamily::ModelList {
        return Ok(ProviderScopedRequest {
            request: input,
            provider: None,
        });
    }

    let body = collect_body_bytes(input.body).await?;
    let (provider, body) =
        strip_provider_prefix_from_request_json(input.operation, input.protocol, body.as_slice())?;

    Ok(ProviderScopedRequest {
        request: TransformRequestPayload::from_bytes(
            input.operation,
            input.protocol,
            Bytes::from(body),
        ),
        provider: Some(provider),
    })
}

pub async fn add_provider_prefix_to_response_payload(
    input: TransformResponsePayload,
    provider: &str,
) -> Result<TransformResponsePayload, MiddlewareTransformError> {
    if provider.is_empty() {
        return Err(MiddlewareTransformError::ProviderPrefix {
            message: "provider cannot be empty".to_string(),
        });
    }

    if input.operation == OperationFamily::StreamGenerateContent {
        let body = prefix_stream_response_body(input.body, input.protocol, provider.to_string());
        return Ok(TransformResponsePayload::new(
            input.operation,
            input.protocol,
            body,
        ));
    }

    let body = collect_body_bytes(input.body).await?;
    let mut response = decode_response_payload(input.operation, input.protocol, body.as_slice())?;
    add_provider_prefix_to_response(&mut response, provider);
    let body = encode_response_payload(response)?;

    Ok(TransformResponsePayload::from_bytes(
        input.operation,
        input.protocol,
        Bytes::from(body),
    ))
}

async fn collect_body_bytes(
    mut body: TransformBodyStream,
) -> Result<Vec<u8>, MiddlewareTransformError> {
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        out.extend_from_slice(&chunk?);
    }
    Ok(out)
}
