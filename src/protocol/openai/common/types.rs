use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::operation::OperationKey;

pub type Extra = BTreeMap<String, Value>;
pub type JsonSchema = BTreeMap<String, Value>;
pub type Metadata = BTreeMap<String, Value>;
pub type OpenAiModelId = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiWireModel<TRequest, TResponse> {
    pub operation_key: OperationKey,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<TRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<TResponse>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrList {
    String(String),
    List(Vec<String>),
}
