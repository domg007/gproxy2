use wreq::{Client as WreqClient, Method as WreqMethod};

use super::settings::{CustomMaskRule, CustomMaskTable};
use crate::channels::ChannelCredential;
use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{UpstreamError, UpstreamResponse};
use crate::channels::utils::{
    anthropic_header_pairs, claude_model_list_query_string, claude_model_to_string,
    default_gproxy_user_agent, gemini_model_list_query_string, is_auth_failure,
    is_transient_server_failure, join_base_url_and_path, resolve_user_agent_or_else,
    retry_after_to_millis, to_wreq_method,
};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

pub async fn execute_custom_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &gproxy_middleware::TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let mut prepared = CustomPreparedRequest::from_transform_request(request)?;
    if let Some(mask_table) = provider.settings.custom_mask_table() {
        apply_custom_mask_table(&mut prepared, mask_table)?;
    }
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }
    let mut url = join_base_url_and_path(base_url, prepared.path.as_str());
    if let Some(query) = prepared
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if url.contains('?') {
            url.push('&');
        } else {
            url.push('?');
        }
        url.push_str(query);
    }

    let state_manager = CredentialStateManager::new(now_unix_ms);
    let model_for_selection = prepared.model.clone();
    let method_template = prepared.method.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let headers_template = prepared.request_headers.clone();
    let auth_template = prepared.auth_scheme;
    let url_template = url.clone();
    let user_agent_template =
        resolve_user_agent_or_else(provider.settings.user_agent(), default_gproxy_user_agent);
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        crate::channels::retry::cache_affinity_protocol_from_transform_request(request).and_then(
            |protocol| {
                cache_affinity_hint_from_transform_request(
                    protocol,
                    prepared.model.as_deref(),
                    prepared.body.as_deref(),
                )
            },
        )
    } else {
        None
    };
    let pick_mode =
        credential_pick_mode(provider.credential_pick_mode, cache_affinity_hint.as_ref());

    retry_with_eligible_credentials_with_affinity(
        provider,
        credential_states,
        model_for_selection.as_deref(),
        now_unix_ms,
        pick_mode,
        cache_affinity_hint,
        |credential| {
            match &credential.credential {
                ChannelCredential::Custom(value) => Some(value.api_key.as_str()),
                _ => None,
            }
            .map(str::trim)
            .filter(|api_key| !api_key.is_empty())
            .map(ToOwned::to_owned)
        },
        |attempt| {
            let method = method_template.clone();
            let body = body_template.clone();
            let model = model_template.clone();
            let request_headers = headers_template.clone();
            let auth_scheme = auth_template;
            let url = url_template.clone();
            let user_agent = user_agent_template.clone();
            async move {
                let mut request_meta_headers = Vec::new();
                request_meta_headers.push(("user-agent".to_string(), user_agent));
                match auth_scheme {
                    AuthScheme::Bearer => {
                        request_meta_headers.push((
                            "authorization".to_string(),
                            format!("Bearer {}", attempt.material),
                        ));
                    }
                    AuthScheme::XApiKey => {
                        request_meta_headers
                            .push(("x-api-key".to_string(), attempt.material.clone()));
                    }
                    AuthScheme::XGoogApiKey => {
                        request_meta_headers
                            .push(("x-goog-api-key".to_string(), attempt.material.clone()));
                    }
                };

                for (name, value) in &request_headers {
                    request_meta_headers.push((name.clone(), value.clone()));
                }

                if body.is_some() {
                    request_meta_headers
                        .push(("content-type".to_string(), "application/json".to_string()));
                }
                let send = crate::channels::upstream::tracked_send_request(
                    client,
                    method,
                    url.as_str(),
                    request_meta_headers,
                    body.as_ref().cloned(),
                )
                .await;
                match send {
                    Ok((response, request_meta)) => {
                        let status = response.status();
                        if status.is_success() {
                            state_manager.mark_success(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                            );
                            return CredentialRetryDecision::Return(
                                UpstreamResponse::from_http(
                                    attempt.credential_id,
                                    attempt.attempts,
                                    response,
                                )
                                .with_request_meta(request_meta),
                            );
                        }

                        let status_code = status.as_u16();
                        if is_auth_failure(status_code) {
                            let message = format!("upstream status {status_code}");
                            state_manager.mark_auth_dead(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                Some(message.clone()),
                            );
                            return CredentialRetryDecision::Retry {
                                last_status: Some(status_code),
                                last_error: Some(message),
                                last_request_meta: None,
                            };
                        }

                        if status_code == 429 {
                            let retry_after_ms = retry_after_to_millis(response.headers());
                            let message = format!("upstream status {status_code}");
                            state_manager.mark_rate_limited(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                model.as_deref(),
                                retry_after_ms,
                                Some(message.clone()),
                            );
                            return CredentialRetryDecision::Retry {
                                last_status: Some(status_code),
                                last_error: Some(message),
                                last_request_meta: None,
                            };
                        }

                        if is_transient_server_failure(status_code) {
                            let message = format!("upstream status {status_code}");
                            state_manager.mark_transient_failure(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                model.as_deref(),
                                None,
                                Some(message.clone()),
                            );
                            return CredentialRetryDecision::Retry {
                                last_status: Some(status_code),
                                last_error: Some(message),
                                last_request_meta: None,
                            };
                        }

                        CredentialRetryDecision::Return(
                            UpstreamResponse::from_http(
                                attempt.credential_id,
                                attempt.attempts,
                                response,
                            )
                            .with_request_meta(request_meta),
                        )
                    }
                    Err(err) => {
                        let message = err.to_string();
                        state_manager.mark_transient_failure(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                            model.as_deref(),
                            None,
                            Some(message.clone()),
                        );
                        CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(message),
                            last_request_meta: None,
                        }
                    }
                }
            }
        },
    )
    .await
}

