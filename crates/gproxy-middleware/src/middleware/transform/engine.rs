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

mod api;
pub use api::{
    transform_request, transform_request_payload, transform_response, transform_response_payload,
};
mod codec;
pub(crate) use codec::encode_response_payload;
use codec::*;
pub use codec::{decode_request_payload, decode_response_payload};
mod request;
use request::*;
mod response;
use response::*;
mod stream;
use stream::{
    demote_stream_response_to_generate, ensure_gemini_ndjson_stream, ensure_gemini_sse_stream,
    promote_generate_response_to_stream, supports_incremental_stream_response_conversion,
    transform_buffered_stream_response_payload, transform_stream_response,
    transform_stream_response_body,
};

#[cfg(test)]
mod tests;
