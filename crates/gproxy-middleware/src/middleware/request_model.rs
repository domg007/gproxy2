use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;
use tower::{Layer, Service};

use crate::middleware::transform::error::MiddlewareTransformError;
use crate::middleware::transform::kinds::{OperationFamily, ProtocolKind};
use crate::middleware::transform::message::{TransformBodyStream, TransformRequestPayload};

pub struct ModelScopedRequest {
    pub request: TransformRequestPayload,
    pub model: Option<String>,
}

pub async fn extract_model_from_request_payload(
    input: TransformRequestPayload,
) -> Result<ModelScopedRequest, MiddlewareTransformError> {
    if input.operation == OperationFamily::ModelList {
        return Ok(ModelScopedRequest {
            request: input,
            model: None,
        });
    }

    let body = collect_body_bytes(input.body).await?;
    let model = extract_model_from_json_payload(input.operation, input.protocol, body.as_slice())?;

    Ok(ModelScopedRequest {
        request: TransformRequestPayload::from_bytes(
            input.operation,
            input.protocol,
            Bytes::from(body),
        ),
        model,
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestModelExtractLayer;

impl RequestModelExtractLayer {
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestModelExtractLayer {
    type Service = RequestModelExtractService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestModelExtractService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct RequestModelExtractService<S> {
    inner: S,
}

#[derive(Debug)]
pub enum RequestModelExtractServiceError<E> {
    Extract(MiddlewareTransformError),
    Inner(E),
}

impl<E: Display> Display for RequestModelExtractServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Extract(err) => Display::fmt(err, f),
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for RequestModelExtractServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<TransformRequestPayload> for RequestModelExtractService<S>
where
    S: Service<ModelScopedRequest> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = RequestModelExtractServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(RequestModelExtractServiceError::Inner)
    }

    fn call(&mut self, request: TransformRequestPayload) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let extracted = extract_model_from_request_payload(request)
                .await
                .map_err(RequestModelExtractServiceError::Extract)?;
            inner
                .call(extracted)
                .await
                .map_err(RequestModelExtractServiceError::Inner)
        })
    }
}

fn extract_model_from_json_payload(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Result<Option<String>, MiddlewareTransformError> {
    let value: Value =
        serde_json::from_slice(body).map_err(|err| MiddlewareTransformError::JsonDecode {
            kind: "request",
            operation,
            protocol,
            message: err.to_string(),
        })?;

    Ok(match (operation, protocol) {
        (OperationFamily::ModelList, _) => None,

        (OperationFamily::ModelGet, ProtocolKind::OpenAi) => {
            json_pointer_string(&value, "/path/model")
        }
        (OperationFamily::ModelGet, ProtocolKind::Claude) => {
            json_pointer_string(&value, "/path/model_id")
        }
        (OperationFamily::ModelGet, ProtocolKind::Gemini)
        | (OperationFamily::ModelGet, ProtocolKind::GeminiNDJson) => {
            json_pointer_string(&value, "/path/name")
        }

        (OperationFamily::CountToken, ProtocolKind::OpenAi) => {
            json_pointer_string(&value, "/body/model")
        }
        (OperationFamily::CountToken, ProtocolKind::Claude) => {
            json_pointer_string(&value, "/body/model")
        }
        (OperationFamily::CountToken, ProtocolKind::Gemini)
        | (OperationFamily::CountToken, ProtocolKind::GeminiNDJson) => {
            json_pointer_string(&value, "/body/generate_content_request/model")
                .or_else(|| json_pointer_string(&value, "/path/model"))
        }

        (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi)
        | (OperationFamily::CreateImage, ProtocolKind::OpenAi)
        | (OperationFamily::StreamCreateImage, ProtocolKind::OpenAi)
        | (OperationFamily::CreateImageEdit, ProtocolKind::OpenAi)
        | (OperationFamily::StreamCreateImageEdit, ProtocolKind::OpenAi) => {
            json_pointer_string(&value, "/body/model")
        }
        (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi) => {
            json_pointer_string(&value, "/body/model")
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
            json_pointer_string(&value, "/body/model")
        }
        (OperationFamily::GenerateContent, ProtocolKind::Claude)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
            json_pointer_string(&value, "/body/model")
        }
        (OperationFamily::GenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::GenerateContent, ProtocolKind::GeminiNDJson)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
            json_pointer_string(&value, "/path/model")
        }
        (OperationFamily::GeminiLive, ProtocolKind::Gemini) => {
            json_pointer_string(&value, "/body/setup/model")
        }

        (OperationFamily::Embedding, ProtocolKind::OpenAi) => {
            json_pointer_string(&value, "/body/model")
        }
        (OperationFamily::Embedding, ProtocolKind::Gemini)
        | (OperationFamily::Embedding, ProtocolKind::GeminiNDJson) => {
            json_pointer_string(&value, "/path/model")
        }

        (OperationFamily::Compact, ProtocolKind::OpenAi) => {
            json_pointer_string(&value, "/body/model")
        }

        _ => None,
    })
}

fn json_pointer_string(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
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
