use serde::{Deserialize, Serialize};

use super::super::{AllowedToolsMode, Extra, ToolChoiceMode, ToolType};
use super::definitions::NamedTool;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    Mode(ToolChoiceMode),
    ChatAllowed(ChatAllowedToolChoice),
    ResponseAllowed(ResponseAllowedToolChoice),
    ChatNamed(ChatNamedToolChoice),
    ResponseNamed(ResponseNamedToolChoice),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatAllowedToolChoice {
    pub allowed_tools: ChatAllowedTools,
    #[serde(rename = "type")]
    pub type_: AllowedToolsType,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatAllowedTools {
    pub mode: AllowedToolsMode,
    pub tools: Vec<Extra>,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseAllowedToolChoice {
    pub mode: AllowedToolsMode,
    pub tools: Vec<Extra>,
    #[serde(rename = "type")]
    pub type_: AllowedToolsType,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AllowedToolsType {
    #[serde(rename = "allowed_tools")]
    AllowedTools,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatNamedToolChoice {
    Function {
        #[serde(rename = "type")]
        type_: FunctionToolChoiceType,
        function: NamedTool,
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    Custom {
        #[serde(rename = "type")]
        type_: CustomToolChoiceType,
        custom: NamedTool,
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseNamedToolChoice {
    #[serde(rename = "type")]
    pub type_: ToolType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_label: Option<String>,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FunctionToolChoiceType {
    #[serde(rename = "function")]
    Function,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CustomToolChoiceType {
    #[serde(rename = "custom")]
    Custom,
}
