use super::*;
use gproxy_protocol::openai::create_video::response::OpenAiCreateVideoResponse;
use gproxy_protocol::openai::video_content_get::response::OpenAiVideoContentGetResponse;
use gproxy_protocol::openai::video_get::response::OpenAiVideoGetResponse;

pub(super) fn transform_model_list_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::ModelListOpenAi(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::ModelListOpenAi(response),
            ProtocolKind::Claude => {
                TransformResponse::ModelListClaude(ClaudeModelListResponse::try_from(response)?)
            }
            ProtocolKind::Gemini => {
                TransformResponse::ModelListGemini(GeminiModelListResponse::try_from(response)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_list does not support this destination protocol",
                ));
            }
        },
        TransformResponse::ModelListClaude(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::ModelListOpenAi(OpenAiModelListResponse::try_from(response)?)
            }
            ProtocolKind::Claude => TransformResponse::ModelListClaude(response),
            ProtocolKind::Gemini => {
                TransformResponse::ModelListGemini(GeminiModelListResponse::try_from(response)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_list does not support this destination protocol",
                ));
            }
        },
        TransformResponse::ModelListGemini(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::ModelListOpenAi(OpenAiModelListResponse::try_from(response)?)
            }
            ProtocolKind::Claude => {
                TransformResponse::ModelListClaude(ClaudeModelListResponse::try_from(response)?)
            }
            ProtocolKind::Gemini => TransformResponse::ModelListGemini(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_list does not support this destination protocol",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "model_list response transform requires model_list destination payload",
            ));
        }
    })
}

pub(super) fn transform_model_get_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::ModelGetOpenAi(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::ModelGetOpenAi(response),
            ProtocolKind::Claude => {
                TransformResponse::ModelGetClaude(ClaudeModelGetResponse::try_from(response)?)
            }
            ProtocolKind::Gemini => {
                TransformResponse::ModelGetGemini(GeminiModelGetResponse::try_from(response)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_get does not support this destination protocol",
                ));
            }
        },
        TransformResponse::ModelGetClaude(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::ModelGetOpenAi(OpenAiModelGetResponse::try_from(response)?)
            }
            ProtocolKind::Claude => TransformResponse::ModelGetClaude(response),
            ProtocolKind::Gemini => {
                TransformResponse::ModelGetGemini(GeminiModelGetResponse::try_from(response)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_get does not support this destination protocol",
                ));
            }
        },
        TransformResponse::ModelGetGemini(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::ModelGetOpenAi(OpenAiModelGetResponse::try_from(response)?)
            }
            ProtocolKind::Claude => {
                TransformResponse::ModelGetClaude(ClaudeModelGetResponse::try_from(response)?)
            }
            ProtocolKind::Gemini => TransformResponse::ModelGetGemini(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_get does not support this destination protocol",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "model_get response transform requires model_get destination payload",
            ));
        }
    })
}

pub(super) fn transform_count_tokens_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::CountTokenOpenAi(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::CountTokenOpenAi(response),
            ProtocolKind::Claude => {
                TransformResponse::CountTokenClaude(ClaudeCountTokensResponse::try_from(response)?)
            }
            ProtocolKind::Gemini => {
                TransformResponse::CountTokenGemini(GeminiCountTokensResponse::try_from(response)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "count_token does not support this destination protocol",
                ));
            }
        },
        TransformResponse::CountTokenClaude(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::CountTokenOpenAi(OpenAiCountTokensResponse::try_from(response)?)
            }
            ProtocolKind::Claude => TransformResponse::CountTokenClaude(response),
            ProtocolKind::Gemini => {
                TransformResponse::CountTokenGemini(GeminiCountTokensResponse::try_from(response)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "count_token does not support this destination protocol",
                ));
            }
        },
        TransformResponse::CountTokenGemini(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::CountTokenOpenAi(OpenAiCountTokensResponse::try_from(response)?)
            }
            ProtocolKind::Claude => {
                TransformResponse::CountTokenClaude(ClaudeCountTokensResponse::try_from(response)?)
            }
            ProtocolKind::Gemini => TransformResponse::CountTokenGemini(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "count_token does not support this destination protocol",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "count_token response transform requires count_token destination payload",
            ));
        }
    })
}