#[derive(Clone)]
struct CustomPreparedRequest {
    method: WreqMethod,
    path: String,
    query: Option<String>,
    body: Option<Vec<u8>>,
    model: Option<String>,
    auth_scheme: AuthScheme,
    request_headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
enum AuthScheme {
    Bearer,
    XApiKey,
    XGoogApiKey,
}

impl CustomPreparedRequest {
    fn from_transform_request(
        request: &gproxy_middleware::TransformRequest,
    ) -> Result<Self, UpstreamError> {
        match request {
            gproxy_middleware::TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/models".to_string(),
                query: None,
                body: None,
                model: None,
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
            }),
            gproxy_middleware::TransformRequest::ModelGetOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: format!("/v1/models/{}", value.path.model),
                query: None,
                body: None,
                model: Some(value.path.model.clone()),
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
            }),
            gproxy_middleware::TransformRequest::CountTokenOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/responses/input_tokens".to_string(),
                query: None,
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: value.body.model.clone(),
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
            }),
            gproxy_middleware::TransformRequest::GenerateContentOpenAiResponse(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/responses".to_string(),
                query: None,
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: value.body.model.clone(),
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
            }),
            gproxy_middleware::TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/chat/completions".to_string(),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(value.body.model.clone()),
                    auth_scheme: AuthScheme::Bearer,
                    request_headers: Vec::new(),
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentOpenAiResponse(value) => {
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/responses".to_string(),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: value.body.model.clone(),
                    auth_scheme: AuthScheme::Bearer,
                    request_headers: Vec::new(),
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentOpenAiChatCompletions(
                value,
            ) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/chat/completions".to_string(),
                query: None,
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
            }),
            gproxy_middleware::TransformRequest::EmbeddingOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/embeddings".to_string(),
                query: None,
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: None,
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
            }),
            gproxy_middleware::TransformRequest::CompactOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/responses/compact".to_string(),
                query: None,
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
            }),
            gproxy_middleware::TransformRequest::ModelListClaude(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/models".to_string(),
                query: Some(claude_model_list_query_string(
                    value.query.after_id.as_deref(),
                    value.query.before_id.as_deref(),
                    value.query.limit,
                ))
                .filter(|query| !query.is_empty()),
                body: None,
                model: None,
                auth_scheme: AuthScheme::XApiKey,
                request_headers: anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?,
            }),
            gproxy_middleware::TransformRequest::ModelGetClaude(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: format!("/v1/models/{}", value.path.model_id),
                query: None,
                body: None,
                model: Some(value.path.model_id.clone()),
                auth_scheme: AuthScheme::XApiKey,
                request_headers: anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?,
            }),
            gproxy_middleware::TransformRequest::CountTokenClaude(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/messages/count_tokens".to_string(),
                query: None,
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(claude_model_to_string(&value.body.model)?),
                auth_scheme: AuthScheme::XApiKey,
                request_headers: anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?,
            }),
            gproxy_middleware::TransformRequest::GenerateContentClaude(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/messages".to_string(),
                query: None,
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(claude_model_to_string(&value.body.model)?),
                auth_scheme: AuthScheme::XApiKey,
                request_headers: anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?,
            }),
            gproxy_middleware::TransformRequest::StreamGenerateContentClaude(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/messages".to_string(),
                query: None,
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(claude_model_to_string(&value.body.model)?),
                auth_scheme: AuthScheme::XApiKey,
                request_headers: anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?,
            }),
            gproxy_middleware::TransformRequest::ModelListGemini(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1beta/models".to_string(),
                query: gemini_model_list_query_string(
                    value.query.page_size,
                    value.query.page_token.as_deref(),
                ),
                body: None,
                model: None,
                auth_scheme: AuthScheme::XGoogApiKey,
                request_headers: Vec::new(),
            }),
            gproxy_middleware::TransformRequest::ModelGetGemini(value) => {
                let name = normalize_gemini_model_name(value.path.name.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{name}"),
                    query: None,
                    body: None,
                    model: Some(name),
                    auth_scheme: AuthScheme::XGoogApiKey,
                    request_headers: Vec::new(),
                })
            }
            gproxy_middleware::TransformRequest::CountTokenGemini(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:countTokens"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                    request_headers: Vec::new(),
                })
            }
            gproxy_middleware::TransformRequest::GenerateContentGemini(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:generateContent"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                    request_headers: Vec::new(),
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentGeminiSse(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:streamGenerateContent"),
                    query: value
                        .query
                        .alt
                        .as_ref()
                        .map(|_| "alt=sse".to_string())
                        .or_else(|| Some("alt=sse".to_string())),
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                    request_headers: Vec::new(),
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:streamGenerateContent"),
                    query: value.query.alt.as_ref().map(|_| "alt=sse".to_string()),
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                    request_headers: Vec::new(),
                })
            }
            gproxy_middleware::TransformRequest::EmbeddingGemini(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:embedContent"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                    request_headers: Vec::new(),
                })
            }
            gproxy_middleware::TransformRequest::OpenAiResponseWebSocket(value) => {
                let _ = value;
                Err(UpstreamError::UnsupportedRequest)
            }
            gproxy_middleware::TransformRequest::GeminiLive(value) => {
                let _ = value;
                Err(UpstreamError::UnsupportedRequest)
            }
        }
    }
}

