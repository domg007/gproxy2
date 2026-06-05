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
    pub tools: Vec<ChatAllowedTool>,
    #[serde(
        default,
        flatten,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatAllowedTool {
    #[serde(rename = "function")]
    Function {
        function: NamedTool,
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "custom")]
    Custom {
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
pub struct ResponseAllowedToolChoice {
    pub mode: AllowedToolsMode,
    pub tools: Vec<ResponseAllowedTool>,
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
#[serde(tag = "type")]
pub enum ResponseAllowedTool {
    #[serde(rename = "function")]
    Function {
        name: String,
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "custom")]
    Custom {
        name: String,
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "mcp")]
    Mcp {
        server_label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "file_search")]
    FileSearch {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "web_search_preview")]
    WebSearchPreview {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "computer")]
    Computer {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "computer_use_preview")]
    ComputerUsePreview {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "computer_use")]
    ComputerUse {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "web_search_preview_2025_03_11")]
    WebSearchPreview20250311 {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "image_generation")]
    ImageGeneration {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "code_interpreter")]
    CodeInterpreter {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "local_shell")]
    LocalShell {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "shell")]
    Shell {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
    #[serde(rename = "apply_patch")]
    ApplyPatch {
        #[serde(
            default,
            flatten,
            skip_serializing_if = "std::collections::BTreeMap::is_empty"
        )]
        extra: Extra,
    },
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
    fn chat_allowed_tools_reject_response_only_entries() {
        let allowed_choice: ChatToolChoice = serde_json::from_value(json!({
            "type": "allowed_tools",
            "allowed_tools": {
                "mode": "required",
                "tools": [
                    { "type": "function", "function": { "name": "get_weather" } },
                    { "type": "custom", "custom": { "name": "raw_tool" } }
                ]
            }
        }))
        .expect("chat allowed tools should deserialize");
        let ChatToolChoice::Allowed(ChatAllowedToolChoice { allowed_tools, .. }) = allowed_choice
        else {
            panic!("expected chat allowed tools");
        };
        assert_eq!(allowed_tools.tools.len(), 2);

        assert!(
            serde_json::from_value::<ChatToolChoice>(json!({
                "type": "allowed_tools",
                "allowed_tools": {
                    "mode": "auto",
                    "tools": [{ "type": "file_search" }]
                }
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

    #[test]
    fn response_allowed_tools_reject_chat_only_entries() {
        let allowed_choice: ResponseToolChoice = serde_json::from_value(json!({
            "type": "allowed_tools",
            "mode": "auto",
            "tools": [
                { "type": "function", "name": "get_weather" },
                { "type": "mcp", "server_label": "deepwiki" },
                { "type": "image_generation" }
            ]
        }))
        .expect("response allowed tools should deserialize");
        let ResponseToolChoice::Allowed(ResponseAllowedToolChoice { tools, .. }) = allowed_choice
        else {
            panic!("expected response allowed tools");
        };
        assert_eq!(tools.len(), 3);

        assert!(
            serde_json::from_value::<ResponseToolChoice>(json!({
                "type": "allowed_tools",
                "mode": "auto",
                "tools": [{
                    "type": "function",
                    "function": { "name": "get_weather" }
                }]
            }))
            .is_err()
        );
    }
}
