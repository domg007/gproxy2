use std::sync::Arc;

use axum::body::to_bytes;
use axum::http::HeaderMap;
use gproxy_middleware::TransformRequest;
use gproxy_protocol::claude::model_list::request as claude_model_list_request;
use gproxy_protocol::gemini::model_list::request as gemini_model_list_request;
use gproxy_protocol::openai::model_list::request as openai_model_list_request;
use serde_json::Value;

use crate::AppState;

use super::{
    BODY_CAPTURE_LIMIT_BYTES, ModelProtocolPreference, RequestAuthContext,
    anthropic_headers_from_request, collect_passthrough_headers, execute_transform_candidates,
    model_protocol_preference, normalize_unscoped_model_id,
};
use gproxy_provider::{ChannelId, ProviderDefinition};

fn collect_model_ids_from_response_json(value: &serde_json::Value, out: &mut Vec<String>) {
    if let Some(items) = value.get("data").and_then(serde_json::Value::as_array) {
        for item in items {
            if let Some(id) = item.get("id").and_then(serde_json::Value::as_str) {
                let model_id = normalize_unscoped_model_id(id);
                if !model_id.is_empty() {
                    out.push(model_id);
                }
            }
        }
    }

    if let Some(items) = value.get("models").and_then(serde_json::Value::as_array) {
        for item in items {
            let name = item
                .get("name")
                .and_then(serde_json::Value::as_str)
                .or_else(|| item.get("id").and_then(serde_json::Value::as_str));
            if let Some(name) = name {
                let model_id = normalize_unscoped_model_id(name);
                if !model_id.is_empty() {
                    out.push(model_id);
                }
            }
        }
    }
}

async fn collect_provider_model_ids(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    headers: &HeaderMap,
) -> Vec<String> {
    let channel_prefix = channel.as_str().to_string();
    let passthrough_headers = collect_passthrough_headers(headers);
    let mut openai = openai_model_list_request::OpenAiModelListRequest::default();
    openai.headers.extra = passthrough_headers.clone();

    let mut claude = claude_model_list_request::ClaudeModelListRequest::default();
    let (version, beta) = anthropic_headers_from_request(headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.headers.extra = passthrough_headers.clone();

    let mut gemini = gemini_model_list_request::GeminiModelListRequest::default();
    gemini.headers.extra = passthrough_headers;
    openai.query = openai_model_list_request::QueryParameters::default();
    let candidates = match model_protocol_preference(headers, None) {
        ModelProtocolPreference::Claude => vec![
            TransformRequest::ModelListClaude(claude),
            TransformRequest::ModelListOpenAi(openai),
            TransformRequest::ModelListGemini(gemini),
        ],
        ModelProtocolPreference::Gemini => vec![TransformRequest::ModelListGemini(gemini)],
        ModelProtocolPreference::OpenAi => vec![
            TransformRequest::ModelListOpenAi(openai),
            TransformRequest::ModelListClaude(claude),
            TransformRequest::ModelListGemini(gemini),
        ],
    };

    let response =
        match execute_transform_candidates(state, channel, provider, auth, candidates).await {
            Ok(response) => response,
            Err(_) => return Vec::new(),
        };

    if !response.status().is_success() {
        return Vec::new();
    }

    let body = match to_bytes(response.into_body(), BODY_CAPTURE_LIMIT_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return Vec::new(),
    };
    let value = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    let mut ids = Vec::new();
    collect_model_ids_from_response_json(&value, &mut ids);
    ids.into_iter()
        .map(|model_id| format!("{channel_prefix}/{model_id}"))
        .collect()
}

pub(super) async fn collect_unscoped_model_ids(
    state: Arc<AppState>,
    auth: RequestAuthContext,
    headers: &HeaderMap,
) -> Vec<String> {
    let providers: Vec<(ChannelId, ProviderDefinition)> = {
        let snapshot = state.load_config();
        snapshot
            .providers
            .providers
            .iter()
            .map(|provider| (provider.channel.clone(), provider.clone()))
            .collect()
    };

    let mut ids = Vec::new();
    for (channel, provider) in providers {
        ids.extend(
            collect_provider_model_ids(state.clone(), channel, provider, auth, headers).await,
        );
    }

    let mut dedup = std::collections::BTreeSet::new();
    ids.retain(|id| dedup.insert(id.clone()));
    ids
}
