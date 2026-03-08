use super::*;

pub(crate) fn extract_local_count_input_tokens(
    response: &gproxy_middleware::TransformResponse,
) -> Option<i64> {
    match response {
        gproxy_middleware::TransformResponse::CountTokenOpenAi(
            openai_count_tokens_response::OpenAiCountTokensResponse::Success { body, .. },
        ) => i64::try_from(body.input_tokens).ok(),
        gproxy_middleware::TransformResponse::CountTokenClaude(
            claude_count_tokens_response::ClaudeCountTokensResponse::Success { body, .. },
        ) => i64::try_from(body.input_tokens).ok(),
        gproxy_middleware::TransformResponse::CountTokenGemini(
            gemini_count_tokens_response::GeminiCountTokensResponse::Success { body, .. },
        ) => i64::try_from(body.total_tokens).ok(),
        _ => None,
    }
}

pub(crate) fn extract_count_tokens_from_raw_json(
    protocol: ProtocolKind,
    body: &[u8],
) -> Option<i64> {
    match protocol {
        ProtocolKind::OpenAi | ProtocolKind::OpenAiChatCompletion => {
            if let Ok(value) =
                serde_json::from_slice::<openai_count_tokens_response::ResponseBody>(body)
            {
                return i64::try_from(value.input_tokens).ok();
            }
            serde_json::from_slice::<openai_count_tokens_response::OpenAiCountTokensResponse>(body)
                .ok()
                .and_then(|value| match value {
                    openai_count_tokens_response::OpenAiCountTokensResponse::Success {
                        body,
                        ..
                    } => i64::try_from(body.input_tokens).ok(),
                    _ => None,
                })
        }
        ProtocolKind::Claude => {
            if let Ok(value) =
                serde_json::from_slice::<claude_count_tokens_response::ResponseBody>(body)
            {
                return i64::try_from(value.input_tokens).ok();
            }
            serde_json::from_slice::<claude_count_tokens_response::ClaudeCountTokensResponse>(body)
                .ok()
                .and_then(|value| match value {
                    claude_count_tokens_response::ClaudeCountTokensResponse::Success {
                        body,
                        ..
                    } => i64::try_from(body.input_tokens).ok(),
                    _ => None,
                })
        }
        ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
            if let Ok(value) =
                serde_json::from_slice::<gemini_count_tokens_response::ResponseBody>(body)
            {
                return i64::try_from(value.total_tokens).ok();
            }
            serde_json::from_slice::<gemini_count_tokens_response::GeminiCountTokensResponse>(body)
                .ok()
                .and_then(|value| match value {
                    gemini_count_tokens_response::GeminiCountTokensResponse::Success {
                        body,
                        ..
                    } => i64::try_from(body.total_tokens).ok(),
                    _ => None,
                })
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UsageMetrics {
    pub(crate) input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) cache_read_input_tokens: Option<i64>,
    pub(crate) cache_creation_input_tokens: Option<i64>,
    pub(crate) cache_creation_input_tokens_5min: Option<i64>,
    pub(crate) cache_creation_input_tokens_1h: Option<i64>,
}

pub(crate) fn u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn usage_metrics_from_openai_response_usage(usage: &ResponseUsage) -> UsageMetrics {
    UsageMetrics {
        input_tokens: Some(u64_to_i64(usage.input_tokens)),
        output_tokens: Some(u64_to_i64(usage.output_tokens)),
        cache_read_input_tokens: Some(u64_to_i64(usage.input_tokens_details.cached_tokens)),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    }
}

pub(crate) fn usage_metrics_from_openai_compact_usage(
    usage: &CompactResponseUsage,
) -> UsageMetrics {
    UsageMetrics {
        input_tokens: Some(u64_to_i64(usage.input_tokens)),
        output_tokens: Some(u64_to_i64(usage.output_tokens)),
        cache_read_input_tokens: Some(u64_to_i64(usage.input_tokens_details.cached_tokens)),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    }
}

pub(crate) fn usage_metrics_from_openai_chat_completion_usage(
    usage: &CompletionUsage,
) -> UsageMetrics {
    UsageMetrics {
        input_tokens: Some(u64_to_i64(usage.prompt_tokens)),
        output_tokens: Some(u64_to_i64(usage.completion_tokens)),
        cache_read_input_tokens: usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|value| value.cached_tokens)
            .map(u64_to_i64),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    }
}

