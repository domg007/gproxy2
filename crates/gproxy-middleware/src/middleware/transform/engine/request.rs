use super::*;

pub(super) fn ensure_request_route_source(
    request: &TransformRequest,
    route: TransformRoute,
) -> Result<(), MiddlewareTransformError> {
    let actual_operation = request.operation();
    let actual_protocol = request.protocol();
    if actual_operation != route.src_operation || actual_protocol != route.src_protocol {
        return Err(MiddlewareTransformError::RouteSourceMismatch {
            expected_operation: route.src_operation,
            expected_protocol: route.src_protocol,
            actual_operation,
            actual_protocol,
        });
    }
    Ok(())
}

pub(super) fn ensure_response_route_destination(
    response: &TransformResponse,
    route: TransformRoute,
) -> Result<(), MiddlewareTransformError> {
    let actual_operation = response.operation();
    let actual_protocol = response.protocol();
    if actual_operation != route.dst_operation || actual_protocol != route.dst_protocol {
        return Err(MiddlewareTransformError::RouteSourceMismatch {
            expected_operation: route.dst_operation,
            expected_protocol: route.dst_protocol,
            actual_operation,
            actual_protocol,
        });
    }
    Ok(())
}

pub(super) fn transform_model_list_request(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    Ok(match input {
        TransformRequest::ModelListOpenAi(request) => match dst_protocol {
            ProtocolKind::OpenAi => TransformRequest::ModelListOpenAi(request),
            ProtocolKind::Claude => {
                TransformRequest::ModelListClaude(ClaudeModelListRequest::try_from(request)?)
            }
            ProtocolKind::Gemini => {
                TransformRequest::ModelListGemini(GeminiModelListRequest::try_from(request)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_list does not support this destination protocol",
                ));
            }
        },
        TransformRequest::ModelListClaude(request) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformRequest::ModelListOpenAi(OpenAiModelListRequest::try_from(request)?)
            }
            ProtocolKind::Claude => TransformRequest::ModelListClaude(request),
            ProtocolKind::Gemini => {
                TransformRequest::ModelListGemini(GeminiModelListRequest::try_from(request)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_list does not support this destination protocol",
                ));
            }
        },
        TransformRequest::ModelListGemini(request) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformRequest::ModelListOpenAi(OpenAiModelListRequest::try_from(request)?)
            }
            ProtocolKind::Claude => {
                TransformRequest::ModelListClaude(ClaudeModelListRequest::try_from(request)?)
            }
            ProtocolKind::Gemini => TransformRequest::ModelListGemini(request),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_list does not support this destination protocol",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "model_list request transform requires model_list source payload",
            ));
        }
    })
}

pub(super) fn transform_model_get_request(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    Ok(match input {
        TransformRequest::ModelGetOpenAi(request) => match dst_protocol {
            ProtocolKind::OpenAi => TransformRequest::ModelGetOpenAi(request),
            ProtocolKind::Claude => {
                TransformRequest::ModelGetClaude(ClaudeModelGetRequest::try_from(request)?)
            }
            ProtocolKind::Gemini => {
                TransformRequest::ModelGetGemini(GeminiModelGetRequest::try_from(request)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_get does not support this destination protocol",
                ));
            }
        },
        TransformRequest::ModelGetClaude(request) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformRequest::ModelGetOpenAi(OpenAiModelGetRequest::try_from(request)?)
            }
            ProtocolKind::Claude => TransformRequest::ModelGetClaude(request),
            ProtocolKind::Gemini => {
                TransformRequest::ModelGetGemini(GeminiModelGetRequest::try_from(request)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_get does not support this destination protocol",
                ));
            }
        },
        TransformRequest::ModelGetGemini(request) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformRequest::ModelGetOpenAi(OpenAiModelGetRequest::try_from(request)?)
            }
            ProtocolKind::Claude => {
                TransformRequest::ModelGetClaude(ClaudeModelGetRequest::try_from(request)?)
            }
            ProtocolKind::Gemini => TransformRequest::ModelGetGemini(request),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "model_get does not support this destination protocol",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "model_get request transform requires model_get source payload",
            ));
        }
    })
}

