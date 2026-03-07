use super::*;

pub(super) fn route_from_implementation(
    src_route: RouteKey,
    implementation: RouteImplementation,
) -> Option<TransformRoute> {
    match implementation {
        RouteImplementation::Passthrough => Some(TransformRoute {
            src_operation: src_route.operation,
            src_protocol: src_route.protocol,
            dst_operation: src_route.operation,
            dst_protocol: src_route.protocol,
        }),
        RouteImplementation::TransformTo { destination } => Some(TransformRoute {
            src_operation: src_route.operation,
            src_protocol: src_route.protocol,
            dst_operation: destination.operation,
            dst_protocol: destination.protocol,
        }),
        RouteImplementation::Local | RouteImplementation::Unsupported => None,
    }
}

pub(super) fn strip_provider_prefix_from_unscoped_openai_ws_message(
    message: &mut OpenAiCreateResponseWebSocketClientMessage,
) -> Result<String, String> {
    let OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(create) = message else {
        return Err("unscoped websocket first frame must be `response.create`".to_string());
    };
    let Some(model) = create.request.model.clone() else {
        return Err("unscoped websocket `response.create` requires `model`".to_string());
    };
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model.as_str())
        .map_err(|err| format!("invalid unscoped websocket model: {err:?}"))?;
    create.request.model = Some(stripped_model);
    Ok(provider_name)
}

pub(super) fn openai_ws_query_from_uri(uri: &Uri) -> OpenAiCreateResponseWebSocketQueryParameters {
    let mut query = OpenAiCreateResponseWebSocketQueryParameters::default();
    for (key, value) in form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes()) {
        let key = key.into_owned();
        let value = value.into_owned();
        if key.eq_ignore_ascii_case("api-version") {
            query.api_version = Some(value);
        } else {
            query.extra.insert(key, value);
        }
    }
    query
}

pub(super) fn openai_ws_headers_from_upgrade_headers(
    headers: &HeaderMap,
) -> OpenAiCreateResponseWebSocketRequestHeaders {
    let mut out = OpenAiCreateResponseWebSocketRequestHeaders::default();
    let mut extra = collect_websocket_passthrough_headers(headers);
    out.openai_beta = extra.remove("openai-beta");
    out.x_codex_turn_state = extra.remove("x-codex-turn-state");
    out.x_codex_turn_metadata = extra.remove("x-codex-turn-metadata");
    out.session_id = extra
        .remove("session_id")
        .or_else(|| extra.remove("session-id"));
    out.chatgpt_account_id = extra.remove("chatgpt-account-id");
    out.extra = extra;
    out
}

