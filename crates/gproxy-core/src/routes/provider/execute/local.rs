use super::*;

pub(crate) fn build_openai_local_count_response(
    input_tokens: u64,
) -> openai_count_tokens_response::OpenAiCountTokensResponse {
    openai_count_tokens_response::OpenAiCountTokensResponse::Success {
        stats_code: StatusCode::OK,
        headers: gproxy_protocol::openai::types::OpenAiResponseHeaders::default(),
        body: openai_count_tokens_response::ResponseBody {
            input_tokens,
            object: openai_count_tokens_response::OpenAiCountTokensObject::ResponseInputTokens,
        },
    }
}

pub(crate) fn serialize_local_response_body(
    response: &gproxy_middleware::TransformResponse,
) -> Result<Vec<u8>, UpstreamError> {
    match response {
        gproxy_middleware::TransformResponse::VideoContentGetOpenAi(
            gproxy_protocol::openai::video_content_get::response::OpenAiVideoContentGetResponse::Success { body, .. },
        ) => return Ok(body.bytes.clone()),
        gproxy_middleware::TransformResponse::VideoContentGetGemini(
            gproxy_protocol::gemini::video_content_get::response::GeminiVideoContentGetResponse::Success { body, .. },
        ) => return Ok(body.bytes.clone()),
        _ => {}
    }

    let value = serde_json::to_value(response)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let inner = match value {
        serde_json::Value::Object(object) if object.len() == 1 => {
            if let Some((_, inner)) = object.into_iter().next() {
                inner
            } else {
                return Ok(Vec::new());
            }
        }
        other => other,
    };

    if let serde_json::Value::Object(wrapper) = &inner
        && wrapper.contains_key("stats_code")
        && wrapper.contains_key("body")
    {
        let body = wrapper
            .get("body")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        return match body {
            serde_json::Value::String(text) => Ok(text.into_bytes()),
            other => serde_json::to_vec(&other)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string())),
        };
    }

    serde_json::to_vec(&inner).map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

pub(crate) fn local_response_status_and_headers(
    response: &gproxy_middleware::TransformResponse,
) -> Option<(StatusCode, Vec<(String, String)>)> {
    match response {
        gproxy_middleware::TransformResponse::VideoContentGetOpenAi(
            gproxy_protocol::openai::video_content_get::response::OpenAiVideoContentGetResponse::Success {
                stats_code,
                headers,
                ..
            },
        ) => Some((*stats_code, headers.extra.clone().into_iter().collect())),
        gproxy_middleware::TransformResponse::VideoContentGetOpenAi(
            gproxy_protocol::openai::video_content_get::response::OpenAiVideoContentGetResponse::Error {
                stats_code,
                headers,
                ..
            },
        ) => Some((*stats_code, headers.extra.clone().into_iter().collect())),
        gproxy_middleware::TransformResponse::VideoContentGetGemini(
            gproxy_protocol::gemini::video_content_get::response::GeminiVideoContentGetResponse::Success {
                stats_code,
                headers,
                ..
            },
        ) => Some((*stats_code, headers.extra.clone().into_iter().collect())),
        gproxy_middleware::TransformResponse::VideoContentGetGemini(
            gproxy_protocol::gemini::video_content_get::response::GeminiVideoContentGetResponse::Error {
                stats_code,
                headers,
                ..
            },
        ) => Some((*stats_code, headers.extra.clone().into_iter().collect())),
        _ => None,
    }
}

pub(crate) async fn execute_local_count_token_request(
    state: &AppState,
    request: &TransformRequest,
) -> Result<UpstreamResponse, UpstreamError> {
    let openai_request = match request {
        TransformRequest::CountTokenOpenAi(value) => value.clone(),
        TransformRequest::CountTokenClaude(value) => {
            openai_count_tokens_request::OpenAiCountTokensRequest::try_from(value.clone())
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
        }
        TransformRequest::CountTokenGemini(value) => {
            openai_count_tokens_request::OpenAiCountTokensRequest::try_from(value.clone())
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
        }
        _ => return Err(UpstreamError::UnsupportedRequest),
    };

    let mut normalized = serde_json::to_value(&openai_request.body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    if let Some(object) = normalized.as_object_mut() {
        object.remove("model");
    }
    let text = serde_json::to_string(&normalized)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let model = openai_request
        .body
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("deepseek_fallback");

    let token_count = state
        .count_tokens_with_local_tokenizer(model, text.as_str())
        .await
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
        .count as u64;

    let response = match request {
        TransformRequest::CountTokenOpenAi(_) => {
            gproxy_middleware::TransformResponse::CountTokenOpenAi(
                build_openai_local_count_response(token_count),
            )
        }
        TransformRequest::CountTokenClaude(_) => {
            gproxy_middleware::TransformResponse::CountTokenClaude(
                claude_count_tokens_response::ClaudeCountTokensResponse::try_from(
                    build_openai_local_count_response(token_count),
                )
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
            )
        }
        TransformRequest::CountTokenGemini(_) => {
            gproxy_middleware::TransformResponse::CountTokenGemini(
                gemini_count_tokens_response::GeminiCountTokensResponse::try_from(
                    build_openai_local_count_response(token_count),
                )
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
            )
        }
        _ => return Err(UpstreamError::UnsupportedRequest),
    };

    Ok(UpstreamResponse::from_local(response))
}

pub(crate) async fn execute_local_request(
    state: &AppState,
    provider: &ProviderDefinition,
    request: &TransformRequest,
) -> Result<UpstreamResponse, UpstreamError> {
    if let Some(local) = try_local_response_for_channel(provider, request)? {
        return Ok(UpstreamResponse::from_local(local));
    }

    execute_local_count_token_request(state, request).await
}