pub(super) fn transform_count_tokens_request(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    Ok(match input {
        TransformRequest::CountTokenOpenAi(request) => match dst_protocol {
            ProtocolKind::OpenAi => TransformRequest::CountTokenOpenAi(request),
            ProtocolKind::Claude => {
                TransformRequest::CountTokenClaude(ClaudeCountTokensRequest::try_from(request)?)
            }
            ProtocolKind::Gemini => {
                TransformRequest::CountTokenGemini(GeminiCountTokensRequest::try_from(request)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "count_token does not support this destination protocol",
                ));
            }
        },
        TransformRequest::CountTokenClaude(request) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformRequest::CountTokenOpenAi(OpenAiCountTokensRequest::try_from(request)?)
            }
            ProtocolKind::Claude => TransformRequest::CountTokenClaude(request),
            ProtocolKind::Gemini => {
                TransformRequest::CountTokenGemini(GeminiCountTokensRequest::try_from(request)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "count_token does not support this destination protocol",
                ));
            }
        },
        TransformRequest::CountTokenGemini(request) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformRequest::CountTokenOpenAi(OpenAiCountTokensRequest::try_from(request)?)
            }
            ProtocolKind::Claude => {
                TransformRequest::CountTokenClaude(ClaudeCountTokensRequest::try_from(request)?)
            }
            ProtocolKind::Gemini => TransformRequest::CountTokenGemini(request),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "count_token does not support this destination protocol",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "count_token request transform requires count_token source payload",
            ));
        }
    })
}

pub(super) fn transform_embeddings_request(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    Ok(match input {
        TransformRequest::EmbeddingOpenAi(request) => match dst_protocol {
            ProtocolKind::OpenAi => TransformRequest::EmbeddingOpenAi(request),
            ProtocolKind::Gemini => {
                TransformRequest::EmbeddingGemini(GeminiEmbedContentRequest::try_from(request)?)
            }
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "embedding supports only openai and gemini",
                ));
            }
        },
        TransformRequest::EmbeddingGemini(request) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformRequest::EmbeddingOpenAi(OpenAiEmbeddingsRequest::try_from(request)?)
            }
            ProtocolKind::Gemini => TransformRequest::EmbeddingGemini(request),
            _ => {
                return Err(MiddlewareTransformError::Unsupported(
                    "embedding supports only openai and gemini",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "embedding request transform requires embedding source payload",
            ));
        }
    })
}

pub(super) fn transform_generate_request(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    let dst_protocol = dst_protocol.normalize_gemini_stream();

    match input {
        TransformRequest::GenerateContentOpenAiResponse(_)
        | TransformRequest::GenerateContentOpenAiChatCompletions(_)
        | TransformRequest::GenerateContentClaude(_)
        | TransformRequest::GenerateContentGemini(_) => {
            convert_generate_request_between_protocols(input, dst_protocol)
        }
        TransformRequest::StreamGenerateContentOpenAiResponse(_)
        | TransformRequest::StreamGenerateContentOpenAiChatCompletions(_)
        | TransformRequest::StreamGenerateContentClaude(_)
        | TransformRequest::StreamGenerateContentGeminiSse(_)
        | TransformRequest::StreamGenerateContentGeminiNdjson(_) => {
            let nonstream = demote_stream_request_to_generate(input)?;
            convert_generate_request_between_protocols(nonstream, dst_protocol)
        }
        TransformRequest::OpenAiResponseWebSocket(request) => {
            let nonstream = TransformRequest::GenerateContentOpenAiResponse(
                OpenAiCreateResponseRequest::try_from(request)?,
            );
            convert_generate_request_between_protocols(nonstream, dst_protocol)
        }
        TransformRequest::GeminiLive(request) => {
            let nonstream = TransformRequest::GenerateContentGemini(
                GeminiGenerateContentRequest::try_from(request)?,
            );
            convert_generate_request_between_protocols(nonstream, dst_protocol)
        }
        TransformRequest::CompactOpenAi(request) => Ok(match dst_protocol {
            ProtocolKind::OpenAi => TransformRequest::GenerateContentOpenAiResponse(
                OpenAiCreateResponseRequest::try_from(request)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformRequest::GenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsRequest::try_from(request)?,
                )
            }
            ProtocolKind::Claude => TransformRequest::GenerateContentClaude(
                ClaudeCreateMessageRequest::try_from(request)?,
            ),
            ProtocolKind::Gemini => TransformRequest::GenerateContentGemini(
                GeminiGenerateContentRequest::try_from(request)?,
            ),
            ProtocolKind::GeminiNDJson => {
                return Err(MiddlewareTransformError::Unsupported(
                    "generate_content request does not support GeminiNDJson destination",
                ));
            }
        }),
        _ => Err(MiddlewareTransformError::Unsupported(
            "generate_content request transform requires generate/stream/websocket/compact source payload",
        )),
    }
}

