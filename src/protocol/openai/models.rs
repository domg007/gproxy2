use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::*;

pub type ModelsWireModel = OpenAiWireModel<(), ModelListResponse>;
pub type ModelRetrieveWireModel = OpenAiWireModel<(), Model>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelListResponse {
    pub data: Vec<Model>,
    pub object: ListObjectType,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: OpenAiModelId,
    pub created: u64,
    pub object: ModelObjectType,
    pub owned_by: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}
