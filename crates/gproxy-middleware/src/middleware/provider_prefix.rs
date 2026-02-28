use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use gproxy_protocol::claude::create_message::stream::ClaudeCreateMessageStreamEvent;
use gproxy_protocol::claude::create_message::types::Model as ClaudeModel;
use gproxy_protocol::openai::create_chat_completions::stream::ChatCompletionChunk;
use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
use gproxy_protocol::openai::embeddings::types::OpenAiEmbeddingModel;
use tower::{Layer, Service};

use crate::middleware::transform::engine::{
    decode_request_payload, decode_response_payload, encode_request_payload,
    encode_response_payload,
};
use crate::middleware::transform::error::MiddlewareTransformError;
use crate::middleware::transform::kinds::{OperationFamily, ProtocolKind};
use crate::middleware::transform::message::{
    TransformBodyStream, TransformRequest, TransformRequestPayload, TransformResponse,
    TransformResponsePayload,
};

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
    let mut request = decode_request_payload(input.operation, input.protocol, body.as_slice())?;
    let provider =
        strip_provider_prefix_from_request(&mut request, input.operation, input.protocol)?;
    let body = encode_request_payload(request)?;

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

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestProviderExtractLayer;

impl RequestProviderExtractLayer {
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestProviderExtractLayer {
    type Service = RequestProviderExtractService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestProviderExtractService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct RequestProviderExtractService<S> {
    inner: S,
}

#[derive(Debug)]
pub enum RequestProviderExtractServiceError<E> {
    Extract(MiddlewareTransformError),
    Inner(E),
}

impl<E: Display> Display for RequestProviderExtractServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Extract(err) => Display::fmt(err, f),
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for RequestProviderExtractServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<TransformRequestPayload> for RequestProviderExtractService<S>
where
    S: Service<ProviderScopedRequest> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = RequestProviderExtractServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(RequestProviderExtractServiceError::Inner)
    }

    fn call(&mut self, request: TransformRequestPayload) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let extracted = extract_provider_from_request_payload(request)
                .await
                .map_err(RequestProviderExtractServiceError::Extract)?;
            inner
                .call(extracted)
                .await
                .map_err(RequestProviderExtractServiceError::Inner)
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResponseProviderPrefixLayer {
    default_provider: String,
}

impl ResponseProviderPrefixLayer {
    pub fn new(default_provider: impl Into<String>) -> Self {
        Self {
            default_provider: default_provider.into(),
        }
    }
}

impl<S> Layer<S> for ResponseProviderPrefixLayer {
    type Service = ResponseProviderPrefixService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseProviderPrefixService {
            inner,
            default_provider: self.default_provider.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResponseProviderPrefixService<S> {
    inner: S,
    default_provider: String,
}

#[derive(Debug)]
pub enum ResponseProviderPrefixServiceError<E> {
    Prefix(MiddlewareTransformError),
    Inner(E),
}

impl<E: Display> Display for ResponseProviderPrefixServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prefix(err) => Display::fmt(err, f),
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for ResponseProviderPrefixServiceError<E> {}

impl<S> Service<ProviderScopedRequest> for ResponseProviderPrefixService<S>
where
    S: Service<ProviderScopedRequest, Response = TransformResponsePayload> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = TransformResponsePayload;
    type Error = ResponseProviderPrefixServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(ResponseProviderPrefixServiceError::Inner)
    }

    fn call(&mut self, request: ProviderScopedRequest) -> Self::Future {
        let provider = request
            .provider
            .clone()
            .unwrap_or_else(|| self.default_provider.clone());
        let fut = self.inner.call(request);
        Box::pin(async move {
            let response = fut
                .await
                .map_err(ResponseProviderPrefixServiceError::Inner)?;
            add_provider_prefix_to_response_payload(response, &provider)
                .await
                .map_err(ResponseProviderPrefixServiceError::Prefix)
        })
    }
}

struct ProviderCapture {
    provider: Option<String>,
}

impl ProviderCapture {
    fn new() -> Self {
        Self { provider: None }
    }

