use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::*;
use super::generate_content::{ResponseConversationRef, ResponseInput};

pub type ResponseInputTokensWireModel =
    OpenAiWireModel<ResponseInputTokensRequest, ResponseInputTokensResponse>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseInputTokensRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<ResponseConversationRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<ResponseInput>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseInputTokensResponse {
    pub input_tokens: u32,
    pub object: OpenAiObjectType,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn response_input_tokens_request_matches_documented_body_fields() {
        let request: ResponseInputTokensRequest = serde_json::from_value(json!({
            "conversation": { "id": "conv_123" },
            "input": "hello",
            "model": "gpt-5.4"
        }))
        .expect("input token count request should deserialize");

        assert!(matches!(
            request.conversation,
            Some(ResponseConversationRef::Object(_))
        ));
        assert!(matches!(request.input, Some(ResponseInput::Text(_))));
        assert!(request.extra.contains_key("model"));
    }
}
