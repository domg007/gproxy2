use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{CacheControl, JsonObject, JsonSchemaObjectType};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ToolCommon {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_callers: Option<Vec<ToolCaller>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_loading: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_examples: Vec<JsonObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonSchema {
    #[serde(rename = "type")]
    pub type_: JsonSchemaObjectType,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: JsonObject,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CustomToolType {
    #[serde(rename = "custom")]
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCaller {
    #[serde(rename = "direct")]
    Direct,
    #[serde(rename = "code_execution_20250825")]
    CodeExecution20250825,
    #[serde(rename = "code_execution_20260120")]
    CodeExecution20260120,
}

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $wire)] $variant,)+
        }
    };
}

string_enum!(CommandToolName {
    Bash => "bash", CodeExecution => "code_execution", Memory => "memory",
    ToolSearchBm25 => "tool_search_tool_bm25", ToolSearchRegex => "tool_search_tool_regex",
});
string_enum!(CommandToolType {
    Bash20241022 => "bash_20241022", Bash20250124 => "bash_20250124",
    CodeExecution20250522 => "code_execution_20250522",
    CodeExecution20250825 => "code_execution_20250825",
    CodeExecution20260120 => "code_execution_20260120",
    Memory20250818 => "memory_20250818",
    ToolSearchBm2520251119 => "tool_search_tool_bm25_20251119",
    ToolSearchBm25 => "tool_search_tool_bm25",
    ToolSearchRegex20251119 => "tool_search_tool_regex_20251119",
    ToolSearchRegex => "tool_search_tool_regex",
});
string_enum!(TextEditorToolName {
    StrReplaceEditor => "str_replace_editor",
    StrReplaceBasedEditTool => "str_replace_based_edit_tool",
});
string_enum!(TextEditorToolType {
    TextEditor20241022 => "text_editor_20241022",
    TextEditor20250124 => "text_editor_20250124",
    TextEditor20250429 => "text_editor_20250429",
    TextEditor20250728 => "text_editor_20250728",
});
string_enum!(ComputerToolName { Computer => "computer" });
string_enum!(ComputerToolType {
    Computer20241022 => "computer_20241022",
    Computer20250124 => "computer_20250124",
    Computer20251124 => "computer_20251124",
});
string_enum!(WebSearchToolName { WebSearch => "web_search" });
string_enum!(WebSearchToolType {
    WebSearch20250305 => "web_search_20250305",
    WebSearch20260209 => "web_search_20260209",
});
string_enum!(WebFetchToolName { WebFetch => "web_fetch" });
string_enum!(WebFetchToolType {
    WebFetch20250910 => "web_fetch_20250910",
    WebFetch20260209 => "web_fetch_20260209",
    WebFetch20260309 => "web_fetch_20260309",
});
string_enum!(AdvisorToolName { Advisor => "advisor" });
string_enum!(AdvisorToolType { Advisor20260301 => "advisor_20260301" });
