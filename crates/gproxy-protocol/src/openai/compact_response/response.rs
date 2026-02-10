use serde::{Deserialize, Serialize};

use crate::openai::create_response::types::{Item, MessageItem, OutputItem, ResponseUsage};
use crate::openai::list_response_items::types::ItemResource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompactResponseObjectType {
    #[serde(rename = "response.compaction")]
    ResponseCompaction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CompactResponseResponse {
    pub id: String,
    pub object: CompactResponseObjectType,
    pub output: Vec<CompactResponseOutputItem>,
    pub created_at: i64,
    pub usage: ResponseUsage,
}

/// OpenAPI schema points to `OutputItem`, but official compact examples include
/// `user` messages and mixed item shapes with optional IDs. Accept message,
/// response-item resources, output-only items, and full item inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompactResponseOutputItem {
    Message(MessageItem),
    ItemResource(ItemResource),
    OutputItem(OutputItem),
    Item(Item),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_compact_response_payload() {
        let json = r#"
        {
          "id": "resp_001",
          "object": "response.compaction",
          "created_at": 1764967971,
          "output": [],
          "usage": {
            "input_tokens": 139,
            "input_tokens_details": { "cached_tokens": 0 },
            "output_tokens": 438,
            "output_tokens_details": { "reasoning_tokens": 64 },
            "total_tokens": 577
          }
        }
        "#;

        let parsed: CompactResponseResponse =
            serde_json::from_str(json).expect("deserialize compact response payload");
        assert_eq!(parsed.id, "resp_001");
        assert_eq!(parsed.object, CompactResponseObjectType::ResponseCompaction);
        assert_eq!(parsed.usage.total_tokens, 577);
    }

    #[test]
    fn deserializes_compact_response_with_user_message_and_compaction() {
        let json = r#"
        {
          "id": "resp_001",
          "object": "response.compaction",
          "created_at": 1764967971,
          "output": [
            {
              "id": "msg_000",
              "type": "message",
              "status": "completed",
              "content": [
                {
                  "type": "input_text",
                  "text": "Create a simple landing page for a dog petting cafe."
                }
              ],
              "role": "user"
            },
            {
              "id": "cmp_001",
              "type": "compaction",
              "encrypted_content": "gAAAAABpM0Yj-...="
            }
          ],
          "usage": {
            "input_tokens": 139,
            "input_tokens_details": { "cached_tokens": 0 },
            "output_tokens": 438,
            "output_tokens_details": { "reasoning_tokens": 64 },
            "total_tokens": 577
          }
        }
        "#;

        let parsed: CompactResponseResponse =
            serde_json::from_str(json).expect("deserialize compact response with user message");
        assert_eq!(parsed.output.len(), 2);
        assert_eq!(parsed.usage.total_tokens, 577);
    }
}
