use std::collections::VecDeque;

use bytes::Bytes;
use futures_util::{StreamExt, stream};
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
    Box::pin(stream::once(async move { Ok(Bytes::from(bytes)) }))
}

pub(crate) fn decode_request_payload(
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

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum SourceStreamEvent {
    OpenAiResponse(OpenAiCreateResponseSseEvent),
    OpenAiChat(OpenAiChatCompletionsSseEvent),
    Claude(ClaudeCreateMessageStreamEvent),
    Gemini(GeminiSseEvent),
}

#[derive(Debug)]
enum SourceStreamDecoder {
    Sse {
        protocol: ProtocolKind,
        buffer: Vec<u8>,
    },
    GeminiNdjson {
        buffer: Vec<u8>,
    },
}

impl SourceStreamDecoder {
    fn new(protocol: ProtocolKind) -> Result<Self, MiddlewareTransformError> {
        match protocol {
            ProtocolKind::OpenAi
            | ProtocolKind::OpenAiChatCompletion
            | ProtocolKind::Claude
            | ProtocolKind::Gemini => Ok(Self::Sse {
                protocol,
                buffer: Vec::new(),
            }),
            ProtocolKind::GeminiNDJson => Ok(Self::GeminiNdjson { buffer: Vec::new() }),
        }
    }

    fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SourceStreamEvent>, MiddlewareTransformError> {
        match self {
            Self::Sse { protocol, buffer } => {
                buffer.extend_from_slice(chunk);
                let mut out = Vec::new();
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(event) = decode_sse_frame(*protocol, frame.as_slice())? {
                        out.push(event);
                    }
                }
                Ok(out)
            }
            Self::GeminiNdjson { buffer } => {
                buffer.extend_from_slice(chunk);
                let mut out = Vec::new();
                while let Some(line) = next_ndjson_line(buffer) {
                    if let Some(event) = decode_gemini_ndjson_line(line.as_slice())? {
                        out.push(event);
                    }
                }
                Ok(out)
            }
        }
    }

    fn finish(&mut self) -> Result<Vec<SourceStreamEvent>, MiddlewareTransformError> {
        match self {
            Self::Sse { protocol, buffer } => {
                if buffer.iter().all(u8::is_ascii_whitespace) {
                    buffer.clear();
                    return Ok(Vec::new());
                }

                let trailing = std::mem::take(buffer);
                let mut out = Vec::new();
                if let Some(event) = decode_sse_frame(*protocol, trailing.as_slice())? {
                    out.push(event);
                }
                Ok(out)
            }
            Self::GeminiNdjson { buffer } => {
                if buffer.is_empty() || buffer.iter().all(u8::is_ascii_whitespace) {
                    buffer.clear();
                    return Ok(Vec::new());
                }

                let trailing = std::mem::take(buffer);
                let mut out = Vec::new();
                if let Some(event) = decode_gemini_ndjson_line(trailing.as_slice())? {
                    out.push(event);
                }
                Ok(out)
            }
        }
    }
}

#[derive(Debug, Default)]
enum ClaudeStreamConverter {
    #[default]
    Identity,
    FromOpenAiResponse(OpenAiResponseToClaudeStream),
    FromOpenAiChat(OpenAiChatCompletionsToClaudeStream),
    FromGemini(GeminiToClaudeStream),
}

impl ClaudeStreamConverter {
    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<ClaudeCreateMessageStreamEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => match event {
                SourceStreamEvent::Claude(event) => Ok(vec![event]),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "claude stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiResponse(converter) => match event {
                SourceStreamEvent::OpenAiResponse(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "claude stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiChat(converter) => match event {
                SourceStreamEvent::OpenAiChat(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "claude stream converter source protocol mismatch",
                )),
            },
            Self::FromGemini(converter) => match event {
                SourceStreamEvent::Gemini(event) => Ok(converter.on_sse_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "claude stream converter source protocol mismatch",
                )),
            },
        }
    }

    fn finish(&mut self) -> Vec<ClaudeCreateMessageStreamEvent> {
        match self {
            Self::Identity => Vec::new(),
            Self::FromOpenAiResponse(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
            Self::FromOpenAiChat(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
            Self::FromGemini(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
        }
    }
}

#[derive(Debug, Default)]
enum GeminiStreamConverter {
    #[default]
    Identity,
    FromOpenAiResponse(OpenAiResponseToGeminiStream),
    FromOpenAiChat(OpenAiChatCompletionsToGeminiStream),
    FromClaude(ClaudeToGeminiStream),
}

impl GeminiStreamConverter {
    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<GeminiSseEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => match event {
                SourceStreamEvent::Gemini(event) => Ok(vec![event]),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiResponse(converter) => match event {
                SourceStreamEvent::OpenAiResponse(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiChat(converter) => match event {
                SourceStreamEvent::OpenAiChat(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream converter source protocol mismatch",
                )),
            },
            Self::FromClaude(converter) => match event {
                SourceStreamEvent::Claude(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream converter source protocol mismatch",
                )),
            },
        }
    }

    fn finish(&mut self) -> Vec<GeminiSseEvent> {
        match self {
            Self::Identity => Vec::new(),
            Self::FromOpenAiResponse(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    vec![gemini_done_event()]
                }
            }
            Self::FromOpenAiChat(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    vec![gemini_done_event()]
                }
            }
            Self::FromClaude(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    vec![gemini_done_event()]
                }
            }
        }
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum StreamOutputConverter {
    OpenAiResponse(OpenAiResponseStreamConverter),
    OpenAiChat(OpenAiChatStreamConverter),
    Claude(ClaudeStreamConverter),
    Gemini {
        converter: GeminiStreamConverter,
        ndjson: bool,
    },
}