pub(super) fn prepare_upstream_websocket_request(
    channel: &ChannelId,
    provider: &ProviderDefinition,
    upstream_request: &TransformRequest,
    credential: &CredentialRef,
) -> Result<(String, Vec<(String, String)>), String> {
    let base_url = to_websocket_base_url(provider.settings.base_url())?;
    match upstream_request {
        TransformRequest::OpenAiResponseWebSocket(request) => {
            let path = if matches!(channel, ChannelId::Builtin(BuiltinChannel::Codex)) {
                "/responses"
            } else {
                "/v1/responses"
            };

            let mut query_pairs = Vec::new();
            if let Some(api_version) = request.query.api_version.as_deref() {
                query_pairs.push(("api-version".to_string(), api_version.to_string()));
            }
            for (key, value) in &request.query.extra {
                query_pairs.push((key.clone(), value.clone()));
            }

            let mut headers = Vec::new();
            if let Some(value) = request.headers.authorization.as_deref() {
                add_or_replace_header(&mut headers, "authorization", value.to_string());
            }
            if let Some(value) = request.headers.openai_beta.as_deref() {
                add_or_replace_header(&mut headers, "openai-beta", value.to_string());
            }
            if let Some(value) = request.headers.x_codex_turn_state.as_deref() {
                add_or_replace_header(&mut headers, "x-codex-turn-state", value.to_string());
            }
            if let Some(value) = request.headers.x_codex_turn_metadata.as_deref() {
                add_or_replace_header(&mut headers, "x-codex-turn-metadata", value.to_string());
            }
            if let Some(value) = request.headers.session_id.as_deref() {
                add_or_replace_header(&mut headers, "session_id", value.to_string());
            }
            if let Some(value) = request.headers.chatgpt_account_id.as_deref() {
                add_or_replace_header(&mut headers, "chatgpt-account-id", value.to_string());
            }
            for (key, value) in &request.headers.extra {
                add_or_replace_header(&mut headers, key.as_str(), value.clone());
            }

            match (&channel, &credential.credential) {
                (
                    ChannelId::Builtin(BuiltinChannel::OpenAi),
                    ChannelCredential::Builtin(BuiltinChannelCredential::OpenAi(value)),
                ) => {
                    add_or_replace_header(
                        &mut headers,
                        "authorization",
                        format!("Bearer {}", value.api_key.trim()),
                    );
                    add_or_replace_header(
                        &mut headers,
                        "user-agent",
                        default_websocket_user_agent(provider),
                    );
                }
                (
                    ChannelId::Builtin(BuiltinChannel::Codex),
                    ChannelCredential::Builtin(BuiltinChannelCredential::Codex(value)),
                ) => {
                    add_or_replace_header(
                        &mut headers,
                        "authorization",
                        format!("Bearer {}", value.access_token.trim()),
                    );
                    add_or_replace_header(
                        &mut headers,
                        "chatgpt-account-id",
                        value.account_id.trim().to_string(),
                    );
                    add_or_replace_header(&mut headers, "originator", "codex_vscode".to_string());
                    if !headers.iter().any(|(name, value)| {
                        name.eq_ignore_ascii_case("openai-beta")
                            && value.contains("responses_websockets=")
                    }) {
                        add_or_replace_header(
                            &mut headers,
                            "openai-beta",
                            "responses_websockets=2026-02-04".to_string(),
                        );
                    }
                    add_or_replace_header(
                        &mut headers,
                        "user-agent",
                        provider
                            .settings
                            .user_agent()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .unwrap_or("codex_vscode/0.110.0")
                            .to_string(),
                    );
                }
                _ => {
                    return Err(format!(
                        "provider {} credential type does not support OpenAI websocket upstream",
                        channel.as_str()
                    ));
                }
            }

            let url = build_websocket_url(base_url.as_str(), path, query_pairs.as_slice());
            Ok((url, headers))
        }
        TransformRequest::GeminiLive(request) => {
            let rpc = match request.path.rpc {
                gproxy_protocol::gemini::live::request::GeminiLiveRpcMethod::BidiGenerateContent => {
                    "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent"
                }
                gproxy_protocol::gemini::live::request::GeminiLiveRpcMethod::BidiGenerateContentConstrained => {
                    "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContentConstrained"
                }
            };
            let path = format!("/ws/{rpc}");
            let mut query_pairs = Vec::new();
            if let Some(value) = request
                .query
                .key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                query_pairs.push(("key".to_string(), value.to_string()));
            }
            if let Some(value) = request
                .query
                .access_token
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                query_pairs.push(("access_token".to_string(), value.to_string()));
            }
            let mut headers = Vec::new();
            if let Some(value) = request
                .headers
                .authorization
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                add_or_replace_header(&mut headers, "authorization", value.to_string());
            }
            for (key, value) in &request.headers.extra {
                add_or_replace_header(&mut headers, key.as_str(), value.clone());
            }

            match (&channel, &credential.credential) {
                (
                    ChannelId::Builtin(BuiltinChannel::AiStudio),
                    ChannelCredential::Builtin(BuiltinChannelCredential::AiStudio(value)),
                ) => {
                    add_or_replace_query(&mut query_pairs, "key", value.api_key.trim().to_string());
                    add_or_replace_header(
                        &mut headers,
                        "x-goog-api-key",
                        value.api_key.trim().to_string(),
                    );
                    add_or_replace_header(
                        &mut headers,
                        "user-agent",
                        default_websocket_user_agent(provider),
                    );
                }
                _ => {
                    return Err(format!(
                        "provider {} credential type does not support Gemini Live websocket upstream",
                        channel.as_str()
                    ));
                }
            }

            let url = build_websocket_url(base_url.as_str(), path.as_str(), query_pairs.as_slice());
            Ok((url, headers))
        }
        _ => Err("upstream transform request is not a websocket request".to_string()),
    }
}

fn openai_model_hint_from_connect_request(
    request: &OpenAiCreateResponseWebSocketConnectRequest,
) -> Option<String> {
    match request.body.as_ref() {
        Some(OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(create)) => {
            create.request.model.clone()
        }
        _ => None,
    }
}

