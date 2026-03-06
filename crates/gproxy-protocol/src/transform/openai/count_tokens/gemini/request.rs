use crate::gemini::count_tokens::request::{
    GeminiCountTokensRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::gemini::count_tokens::types as gt;
use crate::openai::count_tokens::request::OpenAiCountTokensRequest;
use crate::openai::count_tokens::types as ot;
use crate::transform::gemini::model_get::utils::ensure_models_prefix;
use crate::transform::openai::count_tokens::gemini::utils::{
    openai_generation_config, openai_message_content_to_gemini_parts, openai_role_to_gemini,
    openai_tool_choice_to_gemini, openai_tools_to_gemini, output_text_to_json_object,
};
use crate::transform::openai::count_tokens::openai::utils::{
    openai_function_call_output_content_to_text, openai_input_to_items,
    openai_reasoning_summary_to_text,
};
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiCountTokensRequest> for GeminiCountTokensRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCountTokensRequest) -> Result<Self, TransformError> {
        let body = value.body;
        let model = ensure_models_prefix(&body.model.unwrap_or_default());

        let mut contents = Vec::new();
        for item in openai_input_to_items(body.input) {
            match item {
                ot::ResponseInputItem::Message(message) => {
                    let parts = openai_message_content_to_gemini_parts(message.content);
                    if !parts.is_empty() {
                        contents.push(gt::GeminiContent {
                            parts,
                            role: Some(openai_role_to_gemini(message.role)),
                        });
                    }
                }
                ot::ResponseInputItem::OutputMessage(message) => {
                    let text = message
                        .content
                        .into_iter()
                        .map(|part| match part {
                            ot::ResponseOutputContent::Text(text) => text.text,
                            ot::ResponseOutputContent::Refusal(refusal) => refusal.refusal,
                        })
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.is_empty() {
                        contents.push(gt::GeminiContent {
                            parts: vec![gt::GeminiPart {
                                text: Some(text),
                                ..gt::GeminiPart::default()
                            }],
                            role: Some(gt::GeminiContentRole::Model),
                        });
                    }
                }
                ot::ResponseInputItem::FunctionToolCall(tool_call) => {
                    let args = serde_json::from_str::<gt::JsonObject>(&tool_call.arguments)
                        .unwrap_or_default();
                    contents.push(gt::GeminiContent {
                        parts: vec![gt::GeminiPart {
                            function_call: Some(gt::GeminiFunctionCall {
                                id: Some(tool_call.call_id),
                                name: tool_call.name,
                                args: Some(args),
                            }),
                            ..gt::GeminiPart::default()
                        }],
                        role: Some(gt::GeminiContentRole::Model),
                    });
                }
                ot::ResponseInputItem::FunctionCallOutput(tool_result) => {
                    let output_text =
                        openai_function_call_output_content_to_text(&tool_result.output);
                    contents.push(gt::GeminiContent {
                        parts: vec![gt::GeminiPart {
                            function_response: Some(gt::GeminiFunctionResponse {
                                id: Some(tool_result.call_id.clone()),
                                name: tool_result.call_id,
                                response: output_text_to_json_object(&output_text),
                                parts: None,
                                will_continue: None,
                                scheduling: None,
                            }),
                            ..gt::GeminiPart::default()
                        }],
                        role: Some(gt::GeminiContentRole::User),
                    });
                }
                ot::ResponseInputItem::ReasoningItem(reasoning) => {
                    let mut text = openai_reasoning_summary_to_text(&reasoning.summary);
                    if text.is_empty() {
                        text = reasoning
                            .encrypted_content
                            .unwrap_or_else(|| "[reasoning]".to_string());
                    }
                    contents.push(gt::GeminiContent {
                        parts: vec![gt::GeminiPart {
                            thought: Some(true),
                            thought_signature: reasoning.id.filter(|id| !id.is_empty()),
                            text: Some(text),
                            ..gt::GeminiPart::default()
                        }],
                        role: Some(gt::GeminiContentRole::Model),
                    });
                }
                other => {
                    let text = format!("{other:?}");
                    if !text.is_empty() {
                        contents.push(gt::GeminiContent {
                            parts: vec![gt::GeminiPart {
                                text: Some(text),
                                ..gt::GeminiPart::default()
                            }],
                            role: Some(gt::GeminiContentRole::User),
                        });
                    }
                }
            }
        }

        let (tools, has_function_calling_tools) = body
            .tools
            .map(openai_tools_to_gemini)
            .unwrap_or((None, false));

        let tool_config = openai_tool_choice_to_gemini(body.tool_choice, has_function_calling_tools);
        let generation_config =
            openai_generation_config(body.reasoning, body.text, None, None, None, None);
        let system_instruction = body.instructions.and_then(|text| {
            if text.is_empty() {
                None
            } else {
                Some(gt::GeminiContent {
                    parts: vec![gt::GeminiPart {
                        text: Some(text),
                        ..gt::GeminiPart::default()
                    }],
                    role: None,
                })
            }
        });

        Ok(GeminiCountTokensRequest {
            method: gt::HttpMethod::Post,
            path: PathParameters {
                model: model.clone(),
            },
            query: QueryParameters::default(),
            headers: RequestHeaders::default(),
            body: RequestBody {
                contents: None,
                generate_content_request: Some(gt::GeminiGenerateContentRequest {
                    model,
                    contents,
                    tools,
                    tool_config,
                    safety_settings: None,
                    system_instruction,
                    generation_config,
                    cached_content: None,
                }),
            },
        })
    }
}
