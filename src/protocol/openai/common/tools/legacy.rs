use serde::{Deserialize, Serialize};

use super::super::{Extra, ToolType};
use super::definitions::NamedTool;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LegacyFunctionCallChoice {
    Mode(LegacyFunctionCallMode),
    Named(NamedToolChoice),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegacyFunctionCallMode {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "auto")]
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedToolChoice {
    #[serde(rename = "type")]
    pub type_: ToolType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<NamedTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<NamedTool>,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}
