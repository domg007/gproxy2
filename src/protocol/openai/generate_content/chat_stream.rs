use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::super::common::*;
use super::chat_tail::{ChatChoiceLogprobs, CompletionUsage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub choices: Vec<ChatChunkChoice>,
    pub created: u64,
    pub model: OpenAiModelId,
    pub object: ChatCompletionChunkObjectType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<CompletionUsage>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatChunkChoice {
    pub index: u32,
    pub delta: ChatDelta,
    pub finish_reason: Option<ChatFinishReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<ChatChoiceLogprobs>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<ChatDeltaRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCallDelta>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCallDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obfuscation: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatDeltaRole {
    #[serde(rename = "assistant")]
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatToolCallDelta {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<ChatToolCallType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionCallDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<CustomToolCallDelta>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCallDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomToolCallDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn chat_delta_models_assistant_role() {
        let chunk: ChatCompletionChunk = serde_json::from_value(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "created": 1694268190,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": ""
                },
                "logprobs": null,
                "finish_reason": null
            }]
        }))
        .expect("chat completion chunk should deserialize");

        assert_eq!(chunk.choices[0].delta.role, Some(ChatDeltaRole::Assistant));
    }

    #[test]
    fn chat_delta_rejects_response_only_tool_call_type() {
        let result = serde_json::from_value::<ChatCompletionChunk>(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "created": 1694268190,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_123",
                        "type": "file_search"
                    }]
                },
                "finish_reason": null
            }]
        }));

        assert!(result.is_err());
    }

    #[test]
    fn chat_delta_models_stream_obfuscation() {
        let chunk: ChatCompletionChunk = serde_json::from_value(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "created": 1694268190,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "delta": {
                    "content": "hello",
                    "obfuscation": "abc123"
                },
                "finish_reason": null
            }]
        }))
        .expect("chat completion chunk should deserialize");

        assert_eq!(
            chunk.choices[0].delta.obfuscation.as_deref(),
            Some("abc123")
        );
    }
}
