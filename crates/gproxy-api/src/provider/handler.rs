use std::sync::{Arc, Mutex};

use async_stream::try_stream;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;

use gproxy_sdk::provider::engine::{ExecuteBody, ExecuteRequest, Usage};
use gproxy_server::AppState;
use gproxy_server::middleware::classify::Classification;
use gproxy_server::middleware::kinds::{OperationFamily, ProtocolKind};
use gproxy_server::middleware::model_alias::ResolvedAlias;
use gproxy_server::middleware::request_model::ExtractedModel;

use crate::auth::authenticate_user;
use crate::error::HttpError;

/// Proxy handler for provider-scoped routes: `/{provider}/v1/...`
pub async fn proxy(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    request: Request,
) -> Result<Response, HttpError> {
    let headers = request.headers().clone();
    let user_key = authenticate_user(&headers, &state)?;

    // Extract classification from middleware extensions
    let classification = request
        .extensions()
        .get::<Classification>()
        .cloned()
        .ok_or_else(|| HttpError::bad_request("request not classified"))?;

    // Extract model from middleware extensions
    let model = request
        .extensions()
        .get::<ExtractedModel>()
        .and_then(|m| m.0.clone());

    // Check alias resolution
    let resolved_alias = request.extensions().get::<ResolvedAlias>().cloned();
    let (effective_provider, effective_model) = if let Some(alias) = &resolved_alias
        && alias.provider_name.is_some()
    {
        (
            alias.provider_name.clone().unwrap_or(provider_name.clone()),
            alias.model_id.clone().or(model.clone()),
        )
    } else {
        (provider_name.clone(), model.clone())
    };

    // Check permission (whitelist) — provider_id 0 means check all-provider rules
    if let Some(ref m) = effective_model
        && !state.check_model_permission(user_key.user_id, 0, m)
    {
        return Err(HttpError::forbidden("model not authorized for this user"));
    }

    // Check rate limit
    if let Some(ref m) = effective_model
        && let Err(rejection) = state.check_rate_limit(user_key.user_id, m)
    {
        return Err(HttpError::too_many_requests(format!("{rejection:?}")));
    }

    // Map classification to SDK operation/protocol strings
    let operation = operation_to_string(classification.operation);
    let protocol = protocol_to_string(classification.protocol);

    // Collect body
    let body = axum::body::to_bytes(request.into_body(), 50 * 1024 * 1024)
        .await
        .map_err(|_| HttpError::bad_request("failed to read request body"))?;

    let result = state
        .engine()
        .execute(ExecuteRequest {
            provider: effective_provider,
            operation: operation.clone(),
            protocol: protocol.clone(),
            body: body.to_vec(),
            headers,
            model: effective_model.clone(),
        })
        .await?;

    // Record request for rate limiting
    if let Some(ref m) = effective_model {
        state.record_request(user_key.user_id, m);
    }

    // Record usage via storage write channel
    if let Some(ref usage) = result.usage {
        let model_info = effective_model.as_deref().and_then(|m| state.find_model(m));
        let cost = model_info
            .map(|info| compute_cost(usage, &info))
            .unwrap_or(0.0);
        if cost > 0.0 {
            state.add_cost_usage(user_key.user_id, cost);
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let _ = state
            .storage_writes()
            .enqueue(gproxy_storage::StorageWriteEvent::UpsertUsage(
                gproxy_storage::UsageWrite {
                    downstream_trace_id: None,
                    at_unix_ms: now_ms,
                    provider_id: None,
                    credential_id: None,
                    user_id: Some(user_key.user_id),
                    user_key_id: Some(user_key.id),
                    operation: operation.clone(),
                    protocol: protocol.clone(),
                    model: effective_model.clone(),
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    cache_read_input_tokens: usage.cache_read_input_tokens,
                    cache_creation_input_tokens: usage.cache_creation_input_tokens,
                    cache_creation_input_tokens_5min: usage.cache_creation_input_tokens_5min,
                    cache_creation_input_tokens_1h: usage.cache_creation_input_tokens_1h,
                },
            ))
            .await;
    }

    let response_body = match result.body {
        ExecuteBody::Full(body) => Body::from(body),
        ExecuteBody::Stream(stream)
            if matches!(
                classification.operation,
                OperationFamily::StreamGenerateContent
            ) =>
        {
            Body::from_stream(stream_with_usage_tracking(
                state.clone(),
                user_key.user_id,
                user_key.id,
                effective_model.clone(),
                operation.clone(),
                protocol.clone(),
                stream,
            ))
        }
        ExecuteBody::Stream(stream) => Body::from_stream(stream),
    };

    let mut response = Response::builder()
        .status(result.status)
        .body(response_body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());

    *response.headers_mut() = result.headers;
    Ok(response)
}

/// Proxy handler for unscoped routes: `/v1/...`
pub async fn proxy_unscoped(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Result<Response, HttpError> {
    let headers = request.headers().clone();
    let user_key = authenticate_user(&headers, &state)?;

    let model = request
        .extensions()
        .get::<ExtractedModel>()
        .and_then(|m| m.0.clone());

    let Some(model_name) = &model else {
        return Err(HttpError::bad_request("missing model in request"));
    };

    let classification = request
        .extensions()
        .get::<Classification>()
        .cloned()
        .ok_or_else(|| HttpError::bad_request("request not classified"))?;

    let body = axum::body::to_bytes(request.into_body(), 50 * 1024 * 1024)
        .await
        .map_err(|_| HttpError::bad_request("failed to read request body"))?;

    // Resolve provider: alias → prefix → error
    let (target_provider, target_model) = if let Some(alias) = state.resolve_model_alias(model_name)
    {
        (alias.provider_name, alias.model_id)
    } else if let Some((provider, model)) = model_name.split_once('/') {
        (provider.to_string(), model.to_string())
    } else {
        return Err(HttpError::bad_request(
            "model must have provider prefix (provider/model) or match an alias",
        ));
    };

    let operation = operation_to_string(classification.operation);
    let protocol = protocol_to_string(classification.protocol);

    let result = state
        .engine()
        .execute(ExecuteRequest {
            provider: target_provider,
            operation: operation.clone(),
            protocol: protocol.clone(),
            body: body.to_vec(),
            headers,
            model: Some(target_model.clone()),
        })
        .await?;

    let response_body = match result.body {
        ExecuteBody::Full(body) => Body::from(body),
        ExecuteBody::Stream(stream)
            if matches!(
                classification.operation,
                OperationFamily::StreamGenerateContent
            ) =>
        {
            Body::from_stream(stream_with_usage_tracking(
                state.clone(),
                user_key.user_id,
                user_key.id,
                Some(target_model.clone()),
                operation,
                protocol,
                stream,
            ))
        }
        ExecuteBody::Stream(stream) => Body::from_stream(stream),
    };

    let mut response = Response::builder()
        .status(result.status)
        .body(response_body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    *response.headers_mut() = result.headers;
    Ok(response)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn operation_to_string(op: OperationFamily) -> String {
    match op {
        OperationFamily::ModelList => "model_list".to_string(),
        OperationFamily::ModelGet => "model_get".to_string(),
        OperationFamily::GenerateContent => "generate_content".to_string(),
        OperationFamily::StreamGenerateContent => "stream_generate_content".to_string(),
        OperationFamily::CountToken => "count_tokens".to_string(),
        OperationFamily::Compact => "compact".to_string(),
        OperationFamily::Embedding => "embeddings".to_string(),
        OperationFamily::CreateImage => "create_image".to_string(),
        OperationFamily::StreamCreateImage => "stream_create_image".to_string(),
        OperationFamily::CreateImageEdit => "create_image_edit".to_string(),
        OperationFamily::StreamCreateImageEdit => "stream_create_image_edit".to_string(),
        OperationFamily::OpenAiResponseWebSocket => "openai_response_websocket".to_string(),
        OperationFamily::GeminiLive => "gemini_live".to_string(),
    }
}

fn protocol_to_string(proto: ProtocolKind) -> String {
    match proto {
        ProtocolKind::OpenAi => "openai_response".to_string(),
        ProtocolKind::OpenAiChatCompletion => "openai_chat_completions".to_string(),
        ProtocolKind::Claude => "claude".to_string(),
        ProtocolKind::Gemini => "gemini".to_string(),
        ProtocolKind::GeminiNDJson => "gemini_ndjson".to_string(),
    }
}

fn compute_cost(usage: &Usage, model: &gproxy_server::MemoryModel) -> f64 {
    let mut cost = 0.0;
    // Per-call fixed price
    if let Some(price) = model.price_each_call {
        cost += price;
    }
    if let (Some(tokens), Some(price)) = (usage.input_tokens, model.price_input_tokens) {
        cost += tokens as f64 * price / 1_000_000.0;
    }
    if let (Some(tokens), Some(price)) = (usage.output_tokens, model.price_output_tokens) {
        cost += tokens as f64 * price / 1_000_000.0;
    }
    if let (Some(tokens), Some(price)) = (
        usage.cache_read_input_tokens,
        model.price_cache_read_input_tokens,
    ) {
        cost += tokens as f64 * price / 1_000_000.0;
    }
    if let (Some(tokens), Some(price)) = (
        usage.cache_creation_input_tokens,
        model.price_cache_creation_input_tokens,
    ) {
        cost += tokens as f64 * price / 1_000_000.0;
    }
    if let (Some(tokens), Some(price)) = (
        usage.cache_creation_input_tokens_5min,
        model.price_cache_creation_input_tokens_5min,
    ) {
        cost += tokens as f64 * price / 1_000_000.0;
    }
    if let (Some(tokens), Some(price)) = (
        usage.cache_creation_input_tokens_1h,
        model.price_cache_creation_input_tokens_1h,
    ) {
        cost += tokens as f64 * price / 1_000_000.0;
    }
    cost
}

fn stream_with_usage_tracking(
    state: Arc<AppState>,
    user_id: i64,
    user_key_id: i64,
    model: Option<String>,
    operation: String,
    protocol: String,
    mut stream: gproxy_sdk::provider::engine::ExecuteBodyStream,
) -> impl futures_util::Stream<
    Item = Result<bytes::Bytes, gproxy_sdk::provider::response::UpstreamError>,
> + Send {
    let recorder = StreamUsageRecorder::new(
        state.clone(),
        user_id,
        user_key_id,
        model.clone(),
        operation.clone(),
        protocol.clone(),
    );

    try_stream! {
        let mut decoder = UsageChunkDecoder::new(&protocol);

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            for json_chunk in decoder.push_chunk(&chunk) {
                recorder.observe_json_chunk(&json_chunk);
            }
            yield chunk;
        }

        for json_chunk in decoder.finish() {
            recorder.observe_json_chunk(&json_chunk);
        }

        if let Some(usage) = recorder.finish_completed() {
            record_stream_usage(state, user_id, user_key_id, model, operation, protocol, usage).await;
        }
    }
}

#[derive(Clone)]
struct StreamUsageRecorderContext {
    state: Arc<AppState>,
    user_id: i64,
    user_key_id: i64,
    model: Option<String>,
    operation: String,
    protocol: String,
}

#[derive(Default)]
struct StreamUsageRecorderState {
    finalized: bool,
    last_usage: Option<Usage>,
    partial_usage: Usage,
    partial_output: String,
}

struct StreamUsageRecorder {
    ctx: StreamUsageRecorderContext,
    state: Arc<Mutex<StreamUsageRecorderState>>,
}

impl StreamUsageRecorder {
    fn new(
        state: Arc<AppState>,
        user_id: i64,
        user_key_id: i64,
        model: Option<String>,
        operation: String,
        protocol: String,
    ) -> Self {
        Self {
            ctx: StreamUsageRecorderContext {
                state,
                user_id,
                user_key_id,
                model,
                operation,
                protocol,
            },
            state: Arc::new(Mutex::new(StreamUsageRecorderState::default())),
        }
    }

    fn observe_json_chunk(&self, json_chunk: &[u8]) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        if state.finalized {
            return;
        }

        if let Some(usage) =
            gproxy_sdk::provider::usage::extract_stream_usage(&self.ctx.protocol, json_chunk)
        {
            merge_usage(&mut state.partial_usage, &usage);
            state.last_usage = Some(usage);
        } else if let Some(usage) = extract_partial_stream_usage(&self.ctx.protocol, json_chunk) {
            merge_usage(&mut state.partial_usage, &usage);
        }

        if let Some(text) = extract_partial_output_text(&self.ctx.protocol, json_chunk) {
            state.partial_output.push_str(&text);
        }
    }

    fn finish_completed(&self) -> Option<Usage> {
        let mut state = self.state.lock().ok()?;
        state.finalized = true;
        state.last_usage.clone()
    }

    fn take_interrupted_usage(&self) -> Option<Usage> {
        let mut state = self.state.lock().ok()?;
        if state.finalized {
            return None;
        }
        state.finalized = true;

        if let Some(usage) = state.last_usage.clone() {
            return Some(usage);
        }

        let has_partial_usage = usage_has_any_value(&state.partial_usage);
        if !has_partial_usage && state.partial_output.is_empty() {
            return None;
        }

        let mut usage = state.partial_usage.clone();
        if let Some(model) = self.ctx.model.as_deref()
            && !state.partial_output.is_empty()
        {
            let estimated = gproxy_sdk::provider::count_tokens::estimate_partial_usage(
                usage.input_tokens,
                &state.partial_output,
                model,
            );
            usage.output_tokens = estimated.output_tokens;
            if usage.input_tokens.is_none() {
                usage.input_tokens = estimated.input_tokens;
            }
        }

        usage_has_any_value(&usage).then_some(usage)
    }
}

impl Drop for StreamUsageRecorder {
    fn drop(&mut self) {
        let Some(usage) = self.take_interrupted_usage() else {
            return;
        };

        let ctx = self.ctx.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                record_stream_usage(
                    ctx.state,
                    ctx.user_id,
                    ctx.user_key_id,
                    ctx.model,
                    ctx.operation,
                    ctx.protocol,
                    usage,
                )
                .await;
            });
        }
    }
}