#[derive(Debug, Default)]
enum OpenAiResponseStreamConverter {
    #[default]
    Identity,
    FromOpenAiChat(OpenAiChatCompletionsToOpenAiResponseStream),
    FromClaude(ClaudeToOpenAiResponseStream),
    FromGemini(GeminiToOpenAiResponseStream),
}

impl OpenAiResponseStreamConverter {
    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<OpenAiCreateResponseSseEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => match event {
                SourceStreamEvent::OpenAiResponse(event) => Ok(vec![event]),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai response stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiChat(converter) => match event {
                SourceStreamEvent::OpenAiChat(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai response stream converter source protocol mismatch",
                )),
            },
            Self::FromClaude(converter) => match event {
                SourceStreamEvent::Claude(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai response stream converter source protocol mismatch",
                )),
            },
            Self::FromGemini(converter) => match event {
                SourceStreamEvent::Gemini(event) => Ok(converter.on_sse_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai response stream converter source protocol mismatch",
                )),
            },
        }
    }

    fn finish(&mut self) -> Vec<OpenAiCreateResponseSseEvent> {
        match self {
            Self::Identity => Vec::new(),
            Self::FromOpenAiChat(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
            Self::FromClaude(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
            Self::FromGemini(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
        }
    }
}

#[derive(Debug, Default)]
#[allow(clippy::large_enum_variant)]
enum OpenAiChatStreamConverter {
    #[default]
    Identity,
    FromOpenAiResponse(OpenAiResponseToOpenAiChatCompletionsStream),
    FromClaude(ClaudeToOpenAiChatCompletionsStream),
    FromGemini(GeminiToOpenAiChatCompletionsStream),
}

impl OpenAiChatStreamConverter {
    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<OpenAiChatCompletionsSseEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => match event {
                SourceStreamEvent::OpenAiChat(event) => Ok(vec![event]),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiResponse(converter) => match event {
                SourceStreamEvent::OpenAiResponse(event) => Ok(converter.on_event(event)),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream converter source protocol mismatch",
                )),
            },
            Self::FromClaude(converter) => match event {
                SourceStreamEvent::Claude(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream converter source protocol mismatch",
                )),
            },
            Self::FromGemini(converter) => match event {
                SourceStreamEvent::Gemini(event) => Ok(converter.on_sse_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream converter source protocol mismatch",
                )),
            },
        }
    }

    fn finish(&mut self) -> Result<Vec<OpenAiChatCompletionsSseEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => Ok(Vec::new()),
            Self::FromOpenAiResponse(converter) => Ok(converter.finish()),
            Self::FromClaude(converter) => Ok(converter.finish()),
            Self::FromGemini(converter) => Ok(converter.finish()),
        }
    }
}

impl StreamOutputConverter {
    fn new(
        from_protocol: ProtocolKind,
        to_protocol: ProtocolKind,
    ) -> Result<Self, MiddlewareTransformError> {
        match to_protocol {
            ProtocolKind::OpenAi => Ok(Self::OpenAiResponse(match from_protocol {
                ProtocolKind::OpenAi => OpenAiResponseStreamConverter::Identity,
                ProtocolKind::OpenAiChatCompletion => {
                    OpenAiResponseStreamConverter::FromOpenAiChat(Default::default())
                }
                ProtocolKind::Claude => {
                    OpenAiResponseStreamConverter::FromClaude(Default::default())
                }
                ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
                    OpenAiResponseStreamConverter::FromGemini(Default::default())
                }
            })),
            ProtocolKind::OpenAiChatCompletion => Ok(Self::OpenAiChat(match from_protocol {
                ProtocolKind::OpenAiChatCompletion => OpenAiChatStreamConverter::Identity,
                ProtocolKind::OpenAi => {
                    OpenAiChatStreamConverter::FromOpenAiResponse(Default::default())
                }
                ProtocolKind::Claude => OpenAiChatStreamConverter::FromClaude(Default::default()),
                ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
                    OpenAiChatStreamConverter::FromGemini(Default::default())
                }
            })),
            ProtocolKind::Claude => Ok(Self::Claude(match from_protocol {
                ProtocolKind::OpenAi => {
                    ClaudeStreamConverter::FromOpenAiResponse(Default::default())
                }
                ProtocolKind::OpenAiChatCompletion => {
                    ClaudeStreamConverter::FromOpenAiChat(Default::default())
                }
                ProtocolKind::Claude => ClaudeStreamConverter::Identity,
                ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
                    ClaudeStreamConverter::FromGemini(Default::default())
                }
            })),
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => Ok(Self::Gemini {
                converter: match from_protocol {
                    ProtocolKind::OpenAi => {
                        GeminiStreamConverter::FromOpenAiResponse(Default::default())
                    }
                    ProtocolKind::OpenAiChatCompletion => {
                        GeminiStreamConverter::FromOpenAiChat(Default::default())
                    }
                    ProtocolKind::Claude => GeminiStreamConverter::FromClaude(Default::default()),
                    ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
                        GeminiStreamConverter::Identity
                    }
                },
                ndjson: to_protocol == ProtocolKind::GeminiNDJson,
            }),
        }
    }

    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<Bytes>, MiddlewareTransformError> {
        match self {
            Self::OpenAiResponse(converter) => converter
                .on_event(event)?
                .into_iter()
                .map(encode_openai_sse_event)
                .collect(),
            Self::OpenAiChat(converter) => converter
                .on_event(event)?
                .into_iter()
                .map(encode_openai_chat_sse_event)
                .collect(),
            Self::Claude(converter) => converter
                .on_event(event)?
                .into_iter()
                .map(encode_claude_sse_event)
                .collect(),
            Self::Gemini { converter, ndjson } => {
                let events = converter.on_event(event)?;
                if *ndjson {
                    events
                        .into_iter()
                        .filter_map(encode_gemini_ndjson_event)
                        .collect::<Result<Vec<_>, _>>()
                } else {
                    events
                        .into_iter()
                        .filter_map(encode_gemini_sse_event)
                        .collect::<Result<Vec<_>, _>>()
                }
            }
        }
    }

    fn finish(&mut self) -> Result<Vec<Bytes>, MiddlewareTransformError> {
        match self {
            Self::OpenAiResponse(converter) => converter
                .finish()
                .into_iter()
                .map(encode_openai_sse_event)
                .collect(),
            Self::OpenAiChat(converter) => converter
                .finish()?
                .into_iter()
                .map(encode_openai_chat_sse_event)
                .collect(),
            Self::Claude(converter) => converter
                .finish()
                .into_iter()
                .map(encode_claude_sse_event)
                .collect(),
            Self::Gemini { converter, ndjson } => {
                let events = converter.finish();
                if *ndjson {
                    events
                        .into_iter()
                        .filter_map(encode_gemini_ndjson_event)
                        .collect::<Result<Vec<_>, _>>()
                } else {
                    events
                        .into_iter()
                        .filter_map(encode_gemini_sse_event)
                        .collect::<Result<Vec<_>, _>>()
                }
            }
        }
    }
}