pub(super) fn transform_openai_response_websocket_request(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::OpenAi {
        return Err(MiddlewareTransformError::Unsupported(
            "openai websocket request currently requires OpenAi destination protocol",
        ));
    }

    match input {
        TransformRequest::OpenAiResponseWebSocket(request) => {
            Ok(TransformRequest::OpenAiResponseWebSocket(request))
        }
        TransformRequest::GeminiLive(request) => {
            transform_gemini_live_to_openai_response_websocket_request_direct(request)
        }
        TransformRequest::StreamGenerateContentOpenAiResponse(request) => {
            Ok(TransformRequest::OpenAiResponseWebSocket(
                OpenAiCreateResponseWebSocketConnectRequest::try_from(&request)?,
            ))
        }
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(request) => {
            let openai = OpenAiCreateResponseRequest::try_from(&request)?;
            Ok(TransformRequest::OpenAiResponseWebSocket(
                OpenAiCreateResponseWebSocketConnectRequest::try_from(&openai)?,
            ))
        }
        TransformRequest::StreamGenerateContentClaude(request) => {
            let openai = OpenAiCreateResponseRequest::try_from(&request)?;
            Ok(TransformRequest::OpenAiResponseWebSocket(
                OpenAiCreateResponseWebSocketConnectRequest::try_from(&openai)?,
            ))
        }
        TransformRequest::StreamGenerateContentGeminiSse(request)
        | TransformRequest::StreamGenerateContentGeminiNdjson(request) => {
            let openai = OpenAiCreateResponseRequest::try_from(request)?;
            Ok(TransformRequest::OpenAiResponseWebSocket(
                OpenAiCreateResponseWebSocketConnectRequest::try_from(&openai)?,
            ))
        }
        other => {
            let generated = transform_generate_request(other, ProtocolKind::OpenAi)?;
            match generated {
                TransformRequest::GenerateContentOpenAiResponse(request) => {
                    Ok(TransformRequest::OpenAiResponseWebSocket(
                        OpenAiCreateResponseWebSocketConnectRequest::try_from(request)?,
                    ))
                }
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai websocket request transform requires openai generate source payload",
                )),
            }
        }
    }
}

pub(super) fn transform_gemini_live_request(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::Gemini {
        return Err(MiddlewareTransformError::Unsupported(
            "gemini live request currently requires Gemini destination protocol",
        ));
    }

    match input {
        TransformRequest::GeminiLive(request) => Ok(TransformRequest::GeminiLive(request)),
        TransformRequest::OpenAiResponseWebSocket(request) => {
            transform_openai_response_websocket_to_gemini_live_request_direct(request)
        }
        TransformRequest::StreamGenerateContentGeminiSse(request)
        | TransformRequest::StreamGenerateContentGeminiNdjson(request) => Ok(
            TransformRequest::GeminiLive(GeminiLiveConnectRequest::try_from(&request)?),
        ),
        TransformRequest::StreamGenerateContentOpenAiResponse(request) => {
            let gemini = GeminiStreamGenerateContentRequest::try_from(&request)?;
            Ok(TransformRequest::GeminiLive(
                GeminiLiveConnectRequest::try_from(&gemini)?,
            ))
        }
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(request) => {
            let gemini = GeminiStreamGenerateContentRequest::try_from(&request)?;
            Ok(TransformRequest::GeminiLive(
                GeminiLiveConnectRequest::try_from(&gemini)?,
            ))
        }
        TransformRequest::StreamGenerateContentClaude(request) => {
            let gemini = GeminiStreamGenerateContentRequest::try_from(&request)?;
            Ok(TransformRequest::GeminiLive(
                GeminiLiveConnectRequest::try_from(&gemini)?,
            ))
        }
        other => {
            let generated = transform_generate_request(other, ProtocolKind::Gemini)?;
            match generated {
                TransformRequest::GenerateContentGemini(request) => Ok(
                    TransformRequest::GeminiLive(GeminiLiveConnectRequest::try_from(request)?),
                ),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini live request transform requires gemini generate source payload",
                )),
            }
        }
    }
}

