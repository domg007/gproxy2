use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures_util::{StreamExt, stream};
use gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse;
use gproxy_protocol::claude::create_message::stream::{
    BetaMessageDeltaUsage, ClaudeCreateMessageStreamEvent,
};
use gproxy_protocol::claude::create_message::types::{BetaIterationUsage, BetaUsage};
use gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse;
use gproxy_protocol::gemini::generate_content::types::GeminiUsageMetadata;
use gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
use gproxy_protocol::openai::create_chat_completions::stream::ChatCompletionChunk;
use gproxy_protocol::openai::create_chat_completions::types::CompletionUsage;
use gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse;
use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
use gproxy_protocol::openai::create_response::types::ResponseUsage;
use tower::{Layer, Service};

use crate::middleware::transform::kinds::{OperationFamily, ProtocolKind};
use crate::middleware::transform::message::{TransformRequestPayload, TransformResponsePayload};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UsageSnapshot {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_creation_input_tokens_5min: Option<u64>,
    pub cache_creation_input_tokens_1h: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub thoughts_tokens: Option<u64>,
    pub tool_use_prompt_tokens: Option<u64>,
}

impl UsageSnapshot {
    fn with_derived_totals(mut self) -> Self {
        if self.total_tokens.is_none()
            && let (Some(input), Some(output)) = (self.input_tokens, self.output_tokens)
        {
            self.total_tokens = Some(input.saturating_add(output));
        }
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct UsageHandle {
    inner: Arc<Mutex<Option<UsageSnapshot>>>,
}

impl UsageHandle {
    pub fn latest(&self) -> Option<UsageSnapshot> {
        self.inner.lock().ok().and_then(|guard| guard.clone())
    }

    fn set_latest(&self, usage: Option<UsageSnapshot>) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = usage;
        }
    }
}

pub struct UsageExtractedResponse {
    pub response: TransformResponsePayload,
    pub usage: UsageHandle,
}

struct UsageStreamState {
    input: crate::middleware::transform::message::TransformBodyStream,
    parser: UsageParser,
    usage: UsageHandle,
}