    fn strip(
        &mut self,
        operation: OperationFamily,
        protocol: ProtocolKind,
        field: &'static str,
        value: &str,
    ) -> Result<String, MiddlewareTransformError> {
        let Some((has_models_prefix, provider, model_without_provider)) =
            split_provider_prefixed_model(value)
        else {
            return Err(MiddlewareTransformError::ProviderPrefix {
                message: format!(
                    "missing provider prefix in {field} for ({operation:?}, {protocol:?}): {value}",
                ),
            });
        };

        if let Some(existing) = self.provider.as_ref() {
            if existing != provider {
                return Err(MiddlewareTransformError::ProviderPrefix {
                    message: format!(
                        "inconsistent provider prefix in {field} for ({operation:?}, {protocol:?}): expected {existing}, got {provider}",
                    ),
                });
            }
        } else {
            self.provider = Some(provider.to_string());
        }

        Ok(if has_models_prefix {
            format!("models/{model_without_provider}")
        } else {
            model_without_provider.to_string()
        })
    }

    fn finish(
        self,
        operation: OperationFamily,
        protocol: ProtocolKind,
    ) -> Result<String, MiddlewareTransformError> {
        self.provider
            .ok_or(MiddlewareTransformError::ProviderPrefix {
                message: format!(
                    "no model/provider prefix found for ({operation:?}, {protocol:?})"
                ),
            })
    }
}

fn split_provider_prefixed_model(value: &str) -> Option<(bool, &str, &str)> {
    let (has_models_prefix, tail) = if let Some(rest) = value.strip_prefix("models/") {
        (true, rest)
    } else {
        (false, value)
    };
    let (provider, model_without_provider) = tail.split_once('/')?;
    if provider.is_empty() || model_without_provider.is_empty() {
        return None;
    }
    Some((has_models_prefix, provider, model_without_provider))
}

fn add_provider_prefix(value: &str, provider: &str) -> String {
    if provider.is_empty() {
        return value.to_string();
    }
    if split_provider_prefixed_model(value).is_some() {
        return value.to_string();
    }

    if let Some(rest) = value.strip_prefix("models/") {
        return format!("models/{provider}/{rest}");
    }

    if value.is_empty() {
        provider.to_string()
    } else {
        format!("{provider}/{value}")
    }
}

fn strip_provider_prefix_from_request(
    request: &mut TransformRequest,
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> Result<String, MiddlewareTransformError> {
    let mut capture = ProviderCapture::new();

    match request {
        TransformRequest::ModelListOpenAi(_)
        | TransformRequest::ModelListClaude(_)
        | TransformRequest::ModelListGemini(_) => {}
        TransformRequest::ModelGetOpenAi(value) => {
            value.path.model =
                capture.strip(operation, protocol, "path.model", &value.path.model)?;
        }
        TransformRequest::ModelGetClaude(value) => {
            value.path.model_id =
                capture.strip(operation, protocol, "path.model_id", &value.path.model_id)?;
        }
        TransformRequest::ModelGetGemini(value) => {
            value.path.name = capture.strip(operation, protocol, "path.name", &value.path.name)?;
        }
        TransformRequest::CountTokenOpenAi(value) => {
            let Some(model) = value.body.model.as_ref() else {
                return Err(MiddlewareTransformError::ProviderPrefix {
                    message: "missing body.model for OpenAI count-tokens".to_string(),
                });
            };
            value.body.model = Some(capture.strip(operation, protocol, "body.model", model)?);
        }
        TransformRequest::CountTokenClaude(value) => {
            strip_provider_from_claude_model(
                &mut value.body.model,
                &mut capture,
                operation,
                protocol,
                "body.model",
            )?;
        }
        TransformRequest::CountTokenGemini(value) => {
            value.path.model =
                capture.strip(operation, protocol, "path.model", &value.path.model)?;
            if let Some(generate_request) = value.body.generate_content_request.as_mut() {
                generate_request.model = capture.strip(
                    operation,
                    protocol,
                    "body.generate_content_request.model",
                    &generate_request.model,
                )?;
            }
        }
        TransformRequest::GenerateContentOpenAiResponse(value)
        | TransformRequest::StreamGenerateContentOpenAiResponse(value) => {
            let Some(model) = value.body.model.as_ref() else {
                return Err(MiddlewareTransformError::ProviderPrefix {
                    message: "missing body.model for OpenAI responses".to_string(),
                });
            };
            value.body.model = Some(capture.strip(operation, protocol, "body.model", model)?);
        }
        TransformRequest::GenerateContentOpenAiChatCompletions(value)
        | TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            value.body.model =
                capture.strip(operation, protocol, "body.model", &value.body.model)?;
        }
        TransformRequest::GenerateContentClaude(value)
        | TransformRequest::StreamGenerateContentClaude(value) => {
            strip_provider_from_claude_model(
                &mut value.body.model,
                &mut capture,
                operation,
                protocol,
                "body.model",
            )?;
        }
        TransformRequest::GenerateContentGemini(value) => {
            value.path.model =
                capture.strip(operation, protocol, "path.model", &value.path.model)?;
        }
        TransformRequest::StreamGenerateContentGeminiSse(value)
        | TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
            value.path.model =
                capture.strip(operation, protocol, "path.model", &value.path.model)?;
        }
        TransformRequest::EmbeddingOpenAi(value) => {
            strip_provider_from_openai_embedding_model(
                &mut value.body.model,
                &mut capture,
                operation,
                protocol,
                "body.model",
            )?;
        }
        TransformRequest::EmbeddingGemini(value) => {
            value.path.model =
                capture.strip(operation, protocol, "path.model", &value.path.model)?;
        }
        TransformRequest::CompactOpenAi(value) => {
            value.body.model =
                capture.strip(operation, protocol, "body.model", &value.body.model)?;
        }
    }

