use std::collections::VecDeque;

use bytes::Bytes;
use futures_util::{StreamExt, stream as futures_stream};
use gproxy_protocol::claude::count_tokens::request::ClaudeCountTokensRequest;
use gproxy_protocol::claude::count_tokens::response::ClaudeCountTokensResponse;
use gproxy_protocol::claude::create_message::request::{
    ClaudeCreateMessageRequest, PathParameters as ClaudeCreateMessagePathParameters,
    QueryParameters as ClaudeCreateMessageQueryParameters,
    RequestHeaders as ClaudeCreateMessageRequestHeaders,
};
use gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse;
use gproxy_protocol::claude::create_message::stream::{
    ClaudeCreateMessageSseStreamBody, ClaudeCreateMessageStreamEvent,
};
use gproxy_protocol::claude::create_message::types::HttpMethod as ClaudeHttpMethod;
use gproxy_protocol::claude::model_get::request::ClaudeModelGetRequest;
use gproxy_protocol::claude::model_get::response::ClaudeModelGetResponse;
use gproxy_protocol::claude::model_list::request::ClaudeModelListRequest;
use gproxy_protocol::claude::model_list::response::ClaudeModelListResponse;
use gproxy_protocol::gemini::count_tokens::request::GeminiCountTokensRequest;
use gproxy_protocol::gemini::count_tokens::response::GeminiCountTokensResponse;
use gproxy_protocol::gemini::embeddings::request::GeminiEmbedContentRequest;
use gproxy_protocol::gemini::embeddings::response::GeminiEmbedContentResponse;
use gproxy_protocol::gemini::generate_content::request::{
    GeminiGenerateContentRequest, PathParameters as GeminiGenerateContentPathParameters,
    QueryParameters as GeminiGenerateContentQueryParameters,
    RequestHeaders as GeminiGenerateContentRequestHeaders,
};
use gproxy_protocol::gemini::generate_content::response::{
    GeminiGenerateContentResponse, ResponseBody as GeminiGenerateContentResponseBody,
};
use gproxy_protocol::gemini::live::request::GeminiLiveConnectRequest;
use gproxy_protocol::gemini::live::response::GeminiLiveMessageResponse;
use gproxy_protocol::gemini::model_get::request::GeminiModelGetRequest;
use gproxy_protocol::gemini::model_get::response::GeminiModelGetResponse;
use gproxy_protocol::gemini::model_list::request::GeminiModelListRequest;
use gproxy_protocol::gemini::model_list::response::GeminiModelListResponse;
use gproxy_protocol::gemini::stream_generate_content::request::{
    AltQueryParameter as GeminiAltQueryParameter, GeminiStreamGenerateContentRequest,
    PathParameters as GeminiStreamGenerateContentPathParameters,
    QueryParameters as GeminiStreamGenerateContentQueryParameters,
    RequestHeaders as GeminiStreamGenerateContentRequestHeaders,
};
use gproxy_protocol::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
use gproxy_protocol::gemini::stream_generate_content::stream::{
    GeminiNdjsonStreamBody, GeminiSseEvent, GeminiSseEventData, GeminiSseStreamBody,
};
use gproxy_protocol::gemini::types::HttpMethod as GeminiHttpMethod;
use gproxy_protocol::openai::compact_response::response::OpenAiCompactResponse;
use gproxy_protocol::openai::count_tokens::request::OpenAiCountTokensRequest;
use gproxy_protocol::openai::count_tokens::response::OpenAiCountTokensResponse;
use gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest;
use gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
use gproxy_protocol::openai::create_chat_completions::stream::{
    OpenAiChatCompletionsSseData, OpenAiChatCompletionsSseEvent, OpenAiChatCompletionsSseStreamBody,
};
use gproxy_protocol::openai::create_chat_completions::types::HttpMethod as OpenAiChatHttpMethod;
use gproxy_protocol::openai::create_response::request::{
    OpenAiCreateResponseRequest, PathParameters as OpenAiCreateResponsePathParameters,
    QueryParameters as OpenAiCreateResponseQueryParameters,
    RequestHeaders as OpenAiCreateResponseRequestHeaders,
};
use gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse;
use gproxy_protocol::openai::create_response::stream::{
    OpenAiCreateResponseSseData, OpenAiCreateResponseSseEvent, OpenAiCreateResponseSseStreamBody,
};
use gproxy_protocol::openai::create_response::types::HttpMethod as OpenAiResponseHttpMethod;
use gproxy_protocol::openai::create_response::websocket::request::OpenAiCreateResponseWebSocketConnectRequest;
use gproxy_protocol::openai::create_response::websocket::response::OpenAiCreateResponseWebSocketMessageResponse;
use gproxy_protocol::openai::embeddings::request::OpenAiEmbeddingsRequest;
use gproxy_protocol::openai::embeddings::response::OpenAiEmbeddingsResponse;
use gproxy_protocol::openai::model_get::request::OpenAiModelGetRequest;
use gproxy_protocol::openai::model_get::response::OpenAiModelGetResponse;
use gproxy_protocol::openai::model_list::request::OpenAiModelListRequest;
use gproxy_protocol::openai::model_list::response::OpenAiModelListResponse;
use gproxy_protocol::transform::claude::stream_generate_content::gemini::response::GeminiToClaudeStream;
use gproxy_protocol::transform::claude::stream_generate_content::openai_chat_completions::response::OpenAiChatCompletionsToClaudeStream;
use gproxy_protocol::transform::claude::stream_generate_content::openai_response::response::OpenAiResponseToClaudeStream;
use gproxy_protocol::transform::gemini::stream_generate_content::claude::response::ClaudeToGeminiStream;
use gproxy_protocol::transform::gemini::stream_generate_content::openai_chat_completions::response::OpenAiChatCompletionsToGeminiStream;
use gproxy_protocol::transform::gemini::stream_generate_content::openai_response::response::OpenAiResponseToGeminiStream;
use gproxy_protocol::transform::openai::stream_generate_content::openai_chat_completions::claude::response::ClaudeToOpenAiChatCompletionsStream;
use gproxy_protocol::transform::openai::stream_generate_content::openai_chat_completions::gemini::response::GeminiToOpenAiChatCompletionsStream;
use gproxy_protocol::transform::openai::stream_generate_content::openai_chat_completions::openai_response::response::OpenAiResponseToOpenAiChatCompletionsStream;
use gproxy_protocol::transform::openai::stream_generate_content::openai_response::claude::response::ClaudeToOpenAiResponseStream;
use gproxy_protocol::transform::openai::stream_generate_content::openai_response::gemini::response::GeminiToOpenAiResponseStream;
use gproxy_protocol::transform::openai::stream_generate_content::openai_response::openai_chat_completions::response::OpenAiChatCompletionsToOpenAiResponseStream;
use http::StatusCode;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::error::MiddlewareTransformError;
use super::kinds::{OperationFamily, ProtocolKind};
use super::message::{
    TransformBodyStream, TransformRequest, TransformRequestPayload, TransformResponse,
    TransformResponsePayload, TransformRoute,
};