pub fn attach_usage_extractor(payload: TransformResponsePayload) -> UsageExtractedResponse {
    let usage = UsageHandle::default();
    let parser = UsageParser::new(payload.operation, payload.protocol);
    let state = UsageStreamState {
        input: payload.body,
        parser,
        usage: usage.clone(),
    };

    let body = stream::try_unfold(state, |mut state| async move {
        match state.input.next().await {
            Some(Ok(chunk)) => {
                state.parser.feed(chunk.as_ref());
                Ok(Some((chunk, state)))
            }
            Some(Err(err)) => {
                state.usage.set_latest(state.parser.snapshot());
                Err(err)
            }
            None => {
                state.usage.set_latest(state.parser.finish());
                Ok(None)
            }
        }
    });

    UsageExtractedResponse {
        response: TransformResponsePayload::new(
            payload.operation,
            payload.protocol,
            Box::pin(body),
        ),
        usage,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ResponseUsageExtractLayer;

impl ResponseUsageExtractLayer {
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for ResponseUsageExtractLayer {
    type Service = ResponseUsageExtractService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResponseUsageExtractService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct ResponseUsageExtractService<S> {
    inner: S,
}

#[derive(Debug)]
pub enum ResponseUsageExtractServiceError<E> {
    Inner(E),
}

impl<E: Display> Display for ResponseUsageExtractServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for ResponseUsageExtractServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<TransformRequestPayload> for ResponseUsageExtractService<S>
where
    S: Service<TransformRequestPayload, Response = TransformResponsePayload> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = UsageExtractedResponse;
    type Error = ResponseUsageExtractServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(ResponseUsageExtractServiceError::Inner)
    }

    fn call(&mut self, request: TransformRequestPayload) -> Self::Future {
        let fut = self.inner.call(request);
        Box::pin(async move {
            let response = fut.await.map_err(ResponseUsageExtractServiceError::Inner)?;
            Ok(attach_usage_extractor(response))
        })
    }
}

enum NonStreamKind {
    OpenAiResponse,
    OpenAiChatCompletions,
    Claude,
    Gemini,
}

enum UsageParser {
    Unsupported,
    NonStream {
        kind: NonStreamKind,
        buffer: Vec<u8>,
    },
    OpenAiResponseSse {
        buffer: Vec<u8>,
        latest: Option<UsageSnapshot>,
    },
    OpenAiChatCompletionsSse {
        buffer: Vec<u8>,
        latest: Option<UsageSnapshot>,
    },
    ClaudeSse {
        buffer: Vec<u8>,
        from_message_start: Option<UsageSnapshot>,
        from_message_delta: Option<UsageSnapshot>,
    },
    GeminiSse {
        buffer: Vec<u8>,
        latest: Option<UsageSnapshot>,
    },
    GeminiNdjson {
        buffer: Vec<u8>,
        latest: Option<UsageSnapshot>,
    },
}

impl UsageParser {
    fn new(operation: OperationFamily, protocol: ProtocolKind) -> Self {
        match (operation, protocol) {
            (OperationFamily::GenerateContent, ProtocolKind::OpenAi) => Self::NonStream {
                kind: NonStreamKind::OpenAiResponse,
                buffer: Vec::new(),
            },
            (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                Self::NonStream {
                    kind: NonStreamKind::OpenAiChatCompletions,
                    buffer: Vec::new(),
                }
            }
            (OperationFamily::GenerateContent, ProtocolKind::Claude) => Self::NonStream {
                kind: NonStreamKind::Claude,
                buffer: Vec::new(),
            },
            (OperationFamily::GenerateContent, ProtocolKind::Gemini)
            | (OperationFamily::GenerateContent, ProtocolKind::GeminiNDJson) => Self::NonStream {
                kind: NonStreamKind::Gemini,
                buffer: Vec::new(),
            },

            (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
                Self::OpenAiResponseSse {
                    buffer: Vec::new(),
                    latest: None,
                }
            }
            (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                Self::OpenAiChatCompletionsSse {
                    buffer: Vec::new(),
                    latest: None,
                }
            }
            (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => Self::ClaudeSse {
                buffer: Vec::new(),
                from_message_start: None,
                from_message_delta: None,
            },
            (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini) => Self::GeminiSse {
                buffer: Vec::new(),
                latest: None,
            },
            (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
                Self::GeminiNdjson {
                    buffer: Vec::new(),
                    latest: None,
                }
            }
            _ => Self::Unsupported,
        }
    }

    fn feed(&mut self, chunk: &[u8]) {
        match self {
            Self::Unsupported => {}
            Self::NonStream { buffer, .. } => buffer.extend_from_slice(chunk),
            Self::OpenAiResponseSse { buffer, latest } => {
                buffer.extend_from_slice(chunk);
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(data) = parse_sse_data(frame.as_slice())
                        && data != "[DONE]"
                        && let Ok(event) = serde_json::from_str::<ResponseStreamEvent>(&data)
                        && let Some(usage) = usage_from_openai_response_stream_event(&event)
                    {
                        *latest = Some(usage);
                    }
                }
            }
            Self::OpenAiChatCompletionsSse { buffer, latest } => {
                buffer.extend_from_slice(chunk);
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(data) = parse_sse_data(frame.as_slice())
                        && data != "[DONE]"
                        && let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(&data)
                        && let Some(usage) = chunk
                            .usage
                            .as_ref()
                            .map(usage_from_openai_chat_completion_usage)
                    {
                        *latest = Some(usage);
                    }
                }
            }
            Self::ClaudeSse {
                buffer,
                from_message_start,
                from_message_delta,
            } => {
                buffer.extend_from_slice(chunk);
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(data) = parse_sse_data(frame.as_slice())
                        && data != "[DONE]"
                        && let Ok(event) =
                            serde_json::from_str::<ClaudeCreateMessageStreamEvent>(&data)
                    {
                        match event {
                            ClaudeCreateMessageStreamEvent::MessageStart(message_start) => {
                                *from_message_start =
                                    Some(usage_from_claude_usage(&message_start.message.usage));
                            }
                            ClaudeCreateMessageStreamEvent::MessageDelta(message_delta) => {
                                *from_message_delta =
                                    Some(usage_from_claude_delta_usage(&message_delta.usage));
                            }
                            _ => {}
                        }
                    }
                }
            }
            Self::GeminiSse { buffer, latest } => {
                buffer.extend_from_slice(chunk);
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(data) = parse_sse_data(frame.as_slice())
                        && data != "[DONE]"
                        && let Ok(chunk) = serde_json::from_str::<
                            gproxy_protocol::gemini::generate_content::response::ResponseBody,
                        >(&data)
                        && let Some(usage) =
                            chunk.usage_metadata.as_ref().map(usage_from_gemini_usage)
                    {
                        *latest = Some(usage);
                    }
                }
            }
            Self::GeminiNdjson { buffer, latest } => {
                buffer.extend_from_slice(chunk);
                while let Some(line) = next_ndjson_line(buffer) {
                    if let Some(usage) = usage_from_gemini_ndjson_line(line.as_slice()) {
                        *latest = Some(usage);
                    }
                }
            }
        }
    }

    fn snapshot(&self) -> Option<UsageSnapshot> {
        match self {
            Self::Unsupported => None,
            Self::NonStream { .. } => None,
            Self::OpenAiResponseSse { latest, .. }
            | Self::OpenAiChatCompletionsSse { latest, .. }
            | Self::GeminiSse { latest, .. }
            | Self::GeminiNdjson { latest, .. } => {
                latest.clone().map(UsageSnapshot::with_derived_totals)
            }
            Self::ClaudeSse {
                from_message_start,
                from_message_delta,
                ..
            } => merge_usage_snapshots(from_message_delta.clone(), from_message_start.clone())
                .map(UsageSnapshot::with_derived_totals),
        }
    }

    fn finish(mut self) -> Option<UsageSnapshot> {
        match &mut self {
            Self::Unsupported => None,
            Self::NonStream { kind, buffer } => {
                usage_from_nonstream_payload(kind, buffer.as_slice())
                    .map(UsageSnapshot::with_derived_totals)
            }
            Self::OpenAiResponseSse { buffer, latest } => {
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(data) = parse_sse_data(frame.as_slice())
                        && data != "[DONE]"
                        && let Ok(event) = serde_json::from_str::<ResponseStreamEvent>(&data)
                        && let Some(usage) = usage_from_openai_response_stream_event(&event)
                    {
                        *latest = Some(usage);
                    }
                }
                latest.clone().map(UsageSnapshot::with_derived_totals)
            }
            Self::OpenAiChatCompletionsSse { buffer, latest } => {
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(data) = parse_sse_data(frame.as_slice())
                        && data != "[DONE]"
                        && let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(&data)
                        && let Some(usage) = chunk
                            .usage
                            .as_ref()
                            .map(usage_from_openai_chat_completion_usage)
                    {
                        *latest = Some(usage);
                    }
                }
                latest.clone().map(UsageSnapshot::with_derived_totals)
            }
            Self::ClaudeSse {
                buffer,
                from_message_start,
                from_message_delta,
            } => {
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(data) = parse_sse_data(frame.as_slice())
                        && data != "[DONE]"
                        && let Ok(event) =
                            serde_json::from_str::<ClaudeCreateMessageStreamEvent>(&data)
                    {
                        match event {
                            ClaudeCreateMessageStreamEvent::MessageStart(message_start) => {
                                *from_message_start =
                                    Some(usage_from_claude_usage(&message_start.message.usage));
                            }
                            ClaudeCreateMessageStreamEvent::MessageDelta(message_delta) => {
                                *from_message_delta =
                                    Some(usage_from_claude_delta_usage(&message_delta.usage));
                            }
                            _ => {}
                        }
                    }
                }

                merge_usage_snapshots(from_message_delta.clone(), from_message_start.clone())
                    .map(UsageSnapshot::with_derived_totals)
            }
            Self::GeminiSse { buffer, latest } => {
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(data) = parse_sse_data(frame.as_slice())
                        && data != "[DONE]"
                        && let Ok(chunk) = serde_json::from_str::<
                            gproxy_protocol::gemini::generate_content::response::ResponseBody,
                        >(&data)
                        && let Some(usage) =
                            chunk.usage_metadata.as_ref().map(usage_from_gemini_usage)
                    {
                        *latest = Some(usage);
                    }
                }
                latest.clone().map(UsageSnapshot::with_derived_totals)
            }
            Self::GeminiNdjson { buffer, latest } => {
                while let Some(line) = next_ndjson_line(buffer) {
                    if let Some(usage) = usage_from_gemini_ndjson_line(line.as_slice()) {
                        *latest = Some(usage);
                    }
                }
                if !buffer.is_empty() {
                    if let Some(usage) = usage_from_gemini_ndjson_line(buffer.as_slice()) {
                        *latest = Some(usage);
                    }
                    buffer.clear();
                }
                latest.clone().map(UsageSnapshot::with_derived_totals)
            }
        }
    }
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

