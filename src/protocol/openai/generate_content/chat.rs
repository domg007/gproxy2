use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::super::common::*;
use super::chat_stream::ChatCompletionChunk;
use super::chat_tail::{
    ChatAnnotation, ChatAudio, ChatAudioParam, ChatAudioRef, ChatChoiceLogprobs, ChatFileRef,
    ChatWebSearchOptions, CompletionUsage, CustomToolCall, ImageUrl, InputAudio, PredictionContent,
    StreamOptions,
};

pub type ChatCompletionWireModel = OpenAiWireModel<ChatCompletionRequest, ChatCompletionResponse>;
pub type ChatCompletionStreamWireModel =
    OpenAiWireModel<ChatCompletionRequest, ChatCompletionChunk>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub messages: Vec<ChatCompletionMessageParam>,
    pub model: OpenAiModelId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<ChatAudioParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<LegacyFunctionCallChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub functions: Option<Vec<LegacyFunctionDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<LogitBias>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Vec<TextOrAudioModality>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderation: Option<ModerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prediction: Option<PredictionContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<PromptCacheRetention>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<StringOrList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ChatToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<Verbosity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search_options: Option<ChatWebSearchOptions>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn chat_completion_request_models_store_parameter() {
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "gpt-5.4",
            "messages": [
                { "role": "user", "content": "hello" }
            ],
            "store": true
        }))
        .expect("chat completion request should deserialize");

        assert_eq!(request.store, Some(true));
        assert!(!request.extra.contains_key("store"));
    }

    #[test]
    fn chat_completion_request_models_logit_bias_as_token_bias_map() {
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "gpt-5.4",
            "messages": [
                { "role": "user", "content": "hello" }
            ],
            "logit_bias": {
                "50256": -100.0,
                "198": 1.5
            }
        }))
        .expect("chat completion request should deserialize");

        let logit_bias = request.logit_bias.expect("logit_bias");
        assert_eq!(logit_bias.get("50256"), Some(&-100.0));
        assert_eq!(logit_bias.get("198"), Some(&1.5));
        assert!(!request.extra.contains_key("logit_bias"));
    }

    #[test]
    fn chat_completion_user_file_part_matches_chat_file_shape() {
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "gpt-5.4",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "file",
                    "file": {
                        "file_id": "file_123",
                        "filename": "notes.txt"
                    }
                }]
            }]
        }))
        .expect("chat completion request should deserialize");

        let ChatCompletionMessageParam::User { content, .. } =
            request.messages.into_iter().next().expect("message")
        else {
            panic!("expected user message");
        };
        let ChatContent::Parts(parts) = content else {
            panic!("expected content parts");
        };
        assert!(matches!(
            parts.as_slice(),
            [ChatContentPart::File {
                file: ChatFileRef {
                    file_id: Some(file_id),
                    filename: Some(filename),
                    ..
                },
                ..
            }] if file_id == "file_123" && filename == "notes.txt"
        ));
    }

    #[test]
    fn chat_completion_request_models_legacy_function_shapes() {
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "gpt-5.4",
            "messages": [
                { "role": "user", "content": "hello" }
            ],
            "function_call": { "name": "get_weather" },
            "functions": [{
                "name": "get_weather",
                "description": "Get weather",
                "parameters": { "type": "object" },
                "strict": true
            }]
        }))
        .expect("chat completion request should deserialize");

        assert!(matches!(
            request.function_call.expect("function_call"),
            LegacyFunctionCallChoice::Named(LegacyFunctionCallOption { ref name, .. })
                if name == "get_weather"
        ));

        let function = request
            .functions
            .expect("functions")
            .into_iter()
            .next()
            .expect("function");
        assert_eq!(function.name, "get_weather");
        assert!(function.extra.contains_key("strict"));
    }

    #[test]
    fn chat_completion_response_models_moderation_results() {
        let response: ChatCompletionResponse = serde_json::from_value(json!({
            "id": "chatcmpl_123",
            "choices": [],
            "created": 1,
            "model": "gpt-5.4",
            "object": "chat.completion",
            "moderation": {
                "input": {
                    "model": "omni-moderation-latest",
                    "results": [{
                        "categories": { "violence": false },
                        "category_applied_input_types": { "violence": ["text"] },
                        "category_scores": { "violence": 0.01 },
                        "flagged": false,
                        "model": "omni-moderation-latest",
                        "type": "moderation_result"
                    }],
                    "type": "moderation_results"
                },
                "output": {
                    "code": "server_error",
                    "message": "moderation failed",
                    "type": "error"
                }
            }
        }))
        .expect("chat completion response should deserialize");

        assert!(matches!(
            response.moderation.expect("moderation").input,
            ChatCompletionModerationOutcome::Results(_)
        ));
        assert!(!response.extra.contains_key("moderation"));
    }

    #[test]
    fn chat_completion_choice_models_assistant_message_role() {
        let choice: ChatCompletionChoice = serde_json::from_value(json!({
            "finish_reason": "stop",
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "hello"
            }
        }))
        .expect("chat completion choice should deserialize");

        assert!(matches!(
            choice.finish_reason,
            ChatFinishReason::Known(ChatFinishReasonKnown::Stop)
        ));
        assert_eq!(choice.message.role, ChatCompletionMessageRole::Assistant);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum ChatCompletionMessageParam {
    #[serde(rename = "developer")]
    Developer {
        content: ChatTextContent,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "system")]
    System {
        content: ChatTextContent,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "user")]
    User {
        content: ChatContent,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<ChatAssistantContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        audio: Option<ChatAudioRef>,
        #[serde(skip_serializing_if = "Option::is_none")]
        function_call: Option<FunctionCall>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        refusal: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ChatToolCall>>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "tool")]
    Tool {
        content: ChatTextContent,
        tool_call_id: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "function")]
    Function {
        content: String,
        name: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatTextContent {
    Text(String),
    Parts(Vec<ChatTextContentPart>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatTextContentPart {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatAssistantContent {
    Text(String),
    Parts(Vec<ChatAssistantContentPart>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatAssistantContentPart {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "refusal")]
    Refusal {
        refusal: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatContentPart {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "image_url")]
    ImageUrl {
        image_url: ImageUrl,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "input_audio")]
    InputAudio {
        input_audio: InputAudio,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "file")]
    File {
        file: ChatFileRef,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatTool {
    #[serde(rename = "function")]
    Function {
        function: FunctionDefinition,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "custom")]
    Custom {
        custom: CustomToolDefinition,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatToolCall {
    #[serde(rename = "function")]
    Function {
        id: String,
        function: FunctionCall,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "custom")]
    Custom {
        id: String,
        custom: CustomToolCall,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub choices: Vec<ChatCompletionChoice>,
    pub created: u64,
    pub model: OpenAiModelId,
    pub object: ChatCompletionObjectType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderation: Option<ChatCompletionModeration>,
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
pub struct ChatCompletionChoice {
    pub finish_reason: ChatFinishReason,
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<ChatChoiceLogprobs>,
    pub message: ChatMessage,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionModeration {
    pub input: ChatCompletionModerationOutcome,
    pub output: ChatCompletionModerationOutcome,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatCompletionModerationOutcome {
    Results(ChatCompletionModerationResults),
    Error(ModerationError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionModerationResults {
    pub model: OpenAiModelId,
    pub results: Vec<ModerationResult>,
    #[serde(rename = "type")]
    pub type_: ChatCompletionModerationResultsType,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatCompletionModerationResultsType {
    #[serde(rename = "moderation_results")]
    ModerationResults,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatCompletionMessageRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<ChatAnnotation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<ChatAudio>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatToolCall>>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatCompletionMessageRole {
    #[serde(rename = "assistant")]
    Assistant,
}