async fn record_stream_usage(
    state: Arc<AppState>,
    user_id: i64,
    user_key_id: i64,
    model: Option<String>,
    operation: String,
    protocol: String,
    usage: Usage,
) {
    let model_info = model.as_deref().and_then(|m| state.find_model(m));
    let cost = model_info
        .map(|info| compute_cost(&usage, &info))
        .unwrap_or(0.0);
    if cost > 0.0 {
        state.add_cost_usage(user_id, cost);
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let _ = state
        .storage_writes()
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertUsage(
            gproxy_storage::UsageWrite {
                downstream_trace_id: None,
                at_unix_ms: now_ms,
                provider_id: None,
                credential_id: None,
                user_id: Some(user_id),
                user_key_id: Some(user_key_id),
                operation,
                protocol,
                model,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cache_read_input_tokens: usage.cache_read_input_tokens,
                cache_creation_input_tokens: usage.cache_creation_input_tokens,
                cache_creation_input_tokens_5min: usage.cache_creation_input_tokens_5min,
                cache_creation_input_tokens_1h: usage.cache_creation_input_tokens_1h,
            },
        ))
        .await;
}

enum UsageChunkDecoder {
    Sse(gproxy_sdk::protocol::stream::SseToNdjsonRewriter),
    Ndjson(Vec<u8>),
    Disabled,
}

