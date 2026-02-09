use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::body::Body;
use axum::extract::{Extension, Path, Query, RawQuery, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;

use gproxy_core::proxy_engine::{ProxyAuth, ProxyCall, ProxyEngine};
use gproxy_protocol::claude;
use gproxy_protocol::gemini;
use gproxy_protocol::openai;
use gproxy_provider_core::{
    CountTokensRequest as MwCountTokensRequest, DownstreamEvent, Event,
    GenerateContentRequest as MwGenerateContentRequest, Headers,
    ModelGetRequest as MwModelGetRequest, ModelListRequest as MwModelListRequest,
    OAuthCallbackRequest, OAuthStartRequest, Op, OpenAIResponsesPassthroughRequest, Proto, Request,
    UpstreamBody, UpstreamHttpResponse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownstreamKeySource {
    AuthorizationBearer,
    XApiKey,
    XGoogApiKey,
    QueryKey,
}

#[derive(Clone)]
pub struct ProxyState {
    pub engine: Arc<ProxyEngine>,
}

#[derive(Clone)]
struct RequestTraceId(String);

#[derive(Debug, Clone)]
struct ProviderRouteCtx {
    provider: String,
    response_model_prefix_provider: Option<String>,
}

const SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const SSE_HEARTBEAT_FRAME: &[u8] = b": keep-alive\n\n";
const MAX_DOWNSTREAM_LOG_BODY_BYTES: usize = 50 * 1024 * 1024;

pub fn proxy_router(engine: Arc<ProxyEngine>) -> Router {
    let state = ProxyState { engine };

    Router::new()
        // Aggregate routes without provider prefix
        .route("/v1/messages", post(claude_messages_aggregate))
        .route(
            "/v1/messages/count_tokens",
            post(claude_count_tokens_aggregate),
        )
        .route(
            "/v1/chat/completions",
            post(openai_chat_completions_aggregate),
        )
        .route("/v1/responses", post(openai_responses_aggregate))
        .route(
            "/v1/responses/compact",
            post(openai_responses_compact_aggregate),
        )
        .route(
            "/v1/responses/input_tokens",
            post(openai_input_tokens_aggregate),
        )
        .route("/v1/models", get(models_list_v1_aggregate))
        .route("/v1/models/{*model}", get(models_get_v1_aggregate))
        .route("/v1/models/{*model}", post(gemini_post_aggregate))
        .route("/v1beta/models", get(gemini_models_list_aggregate))
        .route("/v1beta/models/{*name}", get(gemini_models_get_aggregate))
        .route("/v1beta/models/{*name}", post(gemini_post_aggregate))
        // Claude
        .route("/{provider}/v1/messages", post(claude_messages))
        .route(
            "/{provider}/v1/messages/count_tokens",
            post(claude_count_tokens),
        )
        // OpenAI
        .route(
            "/{provider}/v1/chat/completions",
            post(openai_chat_completions),
        )
        .route(
            "/{provider}/v1/responses",
            any(openai_responses_passthrough),
        )
        .route(
            "/{provider}/v1/responses/input_tokens",
            post(openai_input_tokens),
        )
        .route(
            "/{provider}/v1/responses/{*rest}",
            any(openai_responses_passthrough_rest),
        )
        // Shared OpenAI/Claude models endpoints (disambiguate by `anthropic-version` header).
        .route("/{provider}/v1/models", get(models_list_v1))
        .route("/{provider}/v1/models/{*model}", get(models_get_v1))
        // Gemini v1/v1beta POST endpoints (generateContent/streamGenerateContent/countTokens).
        .route("/{provider}/v1/models/{*model}", post(gemini_post))
        .route("/{provider}/v1beta/models", get(gemini_models_list))
        .route("/{provider}/v1beta/models/{*name}", get(gemini_models_get))
        .route("/{provider}/v1beta/models/{*name}", post(gemini_post))
        // Provider-internal downstream abilities
        .route("/{provider}/oauth", get(oauth_start))
        .route("/{provider}/oauth/callback", get(oauth_callback))
        .route("/{provider}/usage", get(upstream_usage))
        .layer(middleware::from_fn_with_state(state.clone(), proxy_auth))
        .with_state(state)
}

async fn proxy_auth(
    State(state): State<ProxyState>,
    mut req: axum::http::Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let trace_id = uuid::Uuid::now_v7().to_string();
    let trace_id_opt = Some(trace_id.clone());
    let request_method = req.method().as_str().to_string();
    let redact_sensitive = state.engine.event_redact_sensitive();
    let request_headers = maybe_redact_headers(headers_to_vec(req.headers()), redact_sensitive);
    let request_path = req.uri().path().to_string();
    let request_query = maybe_redact_query(req.uri().query(), redact_sensitive);

    // Extract before stripping.
    let key = extract_user_key(req.headers(), req.uri().query());

    // Defense-in-depth: don't forward downstream auth material to handlers/providers/logs.
    // Do this for both success/failure to avoid accidental propagation.
    strip_downstream_auth_headers(req.headers_mut());
    strip_downstream_auth_query(req.uri_mut());
    req.extensions_mut()
        .insert(RequestTraceId(trace_id.clone()));

    let Some(key) = key else {
        state
            .engine
            .events()
            .emit(Event::Downstream(DownstreamEvent {
                trace_id: trace_id_opt.clone(),
                at: SystemTime::now(),
                user_id: None,
                user_key_id: None,
                request_method,
                request_headers,
                request_path,
                request_query,
                request_body: None,
                response_status: Some(StatusCode::UNAUTHORIZED.as_u16()),
                response_headers: Vec::new(),
                response_body: None,
            }))
            .await;
        return Err(StatusCode::UNAUTHORIZED);
    };

    let user_agent = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let Some(mut auth) = state.engine.authenticate_user_key(&key.0) else {
        state
            .engine
            .events()
            .emit(Event::Downstream(DownstreamEvent {
                trace_id: trace_id_opt.clone(),
                at: SystemTime::now(),
                user_id: None,
                user_key_id: None,
                request_method,
                request_headers,
                request_path,
                request_query,
                request_body: None,
                response_status: Some(StatusCode::UNAUTHORIZED.as_u16()),
                response_headers: Vec::new(),
                response_body: None,
            }))
            .await;
        return Err(StatusCode::UNAUTHORIZED);
    };

    auth.user_agent = user_agent;
    req.extensions_mut().insert(auth);
    req.extensions_mut().insert(key.1);
    let auth = req.extensions().get::<ProxyAuth>().cloned().unwrap();

    let resp = next.run(req).await;
    let status = resp.status().as_u16();
    let response_headers = maybe_redact_headers(headers_to_vec(resp.headers()), redact_sensitive);

    if redact_sensitive {
        state
            .engine
            .events()
            .emit(Event::Downstream(DownstreamEvent {
                trace_id: trace_id_opt,
                at: SystemTime::now(),
                user_id: Some(auth.user_id),
                user_key_id: Some(auth.user_key_id),
                request_method,
                request_headers,
                request_path,
                request_query,
                request_body: None,
                response_status: Some(status),
                response_headers,
                response_body: None,
            }))
            .await;
        return Ok(resp);
    }

    let (parts, body) = resp.into_parts();
    let (tx_out, rx_out) = tokio::sync::mpsc::channel::<Bytes>(32);
    let events = state.engine.events();

    tokio::spawn(async move {
        let mut stream = body.into_data_stream();
        let mut response_body = Vec::new();
        while let Some(item) = stream.next().await {
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(_) => break,
            };
            append_capped(
                &mut response_body,
                chunk.as_ref(),
                MAX_DOWNSTREAM_LOG_BODY_BYTES,
            );
            if tx_out.send(chunk).await.is_err() {
                break;
            }
        }

        events
            .emit(Event::Downstream(DownstreamEvent {
                trace_id: trace_id_opt,
                at: SystemTime::now(),
                user_id: Some(auth.user_id),
                user_key_id: Some(auth.user_key_id),
                request_method,
                request_headers,
                request_path,
                request_query,
                request_body: None,
                response_status: Some(status),
                response_headers,
                response_body: Some(response_body),
            }))
            .await;
    });

    let stream = ReceiverStream::new(rx_out).map(Ok::<_, Infallible>);
    let resp = Response::from_parts(parts, Body::from_stream(stream));
    Ok(resp)
}

fn append_capped(buf: &mut Vec<u8>, chunk: &[u8], cap: usize) -> bool {
    if buf.len() >= cap {
        return true;
    }
    let remaining = cap.saturating_sub(buf.len());
    let take = remaining.min(chunk.len());
    buf.extend_from_slice(&chunk[..take]);
    take < chunk.len()
}

fn strip_downstream_auth_headers(headers: &mut HeaderMap) {
    headers.remove(header::AUTHORIZATION);
    headers.remove("x-api-key");
    headers.remove("x-goog-api-key");
}

fn strip_downstream_auth_query(uri: &mut axum::http::Uri) {
    let Some(q) = uri.query() else { return };

    let Ok(pairs) = serde_urlencoded::from_str::<Vec<(String, String)>>(q) else {
        return;
    };

    let filtered: Vec<(String, String)> = pairs.into_iter().filter(|(k, _)| k != "key").collect();

    let new_q = match serde_urlencoded::to_string(&filtered) {
        Ok(s) => s,
        Err(_) => return,
    };

    let path = uri.path();
    let new_uri_str = if new_q.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{new_q}")
    };
    if let Ok(new_uri) = new_uri_str.parse() {
        *uri = new_uri;
    }
}

