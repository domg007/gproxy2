use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::common::JsonObject;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListModelsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrieveModelPath {
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListModelsResponse {
    pub data: Vec<ModelInfo>,
    pub first_id: String,
    pub has_more: bool,
    pub last_id: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub created_at: String,
    pub display_name: String,
    pub max_input_tokens: u64,
    pub max_tokens: u64,
    pub capabilities: ModelCapabilities,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub batch: CapabilitySupport,
    pub citations: CapabilitySupport,
    pub code_execution: CapabilitySupport,
    pub context_management: ContextManagementCapability,
    pub effort: EffortCapability,
    pub image_input: CapabilitySupport,
    pub pdf_input: CapabilitySupport,
    pub structured_outputs: CapabilitySupport,
    pub thinking: ThinkingCapability,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySupport {
    pub supported: bool,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextManagementCapability {
    pub supported: bool,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub strategies: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffortCapability {
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low: Option<CapabilitySupport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medium: Option<CapabilitySupport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high: Option<CapabilitySupport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xhigh: Option<CapabilitySupport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<CapabilitySupport>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingCapability {
    pub supported: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, CapabilitySupport>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelError {
    #[serde(rename = "type")]
    pub type_: String,
    pub message: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}