pub(super) fn convert_generate_request_between_protocols(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    Ok(match input {
        TransformRequest::GenerateContentOpenAiResponse(request) => match dst_protocol {
            ProtocolKind::OpenAi => TransformRequest::GenerateContentOpenAiResponse(request),
            ProtocolKind::OpenAiChatCompletion => {
                TransformRequest::GenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsRequest::try_from(request)?,
                )
            }
            ProtocolKind::Claude => TransformRequest::GenerateContentClaude(
                ClaudeCreateMessageRequest::try_from(request)?,
            ),
            ProtocolKind::Gemini => TransformRequest::GenerateContentGemini(
                GeminiGenerateContentRequest::try_from(request)?,
            ),
            ProtocolKind::GeminiNDJson => {
                return Err(MiddlewareTransformError::Unsupported(
                    "generate_content request does not support GeminiNDJson destination",
                ));
            }
        },
        TransformRequest::GenerateContentOpenAiChatCompletions(request) => match dst_protocol {
            ProtocolKind::OpenAi => TransformRequest::GenerateContentOpenAiResponse(
                OpenAiCreateResponseRequest::try_from(request)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformRequest::GenerateContentOpenAiChatCompletions(request)
            }
            ProtocolKind::Claude => TransformRequest::GenerateContentClaude(
                ClaudeCreateMessageRequest::try_from(request)?,
            ),
            ProtocolKind::Gemini => TransformRequest::GenerateContentGemini(
                GeminiGenerateContentRequest::try_from(request)?,
            ),
            ProtocolKind::GeminiNDJson => {
                return Err(MiddlewareTransformError::Unsupported(
                    "generate_content request does not support GeminiNDJson destination",
                ));
            }
        },
        TransformRequest::GenerateContentClaude(request) => match dst_protocol {
            ProtocolKind::OpenAi => TransformRequest::GenerateContentOpenAiResponse(
                OpenAiCreateResponseRequest::try_from(request)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformRequest::GenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsRequest::try_from(request)?,
                )
            }
            ProtocolKind::Claude => TransformRequest::GenerateContentClaude(request),
            ProtocolKind::Gemini => TransformRequest::GenerateContentGemini(
                GeminiGenerateContentRequest::try_from(request)?,
            ),
            ProtocolKind::GeminiNDJson => {
                return Err(MiddlewareTransformError::Unsupported(
                    "generate_content request does not support GeminiNDJson destination",
                ));
            }
        },
        TransformRequest::GenerateContentGemini(request) => match dst_protocol {
            ProtocolKind::OpenAi => TransformRequest::GenerateContentOpenAiResponse(
                OpenAiCreateResponseRequest::try_from(request)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformRequest::GenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsRequest::try_from(request)?,
                )
            }
            ProtocolKind::Claude => TransformRequest::GenerateContentClaude(
                ClaudeCreateMessageRequest::try_from(request)?,
            ),
            ProtocolKind::Gemini => TransformRequest::GenerateContentGemini(request),
            ProtocolKind::GeminiNDJson => {
                return Err(MiddlewareTransformError::Unsupported(
                    "generate_content request does not support GeminiNDJson destination",
                ));
            }
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "generate_content request transform requires generate source payload",
            ));
        }
    })
}