fn normalize_gemini_model_name(model: &str) -> String {
    if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    }
}

fn apply_custom_mask_table(
    prepared: &mut CustomPreparedRequest,
    mask_table: &CustomMaskTable,
) -> Result<(), UpstreamError> {
    if mask_table.rules.is_empty() || prepared.body.is_none() {
        return Ok(());
    }
    let mut body = match prepared.body.take() {
        Some(value) => value,
        None => return Ok(()),
    };
    let mut json_body: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            prepared.body = Some(body);
            return Ok(());
        }
    };

    for rule in &mask_table.rules {
        if !mask_rule_matches(rule, prepared) {
            continue;
        }
        for field_path in &rule.remove_fields {
            remove_json_field_path(&mut json_body, field_path);
        }
    }

    body = serde_json::to_vec(&json_body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    prepared.body = Some(body);
    Ok(())
}

fn mask_rule_matches(rule: &CustomMaskRule, prepared: &CustomPreparedRequest) -> bool {
    let method_match = rule
        .method
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| prepared.method.as_str().eq_ignore_ascii_case(value))
        .unwrap_or(true);
    if !method_match {
        return false;
    }
    rule.path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|path| {
            if path == "*" {
                return true;
            }
            if let Some(prefix) = path.strip_suffix('*') {
                return prepared.path.starts_with(prefix);
            }
            prepared.path == path
        })
        .unwrap_or(true)
}

fn remove_json_field_path(root: &mut serde_json::Value, path: &str) {
    let keys: Vec<&str> = path
        .split('.')
        .filter(|key| !key.trim().is_empty())
        .collect();
    if keys.is_empty() {
        return;
    }
    remove_json_field_path_inner(root, &keys);
}

fn remove_json_field_path_inner(node: &mut serde_json::Value, keys: &[&str]) {
    if keys.is_empty() {
        return;
    }
    let current = keys[0];
    let tail = &keys[1..];
    match node {
        serde_json::Value::Object(map) => {
            if current == "*" {
                if tail.is_empty() {
                    map.clear();
                    return;
                }
                for value in map.values_mut() {
                    remove_json_field_path_inner(value, tail);
                }
                return;
            }
            if tail.is_empty() {
                map.remove(current);
                return;
            }
            if let Some(next) = map.get_mut(current) {
                remove_json_field_path_inner(next, tail);
            }
        }
        serde_json::Value::Array(items) => {
            if current == "*" {
                for value in items.iter_mut() {
                    remove_json_field_path_inner(value, tail);
                }
                return;
            }
            if let Ok(index) = current.parse::<usize>()
                && let Some(next) = items.get_mut(index)
            {
                remove_json_field_path_inner(next, tail);
            }
        }
        _ => {}
    }
}