impl UsageChunkDecoder {
    fn new(protocol: &str) -> Self {
        match protocol {
            "claude" | "openai_response" | "openai_chat_completions" | "gemini" => {
                Self::Sse(gproxy_sdk::protocol::stream::SseToNdjsonRewriter::default())
            }
            "gemini_ndjson" => Self::Ndjson(Vec::new()),
            _ => Self::Disabled,
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            Self::Sse(rewriter) => split_usage_lines(&rewriter.push_chunk(chunk), &mut out),
            Self::Ndjson(pending) => {
                pending.extend_from_slice(chunk);
                drain_usage_lines(pending, &mut out);
            }
            Self::Disabled => {}
        }
        out
    }

    fn finish(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            Self::Sse(rewriter) => split_usage_lines(&rewriter.finish(), &mut out),
            Self::Ndjson(pending) => {
                if !pending.is_empty() {
                    let mut line = std::mem::take(pending);
                    if line.last().copied() == Some(b'\r') {
                        line.pop();
                    }
                    if !line.is_empty() {
                        out.push(line);
                    }
                }
            }
            Self::Disabled => {}
        }
        out
    }
}

use gproxy_sdk::protocol::stream::{
    drain_lines as drain_usage_lines, split_lines as split_usage_lines,
};

fn usage_has_any_value(usage: &Usage) -> bool {
    usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.cache_read_input_tokens.is_some()
        || usage.cache_creation_input_tokens.is_some()
        || usage.cache_creation_input_tokens_5min.is_some()
        || usage.cache_creation_input_tokens_1h.is_some()
}