pub(crate) fn usage_metrics_from_claude_usage(usage: &BetaUsage) -> UsageMetrics {
    let input_tokens = usage
        .input_tokens
        .saturating_add(usage.cache_creation_input_tokens)
        .saturating_add(usage.cache_read_input_tokens);
    UsageMetrics {
        input_tokens: Some(u64_to_i64(input_tokens)),
        output_tokens: Some(u64_to_i64(usage.output_tokens)),
        cache_read_input_tokens: Some(u64_to_i64(usage.cache_read_input_tokens)),
        cache_creation_input_tokens: Some(u64_to_i64(usage.cache_creation_input_tokens)),
        cache_creation_input_tokens_5min: Some(u64_to_i64(
            usage.cache_creation.ephemeral_5m_input_tokens,
        )),
        cache_creation_input_tokens_1h: Some(u64_to_i64(
            usage.cache_creation.ephemeral_1h_input_tokens,
        )),
    }
}

pub(crate) fn usage_metrics_from_gemini_usage(usage: &GeminiUsageMetadata) -> UsageMetrics {
    let input_tokens = usage
        .prompt_token_count
        .unwrap_or(0)
        .saturating_add(usage.cached_content_token_count.unwrap_or(0));
    UsageMetrics {
        input_tokens: usage
            .prompt_token_count
            .or(usage.cached_content_token_count)
            .map(|_| u64_to_i64(input_tokens)),
        output_tokens: usage.candidates_token_count.map(u64_to_i64),
        cache_read_input_tokens: usage.cached_content_token_count.map(u64_to_i64),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    }
}

pub(crate) fn usage_metrics_from_openai_embeddings_usage(
    usage: &OpenAiEmbeddingUsage,
) -> UsageMetrics {
    let prompt_tokens = u64_to_i64(usage.prompt_tokens);
    let total_tokens = u64_to_i64(usage.total_tokens);
    UsageMetrics {
        input_tokens: Some(prompt_tokens),
        output_tokens: Some(total_tokens.saturating_sub(prompt_tokens)),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    }
}

pub(crate) fn usage_metrics_from_openai_image_usage(
    usage: &gproxy_protocol::openai::create_image::types::OpenAiImageUsage,
) -> UsageMetrics {
    UsageMetrics {
        input_tokens: Some(u64_to_i64(usage.input_tokens)),
        output_tokens: Some(u64_to_i64(usage.output_tokens)),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    }
}

