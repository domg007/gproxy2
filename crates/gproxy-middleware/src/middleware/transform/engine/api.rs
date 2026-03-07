use super::*;

pub async fn transform_request_payload(
    input: TransformRequestPayload,
    route: TransformRoute,
) -> Result<TransformRequestPayload, MiddlewareTransformError> {
    if input.operation != route.src_operation || input.protocol != route.src_protocol {
        return Err(MiddlewareTransformError::RouteSourceMismatch {
            expected_operation: route.src_operation,
            expected_protocol: route.src_protocol,
            actual_operation: input.operation,
            actual_protocol: input.protocol,
        });
    }

    if select_request_lane(route) == TransformLane::Raw {
        return Ok(input);
    }

    let request_bytes = collect_body_bytes(input.body).await?;
    let decoded =
        decode_request_payload(input.operation, input.protocol, request_bytes.as_slice())?;
    let transformed = transform_request(decoded, route)?;
    let operation = transformed.operation();
    let protocol = transformed.protocol();
    let body = encode_request_payload(transformed)?;

    Ok(TransformRequestPayload::new(
        operation,
        protocol,
        bytes_to_body_stream(body),
    ))
}

pub async fn transform_response_payload(
    input: TransformResponsePayload,
    route: TransformRoute,
) -> Result<TransformResponsePayload, MiddlewareTransformError> {
    if input.operation != route.dst_operation || input.protocol != route.dst_protocol {
        return Err(MiddlewareTransformError::RouteSourceMismatch {
            expected_operation: route.dst_operation,
            expected_protocol: route.dst_protocol,
            actual_operation: input.operation,
            actual_protocol: input.protocol,
        });
    }

    if select_response_lane(route) == TransformLane::Raw {
        return Ok(input);
    }

    if input.operation == OperationFamily::StreamGenerateContent
        && route.src_operation == OperationFamily::StreamGenerateContent
    {
        if supports_incremental_stream_response_conversion(route.dst_protocol, route.src_protocol) {
            let body =
                transform_stream_response_body(input.body, route.dst_protocol, route.src_protocol)?;
            return Ok(TransformResponsePayload::new(
                route.src_operation,
                route.src_protocol,
                body,
            ));
        }
        return transform_buffered_stream_response_payload(input, route).await;
    }

    if input.operation == OperationFamily::StreamGenerateContent {
        return transform_buffered_stream_response_payload(input, route).await;
    }

    let response_bytes = collect_body_bytes(input.body).await?;
    let decoded =
        decode_response_payload(input.operation, input.protocol, response_bytes.as_slice())?;
    let transformed = transform_response(decoded, route)?;
    let operation = transformed.operation();
    let protocol = transformed.protocol();
    let body = encode_response_payload(transformed)?;

    Ok(TransformResponsePayload::new(
        operation,
        protocol,
        bytes_to_body_stream(body),
    ))
}

pub fn transform_request(
    input: TransformRequest,
    route: TransformRoute,
) -> Result<TransformRequest, MiddlewareTransformError> {
    ensure_request_route_source(&input, route)?;
    if route.is_passthrough() {
        return Ok(input);
    }

    let extra_headers = request_extra_headers(&input);
    let mut transformed = match route.dst_operation {
        OperationFamily::ModelList => transform_model_list_request(input, route.dst_protocol),
        OperationFamily::ModelGet => transform_model_get_request(input, route.dst_protocol),
        OperationFamily::CountToken => transform_count_tokens_request(input, route.dst_protocol),
        OperationFamily::Embedding => transform_embeddings_request(input, route.dst_protocol),
        OperationFamily::GenerateContent => transform_generate_request(input, route.dst_protocol),
        OperationFamily::StreamGenerateContent => {
            let generate_request = transform_generate_request(input, route.dst_protocol)?;
            promote_generate_request_to_stream(generate_request, route.dst_protocol)
        }
        OperationFamily::OpenAiResponseWebSocket => {
            transform_openai_response_websocket_request(input, route.dst_protocol)
        }
        OperationFamily::GeminiLive => transform_gemini_live_request(input, route.dst_protocol),
        OperationFamily::Compact => transform_compact_request(input, route.dst_protocol),
    }?;
    apply_request_extra_headers(&mut transformed, extra_headers);
    Ok(transformed)
}