pub(super) fn transform_embeddings_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::EmbeddingOpenAi(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::EmbeddingOpenAi(response),
            ProtocolKind::Gemini => {
                TransformResponse::EmbeddingGemini(GeminiEmbedContentResponse::try_from(response)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "embedding supports only openai and gemini",
                ));
            }
        },
        TransformResponse::EmbeddingGemini(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::EmbeddingOpenAi(OpenAiEmbeddingsResponse::try_from(response)?)
            }
            ProtocolKind::Gemini => TransformResponse::EmbeddingGemini(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "embedding supports only openai and gemini",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "embedding response transform requires embedding destination payload",
            ));
        }
    })
}

pub(super) fn transform_create_video_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::CreateVideoOpenAi(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::CreateVideoOpenAi(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "create_video does not support this destination protocol",
                ));
            }
        },
        TransformResponse::CreateVideoGemini(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::CreateVideoOpenAi(OpenAiCreateVideoResponse::try_from(response)?)
            }
            ProtocolKind::Gemini => TransformResponse::CreateVideoGemini(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "create_video does not support this destination protocol",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "create_video response transform requires create_video destination payload",
            ));
        }
    })
}

pub(super) fn transform_video_get_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::VideoGetOpenAi(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::VideoGetOpenAi(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "video_get does not support this destination protocol",
                ));
            }
        },
        TransformResponse::VideoGetGemini(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::VideoGetOpenAi(OpenAiVideoGetResponse::try_from(response)?)
            }
            ProtocolKind::Gemini => TransformResponse::VideoGetGemini(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "video_get does not support this destination protocol",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "video_get response transform requires video_get destination payload",
            ));
        }
    })
}

pub(super) fn transform_video_content_get_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::VideoContentGetOpenAi(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::VideoContentGetOpenAi(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "video_content_get does not support this destination protocol",
                ));
            }
        },
        TransformResponse::VideoContentGetGemini(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::VideoContentGetOpenAi(
                OpenAiVideoContentGetResponse::try_from(response)?,
            ),
            ProtocolKind::Gemini => TransformResponse::VideoContentGetGemini(response),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "video_content_get does not support this destination protocol",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "video_content_get response transform requires video_content_get destination payload",
            ));
        }
    })
}

pub(super) fn transform_generate_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    let dst_protocol = dst_protocol.normalize_gemini_stream();
    Ok(match input {
        TransformResponse::GenerateContentOpenAiResponse(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::GenerateContentOpenAiResponse(response),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::GenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsResponse::try_from(response)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::GenerateContentClaude(
                ClaudeCreateMessageResponse::try_from(response)?,
            ),
            ProtocolKind::Gemini => TransformResponse::GenerateContentGemini(
                GeminiGenerateContentResponse::try_from(response)?,
            ),
            ProtocolKind::GeminiNDJson => {
                return Err(MiddlewareTransformError::Unsupported(
                    "generate_content response does not support GeminiNDJson destination",
                ));
            }
        },
        TransformResponse::GenerateContentOpenAiChatCompletions(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::GenerateContentOpenAiResponse(
                OpenAiCreateResponseResponse::try_from(response)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::GenerateContentOpenAiChatCompletions(response)
            }
            ProtocolKind::Claude => TransformResponse::GenerateContentClaude(
                ClaudeCreateMessageResponse::try_from(response)?,
            ),
            ProtocolKind::Gemini => TransformResponse::GenerateContentGemini(
                GeminiGenerateContentResponse::try_from(response)?,
            ),
            ProtocolKind::GeminiNDJson => {
                return Err(MiddlewareTransformError::Unsupported(
                    "generate_content response does not support GeminiNDJson destination",
                ));
            }
        },
        TransformResponse::GenerateContentClaude(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::GenerateContentOpenAiResponse(
                OpenAiCreateResponseResponse::try_from(response)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::GenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsResponse::try_from(response)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::GenerateContentClaude(response),
            ProtocolKind::Gemini => TransformResponse::GenerateContentGemini(
                GeminiGenerateContentResponse::try_from(response)?,
            ),
            ProtocolKind::GeminiNDJson => {
                return Err(MiddlewareTransformError::Unsupported(
                    "generate_content response does not support GeminiNDJson destination",
                ));
            }
        },
        TransformResponse::GenerateContentGemini(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::GenerateContentOpenAiResponse(
                OpenAiCreateResponseResponse::try_from(response)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::GenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsResponse::try_from(response)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::GenerateContentClaude(
                ClaudeCreateMessageResponse::try_from(response)?,
            ),
            ProtocolKind::Gemini => TransformResponse::GenerateContentGemini(response),
            ProtocolKind::GeminiNDJson => {
                return Err(MiddlewareTransformError::Unsupported(
                    "generate_content response does not support GeminiNDJson destination",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "generate_content response transform requires generate_content destination payload",
            ));
        }
    })
}

pub(super) fn transform_create_image_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::OpenAi {
        return Err(MiddlewareTransformError::Unsupported(
            "create_image response currently requires OpenAi destination protocol",
        ));
    }

    Ok(match input {
        TransformResponse::CreateImageOpenAi(response) => {
            TransformResponse::CreateImageOpenAi(response)
        }
        TransformResponse::GenerateContentOpenAiResponse(response) => {
            TransformResponse::CreateImageOpenAi(OpenAiCreateImageResponse::try_from(response)?)
        }
        TransformResponse::GenerateContentGemini(response) => {
            TransformResponse::CreateImageOpenAi(OpenAiCreateImageResponse::try_from(response)?)
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "create_image response transform requires openai response or gemini generate payload",
            ));
        }
    })
}

