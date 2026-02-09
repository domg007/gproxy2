use serde::{Deserialize, Serialize};

use crate::openai::create_response::types::InputParam;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CompactResponseRequestBody {
    /// Model ID used to compact the response context.
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<InputParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactResponseRequest {
    pub body: CompactResponseRequestBody,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_compact_request_fields_as_snake_case() {
        let req = CompactResponseRequest {
            body: CompactResponseRequestBody {
                model: "gpt-5.2".to_string(),
                input: None,
                previous_response_id: Some("resp_prev".to_string()),
                instructions: Some("keep this short".to_string()),
            },
        };

        let value = serde_json::to_value(&req).expect("serialize compact response request");
        assert_eq!(value["body"]["model"], "gpt-5.2");
        assert_eq!(value["body"]["previous_response_id"], "resp_prev");
        assert_eq!(value["body"]["instructions"], "keep this short");
    }
}
