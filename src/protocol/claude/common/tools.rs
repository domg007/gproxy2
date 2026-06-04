use serde::{Deserialize, Serialize};

use super::{
    AdvisorToolName, AdvisorToolType, CacheControl, CitationConfig, ClaudeModel, CommandToolName,
    CommandToolType, ComputerToolName, ComputerToolType, CustomToolType, JsonSchema, McpToolset,
    TextEditorToolName, TextEditorToolType, ToolCommon, TypedObject, UserLocation,
    WebFetchToolName, WebFetchToolType, WebSearchToolName, WebSearchToolType,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Tool {
    WebFetch(WebFetchTool),
    WebSearch(WebSearchTool),
    Advisor(AdvisorTool),
    Computer(ComputerTool),
    TextEditor(TextEditorTool),
    Command(CommandTool),
    McpToolset(McpToolset),
    Custom(CustomTool),
    Unknown(TypedObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomTool {
    pub input_schema: JsonSchema,
    pub name: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<CustomToolType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eager_input_streaming: Option<bool>,
    #[serde(flatten)]
    pub common: ToolCommon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandTool {
    pub name: CommandToolName,
    #[serde(rename = "type")]
    pub type_: CommandToolType,
    #[serde(flatten)]
    pub common: ToolCommon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextEditorTool {
    pub name: TextEditorToolName,
    #[serde(rename = "type")]
    pub type_: TextEditorToolType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_characters: Option<u64>,
    #[serde(flatten)]
    pub common: ToolCommon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerTool {
    pub display_height_px: u64,
    pub display_width_px: u64,
    pub name: ComputerToolName,
    #[serde(rename = "type")]
    pub type_: ComputerToolType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_zoom: Option<bool>,
    #[serde(flatten)]
    pub common: ToolCommon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSearchTool {
    pub name: WebSearchToolName,
    #[serde(rename = "type")]
    pub type_: WebSearchToolType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_location: Option<UserLocation>,
    #[serde(flatten)]
    pub common: ToolCommon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebFetchTool {
    pub name: WebFetchToolName,
    #[serde(rename = "type")]
    pub type_: WebFetchToolType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<CitationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_content_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_cache: Option<bool>,
    #[serde(flatten)]
    pub common: ToolCommon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdvisorTool {
    pub model: ClaudeModel,
    pub name: AdvisorToolName,
    #[serde(rename = "type")]
    pub type_: AdvisorToolType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caching: Option<CacheControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<u64>,
    #[serde(flatten)]
    pub common: ToolCommon,
}