pub(super) fn transform_create_image_stream_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::OpenAi {
        return Err(MiddlewareTransformError::Unsupported(
            "create_image stream response currently requires OpenAi destination protocol",
        ));
    }

    Ok(match input {
        TransformResponse::StreamCreateImageOpenAi(response) => {
            TransformResponse::StreamCreateImageOpenAi(response)
        }
        TransformResponse::StreamGenerateContentOpenAiResponse(response) => {
            TransformResponse::StreamCreateImageOpenAi(OpenAiCreateImageSseStreamBody::try_from(
                response,
            )?)
        }
        TransformResponse::StreamGenerateContentGeminiSse(response)
        | TransformResponse::StreamGenerateContentGeminiNdjson(response) => {
            TransformResponse::StreamCreateImageOpenAi(OpenAiCreateImageSseStreamBody::try_from(
                response,
            )?)
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "create_image stream response transform requires openai response or gemini stream payload",
            ));
        }
    })
}

pub(super) fn transform_create_image_edit_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::OpenAi {
        return Err(MiddlewareTransformError::Unsupported(
            "create_image_edit response currently requires OpenAi destination protocol",
        ));
    }

    Ok(match input {
        TransformResponse::CreateImageEditOpenAi(response) => {
            TransformResponse::CreateImageEditOpenAi(response)
        }
        TransformResponse::GenerateContentOpenAiResponse(response) => {
            TransformResponse::CreateImageEditOpenAi(OpenAiCreateImageEditResponse::try_from(
                response,
            )?)
        }
        TransformResponse::GenerateContentGemini(response) => {
            TransformResponse::CreateImageEditOpenAi(OpenAiCreateImageEditResponse::try_from(
                response,
            )?)
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "create_image_edit response transform requires openai response or gemini generate payload",
            ));
        }
    })
}

pub(super) fn transform_create_image_edit_stream_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::OpenAi {
        return Err(MiddlewareTransformError::Unsupported(
            "create_image_edit stream response currently requires OpenAi destination protocol",
        ));
    }

    Ok(match input {
        TransformResponse::StreamCreateImageEditOpenAi(response) => {
            TransformResponse::StreamCreateImageEditOpenAi(response)
        }
        TransformResponse::StreamGenerateContentOpenAiResponse(response) => {
            TransformResponse::StreamCreateImageEditOpenAi(
                OpenAiCreateImageEditSseStreamBody::try_from(response)?,
            )
        }
        TransformResponse::StreamGenerateContentGeminiSse(response)
        | TransformResponse::StreamGenerateContentGeminiNdjson(response) => {
            TransformResponse::StreamCreateImageEditOpenAi(
                OpenAiCreateImageEditSseStreamBody::try_from(response)?,
            )
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "create_image_edit stream response transform requires openai response or gemini stream payload",
            ));
        }
    })
}