mod stream;
use stream::{
    demote_stream_response_to_generate, ensure_gemini_ndjson_stream, ensure_gemini_sse_stream,
    promote_generate_response_to_stream, supports_incremental_stream_response_conversion,
    transform_buffered_stream_response_payload, transform_stream_response,
    transform_stream_response_body,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformLane {
    Raw,
    Typed,
}

pub fn select_request_lane(route: TransformRoute) -> TransformLane {
    if route.is_passthrough() {
        TransformLane::Raw
    } else {
        TransformLane::Typed
    }
}

pub fn select_response_lane(route: TransformRoute) -> TransformLane {
    if route.is_passthrough() {
        TransformLane::Raw
    } else {
        TransformLane::Typed
    }
}

fn request_extra_headers(input: &TransformRequest) -> std::collections::BTreeMap<String, String> {
    match input {
        TransformRequest::ModelListOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::ModelListClaude(value) => value.headers.extra.clone(),
        TransformRequest::ModelListGemini(value) => value.headers.extra.clone(),
        TransformRequest::ModelGetOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::ModelGetClaude(value) => value.headers.extra.clone(),
        TransformRequest::ModelGetGemini(value) => value.headers.extra.clone(),
        TransformRequest::CountTokenOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::CountTokenClaude(value) => value.headers.extra.clone(),
        TransformRequest::CountTokenGemini(value) => value.headers.extra.clone(),
        TransformRequest::GenerateContentOpenAiResponse(value) => value.headers.extra.clone(),
        TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
            value.headers.extra.clone()
        }
        TransformRequest::GenerateContentClaude(value) => value.headers.extra.clone(),
        TransformRequest::GenerateContentGemini(value) => value.headers.extra.clone(),
        TransformRequest::StreamGenerateContentOpenAiResponse(value) => value.headers.extra.clone(),
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            value.headers.extra.clone()
        }
        TransformRequest::StreamGenerateContentClaude(value) => value.headers.extra.clone(),
        TransformRequest::StreamGenerateContentGeminiSse(value) => value.headers.extra.clone(),
        TransformRequest::StreamGenerateContentGeminiNdjson(value) => value.headers.extra.clone(),
        TransformRequest::OpenAiResponseWebSocket(value) => value.headers.extra.clone(),
        TransformRequest::GeminiLive(value) => value.headers.extra.clone(),
        TransformRequest::EmbeddingOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::EmbeddingGemini(value) => value.headers.extra.clone(),
        TransformRequest::CompactOpenAi(value) => value.headers.extra.clone(),
    }
}