fn parse_sse_data(frame: &[u8]) -> Option<String> {
    if frame.is_empty() {
        return None;
    }

    let text = std::str::from_utf8(frame).ok()?;
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            lines.push(value.trim_start().to_string());
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn next_ndjson_line(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let index = buffer.iter().position(|byte| *byte == b'\n')?;
    let line = buffer[..index].to_vec();
    buffer.drain(..=index);
    Some(line)
}

fn usage_from_nonstream_payload(kind: &NonStreamKind, body: &[u8]) -> Option<UsageSnapshot> {
    if body.is_empty() {
        return None;
    }

    match kind {
        NonStreamKind::OpenAiResponse => {
            serde_json::from_slice::<OpenAiCreateResponseResponse>(body)
                .ok()
                .and_then(|response| match response {
                    OpenAiCreateResponseResponse::Success { body, .. } => {
                        body.usage.as_ref().map(usage_from_openai_response_usage)
                    }
                    OpenAiCreateResponseResponse::Error { .. } => None,
                })
        }
        NonStreamKind::OpenAiChatCompletions => {
            serde_json::from_slice::<OpenAiChatCompletionsResponse>(body)
                .ok()
                .and_then(|response| match response {
                    OpenAiChatCompletionsResponse::Success { body, .. } => body
                        .usage
                        .as_ref()
                        .map(usage_from_openai_chat_completion_usage),
                    OpenAiChatCompletionsResponse::Error { .. } => None,
                })
        }
        NonStreamKind::Claude => serde_json::from_slice::<ClaudeCreateMessageResponse>(body)
            .ok()
            .and_then(|response| match response {
                ClaudeCreateMessageResponse::Success { body, .. } => {
                    Some(usage_from_claude_usage(&body.usage))
                }
                ClaudeCreateMessageResponse::Error { .. } => None,
            }),
        NonStreamKind::Gemini => serde_json::from_slice::<GeminiGenerateContentResponse>(body)
            .ok()
            .and_then(|response| match response {
                GeminiGenerateContentResponse::Success { body, .. } => {
                    body.usage_metadata.as_ref().map(usage_from_gemini_usage)
                }
                GeminiGenerateContentResponse::Error { .. } => None,
            }),
    }
}

fn usage_from_openai_response_stream_event(event: &ResponseStreamEvent) -> Option<UsageSnapshot> {
    match event {
        ResponseStreamEvent::Created { response, .. }
        | ResponseStreamEvent::Queued { response, .. }
        | ResponseStreamEvent::InProgress { response, .. }
        | ResponseStreamEvent::Failed { response, .. }
        | ResponseStreamEvent::Incomplete { response, .. }
        | ResponseStreamEvent::Completed { response, .. } => response
            .usage
            .as_ref()
            .map(usage_from_openai_response_usage),
        _ => None,
    }
}

fn usage_from_gemini_ndjson_line(line: &[u8]) -> Option<UsageSnapshot> {
    if line.is_empty() {
        return None;
    }
    let line = std::str::from_utf8(line).ok()?.trim();
    if line.is_empty() {
        return None;
    }
    serde_json::from_str::<gproxy_protocol::gemini::generate_content::response::ResponseBody>(line)
        .ok()
        .and_then(|chunk| chunk.usage_metadata.as_ref().map(usage_from_gemini_usage))
}

fn usage_from_openai_response_usage(usage: &ResponseUsage) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: Some(usage.input_tokens),
        output_tokens: Some(usage.output_tokens),
        total_tokens: Some(usage.total_tokens),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
        cache_read_input_tokens: Some(usage.input_tokens_details.cached_tokens),
        reasoning_tokens: Some(usage.output_tokens_details.reasoning_tokens),
        thoughts_tokens: None,
        tool_use_prompt_tokens: None,
    }
}