fn extract_user_key(
    headers: &HeaderMap,
    query: Option<&str>,
) -> Option<(String, DownstreamKeySource)> {
    // 1) Authorization: Bearer <token>
    if let Some(value) = headers.get(header::AUTHORIZATION)
        && let Ok(s) = value.to_str()
    {
        let s = s.trim();
        let prefix = "Bearer ";
        if s.len() > prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
            let token = s[prefix.len()..].trim();
            if !token.is_empty() {
                return Some((token.to_string(), DownstreamKeySource::AuthorizationBearer));
            }
        }
    }

    // 2) x-api-key
    if let Some(value) = headers.get("x-api-key")
        && let Ok(s) = value.to_str()
    {
        let s = s.trim();
        if !s.is_empty() {
            return Some((s.to_string(), DownstreamKeySource::XApiKey));
        }
    }

    // 3) x-goog-api-key
    if let Some(value) = headers.get("x-goog-api-key")
        && let Ok(s) = value.to_str()
    {
        let s = s.trim();
        if !s.is_empty() {
            return Some((s.to_string(), DownstreamKeySource::XGoogApiKey));
        }
    }

    // 4) query: ?key=...
    let q = query?;
    let pairs = serde_urlencoded::from_str::<Vec<(String, String)>>(q).ok()?;
    pairs
        .into_iter()
        .find(|(k, _)| k == "key")
        .map(|(_, v)| v)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(|v| (v, DownstreamKeySource::QueryKey))
}

#[derive(Debug, Clone, Serialize)]
struct AggregateErrorItem {
    provider: String,
    status: u16,
    error: String,
    detail: serde_json::Value,
}

// ---- Aggregate (no provider prefix) ----

