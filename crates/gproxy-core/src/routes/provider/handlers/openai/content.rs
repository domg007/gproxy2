use super::*;

pub(in crate::routes::provider) async fn openai_chat_completions(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let value = parse_json_body::<serde_json::Value>(
        &body,
        "invalid openai chat completions request body",
    )?;
    let operation = if stream_enabled(&value) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let payload = TransformRequestPayload::from_bytes(
        operation,
        ProtocolKind::OpenAiChatCompletion,
        build_openai_payload(
            value,
            &headers,
            "invalid openai chat completions request body",
        )?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_chat_completions_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body = parse_json_body::<serde_json::Value>(
        &body,
        "invalid openai chat completions request body",
    )?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI chat completions request body",
        "`model` in OpenAI chat completions request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI chat completions request body",
    )?;
    let operation = if stream_enabled(&body) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let body = build_openai_payload(
        body,
        &headers,
        "invalid openai chat completions request body",
    )?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload =
        TransformRequestPayload::from_bytes(operation, ProtocolKind::OpenAiChatCompletion, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_responses(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let value =
        parse_json_body::<serde_json::Value>(&body, "invalid openai responses request body")?;
    let operation = if stream_enabled(&value) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let payload = TransformRequestPayload::from_bytes(
        operation,
        ProtocolKind::OpenAi,
        build_openai_payload(value, &headers, "invalid openai responses request body")?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_responses_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body =
        parse_json_body::<serde_json::Value>(&body, "invalid openai responses request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI responses request body",
        "`model` in OpenAI responses request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI responses request body",
    )?;
    let operation = if stream_enabled(&body) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let body = build_openai_payload(body, &headers, "invalid openai responses request body")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(operation, ProtocolKind::OpenAi, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_create_image(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let value = parse_json_body::<serde_json::Value>(
        &body,
        "invalid openai image generation request body",
    )?;
    let operation = if stream_enabled(&value) {
        OperationFamily::StreamCreateImage
    } else {
        OperationFamily::CreateImage
    };
    let payload = TransformRequestPayload::from_bytes(
        operation,
        ProtocolKind::OpenAi,
        build_openai_payload(
            value,
            &headers,
            "invalid openai image generation request body",
        )?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_create_image_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body = parse_json_body::<serde_json::Value>(
        &body,
        "invalid openai image generation request body",
    )?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI image generation request body",
        "`model` in OpenAI image generation request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI image generation request body",
    )?;
    let operation = if stream_enabled(&body) {
        OperationFamily::StreamCreateImage
    } else {
        OperationFamily::CreateImage
    };
    let body = build_openai_payload(
        body,
        &headers,
        "invalid openai image generation request body",
    )?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(operation, ProtocolKind::OpenAi, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_create_image_edit(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let value =
        parse_json_body::<serde_json::Value>(&body, "invalid openai image edit request body")?;
    let operation = if stream_enabled(&value) {
        OperationFamily::StreamCreateImageEdit
    } else {
        OperationFamily::CreateImageEdit
    };
    let payload = TransformRequestPayload::from_bytes(
        operation,
        ProtocolKind::OpenAi,
        build_openai_payload(value, &headers, "invalid openai image edit request body")?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_create_image_edit_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body =
        parse_json_body::<serde_json::Value>(&body, "invalid openai image edit request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI image edit request body",
        "`model` in OpenAI image edit request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI image edit request body",
    )?;
    let operation = if stream_enabled(&body) {
        OperationFamily::StreamCreateImageEdit
    } else {
        OperationFamily::CreateImageEdit
    };
    let body = build_openai_payload(body, &headers, "invalid openai image edit request body")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(operation, ProtocolKind::OpenAi, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_input_tokens(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let value =
        parse_json_body::<serde_json::Value>(&body, "invalid openai input_tokens request body")?;
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::CountToken,
        ProtocolKind::OpenAi,
        build_openai_payload(value, &headers, "invalid openai input_tokens request body")?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_input_tokens_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body =
        parse_json_body::<serde_json::Value>(&body, "invalid openai input_tokens request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI input_tokens request body",
        "`model` in OpenAI input_tokens request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI input_tokens request body",
    )?;
    let body = build_openai_payload(body, &headers, "invalid openai input_tokens request body")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::CountToken,
        ProtocolKind::OpenAi,
        body,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_embeddings(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let value =
        parse_json_body::<serde_json::Value>(&body, "invalid openai embeddings request body")?;
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::Embedding,
        ProtocolKind::OpenAi,
        build_openai_payload(value, &headers, "invalid openai embeddings request body")?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_embeddings_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body =
        parse_json_body::<serde_json::Value>(&body, "invalid openai embeddings request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI embeddings request body",
        "`model` in OpenAI embeddings request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI embeddings request body",
    )?;
    let body = build_openai_payload(body, &headers, "invalid openai embeddings request body")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload =
        TransformRequestPayload::from_bytes(OperationFamily::Embedding, ProtocolKind::OpenAi, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_compact(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let value = parse_json_body::<serde_json::Value>(&body, "invalid openai compact request body")?;
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::Compact,
        ProtocolKind::OpenAi,
        build_openai_payload(value, &headers, "invalid openai compact request body")?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_compact_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body =
        parse_json_body::<serde_json::Value>(&body, "invalid openai compact request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI compact request body",
        "`model` in OpenAI compact request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI compact request body",
    )?;
    let body = build_openai_payload(body, &headers, "invalid openai compact request body")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload =
        TransformRequestPayload::from_bytes(OperationFamily::Compact, ProtocolKind::OpenAi, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_create_video(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = openai_create_video_request::OpenAiCreateVideoRequest::default();
    request.headers.extra = collect_passthrough_headers(&headers);
    request.body = serde_json::from_slice(&body)
        .map_err(|err| bad_request(format!("invalid openai video creation request body: {err}")))?;
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::CreateVideoOpenAi(request),
    )
    .await
    .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_video_get(
    State(state): State<Arc<AppState>>,
    Path((provider_name, video_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = openai_video_get_request::OpenAiVideoGetRequest::default();
    request.path.video_id = video_id;
    request.headers.extra = collect_passthrough_headers(&headers);
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::VideoGetOpenAi(request),
    )
    .await
    .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_video_content_get(
    State(state): State<Arc<AppState>>,
    Path((provider_name, video_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = openai_video_content_get_request::OpenAiVideoContentGetRequest::default();
    request.path.video_id = video_id;
    request.query.variant = parse_openai_video_content_variant(query.as_deref())?;
    request.headers.extra = collect_passthrough_headers(&headers);
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::VideoContentGetOpenAi(request),
    )
    .await
    .map_err(HttpError::from)
}

fn parse_openai_video_content_variant(
    query: Option<&str>,
) -> Result<Option<openai_video_content_get_types::OpenAiVideoContentVariant>, HttpError> {
    let Some(raw) = parse_query_value(query, "variant") else {
        return Ok(None);
    };
    serde_json::from_value(serde_json::Value::String(raw.clone())).map(Some).map_err(|_| {
        bad_request(format!(
            "invalid query parameter `variant`: {raw}"
        ))
    })
}