fn merge_usage(dst: &mut Usage, src: &Usage) {
    if src.input_tokens.is_some() {
        dst.input_tokens = src.input_tokens;
    }
    if src.output_tokens.is_some() {
        dst.output_tokens = src.output_tokens;
    }
    if src.cache_read_input_tokens.is_some() {
        dst.cache_read_input_tokens = src.cache_read_input_tokens;
    }
    if src.cache_creation_input_tokens.is_some() {
        dst.cache_creation_input_tokens = src.cache_creation_input_tokens;
    }
    if src.cache_creation_input_tokens_5min.is_some() {
        dst.cache_creation_input_tokens_5min = src.cache_creation_input_tokens_5min;
    }
    if src.cache_creation_input_tokens_1h.is_some() {
        dst.cache_creation_input_tokens_1h = src.cache_creation_input_tokens_1h;
    }
}

fn extract_partial_stream_usage(protocol: &str, json_chunk: &[u8]) -> Option<Usage> {
    match protocol {
        "claude" => {
            use gproxy_sdk::protocol::claude::create_message::stream::ClaudeStreamEvent;

            let event: ClaudeStreamEvent = serde_json::from_slice(json_chunk).ok()?;
            match event {
                ClaudeStreamEvent::MessageStart { message } => Some(Usage {
                    input_tokens: i64::try_from(message.usage.input_tokens).ok(),
                    output_tokens: i64::try_from(message.usage.output_tokens).ok(),
                    cache_read_input_tokens: message.usage.cache_read_input_tokens.try_into().ok(),
                    cache_creation_input_tokens: message
                        .usage
                        .cache_creation_input_tokens
                        .try_into()
                        .ok(),
                    cache_creation_input_tokens_5min: None,
                    cache_creation_input_tokens_1h: None,
                }),
                _ => None,
            }
        }
        "openai_response" => {
            use gproxy_sdk::protocol::openai::create_response::stream::ResponseStreamEvent;

            let event: ResponseStreamEvent = serde_json::from_slice(json_chunk).ok()?;
            let response = match event {
                ResponseStreamEvent::Created { response, .. }
                | ResponseStreamEvent::Queued { response, .. }
                | ResponseStreamEvent::InProgress { response, .. }
                | ResponseStreamEvent::Completed { response, .. }
                | ResponseStreamEvent::Incomplete { response, .. }
                | ResponseStreamEvent::Failed { response, .. } => response,
                _ => return None,
            };
            let usage = response.usage?;
            Some(Usage {
                input_tokens: i64::try_from(usage.input_tokens).ok(),
                output_tokens: i64::try_from(usage.output_tokens).ok(),
                cache_read_input_tokens: i64::try_from(usage.input_tokens_details.cached_tokens)
                    .ok(),
                cache_creation_input_tokens: None,
                cache_creation_input_tokens_5min: None,
                cache_creation_input_tokens_1h: None,
            })
        }
        _ => None,
    }
}