struct StreamTransformState {
    input: TransformBodyStream,
    decoder: SourceStreamDecoder,
    converter: StreamOutputConverter,
    output: VecDeque<Bytes>,
    input_ended: bool,
}

impl StreamTransformState {
    fn new(
        input: TransformBodyStream,
        from_protocol: ProtocolKind,
        to_protocol: ProtocolKind,
    ) -> Result<Self, MiddlewareTransformError> {
        Ok(Self {
            input,
            decoder: SourceStreamDecoder::new(from_protocol)?,
            converter: StreamOutputConverter::new(from_protocol, to_protocol)?,
            output: VecDeque::new(),
            input_ended: false,
        })
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), MiddlewareTransformError> {
        let events = self.decoder.feed(chunk)?;
        for event in events {
            self.output.extend(self.converter.on_event(event)?);
        }
        Ok(())
    }

    fn finish_input(&mut self) -> Result<(), MiddlewareTransformError> {
        let trailing_events = self.decoder.finish()?;
        for event in trailing_events {
            self.output.extend(self.converter.on_event(event)?);
        }
        self.output.extend(self.converter.finish()?);
        self.input_ended = true;
        Ok(())
    }

    fn pop_output(&mut self) -> Option<Bytes> {
        self.output.pop_front()
    }
}

fn supports_incremental_stream_response_conversion(
    from_protocol: ProtocolKind,
    to_protocol: ProtocolKind,
) -> bool {
    matches!(
        to_protocol,
        ProtocolKind::OpenAi
            | ProtocolKind::OpenAiChatCompletion
            | ProtocolKind::Claude
            | ProtocolKind::Gemini
            | ProtocolKind::GeminiNDJson
    ) && matches!(
        from_protocol,
        ProtocolKind::OpenAi
            | ProtocolKind::OpenAiChatCompletion
            | ProtocolKind::Claude
            | ProtocolKind::Gemini
            | ProtocolKind::GeminiNDJson
    )
}

fn transform_stream_response_body(
    input: TransformBodyStream,
    from_protocol: ProtocolKind,
    to_protocol: ProtocolKind,
) -> Result<TransformBodyStream, MiddlewareTransformError> {
    let state = StreamTransformState::new(input, from_protocol, to_protocol)?;

    let output = stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(chunk) = state.pop_output() {
                return Ok(Some((chunk, state)));
            }

            if state.input_ended {
                return Ok(None);
            }

            match state.input.next().await {
                Some(Ok(chunk)) => state.push_chunk(chunk.as_ref())?,
                Some(Err(err)) => return Err(err),
                None => state.finish_input()?,
            }
        }
    });

    Ok(Box::pin(output))
}

fn next_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let lf_pos = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf_pos = buffer.windows(4).position(|window| window == b"\r\n\r\n");

    let (pos, delim_len) = match (lf_pos, crlf_pos) {
        (Some(a), Some(b)) if a <= b => (a, 2),
        (Some(_), Some(b)) => (b, 4),
        (Some(a), None) => (a, 2),
        (None, Some(b)) => (b, 4),
        (None, None) => return None,
    };

    let frame = buffer[..pos].to_vec();
    buffer.drain(..pos + delim_len);
    Some(frame)
}

fn next_ndjson_line(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let newline = buffer.iter().position(|b| *b == b'\n')?;
    let line = buffer[..newline].to_vec();
    buffer.drain(..newline + 1);
    Some(line)
}