fn apply_request_extra_headers(
    request: &mut TransformRequest,
    extra: std::collections::BTreeMap<String, String>,
) {
    match request {
        TransformRequest::ModelListOpenAi(value) => value.headers.extra = extra,
        TransformRequest::ModelListClaude(value) => value.headers.extra = extra,
        TransformRequest::ModelListGemini(value) => value.headers.extra = extra,
        TransformRequest::ModelGetOpenAi(value) => value.headers.extra = extra,
        TransformRequest::ModelGetClaude(value) => value.headers.extra = extra,
        TransformRequest::ModelGetGemini(value) => value.headers.extra = extra,
        TransformRequest::CountTokenOpenAi(value) => value.headers.extra = extra,
        TransformRequest::CountTokenClaude(value) => value.headers.extra = extra,
        TransformRequest::CountTokenGemini(value) => value.headers.extra = extra,
        TransformRequest::GenerateContentOpenAiResponse(value) => value.headers.extra = extra,
        TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
            value.headers.extra = extra
        }
        TransformRequest::GenerateContentClaude(value) => value.headers.extra = extra,
        TransformRequest::GenerateContentGemini(value) => value.headers.extra = extra,
        TransformRequest::StreamGenerateContentOpenAiResponse(value) => value.headers.extra = extra,
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            value.headers.extra = extra
        }
        TransformRequest::StreamGenerateContentClaude(value) => value.headers.extra = extra,
        TransformRequest::StreamGenerateContentGeminiSse(value) => value.headers.extra = extra,
        TransformRequest::StreamGenerateContentGeminiNdjson(value) => value.headers.extra = extra,
        TransformRequest::OpenAiResponseWebSocket(value) => value.headers.extra = extra,
        TransformRequest::GeminiLive(value) => value.headers.extra = extra,
        TransformRequest::EmbeddingOpenAi(value) => value.headers.extra = extra,
        TransformRequest::EmbeddingGemini(value) => value.headers.extra = extra,
        TransformRequest::CompactOpenAi(value) => value.headers.extra = extra,
    }
}

fn decode_json<T: DeserializeOwned>(
    kind: &'static str,
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Result<T, MiddlewareTransformError> {
    serde_json::from_slice(body).map_err(|err| MiddlewareTransformError::JsonDecode {
        kind,
        operation,
        protocol,
        message: err.to_string(),
    })
}

fn encode_json<T: Serialize>(
    kind: &'static str,
    operation: OperationFamily,
    protocol: ProtocolKind,
    value: &T,
) -> Result<Vec<u8>, MiddlewareTransformError> {
    serde_json::to_vec(value).map_err(|err| MiddlewareTransformError::JsonEncode {
        kind,
        operation,
        protocol,
        message: err.to_string(),
    })
}

async fn collect_body_bytes(
    mut body: TransformBodyStream,
) -> Result<Vec<u8>, MiddlewareTransformError> {
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        out.extend_from_slice(&chunk?);
    }
    Ok(out)
}

