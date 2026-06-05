use serde::{Deserialize, Serialize};

use super::super::{AllowedToolsMode, Extra, ToolChoiceMode};
use super::definitions::NamedTool;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatToolChoice {
    Mode(ToolChoiceMode),
    Allowed(ChatAllowedToolChoice),
    Named(ChatNamedToolChoice),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseToolChoice {
    Mode(ToolChoiceMode),
    Allowed(ResponseAllowedToolChoice),
    Hosted(ResponseHostedToolChoice),
    Function(ResponseFunctionToolChoice),
    Mcp(ResponseMcpToolChoice),
    Custom(ResponseCustomToolChoice),
    ApplyPatch(ResponseApplyPatchToolChoice),
    Shell(ResponseShellToolChoice),
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
pub struct ResponseHostedToolChoice {
    #[serde(rename = "type")]
    pub type_: ResponseHostedToolChoiceType,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseHostedToolChoiceType {
    #[serde(rename = "file_search")]
    FileSearch,
    #[serde(rename = "web_search_preview")]
    WebSearchPreview,
    #[serde(rename = "computer")]
    Computer,
    #[serde(rename = "computer_use_preview")]
    ComputerUsePreview,
    #[serde(rename = "computer_use")]
    ComputerUse,
    #[serde(rename = "web_search_preview_2025_03_11")]
    WebSearchPreview20250311,
    #[serde(rename = "image_generation")]
    ImageGeneration,
    #[serde(rename = "code_interpreter")]
    CodeInterpreter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseFunctionToolChoice {
    #[serde(rename = "type")]
    pub type_: FunctionToolChoiceType,
    pub name: String,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseMcpToolChoice {
    #[serde(rename = "type")]
    pub type_: McpToolChoiceType,
    pub server_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseCustomToolChoice {
    #[serde(rename = "type")]
    pub type_: CustomToolChoiceType,
    pub name: String,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseApplyPatchToolChoice {
    #[serde(rename = "type")]
    pub type_: ApplyPatchToolChoiceType,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseShellToolChoice {
    #[serde(rename = "type")]
    pub type_: ShellToolChoiceType,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpToolChoiceType {
    #[serde(rename = "mcp")]
    Mcp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplyPatchToolChoiceType {
    #[serde(rename = "apply_patch")]
    ApplyPatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellToolChoiceType {
    #[serde(rename = "shell")]
    Shell,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn chat_tool_choice_only_accepts_chat_shapes() {
        let function_choice: ChatToolChoice = serde_json::from_value(json!({
            "type": "function",
            "function": { "name": "get_weather" }
        }))
        .expect("chat named function tool choice should deserialize");
        assert!(matches!(
            function_choice,
            ChatToolChoice::Named(ChatNamedToolChoice::Function { .. })
        ));

        let custom_choice: ChatToolChoice = serde_json::from_value(json!({
            "type": "custom",
            "custom": { "name": "raw_tool" }
        }))
        .expect("chat named custom tool choice should deserialize");
        assert!(matches!(
            custom_choice,
            ChatToolChoice::Named(ChatNamedToolChoice::Custom { .. })
        ));

        assert!(
            serde_json::from_value::<ChatToolChoice>(json!({
                "type": "file_search"
            }))
            .is_err()
        );
    }

    #[test]
    fn response_tool_choice_only_accepts_response_shapes() {
        let hosted_choice: ResponseToolChoice = serde_json::from_value(json!({
            "type": "file_search"
        }))
        .expect("response hosted tool choice should deserialize");
        assert!(matches!(
            hosted_choice,
            ResponseToolChoice::Hosted(ResponseHostedToolChoice {
                type_: ResponseHostedToolChoiceType::FileSearch,
                ..
            })
        ));

        let function_choice: ResponseToolChoice = serde_json::from_value(json!({
            "type": "function",
            "name": "get_weather"
        }))
        .expect("response function tool choice should deserialize");
        assert!(matches!(
            function_choice,
            ResponseToolChoice::Function(ResponseFunctionToolChoice { .. })
        ));

        assert!(
            serde_json::from_value::<ResponseToolChoice>(json!({
                "type": "function",
                "function": { "name": "get_weather" }
            }))
            .is_err()
        );
    }
}
