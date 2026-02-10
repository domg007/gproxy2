use serde::{Deserialize, Serialize};

use gproxy_protocol::claude::count_tokens::request::CountTokensRequest as ClaudeCountTokensRequest;
use gproxy_protocol::claude::count_tokens::response::CountTokensResponse as ClaudeCountTokensResponse;
use gproxy_protocol::claude::create_message::request::CreateMessageRequest as ClaudeCreateMessageRequest;
use gproxy_protocol::claude::create_message::response::CreateMessageResponse as ClaudeCreateMessageResponse;
use gproxy_protocol::claude::create_message::stream::BetaStreamEvent;
use gproxy_protocol::claude::get_model::request::GetModelRequest as ClaudeGetModelRequest;
use gproxy_protocol::claude::get_model::response::GetModelResponse as ClaudeGetModelResponse;
use gproxy_protocol::claude::list_models::request::ListModelsRequest as ClaudeListModelsRequest;
use gproxy_protocol::claude::list_models::response::ListModelsResponse as ClaudeListModelsResponse;
use gproxy_protocol::gemini::count_tokens::request::CountTokensRequest as GeminiCountTokensRequest;
use gproxy_protocol::gemini::count_tokens::response::CountTokensResponse as GeminiCountTokensResponse;
use gproxy_protocol::gemini::generate_content::request::GenerateContentRequest as GeminiGenerateContentRequest;
use gproxy_protocol::gemini::generate_content::response::GenerateContentResponse as GeminiGenerateContentResponse;
use gproxy_protocol::gemini::get_model::request::GetModelRequest as GeminiGetModelRequest;
use gproxy_protocol::gemini::get_model::response::GetModelResponse as GeminiGetModelResponse;
use gproxy_protocol::gemini::list_models::request::ListModelsRequest as GeminiListModelsRequest;
use gproxy_protocol::gemini::list_models::response::ListModelsResponse as GeminiListModelsResponse;
use gproxy_protocol::gemini::stream_content::request::StreamGenerateContentRequest as GeminiStreamGenerateContentRequest;
use gproxy_protocol::gemini::stream_content::response::StreamGenerateContentResponse;
use gproxy_protocol::openai::cancel_response::request::CancelResponseRequest as OpenAICancelResponseRequest;
use gproxy_protocol::openai::cancel_response::response::CancelResponseResponse as OpenAICancelResponseResponse;
use gproxy_protocol::openai::compact_response::request::CompactResponseRequest as OpenAICompactResponseRequest;
use gproxy_protocol::openai::compact_response::response::CompactResponseResponse as OpenAICompactResponseResponse;
use gproxy_protocol::openai::count_tokens::request::InputTokenCountRequest as OpenAICountTokensRequest;
use gproxy_protocol::openai::count_tokens::response::InputTokenCountResponse as OpenAICountTokensResponse;
use gproxy_protocol::openai::create_chat_completions::request::CreateChatCompletionRequest as OpenAIChatCompletionRequest;
use gproxy_protocol::openai::create_chat_completions::response::CreateChatCompletionResponse as OpenAIChatCompletionResponse;
use gproxy_protocol::openai::create_chat_completions::stream::CreateChatCompletionStreamResponse;
use gproxy_protocol::openai::create_response::request::CreateResponseRequest as OpenAIResponseRequest;
use gproxy_protocol::openai::create_response::response::Response as OpenAIResponse;
use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
use gproxy_protocol::openai::delete_response::request::DeleteResponseRequest as OpenAIDeleteResponseRequest;
use gproxy_protocol::openai::delete_response::response::DeleteResponseResponse as OpenAIDeleteResponseResponse;
use gproxy_protocol::openai::get_model::request::GetModelRequest as OpenAIGetModelRequest;
use gproxy_protocol::openai::get_model::response::GetModelResponse as OpenAIGetModelResponse;
use gproxy_protocol::openai::get_response::request::GetResponseRequest as OpenAIGetResponseRequest;
use gproxy_protocol::openai::get_response::response::GetResponseResponse as OpenAIGetResponseResponse;
use gproxy_protocol::openai::list_input_items::request::ListInputItemsRequest as OpenAIListInputItemsRequest;
use gproxy_protocol::openai::list_input_items::response::ListInputItemsResponse as OpenAIListInputItemsResponse;
use gproxy_protocol::openai::list_models::request::ListModelsRequest as OpenAIListModelsRequest;
use gproxy_protocol::openai::list_models::response::ListModelsResponse as OpenAIListModelsResponse;
use gproxy_protocol::openai::trace_summarize::request::TraceSummarizeRequest as OpenAITraceSummarizeRequest;
use gproxy_protocol::openai::trace_summarize::response::TraceSummarizeResponse as OpenAITraceSummarizeResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Proto {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "openai_chat")]
    OpenAIChat,
    #[serde(rename = "openai_response")]
    OpenAIResponse,
    #[serde(rename = "gemini")]
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    ModelList,
    ModelGet,
    CountTokens,
    GenerateContent,
    StreamGenerateContent,
    ResponseGet,
    ResponseDelete,
    ResponseCancel,
    ResponseListInputItems,
    ResponseCompact,
    MemoryTraceSummarize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransformContext {
    pub src: Proto,
    pub dst: Proto,
    pub src_op: Op,
    pub dst_op: Op,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamFormat {
    SseNamedEvent,
    SseDataOnly,
    JsonStream,
}

pub fn stream_format(proto: Proto) -> Option<StreamFormat> {
    match proto {
        Proto::Claude => Some(StreamFormat::SseNamedEvent),
        Proto::OpenAIChat => Some(StreamFormat::SseDataOnly),
        Proto::OpenAIResponse => Some(StreamFormat::SseNamedEvent),
        Proto::Gemini => Some(StreamFormat::JsonStream),
        Proto::OpenAI => None,
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Request {
    ModelList(ModelListRequest),
    ModelGet(ModelGetRequest),
    CountTokens(CountTokensRequest),
    GenerateContent(GenerateContentRequest),
    ResponseGet(ResponseGetRequest),
    ResponseDelete(ResponseDeleteRequest),
    ResponseCancel(ResponseCancelRequest),
    ResponseListInputItems(ResponseListInputItemsRequest),
    ResponseCompact(ResponseCompactRequest),
    MemoryTraceSummarize(MemoryTraceSummarizeRequest),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Response {
    ModelList(ModelListResponse),
    ModelGet(ModelGetResponse),
    CountTokens(CountTokensResponse),
    GenerateContent(GenerateContentResponse),
    ResponseGet(ResponseGetResponse),
    ResponseDelete(ResponseDeleteResponse),
    ResponseCancel(ResponseCancelResponse),
    ResponseListInputItems(ResponseListInputItemsResponse),
    ResponseCompact(ResponseCompactResponse),
    MemoryTraceSummarize(MemoryTraceSummarizeResponse),
}

#[derive(Debug, Clone)]
pub enum ModelListRequest {
    Claude(ClaudeListModelsRequest),
    OpenAI(OpenAIListModelsRequest),
    Gemini(GeminiListModelsRequest),
}

#[derive(Debug, Clone)]
pub enum ModelListResponse {
    Claude(ClaudeListModelsResponse),
    OpenAI(OpenAIListModelsResponse),
    Gemini(GeminiListModelsResponse),
}

#[derive(Debug, Clone)]
pub enum ModelGetRequest {
    Claude(ClaudeGetModelRequest),
    OpenAI(OpenAIGetModelRequest),
    Gemini(GeminiGetModelRequest),
}

#[derive(Debug, Clone)]
pub enum ModelGetResponse {
    Claude(ClaudeGetModelResponse),
    OpenAI(OpenAIGetModelResponse),
    Gemini(GeminiGetModelResponse),
}

#[derive(Debug, Clone)]
pub enum CountTokensRequest {
    Claude(ClaudeCountTokensRequest),
    OpenAI(OpenAICountTokensRequest),
    Gemini(GeminiCountTokensRequest),
}

#[derive(Debug, Clone)]
pub enum CountTokensResponse {
    Claude(ClaudeCountTokensResponse),
    OpenAI(OpenAICountTokensResponse),
    Gemini(GeminiCountTokensResponse),
}

#[derive(Debug, Clone)]
pub enum GenerateContentRequest {
    Claude(ClaudeCreateMessageRequest),
    OpenAIChat(OpenAIChatCompletionRequest),
    OpenAIResponse(OpenAIResponseRequest),
    Gemini(GeminiGenerateContentRequest),
    GeminiStream(GeminiStreamGenerateContentRequest),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum GenerateContentResponse {
    Claude(ClaudeCreateMessageResponse),
    OpenAIChat(OpenAIChatCompletionResponse),
    OpenAIResponse(OpenAIResponse),
    Gemini(GeminiGenerateContentResponse),
}

#[derive(Debug, Clone)]
pub enum ResponseGetRequest {
    OpenAI(OpenAIGetResponseRequest),
}

#[derive(Debug, Clone)]
pub enum ResponseGetResponse {
    OpenAI(OpenAIGetResponseResponse),
}

#[derive(Debug, Clone)]
pub enum ResponseDeleteRequest {
    OpenAI(OpenAIDeleteResponseRequest),
}

#[derive(Debug, Clone)]
pub enum ResponseDeleteResponse {
    OpenAI(OpenAIDeleteResponseResponse),
}

#[derive(Debug, Clone)]
pub enum ResponseCancelRequest {
    OpenAI(OpenAICancelResponseRequest),
}

#[derive(Debug, Clone)]
pub enum ResponseCancelResponse {
    OpenAI(OpenAICancelResponseResponse),
}

#[derive(Debug, Clone)]
pub enum ResponseListInputItemsRequest {
    OpenAI(OpenAIListInputItemsRequest),
}

#[derive(Debug, Clone)]
pub enum ResponseListInputItemsResponse {
    OpenAI(OpenAIListInputItemsResponse),
}

#[derive(Debug, Clone)]
pub enum ResponseCompactRequest {
    OpenAI(OpenAICompactResponseRequest),
}

#[derive(Debug, Clone)]
pub enum ResponseCompactResponse {
    OpenAI(OpenAICompactResponseResponse),
}

#[derive(Debug, Clone)]
pub enum MemoryTraceSummarizeRequest {
    OpenAI(OpenAITraceSummarizeRequest),
}

#[derive(Debug, Clone)]
pub enum MemoryTraceSummarizeResponse {
    OpenAI(OpenAITraceSummarizeResponse),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Claude(BetaStreamEvent),
    OpenAIChat(CreateChatCompletionStreamResponse),
    OpenAIResponse(ResponseStreamEvent),
    Gemini(StreamGenerateContentResponse),
}

#[derive(Debug, Clone)]
pub enum TransformError {
    OpMismatch,
    ProtoMismatch,
    StreamMismatch,
    UnsupportedPair {
        src: Proto,
        dst: Proto,
        src_op: Op,
        dst_op: Op,
    },
}
