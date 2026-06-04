use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type ExtraFields = BTreeMap<String, Value>;
pub type JsonMap = BTreeMap<String, Value>;
pub type WireEnum = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModalityTokenCount {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modality: Option<WireEnum>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<i64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SafetySetting {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<WireEnum>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<WireEnum>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SafetyRating {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<WireEnum>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probability: Option<WireEnum>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CitationMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citation_sources: Vec<CitationSource>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CitationSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}
