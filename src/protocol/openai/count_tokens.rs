use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::{Extra, Metadata, OpenAiModelId, OpenAiWireModel, ToolChoice};
use super::generate_content::{
    ContextManagement, PromptRef, ReasoningConfig, ResponseTool, TextConfig,
};
use super::generate_content::{ResponseConversationRef, ResponseInput};

pub type ResponseInputTokensWireModel =
    OpenAiWireModel<ResponseInputTokensRequest, ResponseInputTokensResponse>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseInputTokensRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<Vec<ContextManagement>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<ResponseConversationRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<ResponseInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<ResponseInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenAiModelId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponseTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseInputTokensResponse {
    pub input_tokens: u32,
    pub object: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}
