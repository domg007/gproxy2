use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::common::{
    BoolOrStringArray, CacheControl, ClaudeModel, JsonObject, JsonSchemaFormat, Metadata,
    ThinkingConfig, TypedObject,
};
use super::content::{ContentBlock, MessageContent, SystemPrompt};
use super::tools::{McpServer, Tool, ToolChoice};
use super::usage::Usage;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageParam {
    pub role: String,
    pub content: MessageContent,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    pub model: ClaudeModel,
    pub max_tokens: u64,
    pub messages: Vec<MessageParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<ContextManagementConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<McpServer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<JsonSchemaFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
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
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: ClaudeModel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: Usage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_management: Option<ContextManagementResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_details: Option<TypedObject>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextManagementConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edits: Option<Vec<ContextEdit>>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextEdit {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clear_at_least: Option<TypedObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clear_tool_inputs: Option<BoolOrStringArray>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<TypedObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_after_compaction: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextManagementResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_edits: Option<Vec<TypedObject>>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<JsonSchemaFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_budget: Option<TypedObject>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}