fn usage_from_openai_chat_completion_usage(usage: &CompletionUsage) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: Some(usage.prompt_tokens),
        output_tokens: Some(usage.completion_tokens),
        total_tokens: Some(usage.total_tokens),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
        cache_read_input_tokens: usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens),
        reasoning_tokens: usage
            .completion_tokens_details
            .as_ref()
            .and_then(|details| details.reasoning_tokens),
        thoughts_tokens: None,
        tool_use_prompt_tokens: None,
    }
}

fn usage_from_claude_usage(usage: &BetaUsage) -> UsageSnapshot {
    let input_tokens = usage
        .input_tokens
        .saturating_add(usage.cache_creation_input_tokens)
        .saturating_add(usage.cache_read_input_tokens);

    UsageSnapshot {
        input_tokens: Some(input_tokens),
        output_tokens: Some(usage.output_tokens),
        total_tokens: Some(input_tokens.saturating_add(usage.output_tokens)),
        cache_creation_input_tokens: Some(usage.cache_creation_input_tokens),
        cache_creation_input_tokens_5min: Some(usage.cache_creation.ephemeral_5m_input_tokens),
        cache_creation_input_tokens_1h: Some(usage.cache_creation.ephemeral_1h_input_tokens),
        cache_read_input_tokens: Some(usage.cache_read_input_tokens),
        reasoning_tokens: None,
        thoughts_tokens: None,
        tool_use_prompt_tokens: None,
    }
}

