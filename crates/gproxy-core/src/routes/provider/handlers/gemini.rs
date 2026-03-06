use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformRequestPayload};
use gproxy_protocol::gemini::model_get::request as gemini_model_get_request;
use gproxy_protocol::gemini::model_list::request as gemini_model_list_request;
use gproxy_provider::parse_query_value;
use serde_json::{Value, json};

use crate::AppState;

use super::super::{
    HttpError, authorize_provider_access, bad_request, collect_passthrough_headers,
    collect_unscoped_model_ids, execute_transform_request, execute_transform_request_payload,
    internal_error, normalize_gemini_model_path, parse_optional_query_value, resolve_provider,
    response_from_status_headers_and_bytes, split_provider_prefixed_gemini_target,
    split_provider_prefixed_model_path,
};

fn encode_json_value(value: &Value, context: &str) -> Result<Bytes, HttpError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|err| bad_request(format!("{context}: {err}")))
}

fn build_gemini_payload(
    model: String,
    body: Value,
    alt: Option<&str>,
    headers: &HeaderMap,
    context: &str,
) -> Result<Bytes, HttpError> {
    let mut payload = json!({
        "path": {
            "model": model,
        },
        "headers": collect_passthrough_headers(headers),
        "body": body,
    });
    if let Some(alt) = alt
        && let Some(map) = payload.as_object_mut()
    {
        map.insert("query".to_string(), json!({ "alt": alt }));
    }
    encode_json_value(&payload, context)
}

pub(in crate::routes::provider) async fn v1beta_model_list(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = gemini_model_list_request::GeminiModelListRequest::default();
    request.headers.extra = collect_passthrough_headers(&headers);
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
    request.headers.extra = collect_passthrough_headers(&headers);
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
    request.headers.extra = collect_passthrough_headers(&headers);
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
        let body = serde_json::from_slice::<Value>(&body).map_err(|err| {
            bad_request(format!(
                "invalid gemini generateContent request body: {err}"
            ))
        })?;
        let payload_body = build_gemini_payload(
            normalized_model,
            body,
            None,
            &headers,
            "invalid gemini generateContent request body",
        )?;
        let payload = TransformRequestPayload::from_bytes(
            OperationFamily::GenerateContent,
            ProtocolKind::Gemini,
            payload_body,
        );
        return execute_transform_request_payload(state, channel, provider, auth, payload)
            .await
            .map_err(HttpError::from);
    }

    if let Some(model) = target.strip_suffix(":streamGenerateContent") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let body = serde_json::from_slice::<Value>(&body).map_err(|err| {
            bad_request(format!(
                "invalid gemini streamGenerateContent request body: {err}"
            ))
        })?;

        let alt = parse_query_value(query.as_deref(), "alt");
        let (protocol, payload_alt) = match alt.as_deref() {
            Some("sse") | Some("SSE") => (ProtocolKind::Gemini, Some("sse")),
            Some(other) => {
                return Err(bad_request(format!(
                    "unsupported gemini stream `alt` query parameter: {other}"
                )));
            }
            None => (ProtocolKind::GeminiNDJson, None),
        };

        let payload_body = build_gemini_payload(
            normalized_model,
            body,
            payload_alt,
            &headers,
            "invalid gemini streamGenerateContent request body",
        )?;
        let payload = TransformRequestPayload::from_bytes(
            OperationFamily::StreamGenerateContent,
            protocol,
            payload_body,
        );

        return execute_transform_request_payload(state, channel, provider, auth, payload)
            .await
            .map_err(HttpError::from);
    }

    if let Some(model) = target.strip_suffix(":countTokens") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let body = serde_json::from_slice::<Value>(&body).map_err(|err| {
            bad_request(format!("invalid gemini countTokens request body: {err}"))
        })?;
        let payload_body = build_gemini_payload(
            normalized_model,
            body,
            None,
            &headers,
            "invalid gemini countTokens request body",
        )?;
        let payload = TransformRequestPayload::from_bytes(
            OperationFamily::CountToken,
            ProtocolKind::Gemini,
            payload_body,
        );
        return execute_transform_request_payload(state, channel, provider, auth, payload)
            .await
            .map_err(HttpError::from);
    }

    if let Some(model) = target.strip_suffix(":embedContent") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let body = serde_json::from_slice::<Value>(&body).map_err(|err| {
            bad_request(format!("invalid gemini embedContent request body: {err}"))
        })?;
        let payload_body = build_gemini_payload(
            normalized_model,
            body,
            None,
            &headers,
            "invalid gemini embedContent request body",
        )?;
        let payload = TransformRequestPayload::from_bytes(
            OperationFamily::Embedding,
            ProtocolKind::Gemini,
            payload_body,
        );
        return execute_transform_request_payload(state, channel, provider, auth, payload)
            .await
            .map_err(HttpError::from);
    }

    Err(HttpError::new(
        StatusCode::NOT_FOUND,
        format!("unsupported gemini endpoint target: {target}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::build_gemini_payload;
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    #[test]
    fn build_gemini_payload_flattens_passthrough_headers_for_typed_decode() {
        let mut headers = HeaderMap::new();
        headers.insert("x-test", HeaderValue::from_static("value"));

        let payload = build_gemini_payload(
            "models/gemini-2.5-flash".to_string(),
            json!({
                "contents": [
                    {
                        "parts": [{"text": "hello"}],
                        "role": "user"
                    }
                ]
            }),
            None,
            &headers,
            "invalid gemini generateContent request body",
        )
        .expect("payload");

        let decoded: serde_json::Value =
            serde_json::from_slice(payload.as_ref()).expect("payload should be json");

        assert_eq!(
            decoded
                .pointer("/headers/x-test")
                .and_then(serde_json::Value::as_str),
            Some("value")
        );
        assert!(decoded.pointer("/headers/extra").is_none());
    }
}
