use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::*;
use super::generate_content::{
    ComputerScreenshot, ResponseInput, ResponseInputContentPart, ResponseMessageItemType,
    ResponseOutputContentPart, ResponseUsage, TypedResponseItem, UnknownResponseItem,
};

pub type CompactResponseWireModel =
    OpenAiWireModel<CompactResponseRequestBody, CompactedResponseObject>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactResponseRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<ResponseInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub model: OpenAiModelId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<PromptCacheRetention>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<CompactServiceTier>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactedResponseObject {
    pub id: String,
    pub created_at: u64,
    pub object: ResponseCompactionObjectType,
    pub output: Vec<CompactResponseItem>,
    pub usage: ResponseUsage,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompactResponseItem {
    Message(CompactMessageItem),
    Typed(TypedResponseItem),
    Unknown(UnknownResponseItem),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactMessageItem {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: ResponseMessageItemType,
    pub content: Vec<CompactMessageContentPart>,
    pub role: CompactMessageRole,
    pub status: ResponseItemLifecycleStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<ResponsePhase>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompactMessageContentPart {
    Input(ResponseInputContentPart),
    Output(ResponseOutputContentPart),
    Text(CompactTextContent),
    SummaryText(CompactSummaryTextContent),
    ComputerScreenshot(ComputerScreenshot),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactTextContent {
    pub text: String,
    #[serde(rename = "type")]
    pub type_: CompactTextContentType,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactTextContentType {
    #[serde(rename = "text")]
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactSummaryTextContent {
    pub text: String,
    #[serde(rename = "type")]
    pub type_: CompactSummaryTextContentType,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactSummaryTextContentType {
    #[serde(rename = "summary_text")]
    SummaryText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactMessageRole {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "system")]
    System,
    #[serde(rename = "critic")]
    Critic,
    #[serde(rename = "discriminator")]
    Discriminator,
    #[serde(rename = "developer")]
    Developer,
    #[serde(rename = "tool")]
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactServiceTier {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "flex")]
    Flex,
    #[serde(rename = "priority")]
    Priority,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn compact_response_request_matches_documented_body_fields() {
        let request: CompactResponseRequestBody = serde_json::from_value(json!({
            "model": "gpt-5.4",
            "input": "Create a landing page.",
            "instructions": "Keep only the useful context.",
            "previous_response_id": "resp_123",
            "prompt_cache_key": "compact-key",
            "prompt_cache_retention": "24h",
            "service_tier": "priority"
        }))
        .expect("compact request should deserialize");

        assert!(matches!(request.input, Some(ResponseInput::Text(_))));
        assert_eq!(
            request.instructions.as_deref(),
            Some("Keep only the useful context.")
        );
        assert_eq!(request.previous_response_id.as_deref(), Some("resp_123"));
        assert_eq!(
            request.prompt_cache_retention,
            Some(PromptCacheRetention::TwentyFourHours)
        );
        assert_eq!(request.service_tier, Some(CompactServiceTier::Priority));
        assert!(
            serde_json::from_value::<CompactResponseRequestBody>(json!({
                "model": "gpt-5.4",
                "service_tier": "scale"
            }))
            .is_err()
        );
    }

    #[test]
    fn compact_response_models_compaction_object_and_output_items() {
        let response: CompactedResponseObject = serde_json::from_value(json!({
            "id": "resp_001",
            "object": "response.compaction",
            "created_at": 1764967971u64,
            "output": [
                {
                    "id": "msg_000",
                    "type": "message",
                    "status": "completed",
                    "content": [
                        { "type": "input_text", "text": "Create a simple landing page." }
                    ],
                    "role": "user"
                },
                {
                    "id": "cmp_001",
                    "type": "compaction",
                    "encrypted_content": "gAAAAABpM0Yj"
                }
            ],
            "usage": {
                "input_tokens": 139,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens": 438,
                "output_tokens_details": { "reasoning_tokens": 64 },
                "total_tokens": 577
            }
        }))
        .expect("compact response should deserialize");

        assert_eq!(
            response.object,
            ResponseCompactionObjectType::ResponseCompaction
        );
        assert_eq!(response.output.len(), 2);
        assert!(matches!(
            &response.output[0],
            CompactResponseItem::Message(CompactMessageItem {
                role: CompactMessageRole::User,
                ..
            })
        ));
        assert!(matches!(
            &response.output[1],
            CompactResponseItem::Typed(TypedResponseItem::Compaction { .. })
        ));

        assert!(
            serde_json::from_value::<CompactedResponseObject>(json!({
                "id": "resp_001",
                "object": "response.compaction",
                "created_at": 1764967971u64,
                "usage": {
                    "input_tokens": 139,
                    "input_tokens_details": { "cached_tokens": 0 },
                    "output_tokens": 438,
                    "output_tokens_details": { "reasoning_tokens": 64 },
                    "total_tokens": 577
                }
            }))
            .is_err()
        );
    }

    #[test]
    fn compact_message_models_extra_roles_and_content_parts() {
        let item: CompactResponseItem = serde_json::from_value(json!({
            "id": "msg_unknown",
            "type": "message",
            "status": "in_progress",
            "content": [
                { "type": "text", "text": "raw text" },
                { "type": "summary_text", "text": "summary" },
                { "type": "output_text", "annotations": [], "text": "answer" }
            ],
            "role": "unknown",
            "phase": "commentary"
        }))
        .expect("compact message should deserialize");

        let CompactResponseItem::Message(message) = item else {
            panic!("expected compact message");
        };
        assert_eq!(message.role, CompactMessageRole::Unknown);
        assert_eq!(message.phase, Some(ResponsePhase::Commentary));
        assert_eq!(message.content.len(), 3);
    }

    #[test]
    fn compact_response_rejects_undocumented_message_phase() {
        let result = serde_json::from_value::<CompactMessageItem>(json!({
            "id": "msg_123",
            "content": "intermediate",
            "role": "assistant",
            "phase": "analysis",
            "status": "completed",
            "type": "message"
        }))
        .is_err();

        assert!(result);
    }
}
