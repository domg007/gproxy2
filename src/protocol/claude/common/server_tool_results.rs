use serde::{Deserialize, Serialize};

use super::{GenericServerToolResultBlock, JsonObject, TypedObject, WebSearchResultBlock};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WebSearchToolResultContent {
    Error(ServerToolResultError),
    Results(Vec<WebSearchResultBlock>),
    Result(TypedObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerToolResultContent {
    Error(ServerToolResultError),
    Result(TypedObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerToolResultError {
    pub error_code: ServerToolResultErrorCode,
    #[serde(rename = "type")]
    pub type_: ServerToolResultErrorType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerToolResultErrorCode {
    Known(ServerToolResultErrorCodeKnown),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerToolResultErrorCodeKnown {
    #[serde(rename = "invalid_tool_input")]
    InvalidToolInput,
    #[serde(rename = "unavailable")]
    Unavailable,
    #[serde(rename = "max_uses_exceeded")]
    MaxUsesExceeded,
    #[serde(rename = "too_many_requests")]
    TooManyRequests,
    #[serde(rename = "query_too_long")]
    QueryTooLong,
    #[serde(rename = "request_too_large")]
    RequestTooLarge,
    #[serde(rename = "url_too_long")]
    UrlTooLong,
    #[serde(rename = "url_not_allowed")]
    UrlNotAllowed,
    #[serde(rename = "url_not_in_prior_context")]
    UrlNotInPriorContext,
    #[serde(rename = "url_not_accessible")]
    UrlNotAccessible,
    #[serde(rename = "unsupported_content_type")]
    UnsupportedContentType,
    #[serde(rename = "execution_time_exceeded")]
    ExecutionTimeExceeded,
    #[serde(rename = "output_file_too_large")]
    OutputFileTooLarge,
    #[serde(rename = "file_not_found")]
    FileNotFound,
    #[serde(rename = "prompt_too_long")]
    PromptTooLong,
    #[serde(rename = "overloaded")]
    Overloaded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerToolResultErrorType {
    Known(ServerToolResultErrorTypeKnown),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerToolResultErrorTypeKnown {
    #[serde(rename = "web_search_tool_result_error")]
    WebSearchToolResultError,
    #[serde(rename = "web_fetch_tool_result_error")]
    WebFetchToolResultError,
    #[serde(rename = "advisor_tool_result_error")]
    AdvisorToolResultError,
    #[serde(rename = "code_execution_tool_result_error")]
    CodeExecutionToolResultError,
    #[serde(rename = "bash_code_execution_tool_result_error")]
    BashCodeExecutionToolResultError,
    #[serde(rename = "text_editor_code_execution_tool_result_error")]
    TextEditorCodeExecutionToolResultError,
    #[serde(rename = "tool_search_tool_result_error")]
    ToolSearchToolResultError,
}

macro_rules! server_tool_result_block {
    ($block:ident, $tag:ident, $wire:literal, $variant:ident) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $block {
            #[serde(flatten)]
            pub result: GenericServerToolResultBlock,
            #[serde(rename = "type")]
            pub type_: $tag,
        }

        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $tag {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

server_tool_result_block!(
    WebFetchToolResultBlock,
    WebFetchToolResultBlockType,
    "web_fetch_tool_result",
    WebFetchToolResult
);
server_tool_result_block!(
    AdvisorToolResultBlock,
    AdvisorToolResultBlockType,
    "advisor_tool_result",
    AdvisorToolResult
);
server_tool_result_block!(
    CodeExecutionToolResultBlock,
    CodeExecutionToolResultBlockType,
    "code_execution_tool_result",
    CodeExecutionToolResult
);
server_tool_result_block!(
    BashCodeExecutionToolResultBlock,
    BashCodeExecutionToolResultBlockType,
    "bash_code_execution_tool_result",
    BashCodeExecutionToolResult
);
server_tool_result_block!(
    TextEditorCodeExecutionToolResultBlock,
    TextEditorCodeExecutionToolResultBlockType,
    "text_editor_code_execution_tool_result",
    TextEditorCodeExecutionToolResult
);
server_tool_result_block!(
    ToolSearchToolResultBlock,
    ToolSearchToolResultBlockType,
    "tool_search_tool_result",
    ToolSearchToolResult
);