fn parse_sse_fields(
    frame: &[u8],
    protocol: ProtocolKind,
) -> Result<Option<(Option<String>, String)>, MiddlewareTransformError> {
    if frame.is_empty() {
        return Ok(None);
    }

    let text = std::str::from_utf8(frame).map_err(|err| MiddlewareTransformError::JsonDecode {
        kind: "response_stream",
        operation: OperationFamily::StreamGenerateContent,
        protocol,
        message: err.to_string(),
    })?;

    let mut event = None;
    let mut data_lines = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim_start().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        return Ok(None);
    }

    Ok(Some((event, data_lines.join("\n"))))
}

fn decode_sse_frame(
    protocol: ProtocolKind,
    frame: &[u8],
) -> Result<Option<SourceStreamEvent>, MiddlewareTransformError> {
    let Some((event_name, data)) = parse_sse_fields(frame, protocol)? else {
        return Ok(None);
    };

    Ok(Some(match protocol {
        ProtocolKind::OpenAi => SourceStreamEvent::OpenAiResponse(OpenAiCreateResponseSseEvent {
            event: event_name,
            data: if data == "[DONE]" {
                OpenAiCreateResponseSseData::Done(data)
            } else {
                OpenAiCreateResponseSseData::Event(serde_json::from_str(&data).map_err(|err| {
                    MiddlewareTransformError::JsonDecode {
                        kind: "response_stream",
                        operation: OperationFamily::StreamGenerateContent,
                        protocol,
                        message: err.to_string(),
                    }
                })?)
            },
        }),
        ProtocolKind::OpenAiChatCompletion => {
            SourceStreamEvent::OpenAiChat(OpenAiChatCompletionsSseEvent {
                event: event_name,
                data: if data == "[DONE]" {
                    OpenAiChatCompletionsSseData::Done(data)
                } else {
                    OpenAiChatCompletionsSseData::Chunk(serde_json::from_str(&data).map_err(
                        |err| MiddlewareTransformError::JsonDecode {
                            kind: "response_stream",
                            operation: OperationFamily::StreamGenerateContent,
                            protocol,
                            message: err.to_string(),
                        },
                    )?)
                },
            })
        }
        ProtocolKind::Claude => {
            SourceStreamEvent::Claude(serde_json::from_str(&data).map_err(|err| {
                MiddlewareTransformError::JsonDecode {
                    kind: "response_stream",
                    operation: OperationFamily::StreamGenerateContent,
                    protocol,
                    message: err.to_string(),
                }
            })?)
        }
        ProtocolKind::Gemini => SourceStreamEvent::Gemini(GeminiSseEvent {
            event: event_name,
            data: if data == "[DONE]" {
                GeminiSseEventData::Done(data)
            } else {
                GeminiSseEventData::Chunk(serde_json::from_str(&data).map_err(|err| {
                    MiddlewareTransformError::JsonDecode {
                        kind: "response_stream",
                        operation: OperationFamily::StreamGenerateContent,
                        protocol,
                        message: err.to_string(),
                    }
                })?)
            },
        }),
        ProtocolKind::GeminiNDJson => {
            return Err(MiddlewareTransformError::Unsupported(
                "gemini ndjson stream uses line-delimited framing instead of sse framing",
            ));
        }
    }))
}

fn decode_gemini_ndjson_line(
    line: &[u8],
) -> Result<Option<SourceStreamEvent>, MiddlewareTransformError> {
    let trimmed = line
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let text = std::str::from_utf8(trimmed.as_slice()).map_err(|err| {
        MiddlewareTransformError::JsonDecode {
            kind: "response_stream",
            operation: OperationFamily::StreamGenerateContent,
            protocol: ProtocolKind::GeminiNDJson,
            message: err.to_string(),
        }
    })?;
    if text == "[DONE]" {
        return Ok(Some(SourceStreamEvent::Gemini(gemini_done_event())));
    }

    let chunk: GeminiGenerateContentResponseBody =
        serde_json::from_str(text).map_err(|err| MiddlewareTransformError::JsonDecode {
            kind: "response_stream",
            operation: OperationFamily::StreamGenerateContent,
            protocol: ProtocolKind::GeminiNDJson,
            message: err.to_string(),
        })?;
    Ok(Some(SourceStreamEvent::Gemini(GeminiSseEvent {
        event: None,
        data: GeminiSseEventData::Chunk(chunk),
    })))
}

fn encode_sse_frame(event: Option<&str>, data: &str) -> Bytes {
    let mut out = String::new();
    if let Some(event) = event {
        out.push_str("event: ");
        out.push_str(event);
        out.push('\n');
    }
    out.push_str("data: ");
    out.push_str(data);
    out.push_str("\n\n");
    Bytes::from(out)
}

fn encode_openai_sse_event(
    event: OpenAiCreateResponseSseEvent,
) -> Result<Bytes, MiddlewareTransformError> {
    let data = match event.data {
        OpenAiCreateResponseSseData::Event(stream_event) => serde_json::to_string(&stream_event)
            .map_err(|err| MiddlewareTransformError::JsonEncode {
                kind: "response_stream",
                operation: OperationFamily::StreamGenerateContent,
                protocol: ProtocolKind::OpenAi,
                message: err.to_string(),
            })?,
        OpenAiCreateResponseSseData::Done(done) => done,
    };
    Ok(encode_sse_frame(event.event.as_deref(), &data))
}

