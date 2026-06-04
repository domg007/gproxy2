use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[path = "blocks/base.rs"]
pub mod blocks;
pub mod cache;
#[path = "tools/caller.rs"]
pub mod caller;
pub mod container;
#[path = "blocks/content.rs"]
pub mod content;
pub mod context_management;
pub mod diagnostics;
#[path = "tools/location.rs"]
pub mod location;
pub mod mcp;
#[path = "blocks/misc.rs"]
pub mod misc_blocks;
pub mod output;
#[path = "blocks/response.rs"]
pub mod response_blocks;
#[path = "server_tools/response_payloads.rs"]
pub mod response_server_tool_payloads;
#[path = "server_tools/response_results.rs"]
pub mod response_server_tool_results;
#[path = "server_tools/errors.rs"]
pub mod server_tool_errors;
#[path = "server_tools/payloads.rs"]
pub mod server_tool_payloads;
#[path = "server_tools/results.rs"]
pub mod server_tool_results;
#[path = "blocks/sources.rs"]
pub mod sources;
pub mod stop;
#[path = "server_tools/text_editor.rs"]
pub mod text_editor_results;
pub mod thinking;
#[path = "blocks/tool.rs"]
pub mod tool_blocks;
#[path = "tools/choice.rs"]
pub mod tool_choice;
#[path = "tools/support.rs"]
pub mod tool_support;
#[path = "tools/defs.rs"]
pub mod tools;
pub mod types;
pub mod usage;

pub use blocks::*;
pub use cache::*;
pub use caller::*;
pub use container::*;
pub use content::*;
pub use context_management::*;
pub use diagnostics::*;
pub use location::*;
pub use mcp::*;
pub use misc_blocks::*;
pub use output::*;
pub use response_blocks::*;
pub use response_server_tool_payloads::*;
pub use response_server_tool_results::*;
pub use server_tool_errors::*;
pub use server_tool_payloads::*;
pub use server_tool_results::*;
pub use sources::*;
pub use stop::*;
pub use text_editor_results::*;
pub use thinking::*;
pub use tool_blocks::*;
pub use tool_choice::*;
pub use tool_support::*;
pub use tools::*;
pub use types::*;
pub use usage::*;

pub type AnthropicBeta = String;
pub type JsonObject = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrArray<T> {
    String(String),
    Array(Vec<T>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoolOrStringArray {
    Bool(bool),
    Array(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedObject {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    #[serde(rename = "type")]
    pub type_: CitationType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cited_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_char_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_char_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_page_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_page_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_block_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_block_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_result_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_index: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CitationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonSchemaFormat {
    #[serde(rename = "type")]
    pub type_: JsonSchemaFormatType,
    pub schema: JsonObject,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}