async fn claude_messages_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    headers: HeaderMap,
    Json(mut body): Json<claude::create_message::request::CreateMessageRequestBody>,
) -> Response {
    let model = claude_model_to_string_for_route(&body.model);
    let Some((provider, model)) = split_provider_model(&model) else {
        return (StatusCode::BAD_REQUEST, "missing_provider_prefix").into_response();
    };
    body.model = claude::count_tokens::types::Model::Custom(model);

    let anthropic_headers = parse_anthropic_headers(&headers);
    let stream = body.stream.unwrap_or(false);
    let op = if stream {
        Op::StreamGenerateContent
    } else {
        Op::GenerateContent
    };
    let req = claude::create_message::request::CreateMessageRequest {
        headers: anthropic_headers,
        body,
    };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider: provider.clone(),
        response_model_prefix_provider: Some(provider),
        user_proto: Proto::Claude,
        user_op: op,
        req: Box::new(Request::GenerateContent(MwGenerateContentRequest::Claude(
            req,
        ))),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn claude_count_tokens_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    headers: HeaderMap,
    Json(mut body): Json<claude::count_tokens::request::CountTokensRequestBody>,
) -> Response {
    let model = claude_model_to_string_for_route(&body.model);
    let Some((provider, model)) = split_provider_model(&model) else {
        return (StatusCode::BAD_REQUEST, "missing_provider_prefix").into_response();
    };
    body.model = claude::count_tokens::types::Model::Custom(model);

    let anthropic_headers = parse_anthropic_headers(&headers);
    let req = claude::count_tokens::request::CountTokensRequest {
        headers: anthropic_headers,
        body,
    };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider: provider.clone(),
        response_model_prefix_provider: Some(provider),
        user_proto: Proto::Claude,
        user_op: Op::CountTokens,
        req: Box::new(Request::CountTokens(MwCountTokensRequest::Claude(req))),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn openai_chat_completions_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Json(mut body): Json<openai::create_chat_completions::request::CreateChatCompletionRequestBody>,
) -> Response {
    let Some((provider, model)) = split_provider_model(&body.model) else {
        return (StatusCode::BAD_REQUEST, "missing_provider_prefix").into_response();
    };
    body.model = model;
    apply_openai_chat_stream_defaults(&mut body);
    let stream = body.stream.unwrap_or(false);
    let op = if stream {
        Op::StreamGenerateContent
    } else {
        Op::GenerateContent
    };
    let req = openai::create_chat_completions::request::CreateChatCompletionRequest { body };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider: provider.clone(),
        response_model_prefix_provider: Some(provider),
        user_proto: Proto::OpenAIChat,
        user_op: op,
        req: Box::new(Request::GenerateContent(
            MwGenerateContentRequest::OpenAIChat(req),
        )),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn openai_responses_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    method: Method,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some((provider, body)) = split_provider_and_rewrite_model_from_openai_body(&body) else {
        return (StatusCode::BAD_REQUEST, "missing_provider_prefix").into_response();
    };
    forward_openai_responses_passthrough(
        state,
        auth,
        trace_id.0,
        provider,
        "/v1/responses".to_string(),
        method,
        query,
        headers,
        body,
    )
    .await
}

async fn openai_responses_compact_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    method: Method,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some((provider, body)) = split_provider_and_rewrite_model_from_openai_body(&body) else {
        return (StatusCode::BAD_REQUEST, "missing_provider_prefix").into_response();
    };
    if provider != "codex" {
        return (StatusCode::NOT_IMPLEMENTED, "unsupported_operation").into_response();
    }
    forward_openai_responses_passthrough(
        state,
        auth,
        trace_id.0,
        provider,
        "/v1/responses/compact".to_string(),
        method,
        query,
        headers,
        body,
    )
    .await
}

