use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::{
    AnthropicBetaHeaders, AssistantRole, CacheControl, ClaudeModel, Container, ContainerParam,
    ContentBlock, ContextManagementConfig, ContextManagementResponse, Diagnostics,
    DiagnosticsParam, InferenceGeo, JsonObject, JsonSchemaFormat, McpServer, MessageObjectType,
    MessageParam, Metadata, OutputConfig, RequestServiceTier, Speed, StopDetails, StopReason,
    StopSequence, SystemPrompt, ThinkingConfig, Tool, ToolChoice, Usage,
};

pub mod stream;

pub use stream::*;

pub type CreateMessageRequestHeaders = AnthropicBetaHeaders;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateMessageRequestBody {
    pub model: ClaudeModel,
    pub messages: Vec<MessageParam>,
    pub max_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<ContextManagementConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<DiagnosticsParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inference_geo: Option<InferenceGeo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<McpServer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<JsonSchemaFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<RequestServiceTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<Speed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<StopSequence>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_profile_id: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateMessageResponseBody {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: MessageObjectType,
    pub role: AssistantRole,
    pub content: Vec<ContentBlock>,
    pub model: ClaudeModel,
    pub stop_reason: StopReason,
    pub stop_sequence: Option<StopSequence>,
    pub usage: Usage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<Container>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<ContextManagementResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Diagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_details: Option<StopDetails>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}
