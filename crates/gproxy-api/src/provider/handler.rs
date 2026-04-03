use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use gproxy_sdk::provider::engine::ExecuteRequest;
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
            operation,
            protocol,
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
                    operation: operation_to_string(classification.operation),
                    protocol: protocol_to_string(classification.protocol),
                    model: effective_model,
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

    let mut response = Response::builder()
        .status(result.status)
        .body(Body::from(result.body))
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
    let _user_key = authenticate_user(&headers, &state)?;

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

    let result = state
        .engine()
        .execute(ExecuteRequest {
            provider: target_provider,
            operation: operation_to_string(classification.operation),
            protocol: protocol_to_string(classification.protocol),
            body: body.to_vec(),
            headers,
            model: Some(target_model),
        })
        .await?;

    let mut response = Response::builder()
        .status(result.status)
        .body(Body::from(result.body))
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

fn compute_cost(
    usage: &gproxy_sdk::provider::engine::Usage,
    model: &gproxy_server::MemoryModel,
) -> f64 {
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
