use std::sync::{Arc, Mutex};

use async_stream::try_stream;
use axum::body::Body;
use axum::extract::{Extension, Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;

use gproxy_sdk::provider::engine::{
    ExecuteBody, ExecuteRequest, ExecuteResult, UpstreamRequestMeta, Usage,
};
use gproxy_server::middleware::classify::Classification;
use gproxy_server::middleware::model_alias::ResolvedAlias;
use gproxy_server::middleware::request_model::ExtractedModel;
use gproxy_server::{AppState, OperationFamily, ProtocolKind};

use crate::auth::AuthenticatedUser;
use crate::error::HttpError;

/// Proxy handler for provider-scoped routes: `/{provider}/v1/...`
pub async fn proxy(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    Extension(authenticated): Extension<AuthenticatedUser>,
    request: Request,
) -> Result<Response, HttpError> {
    let start = std::time::Instant::now();
    let trace_id = generate_trace_id();
    let req_method = request.method().to_string();
    let req_path = request.uri().path().to_string();
    let req_query = request.uri().query().map(String::from);
    let headers = request.headers().clone();
    let req_headers_json = headers_to_json(&headers);
    let user_key = authenticated.0;

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

    // Check permission (whitelist)
    if let Some(ref m) = effective_model
        && !state.check_model_permission(user_key.user_id, &effective_provider, m)
    {
        return Err(HttpError::forbidden("model not authorized for this user"));
    }

    // Map classification to SDK operation/protocol strings
    let operation = classification.operation;
    let protocol = classification.protocol;

    // Collect body
    let body = axum::body::to_bytes(request.into_body(), 50 * 1024 * 1024)
        .await
        .map_err(|_| HttpError::bad_request("failed to read request body"))?;
    let req_body = build_execute_body(
        classification.operation,
        &req_path,
        req_query.as_deref(),
        body.to_vec(),
    );

    // Check rate limit after buffering so declared token budgets can be enforced.
    if let Some(ref m) = effective_model
        && let Err(rejection) = state.check_rate_limit_request(
            user_key.user_id,
            m,
            extract_requested_total_tokens(operation, protocol, &req_body),
        )
    {
        return Err(HttpError::too_many_requests(format!("{rejection:?}")));
    }

    let result = state
        .engine()
        .execute(ExecuteRequest {
            provider: effective_provider.clone(),
            operation,
            protocol,
            body: req_body.clone(),
            headers,
            model: effective_model.clone(),
        })
        .await?;

    // File affinity: bind file_id to credential on upload, unbind on delete
    bind_file_affinity_if_applicable(&state, &classification, &result, &effective_provider);

    // Build usage context (shared by record_usage and stream_with_usage_tracking)
    let usage_ctx = UsageRecordContext {
        state: state.clone(),
        user_id: user_key.user_id,
        user_key_id: user_key.id,
        provider_name: effective_provider.clone(),
        credential_index: Some(result.credential_index),
        precomputed_cost: result.cost,
        model: effective_model.clone(),
        billing_context: result.billing_context.clone(),
        operation,
        protocol,
        downstream_trace_id: Some(trace_id),
    };

    // Record usage via storage write channel
    if let Some(ref usage) = result.usage {
        record_usage(&usage_ctx, usage).await;
    }

    // Record upstream log
    record_upstream_log(&state, trace_id, &effective_provider, result.meta.as_ref()).await;

    let resp_status = result.status;
    let resp_headers_json = headers_to_json(&result.headers);

    let response_body = match result.body {
        ExecuteBody::Full(ref resp_body) => {
            // Record downstream log (full response available)
            let latency_ms = start.elapsed().as_millis() as u64;
            tracing::info!(
                trace_id,
                method = %req_method,
                path = %req_path,
                status = resp_status,
                latency_ms,
                "downstream"
            );
            record_downstream_log(
                &state,
                trace_id,
                user_key.user_id,
                user_key.id,
                &req_method,
                &req_path,
                req_query.as_deref(),
                &req_headers_json,
                Some(&req_body),
                Some(resp_status as i32),
                &resp_headers_json,
                Some(resp_body),
            )
            .await;
            Body::from(resp_body.clone())
        }
        ExecuteBody::Stream(stream) if classification.operation.is_stream() => {
            // For streaming: log downstream immediately (response body not captured)
            let latency_ms = start.elapsed().as_millis() as u64;
            tracing::info!(
                trace_id,
                method = %req_method,
                path = %req_path,
                status = resp_status,
                latency_ms,
                stream = true,
                "downstream"
            );
            record_downstream_log(
                &state,
                trace_id,
                user_key.user_id,
                user_key.id,
                &req_method,
                &req_path,
                req_query.as_deref(),
                &req_headers_json,
                Some(&req_body),
                Some(resp_status as i32),
                &resp_headers_json,
                None,
            )
            .await;
            Body::from_stream(stream_with_usage_tracking(usage_ctx.clone(), stream))
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
    Extension(authenticated): Extension<AuthenticatedUser>,
    request: Request,
) -> Result<Response, HttpError> {
    let start = std::time::Instant::now();
    let trace_id = generate_trace_id();
    let req_method = request.method().to_string();
    let req_path = request.uri().path().to_string();
    let req_query = request.uri().query().map(String::from);
    let headers = request.headers().clone();
    let req_headers_json = headers_to_json(&headers);
    let user_key = authenticated.0;

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
    let req_body = build_execute_body(
        classification.operation,
        &req_path,
        req_query.as_deref(),
        body.to_vec(),
    );

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

    // Check permission (whitelist)
    if !state.check_model_permission(user_key.user_id, &target_provider, &target_model) {
        return Err(HttpError::forbidden("model not authorized for this user"));
    }

    let operation = classification.operation;
    let protocol = classification.protocol;
    let req_body = normalize_unscoped_request_body(operation, protocol, req_body, &target_model);

    // Check rate limit after rewriting the request body to the canonical target model.
    if let Err(rejection) = state.check_rate_limit_request(
        user_key.user_id,
        &target_model,
        extract_requested_total_tokens(operation, protocol, &req_body),
    ) {
        return Err(HttpError::too_many_requests(format!("{rejection:?}")));
    }

    let result = state
        .engine()
        .execute(ExecuteRequest {
            provider: target_provider.clone(),
            operation,
            protocol,
            body: req_body.clone(),
            headers,
            model: Some(target_model.clone()),
        })
        .await?;

    let usage_ctx = UsageRecordContext {
        state: state.clone(),
        user_id: user_key.user_id,
        user_key_id: user_key.id,
        provider_name: target_provider.clone(),
        credential_index: Some(result.credential_index),
        precomputed_cost: result.cost,
        model: Some(target_model.clone()),
        billing_context: result.billing_context.clone(),
        operation,
        protocol,
        downstream_trace_id: Some(trace_id),
    };

    // Record usage via storage write channel
    if let Some(ref usage) = result.usage {
        record_usage(&usage_ctx, usage).await;
    }

    // Record upstream log
    record_upstream_log(&state, trace_id, &target_provider, result.meta.as_ref()).await;

    let resp_status = result.status;
    let resp_headers_json = headers_to_json(&result.headers);

    let response_body = match result.body {
        ExecuteBody::Full(ref resp_body) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            tracing::info!(
                trace_id,
                method = %req_method,
                path = %req_path,
                status = resp_status,
                latency_ms,
                "downstream"
            );
            record_downstream_log(
                &state,
                trace_id,
                user_key.user_id,
                user_key.id,
                &req_method,
                &req_path,
                req_query.as_deref(),
                &req_headers_json,
                Some(&req_body),
                Some(resp_status as i32),
                &resp_headers_json,
                Some(resp_body),
            )
            .await;
            Body::from(resp_body.clone())
        }
        ExecuteBody::Stream(stream) if classification.operation.is_stream() => {
            let latency_ms = start.elapsed().as_millis() as u64;
            tracing::info!(
                trace_id,
                method = %req_method,
                path = %req_path,
                status = resp_status,
                latency_ms,
                stream = true,
                "downstream"
            );
            record_downstream_log(
                &state,
                trace_id,
                user_key.user_id,
                user_key.id,
                &req_method,
                &req_path,
                req_query.as_deref(),
                &req_headers_json,
                Some(&req_body),
                Some(resp_status as i32),
                &resp_headers_json,
                None,
            )
            .await;
            Body::from_stream(stream_with_usage_tracking(usage_ctx.clone(), stream))
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

/// Proxy handler for unscoped file operations: `/v1/files/...`
///
/// File endpoints have no model in the request. Provider is resolved from
/// the `X-Provider` header.
pub async fn proxy_unscoped_files(
    State(state): State<Arc<AppState>>,
    Extension(authenticated): Extension<AuthenticatedUser>,
    request: Request,
) -> Result<Response, HttpError> {
    let start = std::time::Instant::now();
    let trace_id = generate_trace_id();
    let req_method = request.method().to_string();
    let req_path = request.uri().path().to_string();
    let req_query = request.uri().query().map(String::from);
    let headers = request.headers().clone();
    let req_headers_json = headers_to_json(&headers);
    let user_key = authenticated.0;

    // Resolve provider from X-Provider header
    let target_provider = headers
        .get("x-provider")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .ok_or_else(|| {
            HttpError::bad_request("X-Provider header required for unscoped file operations")
        })?;

    let classification = request
        .extensions()
        .get::<Classification>()
        .cloned()
        .ok_or_else(|| HttpError::bad_request("request not classified"))?;

    let body = axum::body::to_bytes(request.into_body(), 50 * 1024 * 1024)
        .await
        .map_err(|_| HttpError::bad_request("failed to read request body"))?;
    let req_body = build_execute_body(
        classification.operation,
        &req_path,
        req_query.as_deref(),
        body.to_vec(),
    );

    let operation = classification.operation;
    let protocol = classification.protocol;

    let result = state
        .engine()
        .execute(ExecuteRequest {
            provider: target_provider.clone(),
            operation,
            protocol,
            body: req_body.clone(),
            headers,
            model: None,
        })
        .await?;

    // File affinity: bind file_id to credential on upload, unbind on delete
    bind_file_affinity_if_applicable(&state, &classification, &result, &target_provider);

    // Record usage via storage write channel
    if let Some(ref usage) = result.usage {
        let usage_ctx = UsageRecordContext {
            state: state.clone(),
            user_id: user_key.user_id,
            user_key_id: user_key.id,
            provider_name: target_provider.clone(),
            credential_index: Some(result.credential_index),
            precomputed_cost: result.cost,
            model: None,
            billing_context: result.billing_context.clone(),
            operation,
            protocol,
            downstream_trace_id: Some(trace_id),
        };
        record_usage(&usage_ctx, usage).await;
    }

    // Record upstream log
    record_upstream_log(&state, trace_id, &target_provider, result.meta.as_ref()).await;

    let resp_status = result.status;
    let resp_headers_json = headers_to_json(&result.headers);

    let response_body = match result.body {
        ExecuteBody::Full(ref resp_body) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            tracing::info!(
                trace_id,
                method = %req_method,
                path = %req_path,
                status = resp_status,
                latency_ms,
                "downstream"
            );
            record_downstream_log(
                &state,
                trace_id,
                user_key.user_id,
                user_key.id,
                &req_method,
                &req_path,
                req_query.as_deref(),
                &req_headers_json,
                Some(&req_body),
                Some(resp_status as i32),
                &resp_headers_json,
                Some(resp_body),
            )
            .await;
            Body::from(resp_body.clone())
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

/// Shared context for usage recording, avoids passing 8+ args.
#[derive(Clone)]
pub(crate) struct UsageRecordContext {
    pub state: Arc<AppState>,
    pub user_id: i64,
    pub user_key_id: i64,
    pub provider_name: String,
    pub credential_index: Option<usize>,
    pub precomputed_cost: Option<f64>,
    pub model: Option<String>,
    pub billing_context: Option<gproxy_sdk::provider::billing::BillingContext>,
    pub operation: OperationFamily,
    pub protocol: ProtocolKind,
    pub downstream_trace_id: Option<i64>,
}

/// Record usage (cost tracking + storage write). Shared by HTTP and WebSocket handlers.
pub(crate) async fn record_usage(ctx: &UsageRecordContext, usage: &Usage) {
    let cost = ctx
        .precomputed_cost
        .or_else(|| {
            let billing_context = ctx.billing_context.as_ref()?;
            ctx.state
                .engine()
                .estimate_billing(&ctx.provider_name, billing_context, usage)
                .map(|billing| billing.total_cost)
        })
        .unwrap_or(0.0);
    if cost > 0.0 {
        let (quota, cost_used) = ctx.state.add_cost_usage(ctx.user_id, cost);
        let _ = ctx
            .state
            .storage_writes()
            .enqueue(gproxy_storage::StorageWriteEvent::UpsertUserQuota(
                gproxy_storage::UserQuotaWrite {
                    user_id: ctx.user_id,
                    quota,
                    cost_used,
                },
            ))
            .await;
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let provider_id = ctx.state.provider_id_for_name(&ctx.provider_name);
    let credential_id = ctx
        .credential_index
        .and_then(|index| ctx.state.credential_id_for_index(&ctx.provider_name, index));
    let _ = ctx
        .state
        .storage_writes()
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertUsage(
            gproxy_storage::UsageWrite {
                downstream_trace_id: ctx.downstream_trace_id,
                at_unix_ms: now_ms,
                provider_id,
                credential_id,
                user_id: Some(ctx.user_id),
                user_key_id: Some(ctx.user_key_id),
                operation: ctx.operation.to_string(),
                protocol: ctx.protocol.to_string(),
                model: ctx.model.clone(),
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

fn stream_with_usage_tracking(
    ctx: UsageRecordContext,
    mut stream: gproxy_sdk::provider::engine::ExecuteBodyStream,
) -> impl futures_util::Stream<
    Item = Result<bytes::Bytes, gproxy_sdk::provider::response::UpstreamError>,
> + Send {
    let recorder = StreamUsageRecorder::new(ctx.clone());

    try_stream! {
        let mut decoder = UsageChunkDecoder::new(ctx.protocol);

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
            record_stream_usage(&ctx, usage).await;
        }
    }
}

#[derive(Default)]
struct StreamUsageRecorderState {
    finalized: bool,
    last_usage: Option<Usage>,
    partial_usage: Usage,
    partial_output: String,
}

struct StreamUsageRecorder {
    ctx: UsageRecordContext,
    state: Arc<Mutex<StreamUsageRecorderState>>,
}

impl StreamUsageRecorder {
    fn new(ctx: UsageRecordContext) -> Self {
        Self {
            ctx,
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
            gproxy_sdk::provider::usage::extract_stream_usage(self.ctx.protocol, json_chunk)
        {
            merge_usage(&mut state.partial_usage, &usage);
            state.last_usage = Some(usage);
        } else if let Some(usage) = extract_partial_stream_usage(self.ctx.protocol, json_chunk) {
            merge_usage(&mut state.partial_usage, &usage);
        }

        if let Some(text) = extract_partial_output_text(self.ctx.protocol, json_chunk) {
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
                record_stream_usage(&ctx, usage).await;
            });
        }
    }
}

async fn record_stream_usage(ctx: &UsageRecordContext, usage: Usage) {
    let cost = ctx
        .precomputed_cost
        .or_else(|| {
            let billing_context = ctx.billing_context.as_ref()?;
            ctx.state
                .engine()
                .estimate_billing(&ctx.provider_name, billing_context, &usage)
                .map(|billing| billing.total_cost)
        })
        .unwrap_or(0.0);
    if cost > 0.0 {
        let (quota, cost_used) = ctx.state.add_cost_usage(ctx.user_id, cost);
        let _ = ctx
            .state
            .storage_writes()
            .enqueue(gproxy_storage::StorageWriteEvent::UpsertUserQuota(
                gproxy_storage::UserQuotaWrite {
                    user_id: ctx.user_id,
                    quota,
                    cost_used,
                },
            ))
            .await;
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let provider_id = ctx.state.provider_id_for_name(&ctx.provider_name);
    let credential_id = ctx
        .credential_index
        .and_then(|index| ctx.state.credential_id_for_index(&ctx.provider_name, index));
    let _ = ctx
        .state
        .storage_writes()
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertUsage(
            gproxy_storage::UsageWrite {
                downstream_trace_id: ctx.downstream_trace_id,
                at_unix_ms: now_ms,
                provider_id,
                credential_id,
                user_id: Some(ctx.user_id),
                user_key_id: Some(ctx.user_key_id),
                operation: ctx.operation.to_string(),
                protocol: ctx.protocol.to_string(),
                model: ctx.model.clone(),
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
}

impl UsageChunkDecoder {
    fn new(protocol: ProtocolKind) -> Self {
        match protocol {
            ProtocolKind::Claude
            | ProtocolKind::OpenAi
            | ProtocolKind::OpenAiResponse
            | ProtocolKind::OpenAiChatCompletion
            | ProtocolKind::Gemini => {
                Self::Sse(gproxy_sdk::protocol::stream::SseToNdjsonRewriter::default())
            }
            ProtocolKind::GeminiNDJson => Self::Ndjson(Vec::new()),
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

fn extract_partial_stream_usage(protocol: ProtocolKind, json_chunk: &[u8]) -> Option<Usage> {
    match protocol {
        ProtocolKind::Claude => {
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
        ProtocolKind::OpenAiResponse => {
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
        ProtocolKind::OpenAi => {
            use gproxy_sdk::protocol::openai::create_image::stream::ImageGenerationStreamEvent;

            let event: ImageGenerationStreamEvent = serde_json::from_slice(json_chunk).ok()?;
            match event {
                ImageGenerationStreamEvent::Completed { usage, .. } => Some(Usage {
                    input_tokens: i64::try_from(usage.input_tokens).ok(),
                    output_tokens: i64::try_from(usage.output_tokens).ok(),
                    cache_read_input_tokens: None,
                    cache_creation_input_tokens: None,
                    cache_creation_input_tokens_5min: None,
                    cache_creation_input_tokens_1h: None,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn extract_partial_output_text(protocol: ProtocolKind, json_chunk: &[u8]) -> Option<String> {
    match protocol {
        ProtocolKind::Claude => {
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
        ProtocolKind::OpenAiChatCompletion => {
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
        ProtocolKind::OpenAiResponse => {
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
        ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
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

// ---------------------------------------------------------------------------
// Logging helpers
// ---------------------------------------------------------------------------

pub(crate) fn generate_trace_id() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64
}

fn headers_to_json(headers: &http::HeaderMap) -> String {
    let map: Vec<(&str, &str)> = headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.to_str().unwrap_or("")))
        .collect();
    serde_json::to_string(&map).unwrap_or_else(|_| "[]".to_string())
}

/// After a successful file upload, bind the returned file_id to the credential
/// that handled it. After a successful file deletion, remove the binding.
fn bind_file_affinity_if_applicable(
    state: &AppState,
    classification: &Classification,
    result: &ExecuteResult,
    provider_name: &str,
) {
    if !(200..=299).contains(&result.status) {
        return;
    }
    let body = match &result.body {
        ExecuteBody::Full(b) => b,
        _ => return,
    };

    match classification.operation {
        OperationFamily::FileUpload => {
            if let Some(file_id) = extract_id_from_json(body) {
                state
                    .engine()
                    .store()
                    .bind_file(&file_id, provider_name, result.credential_index);
                tracing::debug!(
                    file_id,
                    provider_name,
                    credential = result.credential_index,
                    "file affinity bound"
                );
            }
        }
        OperationFamily::FileDelete => {
            if let Some(file_id) = extract_id_from_json(body) {
                state.engine().store().unbind_file(&file_id);
                tracing::debug!(file_id, "file affinity unbound");
            }
        }
        _ => {}
    }
}

fn extract_id_from_json(body: &[u8]) -> Option<String> {
    let json: serde_json::Value = serde_json::from_slice(body).ok()?;
    json.get("id")?.as_str().map(String::from)
}

fn build_execute_body(
    operation: OperationFamily,
    request_path: &str,
    request_query: Option<&str>,
    original_body: Vec<u8>,
) -> Vec<u8> {
    match operation {
        OperationFamily::ModelList => {
            build_model_request_body(operation, request_path, request_query).unwrap_or_default()
        }
        OperationFamily::FileList
        | OperationFamily::FileGet
        | OperationFamily::FileContent
        | OperationFamily::FileDelete => {
            build_file_request_body(operation, request_path, request_query).unwrap_or_default()
        }
        _ => original_body,
    }
}

fn normalize_unscoped_request_body(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: Vec<u8>,
    target_model: &str,
) -> Vec<u8> {
    let pointers: &[(&str, bool)] = match (operation, protocol) {
        (OperationFamily::CountToken, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => &[
            ("/generate_content_request/model", true),
            ("/generateContentRequest/model", true),
        ],
        (
            OperationFamily::GenerateContent | OperationFamily::StreamGenerateContent,
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson,
        )
        | (OperationFamily::Embedding, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson)
        | (OperationFamily::ModelGet, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson)
        | (OperationFamily::ModelList, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => &[],
        _ => &[("/model", false)],
    };
    if pointers.is_empty() || body.is_empty() {
        return body;
    }

    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };
    for (pointer, gemini_resource) in pointers {
        let Some(slot) = value.pointer_mut(pointer) else {
            continue;
        };
        let Some(raw) = slot.as_str() else { continue };
        let replacement = if *gemini_resource {
            format!("models/{target_model}")
        } else {
            target_model.to_string()
        };
        if raw != replacement {
            *slot = serde_json::Value::String(replacement);
        }
    }

    serde_json::to_vec(&value).unwrap_or(body)
}

fn extract_requested_total_tokens(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Option<i64> {
    let json: serde_json::Value = serde_json::from_slice(body).ok()?;
    match (operation, protocol) {
        (
            OperationFamily::GenerateContent
            | OperationFamily::StreamGenerateContent
            | OperationFamily::Compact,
            ProtocolKind::Claude,
        ) => json.get("max_tokens").and_then(|value| value.as_i64()),
        (
            OperationFamily::GenerateContent | OperationFamily::StreamGenerateContent,
            ProtocolKind::OpenAiChatCompletion,
        ) => json
            .get("max_completion_tokens")
            .and_then(|value| value.as_i64())
            .or_else(|| json.get("max_tokens").and_then(|value| value.as_i64())),
        (
            OperationFamily::GenerateContent
            | OperationFamily::StreamGenerateContent
            | OperationFamily::Compact,
            ProtocolKind::OpenAiResponse,
        )
        | (OperationFamily::CountToken, ProtocolKind::OpenAi) => json
            .get("max_output_tokens")
            .and_then(|value| value.as_i64()),
        (
            OperationFamily::GenerateContent
            | OperationFamily::StreamGenerateContent
            | OperationFamily::CountToken,
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson,
        ) => json
            .pointer("/generationConfig/maxOutputTokens")
            .and_then(|value| value.as_i64())
            .or_else(|| {
                json.pointer("/generation_config/max_output_tokens")
                    .and_then(|value| value.as_i64())
            })
            .or_else(|| {
                json.pointer("/generateContentRequest/generationConfig/maxOutputTokens")
                    .and_then(|value| value.as_i64())
            })
            .or_else(|| {
                json.pointer("/generate_content_request/generation_config/max_output_tokens")
                    .and_then(|value| value.as_i64())
            }),
        _ => None,
    }
}

fn build_model_request_body(
    operation: OperationFamily,
    _request_path: &str,
    request_query: Option<&str>,
) -> Option<Vec<u8>> {
    let mut root = serde_json::Map::new();

    match operation {
        OperationFamily::ModelList => {
            let mut query = serde_json::Map::new();
            if let Some(raw_query) = request_query {
                for (key, value) in url::form_urlencoded::parse(raw_query.as_bytes()) {
                    match key.as_ref() {
                        "after_id" | "before_id" | "pageToken" => {
                            query.insert(
                                key.into_owned(),
                                serde_json::Value::String(value.into_owned()),
                            );
                        }
                        "limit" | "pageSize" => {
                            if let Ok(number) = value.parse::<u64>() {
                                query.insert(
                                    key.into_owned(),
                                    serde_json::Value::Number(number.into()),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            if !query.is_empty() {
                root.insert("query".to_string(), serde_json::Value::Object(query));
            }
        }
        _ => return None,
    }

    serde_json::to_vec(&serde_json::Value::Object(root)).ok()
}

fn build_file_request_body(
    operation: OperationFamily,
    request_path: &str,
    request_query: Option<&str>,
) -> Option<Vec<u8>> {
    let normalized = normalize_routed_api_path(request_path);
    let mut root = serde_json::Map::new();

    match operation {
        OperationFamily::FileList => {
            let mut query = serde_json::Map::new();
            if let Some(raw_query) = request_query {
                for (key, value) in url::form_urlencoded::parse(raw_query.as_bytes()) {
                    match key.as_ref() {
                        "after_id" | "before_id" => {
                            query.insert(
                                key.into_owned(),
                                serde_json::Value::String(value.into_owned()),
                            );
                        }
                        "limit" => {
                            if let Ok(limit) = value.parse::<u64>() {
                                query.insert(
                                    "limit".to_string(),
                                    serde_json::Value::Number(limit.into()),
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            if !query.is_empty() {
                root.insert("query".to_string(), serde_json::Value::Object(query));
            }
        }
        OperationFamily::FileGet | OperationFamily::FileContent | OperationFamily::FileDelete => {
            let file_id = extract_file_id_from_request_path(&normalized)?;
            root.insert(
                "path".to_string(),
                serde_json::json!({ "file_id": file_id }),
            );
        }
        _ => return None,
    }

    serde_json::to_vec(&serde_json::Value::Object(root)).ok()
}

fn normalize_routed_api_path(path: &str) -> String {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let start = if matches!(segments.first(), Some(&"v1" | &"v1beta" | &"v1beta1")) {
        1
    } else if matches!(segments.get(1), Some(&"v1" | &"v1beta" | &"v1beta1")) {
        2
    } else {
        0
    };

    if start >= segments.len() {
        "/".to_string()
    } else {
        format!("/{}", segments[start..].join("/"))
    }
}

fn extract_file_id_from_request_path(path: &str) -> Option<&str> {
    let tail = path.strip_prefix("/files/")?;
    if let Some(file_id) = tail.strip_suffix("/content")
        && !file_id.is_empty()
        && !file_id.contains('/')
    {
        return Some(file_id);
    }
    if !tail.is_empty() && !tail.contains('/') {
        return Some(tail);
    }
    None
}

/// Record upstream request/response log to DB.
async fn record_upstream_log(
    state: &AppState,
    trace_id: i64,
    provider_name: &str,
    meta: Option<&UpstreamRequestMeta>,
) {
    let config = state.config();
    if !config.enable_upstream_log {
        return;
    }
    let Some(meta) = meta else {
        return;
    };
    let include_body = config.enable_upstream_log_body;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let provider_id = state.provider_id_for_name(provider_name);
    let credential_id = meta
        .credential_index
        .and_then(|index| state.credential_id_for_index(provider_name, index));
    let _ = state
        .storage_writes()
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertUpstreamRequest(
            gproxy_storage::UpstreamRequestWrite {
                downstream_trace_id: Some(trace_id),
                at_unix_ms: now_ms,
                internal: false,
                provider_id,
                credential_id,
                request_method: meta.method.clone(),
                request_headers_json: serde_json::to_string(&meta.request_headers)
                    .unwrap_or_else(|_| "[]".to_string()),
                request_url: Some(meta.url.clone()),
                request_body: if include_body {
                    meta.request_body.clone()
                } else {
                    None
                },
                response_status: meta.response_status.map(|s| s as i32),
                response_headers_json: serde_json::to_string(&meta.response_headers)
                    .unwrap_or_else(|_| "[]".to_string()),
                response_body: None,
            },
        ))
        .await;
}

/// Record downstream request/response log to DB.
#[allow(clippy::too_many_arguments)]
async fn record_downstream_log(
    state: &AppState,
    trace_id: i64,
    user_id: i64,
    user_key_id: i64,
    method: &str,
    path: &str,
    query: Option<&str>,
    req_headers_json: &str,
    req_body: Option<&Vec<u8>>,
    resp_status: Option<i32>,
    resp_headers_json: &str,
    resp_body: Option<&Vec<u8>>,
) {
    let config = state.config();
    if !config.enable_downstream_log {
        return;
    }
    let include_body = config.enable_downstream_log_body;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let _ = state
        .storage_writes()
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertDownstreamRequest(
            gproxy_storage::DownstreamRequestWrite {
                trace_id,
                at_unix_ms: now_ms,
                internal: false,
                user_id: Some(user_id),
                user_key_id: Some(user_key_id),
                request_method: method.to_string(),
                request_headers_json: req_headers_json.to_string(),
                request_path: path.to_string(),
                request_query: query.map(String::from),
                request_body: if include_body {
                    req_body.cloned()
                } else {
                    None
                },
                response_status: resp_status,
                response_headers_json: resp_headers_json.to_string(),
                response_body: if include_body {
                    resp_body.cloned()
                } else {
                    None
                },
            },
        ))
        .await;
}