fn gemini_live_model_hint_from_connect_request(
    request: &GeminiLiveConnectRequest,
) -> Option<String> {
    let body = request.body.as_ref()?;
    let value = serde_json::to_value(body).ok()?;
    value
        .pointer("/setup/model")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(super) fn websocket_model_hint_from_upstream_request(
    request: &TransformRequest,
) -> Option<String> {
    match request {
        TransformRequest::OpenAiResponseWebSocket(value) => {
            openai_model_hint_from_connect_request(value)
        }
        TransformRequest::GeminiLive(value) => gemini_live_model_hint_from_connect_request(value),
        _ => None,
    }
}

fn default_websocket_user_agent(provider: &ProviderDefinition) -> String {
    provider
        .settings
        .user_agent()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            format!(
                "gproxy/{}({},{})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        })
}

fn to_websocket_base_url(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("provider base_url is empty".to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return Ok(format!("wss://{rest}"));
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return Ok(format!("ws://{rest}"));
    }
    if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        return Ok(trimmed.to_string());
    }
    Err(format!(
        "provider base_url has unsupported scheme: {trimmed}"
    ))
}

fn build_websocket_url(base: &str, path: &str, query: &[(String, String)]) -> String {
    let mut url = join_base_url_and_path_local(base, path);
    if !query.is_empty() {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (key, value) in query {
            serializer.append_pair(key, value);
        }
        let encoded = serializer.finish();
        if !encoded.is_empty() {
            if url.contains('?') {
                url.push('&');
            } else {
                url.push('?');
            }
            url.push_str(encoded.as_str());
        }
    }
    url
}

pub(super) fn join_base_url_and_path_local(base_url: &str, path: &str) -> String {
    let normalized_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let base = base_url.trim_end_matches('/');
    if normalized_path.starts_with("/ws/") {
        if let Some(base_without_v1beta1) = base.strip_suffix("/v1beta1") {
            return format!("{base_without_v1beta1}{normalized_path}");
        }
        if let Some(base_without_v1beta) = base.strip_suffix("/v1beta") {
            return format!("{base_without_v1beta}{normalized_path}");
        }
        if let Some(base_without_v1) = base.strip_suffix("/v1") {
            return format!("{base_without_v1}{normalized_path}");
        }
    }
    if let Some(base_without_v1) = base.strip_suffix("/v1")
        && normalized_path.starts_with("/v1/")
    {
        return format!("{base_without_v1}{normalized_path}");
    }
    if let Some(base_without_v1beta) = base.strip_suffix("/v1beta")
        && normalized_path.starts_with("/v1beta/")
    {
        return format!("{base_without_v1beta}{normalized_path}");
    }
    if let Some(base_without_v1beta1) = base.strip_suffix("/v1beta1")
        && normalized_path.starts_with("/v1beta1/")
    {
        return format!("{base_without_v1beta1}{normalized_path}");
    }
    format!("{base}{normalized_path}")
}

fn add_or_replace_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    if let Some(existing) = headers
        .iter_mut()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
    {
        existing.1 = value;
        return;
    }
    headers.push((name.to_string(), value));
}

fn add_or_replace_query(query: &mut Vec<(String, String)>, name: &str, value: String) {
    if let Some(existing) = query
        .iter_mut()
        .find(|(query_name, _)| query_name.eq_ignore_ascii_case(name))
    {
        existing.1 = value;
        return;
    }
    query.push((name.to_string(), value));
}

pub(super) fn required_string_field<'a>(
    value: &'a serde_json::Value,
    pointer: &str,
    missing_message: &str,
    invalid_message: &str,
) -> Result<&'a str, HttpError> {
    let Some(raw) = value.pointer(pointer) else {
        return Err(bad_request(missing_message));
    };
    raw.as_str().ok_or_else(|| bad_request(invalid_message))
}

pub(super) fn set_string_field(
    value: &mut serde_json::Value,
    pointer: &str,
    new_value: String,
    missing_message: &str,
) -> Result<(), HttpError> {
    let Some(slot) = value.pointer_mut(pointer) else {
        return Err(bad_request(missing_message));
    };
    *slot = serde_json::Value::String(new_value);
    Ok(())
}

pub(super) fn stream_enabled(value: &serde_json::Value) -> bool {
    value
        .pointer("/stream")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn encode_json_value(value: &serde_json::Value, context: &str) -> Result<Bytes, HttpError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|err| bad_request(format!("{context}: {err}")))
}

pub(super) fn build_openai_payload(
    body: serde_json::Value,
    headers: &HeaderMap,
    context: &str,
) -> Result<Bytes, HttpError> {
    encode_json_value(
        &json!({
            "method": "POST",
            "path": {},
            "query": {},
            "headers": collect_passthrough_headers(headers),
            "body": body,
        }),
        context,
    )
}