    capture.finish(operation, protocol)
}

fn strip_provider_from_claude_model(
    model: &mut ClaudeModel,
    capture: &mut ProviderCapture,
    operation: OperationFamily,
    protocol: ProtocolKind,
    field: &'static str,
) -> Result<(), MiddlewareTransformError> {
    let ClaudeModel::Custom(value) = model else {
        return Err(MiddlewareTransformError::ProviderPrefix {
            message: format!(
                "missing provider prefix in {field} for ({operation:?}, {protocol:?}): Claude known model has no provider segment",
            ),
        });
    };
    *value = capture.strip(operation, protocol, field, value)?;
    Ok(())
}

fn strip_provider_from_openai_embedding_model(
    model: &mut OpenAiEmbeddingModel,
    capture: &mut ProviderCapture,
    operation: OperationFamily,
    protocol: ProtocolKind,
    field: &'static str,
) -> Result<(), MiddlewareTransformError> {
    let OpenAiEmbeddingModel::Custom(value) = model else {
        return Err(MiddlewareTransformError::ProviderPrefix {
            message: format!(
                "missing provider prefix in {field} for ({operation:?}, {protocol:?}): known embedding model has no provider segment",
            ),
        });
    };
    *value = capture.strip(operation, protocol, field, value)?;
    Ok(())
}