fn claude_sse_event_name(event: &ClaudeCreateMessageStreamEvent) -> &'static str {
    match event {
        ClaudeCreateMessageStreamEvent::MessageStart(_) => "message_start",
        ClaudeCreateMessageStreamEvent::ContentBlockStart(_) => "content_block_start",
        ClaudeCreateMessageStreamEvent::ContentBlockDelta(_) => "content_block_delta",
        ClaudeCreateMessageStreamEvent::ContentBlockStop(_) => "content_block_stop",
        ClaudeCreateMessageStreamEvent::MessageDelta(_) => "message_delta",
        ClaudeCreateMessageStreamEvent::MessageStop(_) => "message_stop",
        ClaudeCreateMessageStreamEvent::Ping(_) => "ping",
        ClaudeCreateMessageStreamEvent::Error(_) => "error",
        ClaudeCreateMessageStreamEvent::Unknown(_) => "unknown",
    }
}

fn encode_claude_sse_event(
    event: ClaudeCreateMessageStreamEvent,
) -> Result<Bytes, MiddlewareTransformError> {
    let data =
        serde_json::to_string(&event).map_err(|err| MiddlewareTransformError::JsonEncode {
            kind: "response_stream",
            operation: OperationFamily::StreamGenerateContent,
            protocol: ProtocolKind::Claude,
            message: err.to_string(),
        })?;
    Ok(encode_sse_frame(Some(claude_sse_event_name(&event)), &data))
}

fn encode_gemini_sse_event(
    event: GeminiSseEvent,
) -> Option<Result<Bytes, MiddlewareTransformError>> {
    let GeminiSseEvent { event, data } = event;
    match data {
        GeminiSseEventData::Chunk(chunk) => Some(
            serde_json::to_string(&chunk)
                .map(|json| encode_sse_frame(event.as_deref(), &json))
                .map_err(|err| MiddlewareTransformError::JsonEncode {
                    kind: "response_stream",
                    operation: OperationFamily::StreamGenerateContent,
                    protocol: ProtocolKind::Gemini,
                    message: err.to_string(),
                }),
        ),
        // Gemini SSE clients expect JSON payloads in `data:` lines and may fail on `[DONE]`.
        GeminiSseEventData::Done(_) => None,
    }
}

fn encode_gemini_ndjson_event(
    event: GeminiSseEvent,
) -> Option<Result<Bytes, MiddlewareTransformError>> {
    match event.data {
        GeminiSseEventData::Chunk(chunk) => Some(
            serde_json::to_vec(&chunk)
                .map(|mut json| {
                    json.push(b'\n');
                    Bytes::from(json)
                })
                .map_err(|err| MiddlewareTransformError::JsonEncode {
                    kind: "response_stream",
                    operation: OperationFamily::StreamGenerateContent,
                    protocol: ProtocolKind::GeminiNDJson,
                    message: err.to_string(),
                }),
        ),
        GeminiSseEventData::Done(_) => None,
    }
}

fn gemini_done_event() -> GeminiSseEvent {
    GeminiSseEvent {
        event: None,
        data: GeminiSseEventData::Done("[DONE]".to_string()),
    }
}

fn encode_openai_chat_sse_event(
    event: OpenAiChatCompletionsSseEvent,
) -> Result<Bytes, MiddlewareTransformError> {
    let data = match event.data {
        OpenAiChatCompletionsSseData::Chunk(chunk) => {
            serde_json::to_string(&chunk).map_err(|err| MiddlewareTransformError::JsonEncode {
                kind: "response_stream",
                operation: OperationFamily::StreamGenerateContent,
                protocol: ProtocolKind::OpenAiChatCompletion,
                message: err.to_string(),
            })?
        }
        OpenAiChatCompletionsSseData::Done(done) => done,
    };
    Ok(encode_sse_frame(event.event.as_deref(), &data))
}

fn chunks_to_body_stream(chunks: Vec<Bytes>) -> TransformBodyStream {
    Box::pin(stream::iter(chunks.into_iter().map(Ok)))
}

async fn collect_source_stream_events(
    body: TransformBodyStream,
    protocol: ProtocolKind,
) -> Result<Vec<SourceStreamEvent>, MiddlewareTransformError> {
    let mut decoder = SourceStreamDecoder::new(protocol)?;
    let mut input = body;
    let mut events = Vec::new();
    while let Some(chunk) = input.next().await {
        events.extend(decoder.feed(chunk?.as_ref())?);
    }
    events.extend(decoder.finish()?);
    Ok(events)
}

fn source_events_to_stream_response(
    protocol: ProtocolKind,
    events: Vec<SourceStreamEvent>,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match protocol {
        ProtocolKind::OpenAi => {
            let mut out = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::OpenAiResponse(event) => out.push(event),
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding openai stream",
                        ));
                    }
                }
            }
            Ok(TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody { events: out },
            ))
        }
        ProtocolKind::OpenAiChatCompletion => {
            let mut out = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::OpenAiChat(event) => out.push(event),
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding openai chat stream",
                        ));
                    }
                }
            }
            Ok(
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody { events: out },
                ),
            )
        }
        ProtocolKind::Claude => {
            let mut out = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::Claude(event) => out.push(event),
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding claude stream",
                        ));
                    }
                }
            }
            Ok(TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody { events: out },
            ))
        }
        ProtocolKind::Gemini => {
            let mut out = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::Gemini(event) => out.push(event),
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding gemini stream",
                        ));
                    }
                }
            }
            Ok(TransformResponse::StreamGenerateContentGeminiSse(
                GeminiStreamGenerateContentResponse::SseSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiSseStreamBody { events: out },
                },
            ))
        }
        ProtocolKind::GeminiNDJson => {
            let mut chunks = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::Gemini(event) => {
                        if let GeminiSseEventData::Chunk(chunk) = event.data {
                            chunks.push(chunk);
                        }
                    }
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding gemini ndjson stream",
                        ));
                    }
                }
            }
            Ok(TransformResponse::StreamGenerateContentGeminiNdjson(
                GeminiStreamGenerateContentResponse::NdjsonSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiNdjsonStreamBody { chunks },
                },
            ))
        }
    }
}