async fn openai_input_tokens_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Json(mut body): Json<openai::count_tokens::request::InputTokenCountRequestBody>,
) -> Response {
    let Some((provider, model)) = split_provider_model(&body.model) else {
        return (StatusCode::BAD_REQUEST, "missing_provider_prefix").into_response();
    };
    body.model = model;
    let req = openai::count_tokens::request::InputTokenCountRequest { body };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider: provider.clone(),
        response_model_prefix_provider: Some(provider),
        user_proto: Proto::OpenAI,
        user_op: Op::CountTokens,
        req: Box::new(Request::CountTokens(MwCountTokensRequest::OpenAI(req))),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn models_list_v1_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Extension(key_source): Extension<DownstreamKeySource>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let user_proto = if headers.contains_key("anthropic-version") {
        Proto::Claude
    } else if matches!(
        key_source,
        DownstreamKeySource::XGoogApiKey | DownstreamKeySource::QueryKey
    ) {
        Proto::Gemini
    } else {
        Proto::OpenAI
    };

    let providers = state.engine.enabled_provider_names();
    let anthropic_headers = parse_anthropic_headers(&headers);
    let claude_query: claude::list_models::request::ListModelsQuery = query
        .as_deref()
        .and_then(|q| serde_urlencoded::from_str(q).ok())
        .unwrap_or_default();
    let gemini_query: gemini::list_models::request::ListModelsQuery = query
        .as_deref()
        .and_then(|q| serde_urlencoded::from_str(q).ok())
        .unwrap_or_default();

    let mut errors: Vec<AggregateErrorItem> = Vec::new();
    let mut out_items: Vec<serde_json::Value> = Vec::new();

    for provider in providers {
        let req = match user_proto {
            Proto::Claude => Request::ModelList(MwModelListRequest::Claude(
                claude::list_models::request::ListModelsRequest {
                    headers: anthropic_headers.clone(),
                    query: claude_query.clone(),
                },
            )),
            Proto::Gemini => Request::ModelList(MwModelListRequest::Gemini(
                gemini::list_models::request::ListModelsRequest {
                    query: gemini_query.clone(),
                },
            )),
            Proto::OpenAI => Request::ModelList(MwModelListRequest::OpenAI(
                openai::list_models::request::ListModelsRequest,
            )),
            _ => return (StatusCode::BAD_REQUEST, "unsupported_operation").into_response(),
        };

        let call = ProxyCall::Protocol {
            trace_id: Some(trace_id.0.clone()),
            auth: auth.clone(),
            provider: provider.clone(),
            response_model_prefix_provider: Some(provider.clone()),
            user_proto,
            user_op: Op::ModelList,
            req: Box::new(req),
        };
        let resp = state.engine.handle(call).await;
        if (200..300).contains(&resp.status) {
            let Some(bytes) = response_body_bytes(&resp.body) else {
                errors.push(AggregateErrorItem {
                    provider,
                    status: 502,
                    error: "upstream_body_missing".to_string(),
                    detail: serde_json::Value::Null,
                });
                continue;
            };
            match user_proto {
                Proto::Claude => {
                    match serde_json::from_slice::<claude::list_models::response::ListModelsResponse>(
                        &bytes,
                    ) {
                        Ok(list) => {
                            for item in list.data {
                                out_items.push(
                                    serde_json::to_value(item).unwrap_or(serde_json::Value::Null),
                                );
                            }
                        }
                        Err(err) => errors.push(AggregateErrorItem {
                            provider,
                            status: 502,
                            error: "decode_response_failed".to_string(),
                            detail: serde_json::Value::String(err.to_string()),
                        }),
                    }
                }
                Proto::Gemini => {
                    match serde_json::from_slice::<gemini::list_models::response::ListModelsResponse>(
                        &bytes,
                    ) {
                        Ok(list) => {
                            for item in list.models {
                                out_items.push(
                                    serde_json::to_value(item).unwrap_or(serde_json::Value::Null),
                                );
                            }
                        }
                        Err(err) => errors.push(AggregateErrorItem {
                            provider,
                            status: 502,
                            error: "decode_response_failed".to_string(),
                            detail: serde_json::Value::String(err.to_string()),
                        }),
                    }
                }
                Proto::OpenAI => {
                    match serde_json::from_slice::<openai::list_models::response::ListModelsResponse>(
                        &bytes,
                    ) {
                        Ok(list) => {
                            for item in list.data {
                                out_items.push(
                                    serde_json::to_value(item).unwrap_or(serde_json::Value::Null),
                                );
                            }
                        }
                        Err(err) => errors.push(AggregateErrorItem {
                            provider,
                            status: 502,
                            error: "decode_response_failed".to_string(),
                            detail: serde_json::Value::String(err.to_string()),
                        }),
                    }
                }
                _ => {}
            }
            continue;
        }

        let (error, detail) = parse_upstream_error(&resp);
        if is_silent_aggregate_error(&error) {
            continue;
        }
        errors.push(AggregateErrorItem {
            provider,
            status: resp.status,
            error,
            detail,
        });
    }

    let partial = !errors.is_empty();
    let payload = match user_proto {
        Proto::Claude => serde_json::json!({
            "data": out_items,
            "first_id": serde_json::Value::Null,
            "has_more": false,
            "last_id": serde_json::Value::Null,
            "partial": partial,
        }),
        Proto::Gemini => serde_json::json!({
            "models": out_items,
            "nextPageToken": serde_json::Value::Null,
            "partial": partial,
        }),
        Proto::OpenAI => serde_json::json!({
            "object": "list",
            "data": out_items,
            "partial": partial,
        }),
        _ => serde_json::json!({
            "error": "unsupported_operation"
        }),
    };
    (StatusCode::OK, Json(payload)).into_response()
}

async fn models_get_v1_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Extension(key_source): Extension<DownstreamKeySource>,
    Path(model): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some((provider, model)) = split_provider_model(&model) else {
        return (StatusCode::BAD_REQUEST, "missing_provider_prefix").into_response();
    };
    models_get_v1_inner(
        state,
        auth,
        key_source,
        ProviderRouteCtx {
            provider: provider.clone(),
            response_model_prefix_provider: Some(provider),
        },
        model,
        trace_id.0,
        headers,
    )
    .await
}

async fn gemini_models_list_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Query(query): Query<gemini::list_models::request::ListModelsQuery>,
) -> Response {
    let providers = state.engine.enabled_provider_names();
    let mut errors: Vec<AggregateErrorItem> = Vec::new();
    let mut out_items: Vec<serde_json::Value> = Vec::new();

    for provider in providers {
        let req = Request::ModelList(MwModelListRequest::Gemini(
            gemini::list_models::request::ListModelsRequest {
                query: query.clone(),
            },
        ));
        let call = ProxyCall::Protocol {
            trace_id: Some(trace_id.0.clone()),
            auth: auth.clone(),
            provider: provider.clone(),
            response_model_prefix_provider: Some(provider.clone()),
            user_proto: Proto::Gemini,
            user_op: Op::ModelList,
            req: Box::new(req),
        };
        let resp = state.engine.handle(call).await;
        if (200..300).contains(&resp.status) {
            let Some(bytes) = response_body_bytes(&resp.body) else {
                errors.push(AggregateErrorItem {
                    provider,
                    status: 502,
                    error: "upstream_body_missing".to_string(),
                    detail: serde_json::Value::Null,
                });
                continue;
            };
            match serde_json::from_slice::<gemini::list_models::response::ListModelsResponse>(
                &bytes,
            ) {
                Ok(list) => {
                    for item in list.models {
                        out_items
                            .push(serde_json::to_value(item).unwrap_or(serde_json::Value::Null));
                    }
                }
                Err(err) => errors.push(AggregateErrorItem {
                    provider,
                    status: 502,
                    error: "decode_response_failed".to_string(),
                    detail: serde_json::Value::String(err.to_string()),
                }),
            }
            continue;
        }

        let (error, detail) = parse_upstream_error(&resp);
        if is_silent_aggregate_error(&error) {
            continue;
        }
        errors.push(AggregateErrorItem {
            provider,
            status: resp.status,
            error,
            detail,
        });
    }

    let payload = serde_json::json!({
        "models": out_items,
        "nextPageToken": serde_json::Value::Null,
        "partial": !errors.is_empty(),
    });
    (StatusCode::OK, Json(payload)).into_response()
}

