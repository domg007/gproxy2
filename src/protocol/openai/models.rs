use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::{Extra, OpenAiWireModel};

pub type ModelsWireModel = OpenAiWireModel<(), ModelListResponse>;
pub type ModelRetrieveWireModel = OpenAiWireModel<(), Model>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelListResponse {
    pub data: Vec<Model>,
    pub object: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub created: u64,
    pub object: String,
    pub owned_by: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}