pub(super) fn demote_stream_request_to_generate(
    input: TransformRequest,
) -> Result<TransformRequest, MiddlewareTransformError> {
    Ok(match input {
        TransformRequest::StreamGenerateContentOpenAiResponse(mut request) => {
            request.method = OpenAiResponseHttpMethod::Post;
            request.path = OpenAiCreateResponsePathParameters::default();
            request.query = OpenAiCreateResponseQueryParameters::default();
            request.headers = OpenAiCreateResponseRequestHeaders::default();
            request.body.stream = None;
            request.body.stream_options = None;
            TransformRequest::GenerateContentOpenAiResponse(request)
        }
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(mut request) => {
            request.method = OpenAiChatHttpMethod::Post;
            request.path = Default::default();
            request.query = Default::default();
            request.headers = Default::default();
            request.body.stream = None;
            request.body.stream_options = None;
            TransformRequest::GenerateContentOpenAiChatCompletions(request)
        }
        TransformRequest::StreamGenerateContentClaude(mut request) => {
            request.method = ClaudeHttpMethod::Post;
            request.path = ClaudeCreateMessagePathParameters::default();
            request.query = ClaudeCreateMessageQueryParameters::default();
            request.headers = ClaudeCreateMessageRequestHeaders::default();
            request.body.stream = None;
            TransformRequest::GenerateContentClaude(request)
        }
        TransformRequest::StreamGenerateContentGeminiSse(request)
        | TransformRequest::StreamGenerateContentGeminiNdjson(request) => {
            TransformRequest::GenerateContentGemini(GeminiGenerateContentRequest {
                method: GeminiHttpMethod::Post,
                path: GeminiGenerateContentPathParameters {
                    model: request.path.model,
                },
                query: GeminiGenerateContentQueryParameters::default(),
                headers: GeminiGenerateContentRequestHeaders::default(),
                body: request.body,
            })
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "stream request demotion requires stream_generate_content source payload",
            ));
        }
    })
}

pub(super) fn promote_generate_request_to_stream(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    Ok(match input {
        TransformRequest::GenerateContentOpenAiResponse(mut request) => {
            if dst_protocol != ProtocolKind::OpenAi {
                return Err(MiddlewareTransformError::Unsupported(
                    "openai response stream request requires OpenAi destination protocol",
                ));
            }
            request.body.stream = Some(true);
            TransformRequest::StreamGenerateContentOpenAiResponse(request)
        }
        TransformRequest::GenerateContentOpenAiChatCompletions(mut request) => {
            if dst_protocol != ProtocolKind::OpenAiChatCompletion {
                return Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream request requires OpenAiChatCompletion destination protocol",
                ));
            }
            request.body.stream = Some(true);
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(request)
        }
        TransformRequest::GenerateContentClaude(mut request) => {
            if dst_protocol != ProtocolKind::Claude {
                return Err(MiddlewareTransformError::Unsupported(
                    "claude stream request requires Claude destination protocol",
                ));
            }
            request.body.stream = Some(true);
            TransformRequest::StreamGenerateContentClaude(request)
        }
        TransformRequest::GenerateContentGemini(request) => {
            let stream_request = GeminiStreamGenerateContentRequest {
                method: GeminiHttpMethod::Post,
                path: GeminiStreamGenerateContentPathParameters {
                    model: request.path.model,
                },
                query: GeminiStreamGenerateContentQueryParameters {
                    alt: match dst_protocol {
                        ProtocolKind::Gemini => Some(GeminiAltQueryParameter::Sse),
                        ProtocolKind::GeminiNDJson => None,
                        _ => {
                            return Err(MiddlewareTransformError::Unsupported(
                                "gemini stream request requires Gemini/GeminiNDJson destination protocol",
                            ));
                        }
                    },
                },
                headers: GeminiStreamGenerateContentRequestHeaders::default(),
                body: request.body,
            };

            match dst_protocol {
                ProtocolKind::Gemini => {
                    TransformRequest::StreamGenerateContentGeminiSse(stream_request)
                }
                ProtocolKind::GeminiNDJson => {
                    TransformRequest::StreamGenerateContentGeminiNdjson(stream_request)
                }
                _ => {
                    return Err(MiddlewareTransformError::Unsupported(
                        "gemini stream request requires Gemini/GeminiNDJson destination protocol",
                    ));
                }
            }
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "stream request promotion requires generate_content source payload",
            ));
        }
    })
}

pub(super) fn transform_compact_request(
    input: TransformRequest,
    dst_protocol: ProtocolKind,
) -> Result<TransformRequest, MiddlewareTransformError> {
    if dst_protocol != ProtocolKind::OpenAi {
        return Err(MiddlewareTransformError::Unsupported(
            "compact request currently supports only OpenAi destination protocol",
        ));
    }

    Ok(match input {
        TransformRequest::CompactOpenAi(request) => TransformRequest::CompactOpenAi(request),
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "compact request transform supports source compact only",
            ));
        }
    })
}