fn encode_stream_response_payload(
    response: TransformResponse,
) -> Result<TransformResponsePayload, MiddlewareTransformError> {
    let operation = response.operation();
    let protocol = response.protocol();

    let body = match response {
        TransformResponse::StreamGenerateContentOpenAiResponse(stream_body) => {
            let chunks = stream_body
                .events
                .into_iter()
                .map(encode_openai_sse_event)
                .collect::<Result<Vec<_>, _>>()?;
            chunks_to_body_stream(chunks)
        }
        TransformResponse::StreamGenerateContentOpenAiChatCompletions(stream_body) => {
            let chunks = stream_body
                .events
                .into_iter()
                .map(encode_openai_chat_sse_event)
                .collect::<Result<Vec<_>, _>>()?;
            chunks_to_body_stream(chunks)
        }
        TransformResponse::StreamGenerateContentClaude(stream_body) => {
            let chunks = stream_body
                .events
                .into_iter()
                .map(encode_claude_sse_event)
                .collect::<Result<Vec<_>, _>>()?;
            chunks_to_body_stream(chunks)
        }
        TransformResponse::StreamGenerateContentGeminiSse(stream_response) => {
            match ensure_gemini_sse_stream(stream_response) {
                GeminiStreamGenerateContentResponse::SseSuccess { body, .. } => {
                    let chunks = body
                        .events
                        .into_iter()
                        .filter_map(encode_gemini_sse_event)
                        .collect::<Result<Vec<_>, _>>()?;
                    chunks_to_body_stream(chunks)
                }
                GeminiStreamGenerateContentResponse::Error { body, .. } => {
                    let bytes = serde_json::to_vec(&body).map_err(|err| {
                        MiddlewareTransformError::JsonEncode {
                            kind: "response_stream",
                            operation: OperationFamily::StreamGenerateContent,
                            protocol: ProtocolKind::Gemini,
                            message: err.to_string(),
                        }
                    })?;
                    bytes_to_body_stream(bytes)
                }
                GeminiStreamGenerateContentResponse::NdjsonSuccess { .. } => {
                    return Err(MiddlewareTransformError::Unsupported(
                        "unexpected ndjson variant while encoding gemini sse stream",
                    ));
                }
            }
        }
        TransformResponse::StreamGenerateContentGeminiNdjson(stream_response) => {
            match ensure_gemini_ndjson_stream(stream_response) {
                GeminiStreamGenerateContentResponse::NdjsonSuccess { body, .. } => {
                    let chunks = body
                        .chunks
                        .into_iter()
                        .map(|chunk| {
                            serde_json::to_vec(&chunk)
                                .map(|mut json| {
                                    json.push(b'\n');
                                    Bytes::from(json)
                                })
                                .map_err(|err| MiddlewareTransformError::JsonEncode {
                                    kind: "response_stream",
                                    operation: OperationFamily::StreamGenerateContent,
                                    protocol: ProtocolKind::GeminiNDJson,
                                    message: err.to_string(),
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    chunks_to_body_stream(chunks)
                }
                GeminiStreamGenerateContentResponse::Error { body, .. } => {
                    let bytes = serde_json::to_vec(&body).map_err(|err| {
                        MiddlewareTransformError::JsonEncode {
                            kind: "response_stream",
                            operation: OperationFamily::StreamGenerateContent,
                            protocol: ProtocolKind::GeminiNDJson,
                            message: err.to_string(),
                        }
                    })?;
                    bytes_to_body_stream(bytes)
                }
                GeminiStreamGenerateContentResponse::SseSuccess { .. } => {
                    return Err(MiddlewareTransformError::Unsupported(
                        "unexpected sse variant while encoding gemini ndjson stream",
                    ));
                }
            }
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "encode_stream_response_payload expects a stream response variant",
            ));
        }
    };

    Ok(TransformResponsePayload::new(operation, protocol, body))
}

async fn transform_buffered_stream_response_payload(
    input: TransformResponsePayload,
    route: TransformRoute,
) -> Result<TransformResponsePayload, MiddlewareTransformError> {
    let events = collect_source_stream_events(input.body, input.protocol).await?;
    let decoded = source_events_to_stream_response(input.protocol, events)?;
    let transformed = transform_response(decoded, route)?;
    if transformed.operation() == OperationFamily::StreamGenerateContent {
        encode_stream_response_payload(transformed)
    } else {
        let operation = transformed.operation();
        let protocol = transformed.protocol();
        let body = encode_response_payload(transformed)?;
        Ok(TransformResponsePayload::new(
            operation,
            protocol,
            bytes_to_body_stream(body),
        ))
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

    if route.is_passthrough() {
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

    if route.is_passthrough() {
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

    match route.dst_operation {
        OperationFamily::ModelList => transform_model_list_request(input, route.dst_protocol),
        OperationFamily::ModelGet => transform_model_get_request(input, route.dst_protocol),
        OperationFamily::CountToken => transform_count_tokens_request(input, route.dst_protocol),
        OperationFamily::Embedding => transform_embeddings_request(input, route.dst_protocol),
        OperationFamily::GenerateContent => transform_generate_request(input, route.dst_protocol),
        OperationFamily::StreamGenerateContent => {
            let generate_request = transform_generate_request(input, route.dst_protocol)?;
            promote_generate_request_to_stream(generate_request, route.dst_protocol)
        }
        OperationFamily::Compact => transform_compact_request(input, route.dst_protocol),
    }
}

pub fn transform_response(
    input: TransformResponse,
    route: TransformRoute,
) -> Result<TransformResponse, MiddlewareTransformError> {
    ensure_response_route_destination(&input, route)?;
    if route.is_passthrough() {
        return Ok(input);
    }

    let mut current_operation = route.dst_operation;
    let mut current_response = input;

    if current_operation == OperationFamily::StreamGenerateContent
        && route.src_operation != OperationFamily::StreamGenerateContent
    {
        current_response = demote_stream_response_to_generate(current_response)?;
        current_operation = OperationFamily::GenerateContent;
    }

    if route.src_operation == OperationFamily::StreamGenerateContent
        && current_operation != OperationFamily::StreamGenerateContent
    {
        let generated = transform_generate_response(current_response, route.src_protocol)?;
        return promote_generate_response_to_stream(generated, route.src_protocol);
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
            "generate_content request transform requires generate/stream/compact source payload",
        )),
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

fn demote_stream_response_to_generate(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::StreamGenerateContentOpenAiResponse(response) => {
            TransformResponse::GenerateContentOpenAiResponse(
                OpenAiCreateResponseResponse::try_from(response)?,
            )
        }
        TransformResponse::StreamGenerateContentOpenAiChatCompletions(response) => {
            TransformResponse::GenerateContentOpenAiChatCompletions(
                OpenAiChatCompletionsResponse::try_from(response)?,
            )
        }
        TransformResponse::StreamGenerateContentClaude(response) => {
            TransformResponse::GenerateContentClaude(ClaudeCreateMessageResponse::try_from(
                response,
            )?)
        }
        TransformResponse::StreamGenerateContentGeminiSse(response)
        | TransformResponse::StreamGenerateContentGeminiNdjson(response) => {
            TransformResponse::GenerateContentGemini(GeminiGenerateContentResponse::try_from(
                response,
            )?)
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "stream response demotion requires stream_generate_content destination payload",
            ));
        }
    })
}

fn promote_generate_response_to_stream(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match input {
        TransformResponse::GenerateContentOpenAiResponse(response) => {
            if dst_protocol != ProtocolKind::OpenAi {
                return Err(MiddlewareTransformError::Unsupported(
                    "openai response stream conversion requires OpenAi destination protocol",
                ));
            }
            Ok(TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody::try_from(response)?,
            ))
        }
        TransformResponse::GenerateContentOpenAiChatCompletions(response) => {
            if dst_protocol != ProtocolKind::OpenAiChatCompletion {
                return Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream conversion requires OpenAiChatCompletion destination protocol",
                ));
            }
            Ok(
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(response)?,
                ),
            )
        }
        TransformResponse::GenerateContentClaude(response) => {
            if dst_protocol != ProtocolKind::Claude {
                return Err(MiddlewareTransformError::Unsupported(
                    "claude stream conversion requires Claude destination protocol",
                ));
            }
            Ok(TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody::try_from(response)?,
            ))
        }
        TransformResponse::GenerateContentGemini(response) => {
            let stream = GeminiStreamGenerateContentResponse::try_from(response)?;
            match dst_protocol {
                ProtocolKind::Gemini => Ok(TransformResponse::StreamGenerateContentGeminiSse(
                    ensure_gemini_sse_stream(stream),
                )),
                ProtocolKind::GeminiNDJson => {
                    Ok(TransformResponse::StreamGenerateContentGeminiNdjson(
                        ensure_gemini_ndjson_stream(stream),
                    ))
                }
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream conversion requires Gemini/GeminiNDJson destination protocol",
                )),
            }
        }
        _ => Err(MiddlewareTransformError::Unsupported(
            "stream response promotion requires generate_content destination payload",
        )),
    }
}