async fn gemini_models_get_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(name): Path<String>,
) -> Response {
    let Some((provider, name)) = split_provider_model(&name) else {
        return (StatusCode::BAD_REQUEST, "missing_provider_prefix").into_response();
    };
    let req = gemini::get_model::request::GetModelRequest {
        path: gemini::get_model::request::GetModelPath {
            name: format!("models/{name}"),
        },
    };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider: provider.clone(),
        response_model_prefix_provider: Some(provider),
        user_proto: Proto::Gemini,
        user_op: Op::ModelGet,
        req: Box::new(Request::ModelGet(MwModelGetRequest::Gemini(req))),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn gemini_post_aggregate(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(model_action): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    let Some((provider, model, action)) = split_provider_model_action(&model_action) else {
        return (StatusCode::BAD_REQUEST, "missing_provider_prefix").into_response();
    };
    gemini_post_impl(
        state,
        auth,
        ProviderRouteCtx {
            provider: provider.clone(),
            response_model_prefix_provider: Some(provider),
        },
        format!("{model}:{action}"),
        trace_id.0,
        query,
        body,
    )
    .await
}

fn split_provider_model(input: &str) -> Option<(String, String)> {
    let raw = input.trim().trim_start_matches('/');
    let raw = raw.strip_prefix("models/").unwrap_or(raw);
    let (provider, model) = raw.split_once('/')?;
    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((provider.to_string(), model.to_string()))
}

fn split_provider_model_action(input: &str) -> Option<(String, String, String)> {
    let raw = input.trim().trim_start_matches('/');
    let (model, action) = raw.split_once(':')?;
    let (provider, model) = split_provider_model(model)?;
    let action = action.trim();
    if action.is_empty() {
        return None;
    }
    Some((provider, model, action.to_string()))
}

fn claude_model_to_string_for_route(model: &claude::count_tokens::types::Model) -> String {
    match model {
        claude::count_tokens::types::Model::Custom(v) => v.clone(),
        claude::count_tokens::types::Model::Known(v) => serde_json::to_string(v)
            .unwrap_or_else(|_| format!("{v:?}"))
            .trim_matches('"')
            .to_string(),
    }
}

fn response_body_bytes(body: &UpstreamBody) -> Option<Bytes> {
    match body {
        UpstreamBody::Bytes(b) => Some(b.clone()),
        UpstreamBody::Stream(_) => None,
    }
}

fn parse_upstream_error(resp: &UpstreamHttpResponse) -> (String, serde_json::Value) {
    let Some(bytes) = response_body_bytes(&resp.body) else {
        return ("upstream_error".to_string(), serde_json::Value::Null);
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return ("upstream_error".to_string(), serde_json::Value::Null);
    };
    let error = value
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("upstream_error")
        .to_string();
    let detail = value
        .get("detail")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    (error, detail)
}

fn is_silent_aggregate_error(error: &str) -> bool {
    matches!(
        error,
        "no_active_credentials" | "unsupported_operation" | "provider_disabled"
    )
}

// ---- Internal: oauth ----

async fn oauth_start(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(provider): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let call = ProxyCall::OAuthStart {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        req: OAuthStartRequest {
            query,
            headers: headers_to_vec(&headers),
        },
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn oauth_callback(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(provider): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let call = ProxyCall::OAuthCallback {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        req: OAuthCallbackRequest {
            query,
            headers: headers_to_vec(&headers),
        },
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn upstream_usage(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(provider): Path<String>,
    Query(query): Query<UpstreamUsageQuery>,
) -> Response {
    let call = ProxyCall::UpstreamUsage {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        credential_id: query.credential_id,
    };
    to_axum_response(state.engine.handle(call).await)
}

#[derive(Debug, Clone, Deserialize)]
struct UpstreamUsageQuery {
    credential_id: i64,
}

// ---- Claude ----

async fn claude_messages(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    Json(body): Json<claude::create_message::request::CreateMessageRequestBody>,
) -> Response {
    let anthropic_headers = parse_anthropic_headers(&headers);
    let stream = body.stream.unwrap_or(false);
    let op = if stream {
        Op::StreamGenerateContent
    } else {
        Op::GenerateContent
    };
    let req = claude::create_message::request::CreateMessageRequest {
        headers: anthropic_headers,
        body,
    };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        response_model_prefix_provider: None,
        user_proto: Proto::Claude,
        user_op: op,
        req: Box::new(Request::GenerateContent(MwGenerateContentRequest::Claude(
            req,
        ))),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn claude_count_tokens(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    Json(body): Json<claude::count_tokens::request::CountTokensRequestBody>,
) -> Response {
    let anthropic_headers = parse_anthropic_headers(&headers);
    let req = claude::count_tokens::request::CountTokensRequest {
        headers: anthropic_headers,
        body,
    };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        response_model_prefix_provider: None,
        user_proto: Proto::Claude,
        user_op: Op::CountTokens,
        req: Box::new(Request::CountTokens(MwCountTokensRequest::Claude(req))),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn models_list_v1(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Extension(key_source): Extension<DownstreamKeySource>,
    Path(provider): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    if headers.contains_key("anthropic-version") {
        let anthropic_headers = parse_anthropic_headers(&headers);
        let claude_query: claude::list_models::request::ListModelsQuery = query
            .as_deref()
            .and_then(|q| serde_urlencoded::from_str(q).ok())
            .unwrap_or_default();
        let req = claude::list_models::request::ListModelsRequest {
            headers: anthropic_headers,
            query: claude_query,
        };
        let call = ProxyCall::Protocol {
            trace_id: Some(trace_id.0.clone()),
            auth,
            provider,
            response_model_prefix_provider: None,
            user_proto: Proto::Claude,
            user_op: Op::ModelList,
            req: Box::new(Request::ModelList(MwModelListRequest::Claude(req))),
        };
        return to_axum_response(state.engine.handle(call).await);
    }

    // Gemini v1 models list (disambiguate by downstream auth style).
    if matches!(
        key_source,
        DownstreamKeySource::XGoogApiKey | DownstreamKeySource::QueryKey
    ) {
        let gemini_query: gemini::list_models::request::ListModelsQuery = query
            .as_deref()
            .and_then(|q| serde_urlencoded::from_str(q).ok())
            .unwrap_or_default();
        let req = gemini::list_models::request::ListModelsRequest {
            query: gemini_query,
        };
        let call = ProxyCall::Protocol {
            trace_id: Some(trace_id.0.clone()),
            auth,
            provider,
            response_model_prefix_provider: None,
            user_proto: Proto::Gemini,
            user_op: Op::ModelList,
            req: Box::new(Request::ModelList(MwModelListRequest::Gemini(req))),
        };
        return to_axum_response(state.engine.handle(call).await);
    }

    // Default: OpenAI models list.
    let req = openai::list_models::request::ListModelsRequest;
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        response_model_prefix_provider: None,
        user_proto: Proto::OpenAI,
        user_op: Op::ModelList,
        req: Box::new(Request::ModelList(MwModelListRequest::OpenAI(req))),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn models_get_v1(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Extension(key_source): Extension<DownstreamKeySource>,
    Path((provider, model)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    models_get_v1_inner(
        state,
        auth,
        key_source,
        ProviderRouteCtx {
            provider,
            response_model_prefix_provider: None,
        },
        model.trim_start_matches('/').to_string(),
        trace_id.0,
        headers,
    )
    .await
}

async fn models_get_v1_inner(
    state: ProxyState,
    auth: ProxyAuth,
    key_source: DownstreamKeySource,
    route_ctx: ProviderRouteCtx,
    model: String,
    trace_id: String,
    headers: HeaderMap,
) -> Response {
    let provider = route_ctx.provider;
    let response_model_prefix_provider = route_ctx.response_model_prefix_provider;
    if headers.contains_key("anthropic-version") {
        let anthropic_headers = parse_anthropic_headers(&headers);
        let req = claude::get_model::request::GetModelRequest {
            headers: anthropic_headers,
            path: claude::get_model::request::GetModelPath { model_id: model },
        };
        let call = ProxyCall::Protocol {
            trace_id: Some(trace_id.clone()),
            auth,
            provider,
            response_model_prefix_provider,
            user_proto: Proto::Claude,
            user_op: Op::ModelGet,
            req: Box::new(Request::ModelGet(MwModelGetRequest::Claude(req))),
        };
        return to_axum_response(state.engine.handle(call).await);
    }

    // Gemini v1 getModel (disambiguate by downstream auth style).
    if matches!(
        key_source,
        DownstreamKeySource::XGoogApiKey | DownstreamKeySource::QueryKey
    ) {
        let req = gemini::get_model::request::GetModelRequest {
            path: gemini::get_model::request::GetModelPath {
                name: format!("models/{model}"),
            },
        };
        let call = ProxyCall::Protocol {
            trace_id: Some(trace_id.clone()),
            auth,
            provider,
            response_model_prefix_provider,
            user_proto: Proto::Gemini,
            user_op: Op::ModelGet,
            req: Box::new(Request::ModelGet(MwModelGetRequest::Gemini(req))),
        };
        return to_axum_response(state.engine.handle(call).await);
    }

    let req = openai::get_model::request::GetModelRequest {
        path: openai::get_model::request::GetModelPath { model },
    };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id),
        auth,
        provider,
        response_model_prefix_provider,
        user_proto: Proto::OpenAI,
        user_op: Op::ModelGet,
        req: Box::new(Request::ModelGet(MwModelGetRequest::OpenAI(req))),
    };
    to_axum_response(state.engine.handle(call).await)
}

// ---- OpenAI ----

async fn openai_chat_completions(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(provider): Path<String>,
    Json(mut body): Json<openai::create_chat_completions::request::CreateChatCompletionRequestBody>,
) -> Response {
    apply_openai_chat_stream_defaults(&mut body);
    let stream = body.stream.unwrap_or(false);
    let op = if stream {
        Op::StreamGenerateContent
    } else {
        Op::GenerateContent
    };
    let req = openai::create_chat_completions::request::CreateChatCompletionRequest { body };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        response_model_prefix_provider: None,
        user_proto: Proto::OpenAIChat,
        user_op: op,
        req: Box::new(Request::GenerateContent(
            MwGenerateContentRequest::OpenAIChat(req),
        )),
    };
    to_axum_response(state.engine.handle(call).await)
}

fn apply_openai_chat_stream_defaults(
    body: &mut openai::create_chat_completions::request::CreateChatCompletionRequestBody,
) {
    if !body.stream.unwrap_or(false) {
        return;
    }
    let opts = body.stream_options.get_or_insert(
        openai::create_chat_completions::types::ChatCompletionStreamOptions {
            include_usage: None,
            include_obfuscation: None,
        },
    );
    if opts.include_usage.is_none() {
        opts.include_usage = Some(true);
    }
}

#[allow(clippy::too_many_arguments)]
async fn forward_openai_responses_passthrough(
    state: ProxyState,
    auth: ProxyAuth,
    trace_id: String,
    provider: String,
    path: String,
    method: Method,
    query: Option<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(method) = gproxy_provider_core::HttpMethod::parse(method.as_str()) else {
        return (StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed").into_response();
    };
    let is_stream = openai_responses_stream_hint(method, &headers, &body);
    let req = OpenAIResponsesPassthroughRequest {
        method,
        path,
        query,
        headers: headers_to_vec(&headers),
        body: if body.is_empty() { None } else { Some(body) },
        is_stream,
    };
    let call = ProxyCall::OpenAIResponsesPassthrough {
        trace_id: Some(trace_id),
        auth,
        provider,
        req,
    };
    to_axum_response(state.engine.handle(call).await)
}

fn split_provider_and_rewrite_model_from_openai_body(body: &Bytes) -> Option<(String, Bytes)> {
    let mut value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let obj = value.as_object_mut()?;
    let model = obj.get("model")?.as_str()?;
    let (provider, model) = split_provider_model(model)?;
    obj.insert("model".to_string(), serde_json::Value::String(model));
    let body = serde_json::to_vec(&value).ok()?;
    Some((provider, Bytes::from(body)))
}

fn openai_responses_stream_hint(
    method: gproxy_provider_core::HttpMethod,
    headers: &HeaderMap,
    body: &Bytes,
) -> bool {
    if matches!(
        method,
        gproxy_provider_core::HttpMethod::Get | gproxy_provider_core::HttpMethod::Delete
    ) {
        return false;
    }
    if headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false)
    {
        return true;
    }
    if body.is_empty() {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("stream").and_then(|s| s.as_bool()))
        .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
async fn openai_responses_passthrough(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(provider): Path<String>,
    method: Method,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward_openai_responses_passthrough(
        state,
        auth,
        trace_id.0,
        provider,
        "/v1/responses".to_string(),
        method,
        query,
        headers,
        body,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn openai_responses_passthrough_rest(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path((provider, rest)): Path<(String, String)>,
    method: Method,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = format!("/v1/responses/{}", rest.trim_start_matches('/'));
    forward_openai_responses_passthrough(
        state, auth, trace_id.0, provider, path, method, query, headers, body,
    )
    .await
}

async fn openai_input_tokens(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(provider): Path<String>,
    Json(body): Json<openai::count_tokens::request::InputTokenCountRequestBody>,
) -> Response {
    let req = openai::count_tokens::request::InputTokenCountRequest { body };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        response_model_prefix_provider: None,
        user_proto: Proto::OpenAI,
        user_op: Op::CountTokens,
        req: Box::new(Request::CountTokens(MwCountTokensRequest::OpenAI(req))),
    };
    to_axum_response(state.engine.handle(call).await)
}

// ---- Gemini ----

async fn gemini_models_list(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path(provider): Path<String>,
    Query(query): Query<gemini::list_models::request::ListModelsQuery>,
) -> Response {
    let req = gemini::list_models::request::ListModelsRequest { query };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        response_model_prefix_provider: None,
        user_proto: Proto::Gemini,
        user_op: Op::ModelList,
        req: Box::new(Request::ModelList(MwModelListRequest::Gemini(req))),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn gemini_models_get(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path((provider, name)): Path<(String, String)>,
) -> Response {
    let name = name.trim_start_matches('/');
    let req = gemini::get_model::request::GetModelRequest {
        path: gemini::get_model::request::GetModelPath {
            name: format!("models/{name}"),
        },
    };
    let call = ProxyCall::Protocol {
        trace_id: Some(trace_id.0.clone()),
        auth,
        provider,
        response_model_prefix_provider: None,
        user_proto: Proto::Gemini,
        user_op: Op::ModelGet,
        req: Box::new(Request::ModelGet(MwModelGetRequest::Gemini(req))),
    };
    to_axum_response(state.engine.handle(call).await)
}

async fn gemini_post(
    State(state): State<ProxyState>,
    Extension(auth): Extension<ProxyAuth>,
    Extension(trace_id): Extension<RequestTraceId>,
    Path((provider, model_action)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    gemini_post_impl(
        state,
        auth,
        ProviderRouteCtx {
            provider,
            response_model_prefix_provider: None,
        },
        model_action.trim_start_matches('/').to_string(),
        trace_id.0,
        query,
        body,
    )
    .await
}

async fn gemini_post_impl(
    state: ProxyState,
    auth: ProxyAuth,
    route_ctx: ProviderRouteCtx,
    model_action: String,
    trace_id: String,
    query: Option<String>,
    body: Bytes,
) -> Response {
    let provider = route_ctx.provider;
    let response_model_prefix_provider = route_ctx.response_model_prefix_provider;
    let Some((model, action)) = model_action.split_once(':') else {
        return (StatusCode::BAD_REQUEST, "bad_gemini_model_action").into_response();
    };
    let model = model.trim();
    let action = action.trim();
    if model.is_empty() || action.is_empty() {
        return (StatusCode::BAD_REQUEST, "bad_gemini_model_action").into_response();
    }

    match action {
        "generateContent" => {
            let body: gemini::generate_content::request::GenerateContentRequestBody =
                match serde_json::from_slice(&body) {
                    Ok(v) => v,
                    Err(_) => {
                        return (StatusCode::BAD_REQUEST, "bad_gemini_body").into_response();
                    }
                };
            let req = gemini::generate_content::request::GenerateContentRequest {
                path: gemini::generate_content::request::GenerateContentPath {
                    model: format!("models/{model}"),
                },
                body,
            };
            let call = ProxyCall::Protocol {
                trace_id: Some(trace_id.clone()),
                auth,
                provider,
                response_model_prefix_provider,
                user_proto: Proto::Gemini,
                user_op: Op::GenerateContent,
                req: Box::new(Request::GenerateContent(MwGenerateContentRequest::Gemini(
                    req,
                ))),
            };
            to_axum_response(state.engine.handle(call).await)
        }
        "streamGenerateContent" => {
            let body: gemini::generate_content::request::GenerateContentRequestBody =
                match serde_json::from_slice(&body) {
                    Ok(v) => v,
                    Err(_) => {
                        return (StatusCode::BAD_REQUEST, "bad_gemini_body").into_response();
                    }
                };
            let req = gemini::stream_content::request::StreamGenerateContentRequest {
                path: gemini::generate_content::request::GenerateContentPath {
                    model: format!("models/{model}"),
                },
                body,
                query,
            };
            let call = ProxyCall::Protocol {
                trace_id: Some(trace_id.clone()),
                auth,
                provider,
                response_model_prefix_provider,
                user_proto: Proto::Gemini,
                user_op: Op::StreamGenerateContent,
                req: Box::new(Request::GenerateContent(
                    MwGenerateContentRequest::GeminiStream(req),
                )),
            };
            to_axum_response(state.engine.handle(call).await)
        }
        "countTokens" => {
            let body: gemini::count_tokens::request::CountTokensRequestBody =
                match serde_json::from_slice(&body) {
                    Ok(v) => v,
                    Err(_) => {
                        return (StatusCode::BAD_REQUEST, "bad_gemini_body").into_response();
                    }
                };
            let req = gemini::count_tokens::request::CountTokensRequest {
                path: gemini::count_tokens::request::CountTokensPath {
                    model: format!("models/{model}"),
                },
                body,
            };
            let call = ProxyCall::Protocol {
                trace_id: Some(trace_id),
                auth,
                provider,
                response_model_prefix_provider,
                user_proto: Proto::Gemini,
                user_op: Op::CountTokens,
                req: Box::new(Request::CountTokens(MwCountTokensRequest::Gemini(req))),
            };
            to_axum_response(state.engine.handle(call).await)
        }
        _ => (StatusCode::NOT_FOUND, "unknown_gemini_action").into_response(),
    }
}

// ---- Helpers ----

fn to_axum_response(resp: UpstreamHttpResponse) -> Response {
    let sse_stream =
        has_sse_content_type(&resp.headers) && matches!(&resp.body, UpstreamBody::Stream(_));
    let mut builder = Response::builder().status(resp.status);
    if let Some(h) = builder.headers_mut() {
        for (k, v) in resp.headers {
            // Drop hop-by-hop and framing headers. Hyper sets framing itself.
            if is_hop_by_hop_or_framing_header(&k) {
                continue;
            }
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(&v),
            ) {
                h.append(name, value);
            }
        }
        if sse_stream {
            // Hint common reverse proxies to avoid buffering SSE responses.
            h.entry(header::CACHE_CONTROL)
                .or_insert(HeaderValue::from_static("no-cache"));
            h.entry(HeaderName::from_static("x-accel-buffering"))
                .or_insert(HeaderValue::from_static("no"));
        }
    }

    let body = match resp.body {
        UpstreamBody::Bytes(b) => Body::from(b),
        UpstreamBody::Stream(rx) => {
            let rx = if sse_stream {
                wrap_sse_stream_with_heartbeat(rx)
            } else {
                rx
            };
            let stream = ReceiverStream::new(rx).map(Ok::<_, Infallible>);
            Body::from_stream(stream)
        }
    };

    builder.body(body).unwrap_or_else(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "response_build_failed").into_response()
    })
}

fn has_sse_content_type(headers: &Headers) -> bool {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false)
}

fn wrap_sse_stream_with_heartbeat(
    mut upstream_rx: tokio::sync::mpsc::Receiver<Bytes>,
) -> tokio::sync::mpsc::Receiver<Bytes> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(32);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SSE_HEARTBEAT_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip immediate tick; first heartbeat should be sent after the interval.
        ticker.tick().await;

        loop {
            tokio::select! {
                maybe_chunk = upstream_rx.recv() => {
                    let Some(chunk) = maybe_chunk else {
                        break;
                    };
                    if tx.send(chunk).await.is_err() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    if tx.send(Bytes::from_static(SSE_HEARTBEAT_FRAME)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

fn is_hop_by_hop_or_framing_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("content-length")
        || name.eq_ignore_ascii_case("transfer-encoding")
        || name.eq_ignore_ascii_case("connection")
        || name.eq_ignore_ascii_case("keep-alive")
        || name.eq_ignore_ascii_case("proxy-authenticate")
        || name.eq_ignore_ascii_case("proxy-authorization")
        || name.eq_ignore_ascii_case("te")
        || name.eq_ignore_ascii_case("trailer")
        || name.eq_ignore_ascii_case("upgrade")
}

fn headers_to_vec(headers: &HeaderMap) -> Headers {
    let mut out: Headers = Vec::new();
    for (name, value) in headers {
        if let Ok(v) = value.to_str() {
            out.push((name.as_str().to_string(), v.to_string()));
        }
    }
    out
}

fn maybe_redact_headers(mut headers: Headers, redact: bool) -> Headers {
    if !redact {
        return headers;
    }
    for (k, v) in &mut headers {
        let key = k.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "authorization" | "x-api-key" | "x-goog-api-key" | "cookie" | "set-cookie"
        ) {
            *v = "***".to_string();
        }
    }
    headers
}

fn maybe_redact_query(query: Option<&str>, redact: bool) -> Option<String> {
    let q = query?;
    if !redact {
        return Some(q.to_string());
    }
    let Ok(mut pairs) = serde_urlencoded::from_str::<Vec<(String, String)>>(q) else {
        return Some(q.to_string());
    };
    for (k, v) in &mut pairs {
        let key = k.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "key"
                | "api_key"
                | "access_token"
                | "refresh_token"
                | "authorization"
                | "session_key"
                | "code"
        ) {
            *v = "***".to_string();
        }
    }
    serde_urlencoded::to_string(pairs).ok()
}

fn parse_anthropic_headers(headers: &HeaderMap) -> claude::types::AnthropicHeaders {
    let mut map = serde_json::Map::new();
    if let Some(v) = headers
        .get("anthropic-version")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        map.insert(
            "anthropic-version".to_string(),
            serde_json::Value::String(v.to_string()),
        );
    }

    if let Some(beta) = headers
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        let parts: Vec<_> = beta
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| serde_json::Value::String(s.to_string()))
            .collect();
        if parts.len() == 1 {
            map.insert("anthropic-beta".to_string(), parts[0].clone());
        } else if !parts.is_empty() {
            map.insert(
                "anthropic-beta".to_string(),
                serde_json::Value::Array(parts),
            );
        }
    }

    serde_json::from_value(serde_json::Value::Object(map)).unwrap_or_default()
}
