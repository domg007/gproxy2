use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::StreamExt;
use gproxy_protocol::claude::create_message::types::Model as ClaudeModel;
use gproxy_protocol::openai::embeddings::types::OpenAiEmbeddingModel;
use tower::{Layer, Service};

use crate::middleware::transform::engine::decode_request_payload;
use crate::middleware::transform::error::MiddlewareTransformError;
use crate::middleware::transform::kinds::OperationFamily;
use crate::middleware::transform::message::{
    TransformBodyStream, TransformRequest, TransformRequestPayload,
};

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
    let request = decode_request_payload(input.operation, input.protocol, body.as_slice())?;
    let model = extract_model_from_request(&request);

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

fn extract_model_from_request(request: &TransformRequest) -> Option<String> {
    match request {
        TransformRequest::ModelListOpenAi(_)
        | TransformRequest::ModelListClaude(_)
        | TransformRequest::ModelListGemini(_) => None,

        TransformRequest::ModelGetOpenAi(value) => Some(value.path.model.clone()),
        TransformRequest::ModelGetClaude(value) => Some(value.path.model_id.clone()),
        TransformRequest::ModelGetGemini(value) => Some(value.path.name.clone()),

        TransformRequest::CountTokenOpenAi(value) => value.body.model.clone(),
        TransformRequest::CountTokenClaude(value) => serialize_claude_model(&value.body.model),
        TransformRequest::CountTokenGemini(value) => {
            if let Some(generate_request) = value.body.generate_content_request.as_ref() {
                Some(generate_request.model.clone())
            } else {
                Some(value.path.model.clone())
            }
        }

        TransformRequest::GenerateContentOpenAiResponse(value)
        | TransformRequest::StreamGenerateContentOpenAiResponse(value) => value.body.model.clone(),

        TransformRequest::GenerateContentOpenAiChatCompletions(value)
        | TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            Some(value.body.model.clone())
        }

        TransformRequest::GenerateContentClaude(value)
        | TransformRequest::StreamGenerateContentClaude(value) => {
            serialize_claude_model(&value.body.model)
        }

        TransformRequest::GenerateContentGemini(value) => Some(value.path.model.clone()),
        TransformRequest::StreamGenerateContentGeminiSse(value)
        | TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
            Some(value.path.model.clone())
        }

        TransformRequest::EmbeddingOpenAi(value) => {
            serialize_openai_embedding_model(&value.body.model)
        }
        TransformRequest::EmbeddingGemini(value) => Some(value.path.model.clone()),

        TransformRequest::CompactOpenAi(value) => Some(value.body.model.clone()),
    }
}

fn serialize_claude_model(model: &ClaudeModel) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn serialize_openai_embedding_model(model: &OpenAiEmbeddingModel) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
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