fn transform_stream_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::StreamGenerateContentOpenAiResponse(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::StreamGenerateContentOpenAiResponse(response)
            }
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(response)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody::try_from(response)?,
            ),
            ProtocolKind::Gemini => TransformResponse::StreamGenerateContentGeminiSse(
                GeminiStreamGenerateContentResponse::SseSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiSseStreamBody::try_from(response)?,
                },
            ),
            ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                GeminiStreamGenerateContentResponse::NdjsonSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiNdjsonStreamBody::try_from(response)?,
                },
            ),
        },
        TransformResponse::StreamGenerateContentOpenAiChatCompletions(response) => {
            match dst_protocol {
                ProtocolKind::OpenAi => TransformResponse::StreamGenerateContentOpenAiResponse(
                    OpenAiCreateResponseSseStreamBody::try_from(response)?,
                ),
                ProtocolKind::OpenAiChatCompletion => {
                    TransformResponse::StreamGenerateContentOpenAiChatCompletions(response)
                }
                ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(
                    ClaudeCreateMessageSseStreamBody::try_from(response)?,
                ),
                ProtocolKind::Gemini => TransformResponse::StreamGenerateContentGeminiSse(
                    GeminiStreamGenerateContentResponse::SseSuccess {
                        stats_code: StatusCode::OK,
                        headers: Default::default(),
                        body: GeminiSseStreamBody::try_from(response)?,
                    },
                ),
                ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                    GeminiStreamGenerateContentResponse::NdjsonSuccess {
                        stats_code: StatusCode::OK,
                        headers: Default::default(),
                        body: GeminiNdjsonStreamBody::try_from(response)?,
                    },
                ),
            }
        }
        TransformResponse::StreamGenerateContentClaude(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody::try_from(response)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(response)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(response),
            ProtocolKind::Gemini => TransformResponse::StreamGenerateContentGeminiSse(
                GeminiStreamGenerateContentResponse::SseSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiSseStreamBody::try_from(response)?,
                },
            ),
            ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                GeminiStreamGenerateContentResponse::NdjsonSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiNdjsonStreamBody::try_from(response)?,
                },
            ),
        },
        TransformResponse::StreamGenerateContentGeminiSse(stream) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody::try_from(stream)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(stream)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody::try_from(stream)?,
            ),
            ProtocolKind::Gemini => {
                TransformResponse::StreamGenerateContentGeminiSse(ensure_gemini_sse_stream(stream))
            }
            ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                ensure_gemini_ndjson_stream(stream),
            ),
        },
        TransformResponse::StreamGenerateContentGeminiNdjson(stream) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody::try_from(stream)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(stream)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody::try_from(stream)?,
            ),
            ProtocolKind::Gemini => {
                TransformResponse::StreamGenerateContentGeminiSse(ensure_gemini_sse_stream(stream))
            }
            ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                ensure_gemini_ndjson_stream(stream),
            ),
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "stream response transform requires stream_generate_content destination payload",
            ));
        }
    })
}