pub fn transform_response(
    input: TransformResponse,
    route: TransformRoute,
) -> Result<TransformResponse, MiddlewareTransformError> {
    ensure_response_route_destination(&input, route)?;
    if route.is_passthrough() {
        return Ok(input);
    }

    // Direct websocket-bridge path: OpenAI Responses WS <-> Gemini Live.
    // Keep this path independent from generate-content demotion/promotion.
    if route.src_operation == OperationFamily::OpenAiResponseWebSocket
        && route.dst_operation == OperationFamily::GeminiLive
    {
        return transform_gemini_live_to_openai_response_websocket_response_direct(input);
    }
    if route.src_operation == OperationFamily::GeminiLive
        && route.dst_operation == OperationFamily::OpenAiResponseWebSocket
    {
        return transform_openai_response_websocket_to_gemini_live_response_direct(input);
    }

    let mut current_operation = route.dst_operation;
    let mut current_response = input;

    if current_operation == OperationFamily::StreamGenerateContent
        && route.src_operation != OperationFamily::StreamGenerateContent
    {
        current_response = demote_stream_response_to_generate(current_response)?;
        current_operation = OperationFamily::GenerateContent;
    }
    if current_operation == OperationFamily::OpenAiResponseWebSocket
        && route.src_operation != OperationFamily::OpenAiResponseWebSocket
    {
        current_response = demote_openai_response_websocket_response_to_generate(current_response)?;
        current_operation = OperationFamily::GenerateContent;
    }
    if current_operation == OperationFamily::GeminiLive
        && route.src_operation != OperationFamily::GeminiLive
    {
        current_response = demote_gemini_live_response_to_generate(current_response)?;
        current_operation = OperationFamily::GenerateContent;
    }

    if route.src_operation == OperationFamily::StreamGenerateContent
        && current_operation != OperationFamily::StreamGenerateContent
    {
        let generated = transform_generate_response(current_response, route.src_protocol)?;
        return promote_generate_response_to_stream(generated, route.src_protocol);
    }
    if route.src_operation == OperationFamily::OpenAiResponseWebSocket
        && current_operation != OperationFamily::OpenAiResponseWebSocket
    {
        if current_operation == OperationFamily::StreamGenerateContent {
            let streamed = transform_stream_response(current_response, ProtocolKind::OpenAi)?;
            return promote_stream_response_to_openai_response_websocket(streamed);
        }
        let generated = transform_generate_response(current_response, ProtocolKind::OpenAi)?;
        return promote_generate_response_to_openai_response_websocket(generated);
    }
    if route.src_operation == OperationFamily::GeminiLive
        && current_operation != OperationFamily::GeminiLive
    {
        if current_operation == OperationFamily::StreamGenerateContent {
            let streamed = transform_stream_response(current_response, ProtocolKind::Gemini)?;
            return promote_stream_response_to_gemini_live(streamed);
        }
        let generated = transform_generate_response(current_response, ProtocolKind::Gemini)?;
        return promote_generate_response_to_gemini_live(generated);
    }

    match route.src_operation {
        OperationFamily::ModelList => {
            transform_model_list_response(current_response, route.src_protocol)
        }
        OperationFamily::ModelGet => {
            transform_model_get_response(current_response, route.src_protocol)
        }
        OperationFamily::CountToken => {
            transform_count_tokens_response(current_response, route.src_protocol)
        }
        OperationFamily::Embedding => {
            transform_embeddings_response(current_response, route.src_protocol)
        }
        OperationFamily::GenerateContent => {
            transform_generate_response(current_response, route.src_protocol)
        }
        OperationFamily::StreamGenerateContent => {
            if current_operation == OperationFamily::StreamGenerateContent {
                transform_stream_response(current_response, route.src_protocol)
            } else {
                Err(MiddlewareTransformError::Unsupported(
                    "stream response source requires stream destination",
                ))
            }
        }
        OperationFamily::OpenAiResponseWebSocket => {
            transform_openai_response_websocket_response(current_response, route.src_protocol)
        }
        OperationFamily::GeminiLive => {
            transform_gemini_live_response(current_response, route.src_protocol)
        }
        OperationFamily::Compact => {
            transform_compact_response(current_response, route.src_protocol)
        }
    }
}
