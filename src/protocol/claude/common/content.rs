use serde::{Deserialize, Serialize};

use super::blocks::*;
use super::misc_blocks::*;
use super::server_tool_results::*;
use super::tool_blocks::*;
use super::{JsonObject, MessageRole, StringOrArray, TypedObject};

pub type MessageContent = StringOrArray<ContentBlockParam>;
pub type SystemPrompt = StringOrArray<TextBlock>;
pub type ContentBlock = ResponseContentBlock;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageParam {
    pub role: MessageRole,
    pub content: MessageContent,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentBlockParam {
    Text(TextBlock),
    Image(ImageBlock),
    Document(DocumentBlock),
    SearchResult(SearchResultBlock),
    Thinking(ThinkingBlock),
    RedactedThinking(RedactedThinkingBlock),
    ToolUse(ToolUseBlock),
    ToolResult(ToolResultBlock),
    ServerToolUse(ServerToolUseBlock),
    WebSearchToolResult(WebSearchToolResultBlock),
    WebFetchToolResult(WebFetchToolResultBlock),
    AdvisorToolResult(AdvisorToolResultBlock),
    CodeExecutionToolResult(CodeExecutionToolResultBlock),
    BashCodeExecutionToolResult(BashCodeExecutionToolResultBlock),
    TextEditorCodeExecutionToolResult(TextEditorCodeExecutionToolResultBlock),
    ToolSearchToolResult(ToolSearchToolResultBlock),
    McpToolUse(McpToolUseBlock),
    McpToolResult(McpToolResultBlock),
    ContainerUpload(ContainerUploadBlock),
    Compaction(CompactionBlock),
    MidConversationSystem(MidConversationSystemBlock),
    Raw(TypedObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseContentBlock {
    Text(TextBlock),
    Thinking(ThinkingBlock),
    RedactedThinking(RedactedThinkingBlock),
    ToolUse(ToolUseBlock),
    ServerToolUse(ServerToolUseBlock),
    WebSearchToolResult(WebSearchToolResultBlock),
    WebFetchToolResult(WebFetchToolResultBlock),
    AdvisorToolResult(AdvisorToolResultBlock),
    CodeExecutionToolResult(CodeExecutionToolResultBlock),
    BashCodeExecutionToolResult(BashCodeExecutionToolResultBlock),
    TextEditorCodeExecutionToolResult(TextEditorCodeExecutionToolResultBlock),
    ToolSearchToolResult(ToolSearchToolResultBlock),
    McpToolUse(McpToolUseBlock),
    McpToolResult(McpToolResultBlock),
    ContainerUpload(ContainerUploadBlock),
    Compaction(CompactionBlock),
    Raw(TypedObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultContentBlock>),
    Raw(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    SearchResult(SearchResultBlock),
    Document(DocumentBlock),
    ToolReference(ToolReferenceBlock),
    Raw(TypedObject),
}

pub type McpToolResultContent = StringOrArray<TextBlock>;
