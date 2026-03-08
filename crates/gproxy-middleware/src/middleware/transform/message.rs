use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;
use futures_util::stream;
use gproxy_protocol::claude::count_tokens::request::ClaudeCountTokensRequest;
use gproxy_protocol::claude::count_tokens::response::ClaudeCountTokensResponse;
use gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest;
use gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse;
use gproxy_protocol::claude::create_message::stream::ClaudeCreateMessageSseStreamBody;
use gproxy_protocol::claude::model_get::request::ClaudeModelGetRequest;
use gproxy_protocol::claude::model_get::response::ClaudeModelGetResponse;
use gproxy_protocol::claude::model_list::request::ClaudeModelListRequest;
use gproxy_protocol::claude::model_list::response::ClaudeModelListResponse;
use gproxy_protocol::gemini::count_tokens::request::GeminiCountTokensRequest;
use gproxy_protocol::gemini::count_tokens::response::GeminiCountTokensResponse;
use gproxy_protocol::gemini::embeddings::request::GeminiEmbedContentRequest;
use gproxy_protocol::gemini::embeddings::response::GeminiEmbedContentResponse;
use gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest;
use gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse;
use gproxy_protocol::gemini::live::request::GeminiLiveConnectRequest;
use gproxy_protocol::gemini::live::response::GeminiLiveMessageResponse;
use gproxy_protocol::gemini::model_get::request::GeminiModelGetRequest;
use gproxy_protocol::gemini::model_get::response::GeminiModelGetResponse;
use gproxy_protocol::gemini::model_list::request::GeminiModelListRequest;
use gproxy_protocol::gemini::model_list::response::GeminiModelListResponse;
use gproxy_protocol::gemini::stream_generate_content::request::GeminiStreamGenerateContentRequest;
use gproxy_protocol::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
use gproxy_protocol::openai::compact_response::request::OpenAiCompactRequest;
use gproxy_protocol::openai::compact_response::response::OpenAiCompactResponse;
use gproxy_protocol::openai::count_tokens::request::OpenAiCountTokensRequest;
use gproxy_protocol::openai::count_tokens::response::OpenAiCountTokensResponse;
use gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest;
use gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
use gproxy_protocol::openai::create_chat_completions::stream::OpenAiChatCompletionsSseStreamBody;
use gproxy_protocol::openai::create_image::request::OpenAiCreateImageRequest;
use gproxy_protocol::openai::create_image::response::OpenAiCreateImageResponse;
use gproxy_protocol::openai::create_image::stream::OpenAiCreateImageSseStreamBody;
use gproxy_protocol::openai::create_image_edit::request::OpenAiCreateImageEditRequest;
use gproxy_protocol::openai::create_image_edit::response::OpenAiCreateImageEditResponse;
use gproxy_protocol::openai::create_image_edit::stream::OpenAiCreateImageEditSseStreamBody;
use gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest;
use gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse;
use gproxy_protocol::openai::create_response::stream::OpenAiCreateResponseSseStreamBody;
use gproxy_protocol::openai::create_response::websocket::request::OpenAiCreateResponseWebSocketConnectRequest;
use gproxy_protocol::openai::create_response::websocket::response::OpenAiCreateResponseWebSocketMessageResponse;
use gproxy_protocol::openai::embeddings::request::OpenAiEmbeddingsRequest;
use gproxy_protocol::openai::embeddings::response::OpenAiEmbeddingsResponse;
use gproxy_protocol::openai::model_get::request::OpenAiModelGetRequest;
use gproxy_protocol::openai::model_get::response::OpenAiModelGetResponse;
use gproxy_protocol::openai::model_list::request::OpenAiModelListRequest;
use gproxy_protocol::openai::model_list::response::OpenAiModelListResponse;
use serde::{Deserialize, Serialize};