fn extract_partial_output_text(protocol: &str, json_chunk: &[u8]) -> Option<String> {
    match protocol {
        "claude" => {
            use gproxy_sdk::protocol::claude::create_message::stream::{
                BetaRawContentBlockDelta, ClaudeStreamEvent,
            };

            let event: ClaudeStreamEvent = serde_json::from_slice(json_chunk).ok()?;
            match event {
                ClaudeStreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    BetaRawContentBlockDelta::Text { text } => Some(text),
                    BetaRawContentBlockDelta::Thinking { thinking } => Some(thinking),
                    BetaRawContentBlockDelta::InputJson { partial_json } => Some(partial_json),
                    BetaRawContentBlockDelta::Compaction { content } => content,
                    BetaRawContentBlockDelta::Citations { .. }
                    | BetaRawContentBlockDelta::Signature { .. } => None,
                },
                _ => None,
            }
        }
        "openai_chat_completions" => {
            use gproxy_sdk::protocol::openai::create_chat_completions::stream::ChatCompletionChunk;

            let chunk: ChatCompletionChunk = serde_json::from_slice(json_chunk).ok()?;
            let mut parts = Vec::new();
            for choice in chunk.choices {
                let delta = choice.delta;
                if let Some(text) = delta.content
                    && !text.is_empty()
                {
                    parts.push(text);
                }
                if let Some(text) = delta.reasoning_content
                    && !text.is_empty()
                {
                    parts.push(text);
                }
                if let Some(text) = delta.refusal
                    && !text.is_empty()
                {
                    parts.push(text);
                }
                if let Some(function_call) = delta.function_call {
                    if let Some(name) = function_call.name
                        && !name.is_empty()
                    {
                        parts.push(name);
                    }
                    if let Some(arguments) = function_call.arguments
                        && !arguments.is_empty()
                    {
                        parts.push(arguments);
                    }
                }
                if let Some(tool_calls) = delta.tool_calls {
                    for tool_call in tool_calls {
                        if let Some(function) = tool_call.function {
                            if let Some(name) = function.name
                                && !name.is_empty()
                            {
                                parts.push(name);
                            }
                            if let Some(arguments) = function.arguments
                                && !arguments.is_empty()
                            {
                                parts.push(arguments);
                            }
                        }
                    }
                }
            }
            (!parts.is_empty()).then_some(parts.join("\n"))
        }
        "openai_response" => {
            use gproxy_sdk::protocol::openai::create_response::stream::ResponseStreamEvent;

            let event: ResponseStreamEvent = serde_json::from_slice(json_chunk).ok()?;
            match event {
                ResponseStreamEvent::OutputTextDelta { delta, .. }
                | ResponseStreamEvent::RefusalDelta { delta, .. }
                | ResponseStreamEvent::ReasoningTextDelta { delta, .. }
                | ResponseStreamEvent::ReasoningSummaryTextDelta { delta, .. }
                | ResponseStreamEvent::FunctionCallArgumentsDelta { delta, .. }
                | ResponseStreamEvent::CustomToolCallInputDelta { delta, .. }
                | ResponseStreamEvent::McpCallArgumentsDelta { delta, .. }
                | ResponseStreamEvent::AudioTranscriptDelta { delta, .. }
                | ResponseStreamEvent::CodeInterpreterCallCodeDelta { delta, .. } => {
                    (!delta.is_empty()).then_some(delta)
                }
                _ => None,
            }
        }
        "gemini" | "gemini_ndjson" => {
            use gproxy_sdk::protocol::gemini::generate_content::response::ResponseBody;

            let chunk: ResponseBody = serde_json::from_slice(json_chunk).ok()?;
            let mut parts = Vec::new();
            if let Some(candidates) = chunk.candidates {
                for candidate in candidates {
                    if let Some(content) = candidate.content {
                        for part in content.parts {
                            if let Some(text) = part.text
                                && !text.is_empty()
                            {
                                parts.push(text);
                            }
                            if let Some(function_call) = part.function_call {
                                if !function_call.name.is_empty() {
                                    parts.push(function_call.name);
                                }
                                if let Some(args) = function_call.args
                                    && let Ok(json) = serde_json::to_string(&args)
                                    && !json.is_empty()
                                {
                                    parts.push(json);
                                }
                            }
                        }
                    }
                    if let Some(message) = candidate.finish_message
                        && !message.is_empty()
                    {
                        parts.push(message);
                    }
                }
            }
            if let Some(status) = chunk.model_status
                && let Some(message) = status.message
                && !message.is_empty()
            {
                parts.push(message);
            }
            (!parts.is_empty()).then_some(parts.join("\n"))
        }
        _ => None,
    }
}
