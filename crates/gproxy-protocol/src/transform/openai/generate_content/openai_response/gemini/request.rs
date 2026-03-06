use crate::gemini::count_tokens::types as gt;
use crate::gemini::generate_content::request::{
    GeminiGenerateContentRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::gemini::generate_content::types::HttpMethod as GeminiHttpMethod;
use crate::openai::count_tokens::types as ot;
use crate::openai::create_response::request::OpenAiCreateResponseRequest;
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

impl TryFrom<OpenAiCreateResponseRequest> for GeminiGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseRequest) -> Result<Self, TransformError> {
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
        let generation_config = openai_generation_config(
            body.reasoning,
            body.text,
            body.max_output_tokens,
            body.temperature,
            body.top_p,
            body.top_logprobs,
        );
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

        Ok(GeminiGenerateContentRequest {
            method: GeminiHttpMethod::Post,
            path: PathParameters { model },
            query: QueryParameters::default(),
            headers: RequestHeaders::default(),
            body: RequestBody {
                contents,
                tools,
                tool_config,
                safety_settings: None,
                system_instruction,
                generation_config,
                cached_content: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::create_response::request as oreq;

    #[test]
    fn mixed_function_and_web_search_drops_builtin_search_for_gemini() {
        let request = OpenAiCreateResponseRequest {
            method: ot::HttpMethod::Post,
            path: oreq::PathParameters::default(),
            query: oreq::QueryParameters::default(),
            headers: oreq::RequestHeaders::default(),
            body: oreq::RequestBody {
                model: Some("gemini-2.5-flash".to_string()),
                tool_choice: Some(ot::ResponseToolChoice::Options(
                    ot::ResponseToolChoiceOptions::Auto,
                )),
                tools: Some(vec![
                    ot::ResponseTool::Function(ot::ResponseFunctionTool {
                        name: "exec_command".to_string(),
                        parameters: serde_json::from_value(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "cmd": {"type": "string"}
                            }
                        }))
                        .expect("json object"),
                        strict: Some(true),
                        type_: ot::ResponseFunctionToolType::Function,
                        description: Some("Run a command".to_string()),
                    }),
                    ot::ResponseTool::WebSearchPreview(ot::ResponseWebSearchPreviewTool {
                        type_: ot::ResponseWebSearchPreviewToolType::WebSearchPreview,
                        search_context_size: None,
                        user_location: None,
                    }),
                ]),
                ..oreq::RequestBody::default()
            },
        };

        let converted = GeminiGenerateContentRequest::try_from(request).expect("convert request");
        let tools = converted.body.tools.expect("tools");
        assert_eq!(tools.len(), 1);
        assert!(tools[0].function_declarations.is_some());
        assert!(tools[0].google_search.is_none());

        let tool_config = converted.body.tool_config.expect("tool config");
        let function_calling = tool_config
            .function_calling_config
            .expect("function calling config");
        assert_eq!(function_calling.mode, Some(gt::GeminiFunctionCallingMode::Auto));
    }

    #[test]
    fn web_search_only_omits_function_calling_config() {
        let request = OpenAiCreateResponseRequest {
            method: ot::HttpMethod::Post,
            path: oreq::PathParameters::default(),
            query: oreq::QueryParameters::default(),
            headers: oreq::RequestHeaders::default(),
            body: oreq::RequestBody {
                model: Some("gemini-2.5-flash".to_string()),
                tool_choice: Some(ot::ResponseToolChoice::Options(
                    ot::ResponseToolChoiceOptions::Auto,
                )),
                tools: Some(vec![ot::ResponseTool::WebSearchPreview(
                    ot::ResponseWebSearchPreviewTool {
                        type_: ot::ResponseWebSearchPreviewToolType::WebSearchPreview,
                        search_context_size: None,
                        user_location: None,
                    },
                )]),
                ..oreq::RequestBody::default()
            },
        };

        let converted = GeminiGenerateContentRequest::try_from(request).expect("convert request");
        let tools = converted.body.tools.expect("tools");
        assert_eq!(tools.len(), 1);
        assert!(tools[0].google_search.is_some());
        assert!(converted.body.tool_config.is_none());
    }
}
