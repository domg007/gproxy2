use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use gproxy_middleware::TransformRequest;
use gproxy_protocol::gemini::count_tokens::request as gemini_count_tokens_request;
use gproxy_protocol::gemini::embeddings::request as gemini_embeddings_request;
use gproxy_protocol::gemini::generate_content::request as gemini_generate_content_request;
use gproxy_protocol::gemini::model_get::request as gemini_model_get_request;
use gproxy_protocol::gemini::model_list::request as gemini_model_list_request;
use gproxy_protocol::gemini::stream_generate_content::request as gemini_stream_generate_content_request;
use gproxy_provider::parse_query_value;
use serde_json::json;

use crate::AppState;

use super::super::{
    HttpError, authorize_provider_access, bad_request, collect_unscoped_model_ids,
    execute_transform_request, internal_error, normalize_gemini_model_path, parse_json_body,
    parse_optional_query_value, resolve_provider, response_from_status_headers_and_bytes,
    split_provider_prefixed_gemini_target, split_provider_prefixed_model_path,
};

pub(in crate::routes::provider) async fn v1beta_model_list(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = gemini_model_list_request::GeminiModelListRequest::default();
    request.query.page_size = parse_optional_query_value::<u32>(query.as_deref(), "pageSize")?;
    request.query.page_token = parse_query_value(query.as_deref(), "pageToken");
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::ModelListGemini(request),
    )
    .await
    .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn v1beta_model_list_unscoped(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let ids = collect_unscoped_model_ids(state, auth, &headers).await;
    let models = ids
        .into_iter()
        .map(|id| {
            json!({
                "name": format!("models/{id}"),
                "displayName": id,
            })
        })
        .collect::<Vec<_>>();
    let body = serde_json::to_vec(&json!({
        "models": models,
    }))
    .map_err(|err| internal_error(format!("serialize model list response failed: {err}")))?;
    response_from_status_headers_and_bytes(
        StatusCode::OK,
        &[("content-type".to_string(), "application/json".to_string())],
        body,
    )
    .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn v1beta_model_get(
    State(state): State<Arc<AppState>>,
    Path((provider_name, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = gemini_model_get_request::GeminiModelGetRequest::default();
    request.path.name = normalize_gemini_model_path(name.as_str())?;
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::ModelGetGemini(request),
    )
    .await
    .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn v1beta_model_get_unscoped(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (provider_name, stripped_name) = split_provider_prefixed_model_path(name.as_str())?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = gemini_model_get_request::GeminiModelGetRequest::default();
    request.path.name = normalize_gemini_model_path(stripped_name.as_str())?;
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::ModelGetGemini(request),
    )
    .await
    .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn v1beta_post_target(
    State(state): State<Arc<AppState>>,
    Path((provider_name, target)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    handle_gemini_post_target(state, provider_name, target, query, headers, body).await
}

pub(in crate::routes::provider) async fn v1beta_post_target_unscoped(
    State(state): State<Arc<AppState>>,
    Path(target): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let (provider_name, stripped_target) = split_provider_prefixed_gemini_target(target.as_str())?;
    handle_gemini_post_target(state, provider_name, stripped_target, query, headers, body).await
}

pub(in crate::routes::provider) async fn v1_post_target(
    State(state): State<Arc<AppState>>,
    Path((provider_name, target)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    handle_gemini_post_target(state, provider_name, target, query, headers, body).await
}

pub(in crate::routes::provider) async fn v1_post_target_unscoped(
    State(state): State<Arc<AppState>>,
    Path(target): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let (provider_name, stripped_target) = split_provider_prefixed_gemini_target(target.as_str())?;
    handle_gemini_post_target(state, provider_name, stripped_target, query, headers, body).await
}

pub(in crate::routes::provider) async fn handle_gemini_post_target(
    state: Arc<AppState>,
    provider_name: String,
    target: String,
    query: Option<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;

    if let Some(model) = target.strip_suffix(":generateContent") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let request_body = parse_json_body::<gemini_generate_content_request::RequestBody>(
            &body,
            "invalid gemini generateContent request body",
        )?;
        let mut request = gemini_generate_content_request::GeminiGenerateContentRequest::default();
        request.path.model = normalized_model;
        request.body = request_body;
        return execute_transform_request(
            state,
            channel,
            provider,
            auth,
            TransformRequest::GenerateContentGemini(request),
        )
        .await
        .map_err(HttpError::from);
    }

    if let Some(model) = target.strip_suffix(":streamGenerateContent") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let request_body = parse_json_body::<gemini_stream_generate_content_request::RequestBody>(
            &body,
            "invalid gemini streamGenerateContent request body",
        )?;
        let mut request =
            gemini_stream_generate_content_request::GeminiStreamGenerateContentRequest::default();
        request.path.model = normalized_model;
        request.body = request_body;

        let alt = parse_query_value(query.as_deref(), "alt");
        let envelope = match alt.as_deref() {
            Some("sse") | Some("SSE") => {
                request.query.alt =
                    Some(gemini_stream_generate_content_request::AltQueryParameter::Sse);
                TransformRequest::StreamGenerateContentGeminiSse(request)
            }
            Some(other) => {
                return Err(bad_request(format!(
                    "unsupported gemini stream `alt` query parameter: {other}"
                )));
            }
            None => TransformRequest::StreamGenerateContentGeminiNdjson(request),
        };

        return execute_transform_request(state, channel, provider, auth, envelope)
            .await
            .map_err(HttpError::from);
    }

    if let Some(model) = target.strip_suffix(":countTokens") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let request_body = parse_json_body::<gemini_count_tokens_request::RequestBody>(
            &body,
            "invalid gemini countTokens request body",
        )?;
        let mut request = gemini_count_tokens_request::GeminiCountTokensRequest::default();
        request.path.model = normalized_model;
        request.body = request_body;
        return execute_transform_request(
            state,
            channel,
            provider,
            auth,
            TransformRequest::CountTokenGemini(request),
        )
        .await
        .map_err(HttpError::from);
    }

    if let Some(model) = target.strip_suffix(":embedContent") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let request_body = parse_json_body::<gemini_embeddings_request::RequestBody>(
            &body,
            "invalid gemini embedContent request body",
        )?;
        let mut request = gemini_embeddings_request::GeminiEmbedContentRequest::default();
        request.path.model = normalized_model;
        request.body = request_body;
        return execute_transform_request(
            state,
            channel,
            provider,
            auth,
            TransformRequest::EmbeddingGemini(request),
        )
        .await
        .map_err(HttpError::from);
    }

    Err(HttpError::new(
        StatusCode::NOT_FOUND,
        format!("unsupported gemini endpoint target: {target}"),
    ))
}
