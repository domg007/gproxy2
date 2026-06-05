use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::*;
use super::generate_content::{
    ReasoningConfig, ResponseConversationRef, ResponseInput, ResponseTool, TextConfig,
};

pub type ResponseInputTokensWireModel =
    OpenAiWireModel<ResponseInputTokensRequest, ResponseInputTokensResponse>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseInputTokensRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<ResponseConversationRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<ResponseInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenAiModelId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personality: Option<ResponsePersonality>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ResponseToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponseTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<TruncationStrategy>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseInputTokensResponse {
    pub input_tokens: u32,
    pub object: ResponseInputTokensObjectType,
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
            "instructions": "Count this request.",
            "model": "gpt-5.4",
            "parallel_tool_calls": true,
            "personality": "pragmatic",
            "previous_response_id": "resp_123",
            "reasoning": { "effort": "low" },
            "text": { "verbosity": "low" },
            "tool_choice": "auto",
            "tools": [{ "type": "function", "name": "lookup", "parameters": {}, "strict": true }],
            "truncation": "auto"
        }))
        .expect("input token count request should deserialize");

        assert!(matches!(
            request.conversation,
            Some(ResponseConversationRef::Object(_))
        ));
        assert!(matches!(request.input, Some(ResponseInput::Text(_))));
        assert_eq!(request.instructions.as_deref(), Some("Count this request."));
        assert!(matches!(
            request.model,
            Some(OpenAiModelId::Known(OpenAiModelIdKnown::Gpt54))
        ));
        assert_eq!(request.parallel_tool_calls, Some(true));
        assert!(matches!(
            request.personality,
            Some(ResponsePersonality::Known(
                ResponsePersonalityKnown::Pragmatic
            ))
        ));
        assert_eq!(request.previous_response_id.as_deref(), Some("resp_123"));
        assert!(matches!(
            request.reasoning.expect("reasoning").effort,
            Some(ReasoningEffort::Known(ReasoningEffortKnown::Low))
        ));
        assert!(matches!(
            request.text.expect("text").verbosity,
            Some(Verbosity::Known(VerbosityKnown::Low))
        ));
        assert!(matches!(
            request.tool_choice,
            Some(ResponseToolChoice::Mode(ToolChoiceMode::Known(
                ToolChoiceModeKnown::Auto
            )))
        ));
        assert_eq!(request.tools.expect("tools").len(), 1);
        assert!(matches!(
            request.truncation,
            Some(TruncationStrategy::Known(TruncationStrategyKnown::Auto))
        ));
        assert!(!request.extra.contains_key("model"));
        assert!(!request.extra.contains_key("personality"));
    }
}
