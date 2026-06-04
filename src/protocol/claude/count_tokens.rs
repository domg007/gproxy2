use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::{CacheControl, ClaudeModel, JsonObject, JsonSchemaFormat, ThinkingConfig};
use super::content::SystemPrompt;
use super::messages::{ContextManagementConfig, MessageParam, OutputConfig};
use super::tools::{McpServer, Tool, ToolChoice};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CountTokensRequest {
    pub model: ClaudeModel,
    pub messages: Vec<MessageParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<ContextManagementConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<McpServer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<JsonSchemaFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CountTokensResponse {
    pub input_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<CountTokensContextManagement>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CountTokensContextManagement {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_input_tokens: Option<u64>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}