fn bytes_to_body_stream(bytes: Vec<u8>) -> TransformBodyStream {
    Box::pin(futures_stream::once(async move { Ok(Bytes::from(bytes)) }))
}

pub fn decode_request_payload(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Result<TransformRequest, MiddlewareTransformError> {
    match (operation, protocol) {
        (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(
            TransformRequest::ModelListOpenAi(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::ModelList, ProtocolKind::Claude) => Ok(
            TransformRequest::ModelListClaude(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::ModelList, ProtocolKind::Gemini) => Ok(
            TransformRequest::ModelListGemini(decode_json("request", operation, protocol, body)?),
        ),

        (OperationFamily::ModelGet, ProtocolKind::OpenAi) => Ok(TransformRequest::ModelGetOpenAi(
            decode_json("request", operation, protocol, body)?,
        )),
        (OperationFamily::ModelGet, ProtocolKind::Claude) => Ok(TransformRequest::ModelGetClaude(
            decode_json("request", operation, protocol, body)?,
        )),
        (OperationFamily::ModelGet, ProtocolKind::Gemini) => Ok(TransformRequest::ModelGetGemini(
            decode_json("request", operation, protocol, body)?,
        )),

        (OperationFamily::CountToken, ProtocolKind::OpenAi) => Ok(
            TransformRequest::CountTokenOpenAi(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::CountToken, ProtocolKind::Claude) => Ok(
            TransformRequest::CountTokenClaude(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::CountToken, ProtocolKind::Gemini) => Ok(
            TransformRequest::CountTokenGemini(decode_json("request", operation, protocol, body)?),
        ),

        (OperationFamily::GenerateContent, ProtocolKind::OpenAi) => {
            Ok(TransformRequest::GenerateContentOpenAiResponse(
                decode_json("request", operation, protocol, body)?,
            ))
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion) => {
            Ok(TransformRequest::GenerateContentOpenAiChatCompletions(
                decode_json("request", operation, protocol, body)?,
            ))
        }
        (OperationFamily::GenerateContent, ProtocolKind::Claude) => {
            Ok(TransformRequest::GenerateContentClaude(decode_json(
                "request", operation, protocol, body,
            )?))
        }
        (OperationFamily::GenerateContent, ProtocolKind::Gemini) => {
            Ok(TransformRequest::GenerateContentGemini(decode_json(
                "request", operation, protocol, body,
            )?))
        }

        (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
            Ok(TransformRequest::StreamGenerateContentOpenAiResponse(
                decode_json("request", operation, protocol, body)?,
            ))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => Ok(
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(decode_json(
                "request", operation, protocol, body,
            )?),
        ),
        (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
            Ok(TransformRequest::StreamGenerateContentClaude(decode_json(
                "request", operation, protocol, body,
            )?))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini) => {
            let request: GeminiStreamGenerateContentRequest =
                decode_json("request", operation, protocol, body)?;
            Ok(TransformRequest::StreamGenerateContentGeminiSse(request))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
            let request: GeminiStreamGenerateContentRequest =
                decode_json("request", operation, protocol, body)?;
            Ok(TransformRequest::StreamGenerateContentGeminiNdjson(request))
        }

        (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi) => {
            Ok(TransformRequest::OpenAiResponseWebSocket(decode_json(
                "request", operation, protocol, body,
            )?))
        }
        (OperationFamily::GeminiLive, ProtocolKind::Gemini) => Ok(TransformRequest::GeminiLive(
            decode_json("request", operation, protocol, body)?,
        )),

        (OperationFamily::Embedding, ProtocolKind::OpenAi) => Ok(
            TransformRequest::EmbeddingOpenAi(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::Embedding, ProtocolKind::Gemini) => Ok(
            TransformRequest::EmbeddingGemini(decode_json("request", operation, protocol, body)?),
        ),

        (OperationFamily::Compact, ProtocolKind::OpenAi) => Ok(TransformRequest::CompactOpenAi(
            decode_json("request", operation, protocol, body)?,
        )),

        _ => Err(MiddlewareTransformError::Unsupported(
            "unsupported request payload operation/protocol",
        )),
    }
}

pub(crate) fn encode_request_payload(
    request: TransformRequest,
) -> Result<Vec<u8>, MiddlewareTransformError> {
    let operation = request.operation();
    let protocol = request.protocol();

    match request {
        TransformRequest::ModelListOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelListClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelListGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelGetOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelGetClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelGetGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CountTokenOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CountTokenClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CountTokenGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GenerateContentOpenAiResponse(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GenerateContentClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GenerateContentGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentOpenAiResponse(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentGeminiSse(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::OpenAiResponseWebSocket(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GeminiLive(value) => encode_json("request", operation, protocol, &value),
        TransformRequest::EmbeddingOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::EmbeddingGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CompactOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
    }
}

pub fn decode_response_payload(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Result<TransformResponse, MiddlewareTransformError> {
    match (operation, protocol) {
        (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(
            TransformResponse::ModelListOpenAi(decode_json("response", operation, protocol, body)?),
        ),
        (OperationFamily::ModelList, ProtocolKind::Claude) => Ok(
            TransformResponse::ModelListClaude(decode_json("response", operation, protocol, body)?),
        ),
        (OperationFamily::ModelList, ProtocolKind::Gemini) => Ok(
            TransformResponse::ModelListGemini(decode_json("response", operation, protocol, body)?),
        ),

        (OperationFamily::ModelGet, ProtocolKind::OpenAi) => Ok(TransformResponse::ModelGetOpenAi(
            decode_json("response", operation, protocol, body)?,
        )),
        (OperationFamily::ModelGet, ProtocolKind::Claude) => Ok(TransformResponse::ModelGetClaude(
            decode_json("response", operation, protocol, body)?,
        )),
        (OperationFamily::ModelGet, ProtocolKind::Gemini) => Ok(TransformResponse::ModelGetGemini(
            decode_json("response", operation, protocol, body)?,
        )),

        (OperationFamily::CountToken, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::CountTokenOpenAi(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::CountToken, ProtocolKind::Claude) => {
            Ok(TransformResponse::CountTokenClaude(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::CountToken, ProtocolKind::Gemini) => {
            Ok(TransformResponse::CountTokenGemini(decode_json(
                "response", operation, protocol, body,
            )?))
        }

        (OperationFamily::GenerateContent, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::GenerateContentOpenAiResponse(
                decode_json("response", operation, protocol, body)?,
            ))
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion) => {
            Ok(TransformResponse::GenerateContentOpenAiChatCompletions(
                decode_json("response", operation, protocol, body)?,
            ))
        }
        (OperationFamily::GenerateContent, ProtocolKind::Claude) => {
            Ok(TransformResponse::GenerateContentClaude(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::GenerateContent, ProtocolKind::Gemini) => {
            Ok(TransformResponse::GenerateContentGemini(decode_json(
                "response", operation, protocol, body,
            )?))
        }

        (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::StreamGenerateContentOpenAiResponse(
                decode_json("response", operation, protocol, body)?,
            ))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => Ok(
            TransformResponse::StreamGenerateContentOpenAiChatCompletions(decode_json(
                "response", operation, protocol, body,
            )?),
        ),
        (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
            Ok(TransformResponse::StreamGenerateContentClaude(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini) => {
            let response: GeminiStreamGenerateContentResponse =
                decode_json("response", operation, protocol, body)?;
            Ok(TransformResponse::StreamGenerateContentGeminiSse(
                ensure_gemini_sse_stream(response),
            ))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
            let response: GeminiStreamGenerateContentResponse =
                decode_json("response", operation, protocol, body)?;
            Ok(TransformResponse::StreamGenerateContentGeminiNdjson(
                ensure_gemini_ndjson_stream(response),
            ))
        }

        (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::OpenAiResponseWebSocket(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::GeminiLive, ProtocolKind::Gemini) => Ok(TransformResponse::GeminiLive(
            decode_json("response", operation, protocol, body)?,
        )),

        (OperationFamily::Embedding, ProtocolKind::OpenAi) => Ok(
            TransformResponse::EmbeddingOpenAi(decode_json("response", operation, protocol, body)?),
        ),
        (OperationFamily::Embedding, ProtocolKind::Gemini) => Ok(
            TransformResponse::EmbeddingGemini(decode_json("response", operation, protocol, body)?),
        ),

        (OperationFamily::Compact, ProtocolKind::OpenAi) => Ok(TransformResponse::CompactOpenAi(
            decode_json("response", operation, protocol, body)?,
        )),

        _ => Err(MiddlewareTransformError::Unsupported(
            "unsupported response payload operation/protocol",
        )),
    }
}

pub(crate) fn encode_response_payload(
    response: TransformResponse,
) -> Result<Vec<u8>, MiddlewareTransformError> {
    let operation = response.operation();
    let protocol = response.protocol();

    match response {
        TransformResponse::ModelListOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelListClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelListGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelGetOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelGetClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelGetGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::CountTokenOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::CountTokenClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::CountTokenGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GenerateContentOpenAiResponse(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GenerateContentOpenAiChatCompletions(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GenerateContentClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GenerateContentGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamGenerateContentOpenAiResponse(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamGenerateContentOpenAiChatCompletions(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamGenerateContentClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamGenerateContentGeminiSse(value) => encode_json(
            "response",
            operation,
            protocol,
            &ensure_gemini_sse_stream(value),
        ),
        TransformResponse::StreamGenerateContentGeminiNdjson(value) => encode_json(
            "response",
            operation,
            protocol,
            &ensure_gemini_ndjson_stream(value),
        ),
        TransformResponse::OpenAiResponseWebSocket(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GeminiLive(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::EmbeddingOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::EmbeddingGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::CompactOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
    }
}

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

fn ensure_request_route_source(
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

fn ensure_response_route_destination(
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

fn transform_model_list_request(
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

fn transform_model_get_request(
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

fn transform_count_tokens_request(
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

fn transform_embeddings_request(
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

fn transform_generate_request(
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

fn transform_openai_response_websocket_request(
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

fn transform_gemini_live_request(
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

fn convert_generate_request_between_protocols(
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

fn demote_stream_request_to_generate(
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

fn promote_generate_request_to_stream(
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

fn transform_compact_request(
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

fn transform_model_list_response(
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

fn transform_model_get_response(
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

fn transform_count_tokens_response(
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

fn transform_embeddings_response(
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

fn transform_generate_response(
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

fn demote_openai_response_websocket_response_to_generate(
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

fn demote_gemini_live_response_to_generate(
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

fn promote_generate_response_to_openai_response_websocket(
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

fn promote_stream_response_to_openai_response_websocket(
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

fn promote_generate_response_to_gemini_live(
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

fn promote_stream_response_to_gemini_live(
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

fn transform_openai_response_websocket_response(
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

fn transform_gemini_live_response(
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

fn transform_openai_response_websocket_to_gemini_live_request_direct(
    request: OpenAiCreateResponseWebSocketConnectRequest,
) -> Result<TransformRequest, MiddlewareTransformError> {
    let openai_request = OpenAiCreateResponseRequest::try_from(&request)?;
    let gemini_stream_request = GeminiStreamGenerateContentRequest::try_from(&openai_request)?;
    let gemini_live_request = GeminiLiveConnectRequest::try_from(&gemini_stream_request)?;
    Ok(TransformRequest::GeminiLive(gemini_live_request))
}

fn transform_gemini_live_to_openai_response_websocket_request_direct(
    request: GeminiLiveConnectRequest,
) -> Result<TransformRequest, MiddlewareTransformError> {
    let gemini_stream_request = GeminiStreamGenerateContentRequest::try_from(&request)?;
    let openai_request = OpenAiCreateResponseRequest::try_from(gemini_stream_request)?;
    let openai_ws_request = OpenAiCreateResponseWebSocketConnectRequest::try_from(&openai_request)?;
    Ok(TransformRequest::OpenAiResponseWebSocket(openai_ws_request))
}

fn transform_openai_response_websocket_messages_to_gemini_live_direct(
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

fn transform_gemini_live_messages_to_openai_response_websocket_direct(
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

fn transform_openai_response_websocket_to_gemini_live_response_direct(
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

fn transform_gemini_live_to_openai_response_websocket_response_direct(
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

fn transform_compact_response(
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

#[cfg(test)]
mod tests {
    use super::*;
    use gproxy_protocol::gemini::count_tokens::types::{
        GeminiContent, GeminiContentRole, GeminiPart,
    };
    use gproxy_protocol::gemini::generate_content::response::ResponseBody as GeminiGenerateContentResponseBody;
    use gproxy_protocol::gemini::live::types::{
        GeminiBidiGenerateContentClientMessage, GeminiBidiGenerateContentClientMessageType,
        GeminiBidiGenerateContentServerContent, GeminiBidiGenerateContentServerMessage,
        GeminiBidiGenerateContentServerMessageType, GeminiBidiGenerateContentSetup,
    };
    use gproxy_protocol::openai::create_response::request::RequestBody as OpenAiCreateResponseRequestBody;
    use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
    use gproxy_protocol::openai::create_response::types as rt;
    use gproxy_protocol::openai::create_response::websocket::types::{
        OpenAiCreateResponseCreateWebSocketRequestBody, OpenAiCreateResponseWebSocketClientMessage,
        OpenAiCreateResponseWebSocketServerMessage,
    };
    use gproxy_protocol::transform::openai::stream_generate_content::openai_response::utils::response_snapshot;

    #[test]
    fn encode_gemini_sse_event_filters_done_marker() {
        let encoded = stream::encode_gemini_sse_event(GeminiSseEvent {
            event: None,
            data: GeminiSseEventData::Done("[DONE]".to_string()),
        });
        assert!(encoded.is_none());
    }

    #[test]
    fn encode_gemini_sse_event_keeps_json_chunk() {
        let encoded = stream::encode_gemini_sse_event(GeminiSseEvent {
            event: None,
            data: GeminiSseEventData::Chunk(GeminiGenerateContentResponseBody::default()),
        })
        .expect("chunk should be encoded")
        .expect("chunk should serialize");
        let text = std::str::from_utf8(encoded.as_ref()).expect("valid utf8");
        assert_eq!(text, "data: {}\n\n");
    }

    #[test]
    fn stream_output_converter_chat_routes_directly() {
        let from_openai = stream::stream_output_converter_route_kind(
            ProtocolKind::OpenAi,
            ProtocolKind::OpenAiChatCompletion,
        )
        .expect("openai -> chat converter");
        assert_eq!(from_openai, "openai_response_to_chat");

        let from_claude = stream::stream_output_converter_route_kind(
            ProtocolKind::Claude,
            ProtocolKind::OpenAiChatCompletion,
        )
        .expect("claude -> chat converter");
        assert_eq!(from_claude, "claude_to_chat");

        let from_gemini = stream::stream_output_converter_route_kind(
            ProtocolKind::Gemini,
            ProtocolKind::OpenAiChatCompletion,
        )
        .expect("gemini -> chat converter");
        assert_eq!(from_gemini, "gemini_to_chat");
    }

    #[test]
    fn transform_stream_response_non_stream_input_is_unsupported() {
        let response = OpenAiCreateResponseResponse::Success {
            stats_code: StatusCode::OK,
            headers: Default::default(),
            body: response_snapshot(
                "resp_1",
                "gpt-5",
                Some(rt::ResponseStatus::Completed),
                None,
                None,
                None,
                None,
            ),
        };

        let err = transform_stream_response(
            TransformResponse::GenerateContentOpenAiResponse(response),
            ProtocolKind::OpenAiChatCompletion,
        )
        .expect_err("non-stream payload should be rejected");

        assert!(matches!(
            err,
            MiddlewareTransformError::Unsupported(
                "stream response transform requires stream_generate_content destination payload"
            )
        ));
    }

    #[test]
    fn transform_request_openai_ws_to_gemini_live_direct() {
        let input = TransformRequest::OpenAiResponseWebSocket(
            OpenAiCreateResponseWebSocketConnectRequest {
                body: Some(OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(
                    OpenAiCreateResponseCreateWebSocketRequestBody {
                        request: OpenAiCreateResponseRequestBody {
                            model: Some("gpt-5.3-codex".to_string()),
                            stream: Some(true),
                            ..OpenAiCreateResponseRequestBody::default()
                        },
                        generate: None,
                        client_metadata: None,
                    },
                )),
                ..OpenAiCreateResponseWebSocketConnectRequest::default()
            },
        );
        let route = TransformRoute {
            src_operation: OperationFamily::OpenAiResponseWebSocket,
            src_protocol: ProtocolKind::OpenAi,
            dst_operation: OperationFamily::GeminiLive,
            dst_protocol: ProtocolKind::Gemini,
        };

        let transformed = transform_request(input, route).expect("conversion should succeed");
        let TransformRequest::GeminiLive(request) = transformed else {
            panic!("expected gemini live request");
        };

        let Some(GeminiBidiGenerateContentClientMessage {
            message_type: GeminiBidiGenerateContentClientMessageType::Setup { setup },
        }) = request.body
        else {
            panic!("expected setup frame");
        };
        assert!(setup.model.starts_with("models/"));
    }

    #[test]
    fn transform_request_gemini_live_to_openai_ws_direct() {
        let input = TransformRequest::GeminiLive(GeminiLiveConnectRequest {
            body: Some(GeminiBidiGenerateContentClientMessage {
                message_type: GeminiBidiGenerateContentClientMessageType::Setup {
                    setup: GeminiBidiGenerateContentSetup {
                        model: "models/gemini-2.5-flash".to_string(),
                        ..GeminiBidiGenerateContentSetup::default()
                    },
                },
            }),
            ..GeminiLiveConnectRequest::default()
        });
        let route = TransformRoute {
            src_operation: OperationFamily::GeminiLive,
            src_protocol: ProtocolKind::Gemini,
            dst_operation: OperationFamily::OpenAiResponseWebSocket,
            dst_protocol: ProtocolKind::OpenAi,
        };

        let transformed = transform_request(input, route).expect("conversion should succeed");
        let TransformRequest::OpenAiResponseWebSocket(request) = transformed else {
            panic!("expected openai websocket request");
        };

        let Some(OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(create)) = request.body
        else {
            panic!("expected response.create frame");
        };
        assert_eq!(
            create.request.model.as_deref(),
            Some("gemini-2.5-flash"),
            "gemini model should map to openai model id"
        );
    }

    #[test]
    fn transform_response_openai_ws_to_gemini_live_direct() {
        let input = TransformResponse::OpenAiResponseWebSocket(vec![
            OpenAiCreateResponseWebSocketServerMessage::StreamEvent(ResponseStreamEvent::Error {
                code: "invalid_prompt".to_string(),
                message: "bad prompt".to_string(),
                param: None,
                sequence_number: 1,
            }),
        ]);
        let route = TransformRoute {
            src_operation: OperationFamily::GeminiLive,
            src_protocol: ProtocolKind::Gemini,
            dst_operation: OperationFamily::OpenAiResponseWebSocket,
            dst_protocol: ProtocolKind::OpenAi,
        };

        let transformed = transform_response(input, route).expect("conversion should succeed");
        let TransformResponse::GeminiLive(messages) = transformed else {
            panic!("expected gemini live response");
        };
        assert!(!messages.is_empty());
    }

    #[test]
    fn transform_response_gemini_live_to_openai_ws_direct() {
        let input = TransformResponse::GeminiLive(vec![GeminiLiveMessageResponse::Message(
            GeminiBidiGenerateContentServerMessage {
                usage_metadata: None,
                message_type: GeminiBidiGenerateContentServerMessageType::ServerContent {
                    server_content: GeminiBidiGenerateContentServerContent {
                        model_turn: Some(GeminiContent {
                            parts: vec![GeminiPart {
                                text: Some("hello".to_string()),
                                ..GeminiPart::default()
                            }],
                            role: Some(GeminiContentRole::Model),
                        }),
                        generation_complete: Some(true),
                        turn_complete: Some(true),
                        ..GeminiBidiGenerateContentServerContent::default()
                    },
                },
            },
        )]);
        let route = TransformRoute {
            src_operation: OperationFamily::OpenAiResponseWebSocket,
            src_protocol: ProtocolKind::OpenAi,
            dst_operation: OperationFamily::GeminiLive,
            dst_protocol: ProtocolKind::Gemini,
        };

        let transformed = transform_response(input, route).expect("conversion should succeed");
        let TransformResponse::OpenAiResponseWebSocket(messages) = transformed else {
            panic!("expected openai websocket response");
        };
        assert!(!messages.is_empty());
    }
}