pub(crate) fn extract_usage_from_local_response(
    response: &gproxy_middleware::TransformResponse,
) -> Option<UsageMetrics> {
    match response {
        gproxy_middleware::TransformResponse::CountTokenOpenAi(
            openai_count_tokens_response::OpenAiCountTokensResponse::Success { body, .. },
        ) => Some(UsageMetrics {
            input_tokens: Some(u64_to_i64(body.input_tokens)),
            output_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
            cache_creation_input_tokens_5min: None,
            cache_creation_input_tokens_1h: None,
        }),
        gproxy_middleware::TransformResponse::CountTokenClaude(
            claude_count_tokens_response::ClaudeCountTokensResponse::Success { body, .. },
        ) => Some(UsageMetrics {
            input_tokens: Some(u64_to_i64(body.input_tokens)),
            output_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
            cache_creation_input_tokens_5min: None,
            cache_creation_input_tokens_1h: None,
        }),
        gproxy_middleware::TransformResponse::CountTokenGemini(
            gemini_count_tokens_response::GeminiCountTokensResponse::Success { body, .. },
        ) => Some(UsageMetrics {
            input_tokens: Some(u64_to_i64(body.total_tokens)),
            output_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
            cache_creation_input_tokens_5min: None,
            cache_creation_input_tokens_1h: None,
        }),
        gproxy_middleware::TransformResponse::GenerateContentOpenAiResponse(
            openai_create_response_response::OpenAiCreateResponseResponse::Success { body, .. },
        ) => body
            .usage
            .as_ref()
            .map(usage_metrics_from_openai_response_usage),
        gproxy_middleware::TransformResponse::GenerateContentOpenAiChatCompletions(
            openai_chat_completions_response::OpenAiChatCompletionsResponse::Success {
                body, ..
            },
        ) => body
            .usage
            .as_ref()
            .map(usage_metrics_from_openai_chat_completion_usage),
        gproxy_middleware::TransformResponse::GenerateContentClaude(
            claude_create_message_response::ClaudeCreateMessageResponse::Success { body, .. },
        ) => Some(usage_metrics_from_claude_usage(&body.usage)),
        gproxy_middleware::TransformResponse::GenerateContentGemini(
            gemini_generate_content_response::GeminiGenerateContentResponse::Success {
                body, ..
            },
        ) => body
            .usage_metadata
            .as_ref()
            .map(usage_metrics_from_gemini_usage),
        gproxy_middleware::TransformResponse::OpenAiResponseWebSocket(messages) => {
            match openai_create_response_response::OpenAiCreateResponseResponse::try_from(
                messages.clone(),
            ) {
                Ok(openai_create_response_response::OpenAiCreateResponseResponse::Success {
                    body,
                    ..
                }) => body
                    .usage
                    .as_ref()
                    .map(usage_metrics_from_openai_response_usage),
                _ => None,
            }
        }
        gproxy_middleware::TransformResponse::GeminiLive(messages) => {
            match gemini_generate_content_response::GeminiGenerateContentResponse::try_from(
                messages.clone(),
            ) {
                Ok(gemini_generate_content_response::GeminiGenerateContentResponse::Success {
                    body,
                    ..
                }) => body
                    .usage_metadata
                    .as_ref()
                    .map(usage_metrics_from_gemini_usage),
                _ => None,
            }
        }
        gproxy_middleware::TransformResponse::CreateImageOpenAi(
            gproxy_protocol::openai::create_image::response::OpenAiCreateImageResponse::Success {
                body, ..
            },
        ) => body
            .usage
            .as_ref()
            .map(usage_metrics_from_openai_image_usage),
        gproxy_middleware::TransformResponse::CreateImageEditOpenAi(
            gproxy_protocol::openai::create_image_edit::response::OpenAiCreateImageEditResponse::Success {
                body, ..
            },
        ) => body
            .usage
            .as_ref()
            .map(usage_metrics_from_openai_image_usage),
        gproxy_middleware::TransformResponse::EmbeddingOpenAi(
            openai_embeddings_response::OpenAiEmbeddingsResponse::Success { body, .. },
        ) => Some(usage_metrics_from_openai_embeddings_usage(&body.usage)),
        gproxy_middleware::TransformResponse::CompactOpenAi(
            openai_compact_response_response::OpenAiCompactResponse::Success { body, .. },
        ) => Some(usage_metrics_from_openai_compact_usage(&body.usage)),
        _ => None,
    }
}

pub(crate) fn decode_response_for_usage(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Option<gproxy_middleware::TransformResponse> {
    gproxy_middleware::decode_response_payload(operation, protocol, body).ok()
}

pub(crate) fn normalize_upstream_response_body_for_channel(
    channel: &ChannelId,
    body: &[u8],
) -> Option<Vec<u8>> {
    provider_normalize_upstream_response_body_for_channel(channel, body)
}

pub(crate) fn normalize_upstream_stream_ndjson_chunk_for_channel(
    channel: &ChannelId,
    chunk: &[u8],
) -> Option<Vec<u8>> {
    provider_normalize_upstream_stream_ndjson_chunk_for_channel(channel, chunk)
}

pub(crate) fn is_wrapped_stream_channel(channel: &ChannelId) -> bool {
    matches!(
        channel,
        ChannelId::Builtin(BuiltinChannel::GeminiCli)
            | ChannelId::Builtin(BuiltinChannel::Antigravity)
    )
}

pub(crate) fn ndjson_chunk_to_sse_chunk(chunk: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(chunk) else {
        return chunk.to_vec();
    };
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        out.push_str("data: ");
        out.push_str(line);
        out.push_str("\n\n");
    }
    out.into_bytes()
}

pub(crate) fn strip_model_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.retain(|key, _| !key.eq_ignore_ascii_case("model"));
            for child in object.values_mut() {
                strip_model_fields(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_model_fields(item);
            }
        }
        _ => {}
    }
}