fn ensure_gemini_sse_stream(
    stream: GeminiStreamGenerateContentResponse,
) -> GeminiStreamGenerateContentResponse {
    match stream {
        GeminiStreamGenerateContentResponse::SseSuccess { .. }
        | GeminiStreamGenerateContentResponse::Error { .. } => stream,
        GeminiStreamGenerateContentResponse::NdjsonSuccess {
            stats_code,
            headers,
            body,
        } => GeminiStreamGenerateContentResponse::SseSuccess {
            stats_code,
            headers,
            body: GeminiSseStreamBody {
                events: body
                    .chunks
                    .into_iter()
                    .map(|chunk| GeminiSseEvent {
                        event: None,
                        data: GeminiSseEventData::Chunk(chunk),
                    })
                    .chain(std::iter::once(GeminiSseEvent {
                        event: None,
                        data: GeminiSseEventData::Done("[DONE]".to_string()),
                    }))
                    .collect(),
            },
        },
    }
}

fn ensure_gemini_ndjson_stream(
    stream: GeminiStreamGenerateContentResponse,
) -> GeminiStreamGenerateContentResponse {
    match stream {
        GeminiStreamGenerateContentResponse::NdjsonSuccess { .. }
        | GeminiStreamGenerateContentResponse::Error { .. } => stream,
        GeminiStreamGenerateContentResponse::SseSuccess {
            stats_code,
            headers,
            body,
        } => GeminiStreamGenerateContentResponse::NdjsonSuccess {
            stats_code,
            headers,
            body: GeminiNdjsonStreamBody {
                chunks: body
                    .events
                    .into_iter()
                    .filter_map(|event| match event.data {
                        GeminiSseEventData::Chunk(chunk) => Some(chunk),
                        GeminiSseEventData::Done(_) => None,
                    })
                    .collect(),
            },
        },
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
    use gproxy_protocol::gemini::generate_content::response::ResponseBody as GeminiGenerateContentResponseBody;
    use gproxy_protocol::openai::create_response::types as rt;
    use gproxy_protocol::transform::openai::stream_generate_content::openai_response::utils::response_snapshot;

    #[test]
    fn encode_gemini_sse_event_filters_done_marker() {
        let encoded = encode_gemini_sse_event(GeminiSseEvent {
            event: None,
            data: GeminiSseEventData::Done("[DONE]".to_string()),
        });
        assert!(encoded.is_none());
    }

    #[test]
    fn encode_gemini_sse_event_keeps_json_chunk() {
        let encoded = encode_gemini_sse_event(GeminiSseEvent {
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
        let from_openai =
            StreamOutputConverter::new(ProtocolKind::OpenAi, ProtocolKind::OpenAiChatCompletion)
                .expect("openai -> chat converter");
        assert!(matches!(
            from_openai,
            StreamOutputConverter::OpenAiChat(OpenAiChatStreamConverter::FromOpenAiResponse(_))
        ));

        let from_claude =
            StreamOutputConverter::new(ProtocolKind::Claude, ProtocolKind::OpenAiChatCompletion)
                .expect("claude -> chat converter");
        assert!(matches!(
            from_claude,
            StreamOutputConverter::OpenAiChat(OpenAiChatStreamConverter::FromClaude(_))
        ));

        let from_gemini =
            StreamOutputConverter::new(ProtocolKind::Gemini, ProtocolKind::OpenAiChatCompletion)
                .expect("gemini -> chat converter");
        assert!(matches!(
            from_gemini,
            StreamOutputConverter::OpenAiChat(OpenAiChatStreamConverter::FromGemini(_))
        ));
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
}