fn usage_from_claude_delta_usage(usage: &BetaMessageDeltaUsage) -> UsageSnapshot {
    let (cache_creation_input_tokens_5min, cache_creation_input_tokens_1h) =
        usage.cache_creation_windows_from_iterations();
    let has_input = usage.input_tokens.is_some()
        || usage.cache_creation_input_tokens.is_some()
        || usage.cache_read_input_tokens.is_some();
    let input_tokens = usage
        .input_tokens
        .unwrap_or(0)
        .saturating_add(usage.cache_creation_input_tokens.unwrap_or(0))
        .saturating_add(usage.cache_read_input_tokens.unwrap_or(0));

    UsageSnapshot {
        input_tokens: has_input.then_some(input_tokens),
        output_tokens: Some(usage.output_tokens),
        total_tokens: has_input.then_some(input_tokens.saturating_add(usage.output_tokens)),
        cache_creation_input_tokens: usage.cache_creation_input_tokens,
        cache_creation_input_tokens_5min,
        cache_creation_input_tokens_1h,
        cache_read_input_tokens: usage.cache_read_input_tokens,
        reasoning_tokens: None,
        thoughts_tokens: None,
        tool_use_prompt_tokens: None,
    }
}

fn merge_usage_snapshots(
    preferred: Option<UsageSnapshot>,
    fallback: Option<UsageSnapshot>,
) -> Option<UsageSnapshot> {
    match (preferred, fallback) {
        (Some(mut preferred), Some(fallback)) => {
            preferred.input_tokens = preferred.input_tokens.or(fallback.input_tokens);
            preferred.output_tokens = preferred.output_tokens.or(fallback.output_tokens);
            preferred.total_tokens = preferred.total_tokens.or(fallback.total_tokens);
            preferred.cache_creation_input_tokens = preferred
                .cache_creation_input_tokens
                .or(fallback.cache_creation_input_tokens);
            preferred.cache_creation_input_tokens_5min = preferred
                .cache_creation_input_tokens_5min
                .or(fallback.cache_creation_input_tokens_5min);
            preferred.cache_creation_input_tokens_1h = preferred
                .cache_creation_input_tokens_1h
                .or(fallback.cache_creation_input_tokens_1h);
            preferred.cache_read_input_tokens = preferred
                .cache_read_input_tokens
                .or(fallback.cache_read_input_tokens);
            preferred.reasoning_tokens = preferred.reasoning_tokens.or(fallback.reasoning_tokens);
            preferred.thoughts_tokens = preferred.thoughts_tokens.or(fallback.thoughts_tokens);
            preferred.tool_use_prompt_tokens = preferred
                .tool_use_prompt_tokens
                .or(fallback.tool_use_prompt_tokens);
            Some(preferred)
        }
        (Some(preferred), None) => Some(preferred),
        (None, Some(fallback)) => Some(fallback),
        (None, None) => None,
    }
}

fn usage_from_gemini_usage(usage: &GeminiUsageMetadata) -> UsageSnapshot {
    let has_input =
        usage.prompt_token_count.is_some() || usage.cached_content_token_count.is_some();
    let input_tokens = usage
        .prompt_token_count
        .unwrap_or(0)
        .saturating_add(usage.cached_content_token_count.unwrap_or(0));

    UsageSnapshot {
        input_tokens: has_input.then_some(input_tokens),
        output_tokens: usage.candidates_token_count,
        total_tokens: usage.total_token_count,
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
        cache_read_input_tokens: usage.cached_content_token_count,
        reasoning_tokens: None,
        thoughts_tokens: usage.thoughts_token_count,
        tool_use_prompt_tokens: usage.tool_use_prompt_token_count,
    }
}

trait ClaudeDeltaCacheCreationWindows {
    fn cache_creation_windows_from_iterations(&self) -> (Option<u64>, Option<u64>);
}

impl ClaudeDeltaCacheCreationWindows for BetaMessageDeltaUsage {
    fn cache_creation_windows_from_iterations(&self) -> (Option<u64>, Option<u64>) {
        let Some(iterations) = self.iterations.as_ref() else {
            return (None, None);
        };
        let Some(last) = iterations.last() else {
            return (None, None);
        };
        match last {
            BetaIterationUsage::Message(item) => (
                Some(item.cache_creation.ephemeral_5m_input_tokens),
                Some(item.cache_creation.ephemeral_1h_input_tokens),
            ),
            BetaIterationUsage::Compaction(item) => (
                Some(item.cache_creation.ephemeral_5m_input_tokens),
                Some(item.cache_creation.ephemeral_1h_input_tokens),
            ),
        }
    }
}