pub(super) fn demote_openai_response_websocket_response_to_generate(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::OpenAiResponseWebSocket(messages) => {
            TransformResponse::GenerateContentOpenAiResponse(
                OpenAiCreateResponseResponse::try_from(messages)?,
            )
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "openai websocket response demotion requires openai websocket payload",
            ));
        }
    })
}

pub(super) fn demote_gemini_live_response_to_generate(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::GeminiLive(messages) => TransformResponse::GenerateContentGemini(
            GeminiGenerateContentResponse::try_from(messages)?,
        ),
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "gemini live response demotion requires gemini live payload",
            ));
        }
    })
}

pub(super) fn promote_generate_response_to_openai_response_websocket(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match input {
        TransformResponse::GenerateContentOpenAiResponse(response) => {
            Ok(TransformResponse::OpenAiResponseWebSocket(Vec::<
                OpenAiCreateResponseWebSocketMessageResponse,
            >::try_from(
                response
            )?))
        }
        _ => Err(MiddlewareTransformError::Unsupported(
            "openai websocket response promotion requires openai generate payload",
        )),
    }
}

pub(super) fn promote_stream_response_to_openai_response_websocket(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match input {
        TransformResponse::StreamGenerateContentOpenAiResponse(response) => {
            Ok(TransformResponse::OpenAiResponseWebSocket(Vec::<
                OpenAiCreateResponseWebSocketMessageResponse,
            >::try_from(
                &response
            )?))
        }
        _ => Err(MiddlewareTransformError::Unsupported(
            "openai websocket response promotion requires openai stream payload",
        )),
    }
}

pub(super) fn promote_generate_response_to_gemini_live(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match input {
        TransformResponse::GenerateContentGemini(response) => {
            Ok(TransformResponse::GeminiLive(Vec::<
                GeminiLiveMessageResponse,
            >::try_from(
                response
            )?))
        }
        _ => Err(MiddlewareTransformError::Unsupported(
            "gemini live response promotion requires gemini generate payload",
        )),
    }
}

pub(super) fn promote_stream_response_to_gemini_live(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match input {
        TransformResponse::StreamGenerateContentGeminiSse(response)
        | TransformResponse::StreamGenerateContentGeminiNdjson(response) => {
            Ok(TransformResponse::GeminiLive(Vec::<
                GeminiLiveMessageResponse,
            >::try_from(
                response
            )?))
        }
        _ => Err(MiddlewareTransformError::Unsupported(
            "gemini live response promotion requires gemini stream payload",
        )),
    }
}

pub(super) fn transform_openai_response_websocket_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::OpenAi {
        return Err(MiddlewareTransformError::Unsupported(
            "openai websocket response currently requires OpenAi destination protocol",
        ));
    }

    match input {
        TransformResponse::OpenAiResponseWebSocket(messages) => {
            Ok(TransformResponse::OpenAiResponseWebSocket(messages))
        }
        TransformResponse::GeminiLive(messages) => {
            transform_gemini_live_messages_to_openai_response_websocket_direct(messages)
        }
        other if other.operation() == OperationFamily::StreamGenerateContent => {
            let streamed = transform_stream_response(other, ProtocolKind::OpenAi)?;
            promote_stream_response_to_openai_response_websocket(streamed)
        }
        other => {
            let generated = transform_generate_response(other, ProtocolKind::OpenAi)?;
            promote_generate_response_to_openai_response_websocket(generated)
        }
    }
}

pub(super) fn transform_gemini_live_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::Gemini {
        return Err(MiddlewareTransformError::Unsupported(
            "gemini live response currently requires Gemini destination protocol",
        ));
    }

    match input {
        TransformResponse::GeminiLive(messages) => Ok(TransformResponse::GeminiLive(messages)),
        TransformResponse::OpenAiResponseWebSocket(messages) => {
            transform_openai_response_websocket_messages_to_gemini_live_direct(messages)
        }
        other if other.operation() == OperationFamily::StreamGenerateContent => {
            let streamed = transform_stream_response(other, ProtocolKind::Gemini)?;
            promote_stream_response_to_gemini_live(streamed)
        }
        other => {
            let generated = transform_generate_response(other, ProtocolKind::Gemini)?;
            promote_generate_response_to_gemini_live(generated)
        }
    }
}