fn add_provider_prefix_to_response(response: &mut TransformResponse, provider: &str) {
    match response {
        TransformResponse::ModelListOpenAi(
            gproxy_protocol::openai::model_list::response::OpenAiModelListResponse::Success {
                body,
                ..
            },
        ) => {
            for model in &mut body.data {
                model.id = add_provider_prefix(&model.id, provider);
            }
        }
        TransformResponse::ModelListClaude(
            gproxy_protocol::claude::model_list::response::ClaudeModelListResponse::Success {
                body,
                ..
            },
        ) => {
            for model in &mut body.data {
                model.id = add_provider_prefix(&model.id, provider);
            }
            body.first_id = add_provider_prefix(&body.first_id, provider);
            body.last_id = add_provider_prefix(&body.last_id, provider);
        }
        TransformResponse::ModelListGemini(
            gproxy_protocol::gemini::model_list::response::GeminiModelListResponse::Success {
                body,
                ..
            },
        ) => {
            for model in &mut body.models {
                model.name = add_provider_prefix(&model.name, provider);
                if let Some(base_model_id) = model.base_model_id.as_mut() {
                    *base_model_id = add_provider_prefix(base_model_id, provider);
                }
            }
        }
        TransformResponse::ModelGetOpenAi(
            gproxy_protocol::openai::model_get::response::OpenAiModelGetResponse::Success {
                body,
                ..
            },
        ) => {
            body.id = add_provider_prefix(&body.id, provider);
        }
        TransformResponse::ModelGetClaude(
            gproxy_protocol::claude::model_get::response::ClaudeModelGetResponse::Success {
                body,
                ..
            },
        ) => {
            body.id = add_provider_prefix(&body.id, provider);
        }
        TransformResponse::ModelGetGemini(
            gproxy_protocol::gemini::model_get::response::GeminiModelGetResponse::Success {
                body,
                ..
            },
        ) => {
            body.name = add_provider_prefix(&body.name, provider);
            if let Some(base_model_id) = body.base_model_id.as_mut() {
                *base_model_id = add_provider_prefix(base_model_id, provider);
            }
        }
        TransformResponse::GenerateContentOpenAiResponse(
            gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse::Success {
                body,
                ..
            },
        ) => {
            body.model = add_provider_prefix(&body.model, provider);
        }
        TransformResponse::GenerateContentOpenAiChatCompletions(
            gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse::Success {
                body,
                ..
            },
        ) => {
            body.model = add_provider_prefix(&body.model, provider);
        }
        TransformResponse::GenerateContentClaude(value) => {
            if let gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse::Success {
                body,
                ..
            } = value
                && let Some(raw) = serialize_claude_model(&body.model)
            {
                body.model = ClaudeModel::Custom(add_provider_prefix(&raw, provider));
            }
        }
        TransformResponse::EmbeddingOpenAi(
            gproxy_protocol::openai::embeddings::response::OpenAiEmbeddingsResponse::Success {
                body,
                ..
            },
        ) => {
            body.model = add_provider_prefix(&body.model, provider);
        }
        _ => {}
    }
}

fn serialize_claude_model(model: &ClaudeModel) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

enum StreamRewriteProtocol {
    OpenAiResponse,
    OpenAiChatCompletions,
    Claude,
    Passthrough,
}

struct StreamRewriteState {
    input: TransformBodyStream,
    protocol: StreamRewriteProtocol,
    provider: String,
    buffer: Vec<u8>,
    output: VecDeque<Bytes>,
    ended: bool,
}

impl StreamRewriteState {
    fn new(input: TransformBodyStream, protocol: ProtocolKind, provider: String) -> Self {
        let protocol = match protocol {
            ProtocolKind::OpenAi => StreamRewriteProtocol::OpenAiResponse,
            ProtocolKind::OpenAiChatCompletion => StreamRewriteProtocol::OpenAiChatCompletions,
            ProtocolKind::Claude => StreamRewriteProtocol::Claude,
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => StreamRewriteProtocol::Passthrough,
        };
        Self {
            input,
            protocol,
            provider,
            buffer: Vec::new(),
            output: VecDeque::new(),
            ended: false,
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) {
        if matches!(self.protocol, StreamRewriteProtocol::Passthrough) {
            self.output.push_back(Bytes::copy_from_slice(chunk));
            return;
        }

        self.buffer.extend_from_slice(chunk);
        while let Some(frame) = next_sse_frame(&mut self.buffer) {
            self.output.push_back(rewrite_sse_frame(
                &self.protocol,
                frame,
                self.provider.as_str(),
            ));
        }
    }

    fn finish_input(&mut self) {
        if !self.buffer.is_empty() {
            self.output.push_back(Bytes::from(self.buffer.clone()));
            self.buffer.clear();
        }
        self.ended = true;
    }

    fn pop_output(&mut self) -> Option<Bytes> {
        self.output.pop_front()
    }
}

fn prefix_stream_response_body(
    input: TransformBodyStream,
    protocol: ProtocolKind,
    provider: String,
) -> TransformBodyStream {
    let state = StreamRewriteState::new(input, protocol, provider);
    let stream = stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(output) = state.pop_output() {
                return Ok(Some((output, state)));
            }

            if state.ended {
                return Ok(None);
            }

            match state.input.next().await {
                Some(Ok(chunk)) => state.push_chunk(chunk.as_ref()),
                Some(Err(err)) => return Err(err),
                None => state.finish_input(),
            }
        }
    });
    Box::pin(stream)
}