use super::error::MiddlewareTransformError;
use super::kinds::{OperationFamily, ProtocolKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransformRoute {
    pub src_operation: OperationFamily,
    pub src_protocol: ProtocolKind,
    pub dst_operation: OperationFamily,
    pub dst_protocol: ProtocolKind,
}

impl TransformRoute {
    pub fn is_passthrough(self) -> bool {
        self.src_operation == self.dst_operation && self.src_protocol == self.dst_protocol
    }
}

pub type TransformBodyStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, MiddlewareTransformError>> + Send + 'static>>;

pub struct TransformRequestPayload {
    pub operation: OperationFamily,
    pub protocol: ProtocolKind,
    pub body: TransformBodyStream,
}

impl TransformRequestPayload {
    pub fn new(
        operation: OperationFamily,
        protocol: ProtocolKind,
        body: TransformBodyStream,
    ) -> Self {
        Self {
            operation,
            protocol,
            body,
        }
    }

    pub fn from_bytes(operation: OperationFamily, protocol: ProtocolKind, body: Bytes) -> Self {
        Self {
            operation,
            protocol,
            body: Box::pin(stream::once(async move { Ok(body) })),
        }
    }
}

pub struct TransformResponsePayload {
    pub operation: OperationFamily,
    pub protocol: ProtocolKind,
    pub body: TransformBodyStream,
}

impl TransformResponsePayload {
    pub fn new(
        operation: OperationFamily,
        protocol: ProtocolKind,
        body: TransformBodyStream,
    ) -> Self {
        Self {
            operation,
            protocol,
            body,
        }
    }

    pub fn from_bytes(operation: OperationFamily, protocol: ProtocolKind, body: Bytes) -> Self {
        Self {
            operation,
            protocol,
            body: Box::pin(stream::once(async move { Ok(body) })),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TransformRequest {
    ModelListOpenAi(OpenAiModelListRequest),
    ModelListClaude(ClaudeModelListRequest),
    ModelListGemini(GeminiModelListRequest),

    ModelGetOpenAi(OpenAiModelGetRequest),
    ModelGetClaude(ClaudeModelGetRequest),
    ModelGetGemini(GeminiModelGetRequest),

    CountTokenOpenAi(OpenAiCountTokensRequest),
    CountTokenClaude(ClaudeCountTokensRequest),
    CountTokenGemini(GeminiCountTokensRequest),

    GenerateContentOpenAiResponse(OpenAiCreateResponseRequest),
    GenerateContentOpenAiChatCompletions(OpenAiChatCompletionsRequest),
    GenerateContentClaude(ClaudeCreateMessageRequest),
    GenerateContentGemini(GeminiGenerateContentRequest),

    StreamGenerateContentOpenAiResponse(OpenAiCreateResponseRequest),
    StreamGenerateContentOpenAiChatCompletions(OpenAiChatCompletionsRequest),
    StreamGenerateContentClaude(ClaudeCreateMessageRequest),
    StreamGenerateContentGeminiSse(GeminiStreamGenerateContentRequest),
    StreamGenerateContentGeminiNdjson(GeminiStreamGenerateContentRequest),

    CreateImageOpenAi(OpenAiCreateImageRequest),
    StreamCreateImageOpenAi(OpenAiCreateImageRequest),
    CreateImageEditOpenAi(OpenAiCreateImageEditRequest),
    StreamCreateImageEditOpenAi(OpenAiCreateImageEditRequest),

    OpenAiResponseWebSocket(OpenAiCreateResponseWebSocketConnectRequest),
    GeminiLive(GeminiLiveConnectRequest),

    EmbeddingOpenAi(OpenAiEmbeddingsRequest),
    EmbeddingGemini(GeminiEmbedContentRequest),

    CompactOpenAi(OpenAiCompactRequest),
}

impl TransformRequest {
    pub const fn operation(&self) -> OperationFamily {
        match self {
            Self::ModelListOpenAi(_) | Self::ModelListClaude(_) | Self::ModelListGemini(_) => {
                OperationFamily::ModelList
            }
            Self::ModelGetOpenAi(_) | Self::ModelGetClaude(_) | Self::ModelGetGemini(_) => {
                OperationFamily::ModelGet
            }
            Self::CountTokenOpenAi(_) | Self::CountTokenClaude(_) | Self::CountTokenGemini(_) => {
                OperationFamily::CountToken
            }
            Self::GenerateContentOpenAiResponse(_)
            | Self::GenerateContentOpenAiChatCompletions(_)
            | Self::GenerateContentClaude(_)
            | Self::GenerateContentGemini(_) => OperationFamily::GenerateContent,
            Self::StreamGenerateContentOpenAiResponse(_)
            | Self::StreamGenerateContentOpenAiChatCompletions(_)
            | Self::StreamGenerateContentClaude(_)
            | Self::StreamGenerateContentGeminiSse(_)
            | Self::StreamGenerateContentGeminiNdjson(_) => OperationFamily::StreamGenerateContent,
            Self::CreateImageOpenAi(_) => OperationFamily::CreateImage,
            Self::StreamCreateImageOpenAi(_) => OperationFamily::StreamCreateImage,
            Self::CreateImageEditOpenAi(_) => OperationFamily::CreateImageEdit,
            Self::StreamCreateImageEditOpenAi(_) => OperationFamily::StreamCreateImageEdit,
            Self::OpenAiResponseWebSocket(_) => OperationFamily::OpenAiResponseWebSocket,
            Self::GeminiLive(_) => OperationFamily::GeminiLive,
            Self::EmbeddingOpenAi(_) | Self::EmbeddingGemini(_) => OperationFamily::Embedding,
            Self::CompactOpenAi(_) => OperationFamily::Compact,
        }
    }

    pub const fn protocol(&self) -> ProtocolKind {
        match self {
            Self::ModelListOpenAi(_) => ProtocolKind::OpenAi,
            Self::ModelListClaude(_) => ProtocolKind::Claude,
            Self::ModelListGemini(_) => ProtocolKind::Gemini,

            Self::ModelGetOpenAi(_) => ProtocolKind::OpenAi,
            Self::ModelGetClaude(_) => ProtocolKind::Claude,
            Self::ModelGetGemini(_) => ProtocolKind::Gemini,

            Self::CountTokenOpenAi(_) => ProtocolKind::OpenAi,
            Self::CountTokenClaude(_) => ProtocolKind::Claude,
            Self::CountTokenGemini(_) => ProtocolKind::Gemini,

            Self::GenerateContentOpenAiResponse(_) => ProtocolKind::OpenAi,
            Self::GenerateContentOpenAiChatCompletions(_) => ProtocolKind::OpenAiChatCompletion,
            Self::GenerateContentClaude(_) => ProtocolKind::Claude,
            Self::GenerateContentGemini(_) => ProtocolKind::Gemini,

            Self::StreamGenerateContentOpenAiResponse(_) => ProtocolKind::OpenAi,
            Self::StreamGenerateContentOpenAiChatCompletions(_) => {
                ProtocolKind::OpenAiChatCompletion
            }
            Self::StreamGenerateContentClaude(_) => ProtocolKind::Claude,
            Self::StreamGenerateContentGeminiSse(_) => ProtocolKind::Gemini,
            Self::StreamGenerateContentGeminiNdjson(_) => ProtocolKind::GeminiNDJson,

            Self::CreateImageOpenAi(_) => ProtocolKind::OpenAi,
            Self::StreamCreateImageOpenAi(_) => ProtocolKind::OpenAi,
            Self::CreateImageEditOpenAi(_) => ProtocolKind::OpenAi,
            Self::StreamCreateImageEditOpenAi(_) => ProtocolKind::OpenAi,

            Self::OpenAiResponseWebSocket(_) => ProtocolKind::OpenAi,
            Self::GeminiLive(_) => ProtocolKind::Gemini,

            Self::EmbeddingOpenAi(_) => ProtocolKind::OpenAi,
            Self::EmbeddingGemini(_) => ProtocolKind::Gemini,

            Self::CompactOpenAi(_) => ProtocolKind::OpenAi,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum TransformResponse {
    ModelListOpenAi(OpenAiModelListResponse),
    ModelListClaude(ClaudeModelListResponse),
    ModelListGemini(GeminiModelListResponse),

    ModelGetOpenAi(OpenAiModelGetResponse),
    ModelGetClaude(ClaudeModelGetResponse),
    ModelGetGemini(GeminiModelGetResponse),

    CountTokenOpenAi(OpenAiCountTokensResponse),
    CountTokenClaude(ClaudeCountTokensResponse),
    CountTokenGemini(GeminiCountTokensResponse),

    GenerateContentOpenAiResponse(OpenAiCreateResponseResponse),
    GenerateContentOpenAiChatCompletions(OpenAiChatCompletionsResponse),
    GenerateContentClaude(ClaudeCreateMessageResponse),
    GenerateContentGemini(GeminiGenerateContentResponse),

    StreamGenerateContentOpenAiResponse(OpenAiCreateResponseSseStreamBody),
    StreamGenerateContentOpenAiChatCompletions(OpenAiChatCompletionsSseStreamBody),
    StreamGenerateContentClaude(ClaudeCreateMessageSseStreamBody),
    StreamGenerateContentGeminiSse(GeminiStreamGenerateContentResponse),
    StreamGenerateContentGeminiNdjson(GeminiStreamGenerateContentResponse),

    CreateImageOpenAi(OpenAiCreateImageResponse),
    StreamCreateImageOpenAi(OpenAiCreateImageSseStreamBody),
    CreateImageEditOpenAi(OpenAiCreateImageEditResponse),
    StreamCreateImageEditOpenAi(OpenAiCreateImageEditSseStreamBody),

    OpenAiResponseWebSocket(Vec<OpenAiCreateResponseWebSocketMessageResponse>),
    GeminiLive(Vec<GeminiLiveMessageResponse>),

    EmbeddingOpenAi(OpenAiEmbeddingsResponse),
    EmbeddingGemini(GeminiEmbedContentResponse),

    CompactOpenAi(OpenAiCompactResponse),
}

impl TransformResponse {
    pub const fn operation(&self) -> OperationFamily {
        match self {
            Self::ModelListOpenAi(_) | Self::ModelListClaude(_) | Self::ModelListGemini(_) => {
                OperationFamily::ModelList
            }
            Self::ModelGetOpenAi(_) | Self::ModelGetClaude(_) | Self::ModelGetGemini(_) => {
                OperationFamily::ModelGet
            }
            Self::CountTokenOpenAi(_) | Self::CountTokenClaude(_) | Self::CountTokenGemini(_) => {
                OperationFamily::CountToken
            }
            Self::GenerateContentOpenAiResponse(_)
            | Self::GenerateContentOpenAiChatCompletions(_)
            | Self::GenerateContentClaude(_)
            | Self::GenerateContentGemini(_) => OperationFamily::GenerateContent,
            Self::StreamGenerateContentOpenAiResponse(_)
            | Self::StreamGenerateContentOpenAiChatCompletions(_)
            | Self::StreamGenerateContentClaude(_)
            | Self::StreamGenerateContentGeminiSse(_)
            | Self::StreamGenerateContentGeminiNdjson(_) => OperationFamily::StreamGenerateContent,
            Self::CreateImageOpenAi(_) => OperationFamily::CreateImage,
            Self::StreamCreateImageOpenAi(_) => OperationFamily::StreamCreateImage,
            Self::CreateImageEditOpenAi(_) => OperationFamily::CreateImageEdit,
            Self::StreamCreateImageEditOpenAi(_) => OperationFamily::StreamCreateImageEdit,
            Self::OpenAiResponseWebSocket(_) => OperationFamily::OpenAiResponseWebSocket,
            Self::GeminiLive(_) => OperationFamily::GeminiLive,
            Self::EmbeddingOpenAi(_) | Self::EmbeddingGemini(_) => OperationFamily::Embedding,
            Self::CompactOpenAi(_) => OperationFamily::Compact,
        }
    }

    pub const fn protocol(&self) -> ProtocolKind {
        match self {
            Self::ModelListOpenAi(_) => ProtocolKind::OpenAi,
            Self::ModelListClaude(_) => ProtocolKind::Claude,
            Self::ModelListGemini(_) => ProtocolKind::Gemini,

            Self::ModelGetOpenAi(_) => ProtocolKind::OpenAi,
            Self::ModelGetClaude(_) => ProtocolKind::Claude,
            Self::ModelGetGemini(_) => ProtocolKind::Gemini,

            Self::CountTokenOpenAi(_) => ProtocolKind::OpenAi,
            Self::CountTokenClaude(_) => ProtocolKind::Claude,
            Self::CountTokenGemini(_) => ProtocolKind::Gemini,

            Self::GenerateContentOpenAiResponse(_) => ProtocolKind::OpenAi,
            Self::GenerateContentOpenAiChatCompletions(_) => ProtocolKind::OpenAiChatCompletion,
            Self::GenerateContentClaude(_) => ProtocolKind::Claude,
            Self::GenerateContentGemini(_) => ProtocolKind::Gemini,

            Self::StreamGenerateContentOpenAiResponse(_) => ProtocolKind::OpenAi,
            Self::StreamGenerateContentOpenAiChatCompletions(_) => {
                ProtocolKind::OpenAiChatCompletion
            }
            Self::StreamGenerateContentClaude(_) => ProtocolKind::Claude,
            Self::StreamGenerateContentGeminiSse(_) => ProtocolKind::Gemini,
            Self::StreamGenerateContentGeminiNdjson(_) => ProtocolKind::GeminiNDJson,

            Self::CreateImageOpenAi(_) => ProtocolKind::OpenAi,
            Self::StreamCreateImageOpenAi(_) => ProtocolKind::OpenAi,
            Self::CreateImageEditOpenAi(_) => ProtocolKind::OpenAi,
            Self::StreamCreateImageEditOpenAi(_) => ProtocolKind::OpenAi,

            Self::OpenAiResponseWebSocket(_) => ProtocolKind::OpenAi,
            Self::GeminiLive(_) => ProtocolKind::Gemini,

            Self::EmbeddingOpenAi(_) => ProtocolKind::OpenAi,
            Self::EmbeddingGemini(_) => ProtocolKind::Gemini,

            Self::CompactOpenAi(_) => ProtocolKind::OpenAi,
        }
    }
}
