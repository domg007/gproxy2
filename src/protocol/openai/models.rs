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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn model_list_uses_literal_object_types_and_model_ids() {
        let response: ModelListResponse = serde_json::from_value(json!({
            "object": "list",
            "data": [{
                "id": "gpt-5.4",
                "created": 1,
                "object": "model",
                "owned_by": "openai"
            }]
        }))
        .expect("model list should deserialize");

        assert_eq!(response.object, ListObjectType::List);
        assert_eq!(response.data[0].object, ModelObjectType::Model);
        assert!(matches!(
            response.data[0].id,
            OpenAiModelId::Known(OpenAiModelIdKnown::Gpt54)
        ));
    }
}