fn next_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let lf_pos = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf_pos = buffer.windows(4).position(|window| window == b"\r\n\r\n");

    let (pos, delim_len) = match (lf_pos, crlf_pos) {
        (Some(a), Some(b)) if a <= b => (a, 2),
        (Some(_), Some(b)) => (b, 4),
        (Some(a), None) => (a, 2),
        (None, Some(b)) => (b, 4),
        (None, None) => return None,
    };

    let frame = buffer[..pos].to_vec();
    buffer.drain(..pos + delim_len);
    Some(frame)
}

fn parse_sse_fields(frame: &[u8]) -> Option<(Option<String>, String)> {
    let text = std::str::from_utf8(frame).ok()?;
    let mut event = None;
    let mut data_lines = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim_start().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        None
    } else {
        Some((event, data_lines.join("\n")))
    }
}

fn encode_sse_frame(event: Option<&str>, data: &str) -> Bytes {
    let mut out = String::new();
    if let Some(event_name) = event {
        out.push_str("event: ");
        out.push_str(event_name);
        out.push('\n');
    }
    for line in data.lines() {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    Bytes::from(out)
}

fn raw_sse_frame(frame: Vec<u8>) -> Bytes {
    let mut out = frame;
    out.extend_from_slice(b"\n\n");
    Bytes::from(out)
}

fn rewrite_sse_frame(protocol: &StreamRewriteProtocol, frame: Vec<u8>, provider: &str) -> Bytes {
    let Some((event, data)) = parse_sse_fields(frame.as_slice()) else {
        return raw_sse_frame(frame);
    };
    if data == "[DONE]" {
        return encode_sse_frame(event.as_deref(), data.as_str());
    }

    match protocol {
        StreamRewriteProtocol::OpenAiResponse => {
            let Ok(mut event_data) = serde_json::from_str::<ResponseStreamEvent>(&data) else {
                return raw_sse_frame(frame);
            };
            add_prefix_to_openai_response_stream_event(&mut event_data, provider);
            match serde_json::to_string(&event_data) {
                Ok(json) => encode_sse_frame(event.as_deref(), &json),
                Err(_) => raw_sse_frame(frame),
            }
        }
        StreamRewriteProtocol::OpenAiChatCompletions => {
            let Ok(mut chunk) = serde_json::from_str::<ChatCompletionChunk>(&data) else {
                return raw_sse_frame(frame);
            };
            chunk.model = add_provider_prefix(&chunk.model, provider);
            match serde_json::to_string(&chunk) {
                Ok(json) => encode_sse_frame(event.as_deref(), &json),
                Err(_) => raw_sse_frame(frame),
            }
        }
        StreamRewriteProtocol::Claude => {
            let Ok(mut event_data) = serde_json::from_str::<ClaudeCreateMessageStreamEvent>(&data)
            else {
                return raw_sse_frame(frame);
            };
            if let ClaudeCreateMessageStreamEvent::MessageStart(message_start) = &mut event_data
                && let Some(raw) = serialize_claude_model(&message_start.message.model)
            {
                message_start.message.model =
                    ClaudeModel::Custom(add_provider_prefix(&raw, provider));
            }
            match serde_json::to_string(&event_data) {
                Ok(json) => encode_sse_frame(event.as_deref(), &json),
                Err(_) => raw_sse_frame(frame),
            }
        }
        StreamRewriteProtocol::Passthrough => raw_sse_frame(frame),
    }
}

fn add_prefix_to_openai_response_stream_event(event: &mut ResponseStreamEvent, provider: &str) {
    match event {
        ResponseStreamEvent::Created { response, .. }
        | ResponseStreamEvent::Queued { response, .. }
        | ResponseStreamEvent::InProgress { response, .. }
        | ResponseStreamEvent::Failed { response, .. }
        | ResponseStreamEvent::Incomplete { response, .. }
        | ResponseStreamEvent::Completed { response, .. } => {
            response.model = add_provider_prefix(&response.model, provider);
        }
        _ => {}
    }
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
