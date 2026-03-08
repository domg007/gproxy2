use super::*;

pub(in crate::routes::provider) async fn v1_model_list(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let mut auth = authorize_provider_access(&headers, &state)?;
    let (channel, mut provider) = resolve_provider(&state, provider_name.as_str())?;
    if let Some(credential_id) =
        parse_optional_query_value::<i64>(query.as_deref(), "credential_id")?
    {
        provider = restrict_provider_to_credential(provider, credential_id)?;
        auth.forced_credential_id = Some(credential_id);
    }
    let passthrough_headers = collect_passthrough_headers(&headers);

    let mut openai = openai_model_list_request::OpenAiModelListRequest::default();
    openai.headers.extra = passthrough_headers.clone();

    let mut claude = claude_model_list_request::ClaudeModelListRequest::default();
    let (version, beta) = anthropic_headers_from_request(&headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.headers.extra = passthrough_headers.clone();
    claude.query.after_id = parse_query_value(query.as_deref(), "after_id");
    claude.query.before_id = parse_query_value(query.as_deref(), "before_id");
    claude.query.limit = parse_optional_query_value::<u16>(query.as_deref(), "limit")?;

    let mut gemini = gemini_model_list_request::GeminiModelListRequest::default();
    gemini.headers.extra = passthrough_headers;
    gemini.query.page_size = parse_optional_query_value::<u32>(query.as_deref(), "pageSize")?;
    gemini.query.page_token = parse_query_value(query.as_deref(), "pageToken");

    openai.query = openai_model_list_request::QueryParameters::default();

    let candidates = match model_protocol_preference(&headers, query.as_deref()) {
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

    execute_transform_candidates(state, channel, provider, auth, candidates).await
}

pub(in crate::routes::provider) async fn v1_model_list_unscoped(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let ids = collect_unscoped_model_ids(state, auth, &headers).await;
    let data = ids
        .into_iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": 0,
                "owned_by": "GPROXY",
            })
        })
        .collect::<Vec<_>>();
    let body = serde_json::to_vec(&json!({
        "object": "list",
        "data": data,
    }))
    .map_err(|err| internal_error(format!("serialize model list response failed: {err}")))?;
    response_from_status_headers_and_bytes(
        StatusCode::OK,
        &[("content-type".to_string(), "application/json".to_string())],
        body,
    )
    .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn v1_model_get(
    State(state): State<Arc<AppState>>,
    Path((provider_name, model_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let passthrough_headers = collect_passthrough_headers(&headers);

    let mut openai = openai_model_get_request::OpenAiModelGetRequest::default();
    openai.path.model = model_id.clone();
    openai.headers.extra = passthrough_headers.clone();

    let mut claude = claude_model_get_request::ClaudeModelGetRequest::default();
    let (version, beta) = anthropic_headers_from_request(&headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.headers.extra = passthrough_headers.clone();
    claude.path.model_id = model_id.clone();

    let mut gemini = gemini_model_get_request::GeminiModelGetRequest::default();
    gemini.path.name = normalize_gemini_model_path(model_id.as_str())?;
    gemini.headers.extra = passthrough_headers;

    let candidates = match model_protocol_preference(&headers, query.as_deref()) {
        ModelProtocolPreference::Claude => vec![
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetGemini(gemini),
        ],
        ModelProtocolPreference::Gemini => vec![TransformRequest::ModelGetGemini(gemini)],
        ModelProtocolPreference::OpenAi => vec![
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetGemini(gemini),
        ],
    };

    execute_transform_candidates(state, channel, provider, auth, candidates).await
}

pub(in crate::routes::provider) async fn v1_model_get_unscoped(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (provider_name, stripped_model_id) =
        split_provider_prefixed_plain_model(model_id.as_str())?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let passthrough_headers = collect_passthrough_headers(&headers);

    let mut openai = openai_model_get_request::OpenAiModelGetRequest::default();
    openai.path.model = stripped_model_id.clone();
    openai.headers.extra = passthrough_headers.clone();

    let mut claude = claude_model_get_request::ClaudeModelGetRequest::default();
    let (version, beta) = anthropic_headers_from_request(&headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.headers.extra = passthrough_headers.clone();
    claude.path.model_id = stripped_model_id.clone();

    let mut gemini = gemini_model_get_request::GeminiModelGetRequest::default();
    gemini.path.name = normalize_gemini_model_path(stripped_model_id.as_str())?;
    gemini.headers.extra = passthrough_headers;

    let candidates = match model_protocol_preference(&headers, query.as_deref()) {
        ModelProtocolPreference::Claude => vec![
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetGemini(gemini),
        ],
        ModelProtocolPreference::Gemini => vec![TransformRequest::ModelGetGemini(gemini)],
        ModelProtocolPreference::OpenAi => vec![
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetGemini(gemini),
        ],
    };

    execute_transform_candidates(state, channel, provider, auth, candidates).await
}
