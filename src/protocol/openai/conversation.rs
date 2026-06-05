use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::*;
use super::generate_content::ResponseItem;

pub type CreateConversationWireModel =
    OpenAiWireModel<CreateConversationRequestBody, ConversationObject>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateConversationRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<ResponseItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationObject {
    pub id: String,
    pub created_at: u64,
    pub metadata: Metadata,
    pub object: ConversationObjectType,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn create_conversation_models_items_and_metadata() {
        let request: CreateConversationRequestBody = serde_json::from_value(json!({
            "metadata": { "topic": "demo" },
            "items": [{
                "type": "message",
                "role": "user",
                "content": "Hello!"
            }]
        }))
        .expect("conversation request should deserialize");

        assert_eq!(
            request
                .metadata
                .expect("metadata")
                .get("topic")
                .map(String::as_str),
            Some("demo")
        );
        assert_eq!(request.items.expect("items").len(), 1);
    }

    #[test]
    fn create_conversation_rejects_non_string_metadata_values() {
        let result = serde_json::from_value::<CreateConversationRequestBody>(json!({
            "metadata": { "priority": 1 }
        }));

        assert!(result.is_err());
    }

    #[test]
    fn conversation_response_models_literal_object_type() {
        let response: ConversationObject = serde_json::from_value(json!({
            "id": "conv_123",
            "created_at": 1,
            "metadata": {},
            "object": "conversation"
        }))
        .expect("conversation response should deserialize");

        assert_eq!(response.object, ConversationObjectType::Conversation);
    }
}
