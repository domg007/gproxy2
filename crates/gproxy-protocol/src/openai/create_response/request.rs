use serde::{Deserialize, Serialize};

use crate::openai::create_response::types::{
    HttpMethod, Metadata, Model, ResponseContextManagementEntry, ResponseConversation,
    ResponseIncludable, ResponseInput, ResponsePrompt, ResponsePromptCacheRetention,
    ResponseReasoning, ResponseServiceTier, ResponseStreamOptions, ResponseTextConfig,
    ResponseTool, ResponseToolChoice, ResponseTruncation,
};

/// Request descriptor for OpenAI `responses.create` endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiCreateResponseRequest {
    /// HTTP method.
    pub method: HttpMethod,
    /// Path parameters.
    pub path: PathParameters,
    /// Query parameters.
    pub query: QueryParameters,
    /// Request headers.
    pub headers: RequestHeaders,
    /// Request body.
    pub body: RequestBody,
}

impl Default for OpenAiCreateResponseRequest {
    fn default() -> Self {
        Self {
            method: HttpMethod::Post,
            path: PathParameters::default(),
            query: QueryParameters::default(),
            headers: RequestHeaders::default(),
            body: RequestBody::default(),
        }
    }
}

/// `responses.create` does not define path params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PathParameters {}

/// `responses.create` does not define query params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct QueryParameters {}

/// Proxy-side request model does not carry auth headers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RequestHeaders {}

/// Body payload for `POST /responses`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RequestBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_management: Option<Vec<ResponseContextManagementEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<ResponseConversation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<ResponseIncludable>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<ResponseInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<ResponsePrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_retention: Option<ResponsePromptCacheRetention>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponseReasoning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ResponseServiceTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<ResponseStreamOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<ResponseTextConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ResponseToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponseTool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<ResponseTruncation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::RequestBody;
    use crate::openai::count_tokens::types::{ResponseInput, ResponseInputItem};

    #[test]
    fn request_body_accepts_reasoning_item_without_id() {
        let value = json!({
            "model": "gpt-5.3-codex",
            "stream": true,
            "input": [
                {
                    "type": "message",
                    "role": "developer",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "developer prompt"
                        }
                    ]
                },
                {
                    "type": "reasoning",
                    "summary": [
                        {
                            "type": "summary_text",
                            "text": "plan"
                        }
                    ],
                    "content": null,
                    "encrypted_content": "abc"
                },
                {
                    "type": "function_call",
                    "name": "exec_command",
                    "arguments": "{\"cmd\":\"ls\"}",
                    "call_id": "call_1"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "ok"
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "phase": "commentary",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "working"
                        }
                    ]
                }
            ]
        });

        let body: RequestBody = serde_json::from_value(value).expect("request body should parse");
        let Some(ResponseInput::Items(ref items)) = body.input else {
            panic!("expected input items");
        };
        let Some(ResponseInputItem::ReasoningItem(reasoning)) = items.get(1) else {
            panic!("expected reasoning item");
        };
        assert!(reasoning.id.is_none());

        let encoded = serde_json::to_value(body).expect("request body should serialize");
        assert!(encoded["input"][1].get("id").is_none());
    }
}