pub(super) fn transform_openai_response_websocket_to_gemini_live_request_direct(
    request: OpenAiCreateResponseWebSocketConnectRequest,
) -> Result<TransformRequest, MiddlewareTransformError> {
    let openai_request = OpenAiCreateResponseRequest::try_from(&request)?;
    let gemini_stream_request = GeminiStreamGenerateContentRequest::try_from(&openai_request)?;
    let gemini_live_request = GeminiLiveConnectRequest::try_from(&gemini_stream_request)?;
    Ok(TransformRequest::GeminiLive(gemini_live_request))
}

pub(super) fn transform_gemini_live_to_openai_response_websocket_request_direct(
    request: GeminiLiveConnectRequest,
) -> Result<TransformRequest, MiddlewareTransformError> {
    let gemini_stream_request = GeminiStreamGenerateContentRequest::try_from(&request)?;
    let openai_request = OpenAiCreateResponseRequest::try_from(gemini_stream_request)?;
    let openai_ws_request = OpenAiCreateResponseWebSocketConnectRequest::try_from(&openai_request)?;
    Ok(TransformRequest::OpenAiResponseWebSocket(openai_ws_request))
}

pub(super) fn transform_openai_response_websocket_messages_to_gemini_live_direct(
    messages: Vec<OpenAiCreateResponseWebSocketMessageResponse>,
) -> Result<TransformResponse, MiddlewareTransformError> {
    let openai_sse = OpenAiCreateResponseSseStreamBody::try_from(messages.as_slice())?;
    let gemini_sse = GeminiSseStreamBody::try_from(openai_sse)?;
    let gemini_stream = GeminiStreamGenerateContentResponse::SseSuccess {
        stats_code: StatusCode::OK,
        headers: Default::default(),
        body: gemini_sse,
    };
    Ok(TransformResponse::GeminiLive(Vec::<
        GeminiLiveMessageResponse,
    >::try_from(
        gemini_stream
    )?))
}

pub(super) fn transform_gemini_live_messages_to_openai_response_websocket_direct(
    messages: Vec<GeminiLiveMessageResponse>,
) -> Result<TransformResponse, MiddlewareTransformError> {
    let gemini_stream = GeminiStreamGenerateContentResponse::try_from(messages)?;
    let openai_sse = OpenAiCreateResponseSseStreamBody::try_from(gemini_stream)?;
    Ok(TransformResponse::OpenAiResponseWebSocket(Vec::<
        OpenAiCreateResponseWebSocketMessageResponse,
    >::try_from(
        &openai_sse
    )?))
}

pub(super) fn transform_openai_response_websocket_to_gemini_live_response_direct(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match input {
        TransformResponse::OpenAiResponseWebSocket(messages) => {
            transform_openai_response_websocket_messages_to_gemini_live_direct(messages)
        }
        _ => Err(MiddlewareTransformError::Unsupported(
            "openai websocket to gemini live response direct transform requires openai websocket destination payload",
        )),
    }
}

pub(super) fn transform_gemini_live_to_openai_response_websocket_response_direct(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match input {
        TransformResponse::GeminiLive(messages) => {
            transform_gemini_live_messages_to_openai_response_websocket_direct(messages)
        }
        _ => Err(MiddlewareTransformError::Unsupported(
            "gemini live to openai websocket response direct transform requires gemini live destination payload",
        )),
    }
}

pub(super) fn transform_compact_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::OpenAi {
        return Err(MiddlewareTransformError::Unsupported(
            "compact response currently supports only OpenAi destination protocol",
        ));
    }

    Ok(match input {
        TransformResponse::CompactOpenAi(response) => TransformResponse::CompactOpenAi(response),
        TransformResponse::GenerateContentOpenAiResponse(response) => {
            TransformResponse::CompactOpenAi(OpenAiCompactResponse::try_from(response)?)
        }
        TransformResponse::GenerateContentOpenAiChatCompletions(response) => {
            TransformResponse::CompactOpenAi(OpenAiCompactResponse::try_from(response)?)
        }
        TransformResponse::GenerateContentClaude(response) => {
            TransformResponse::CompactOpenAi(OpenAiCompactResponse::try_from(response)?)
        }
        TransformResponse::GenerateContentGemini(response) => {
            TransformResponse::CompactOpenAi(OpenAiCompactResponse::try_from(response)?)
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "compact response transform requires compact or generate_content destination payload",
            ));
        }
    })
}
