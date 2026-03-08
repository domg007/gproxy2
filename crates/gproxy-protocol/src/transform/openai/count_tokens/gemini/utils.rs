use crate::gemini::count_tokens::types as gt;
use crate::openai::count_tokens::types as ot;
use crate::transform::openai::count_tokens::openai::utils::{
    openai_function_call_output_content_to_text, openai_reasoning_summary_to_text,
};

pub const GEMINI_SKIP_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";

fn parse_data_url_to_blob(url: &str) -> Option<gt::GeminiBlob> {
    if !url.starts_with("data:") {
        return None;
    }

    let data_index = url.find(";base64,")?;
    let mime = &url[5..data_index];
    let data = &url[(data_index + ";base64,".len())..];

    Some(gt::GeminiBlob {
        mime_type: mime.to_string(),
        data: data.to_string(),
    })
}

fn openai_input_content_to_gemini_parts(content: ot::ResponseInputContent) -> Vec<gt::GeminiPart> {
    match content {
        ot::ResponseInputContent::Text(part) => {
            if part.text.is_empty() {
                Vec::new()
            } else {
                vec![gt::GeminiPart {
                    text: Some(part.text),
                    ..gt::GeminiPart::default()
                }]
            }
        }
        ot::ResponseInputContent::Image(part) => {
            if let Some(image_url) = part.image_url {
                if let Some(blob) = parse_data_url_to_blob(&image_url) {
                    return vec![gt::GeminiPart {
                        inline_data: Some(blob),
                        ..gt::GeminiPart::default()
                    }];
                }
                if !image_url.is_empty() {
                    return vec![gt::GeminiPart {
                        file_data: Some(gt::GeminiFileData {
                            mime_type: Some("image/*".to_string()),
                            file_uri: image_url,
                        }),
                        ..gt::GeminiPart::default()
                    }];
                }
            }
            if let Some(file_id) = part.file_id {
                return vec![gt::GeminiPart {
                    file_data: Some(gt::GeminiFileData {
                        mime_type: None,
                        file_uri: format!("openai-file:{file_id}"),
                    }),
                    ..gt::GeminiPart::default()
                }];
            }
            Vec::new()
        }
        ot::ResponseInputContent::File(part) => {
            if let Some(file_data) = part.file_data {
                return vec![gt::GeminiPart {
                    inline_data: Some(gt::GeminiBlob {
                        mime_type: "application/octet-stream".to_string(),
                        data: file_data,
                    }),
                    ..gt::GeminiPart::default()
                }];
            }
            if let Some(file_url) = part.file_url {
                return vec![gt::GeminiPart {
                    file_data: Some(gt::GeminiFileData {
                        mime_type: None,
                        file_uri: file_url,
                    }),
                    ..gt::GeminiPart::default()
                }];
            }
            if let Some(file_id) = part.file_id {
                return vec![gt::GeminiPart {
                    file_data: Some(gt::GeminiFileData {
                        mime_type: None,
                        file_uri: format!("openai-file:{file_id}"),
                    }),
                    ..gt::GeminiPart::default()
                }];
            }
            Vec::new()
        }
    }
}

pub fn openai_message_content_to_gemini_parts(
    content: ot::ResponseInputMessageContent,
) -> Vec<gt::GeminiPart> {
    match content {
        ot::ResponseInputMessageContent::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![gt::GeminiPart {
                    text: Some(text),
                    ..gt::GeminiPart::default()
                }]
            }
        }
        ot::ResponseInputMessageContent::List(parts) => parts
            .into_iter()
            .flat_map(openai_input_content_to_gemini_parts)
            .collect::<Vec<_>>(),
    }
}

pub fn openai_role_to_gemini(role: ot::ResponseInputMessageRole) -> gt::GeminiContentRole {
    match role {
        ot::ResponseInputMessageRole::Assistant => gt::GeminiContentRole::Model,
        ot::ResponseInputMessageRole::User
        | ot::ResponseInputMessageRole::System
        | ot::ResponseInputMessageRole::Developer => gt::GeminiContentRole::User,
    }
}

pub fn output_text_to_json_object(text: &str) -> gt::JsonObject {
    serde_json::from_str::<gt::JsonObject>(text).unwrap_or_else(|_| {
        let escaped = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
        serde_json::from_str::<gt::JsonObject>(&format!(r#"{{"output":{escaped}}}"#))
            .unwrap_or_default()
    })
}

fn thought_signature_or_dummy(signature: Option<String>) -> String {
    signature.unwrap_or_else(|| GEMINI_SKIP_THOUGHT_SIGNATURE.to_string())
}

pub fn openai_input_items_to_gemini_contents(
    items: Vec<ot::ResponseInputItem>,
) -> Vec<gt::GeminiContent> {
    let mut contents = Vec::new();
    let mut pending_function_call_signature = None;
    let mut model_step_has_function_call = false;

    for item in items {
        match item {
            ot::ResponseInputItem::Message(message) => {
                if !matches!(message.role, ot::ResponseInputMessageRole::Assistant) {
                    pending_function_call_signature = None;
                    model_step_has_function_call = false;
                }

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
                let thought_signature = if model_step_has_function_call {
                    None
                } else {
                    Some(thought_signature_or_dummy(
                        pending_function_call_signature.take(),
                    ))
                };

                contents.push(gt::GeminiContent {
                    parts: vec![gt::GeminiPart {
                        thought_signature,
                        function_call: Some(gt::GeminiFunctionCall {
                            id: Some(tool_call.call_id),
                            name: tool_call.name,
                            args: Some(args),
                        }),
                        ..gt::GeminiPart::default()
                    }],
                    role: Some(gt::GeminiContentRole::Model),
                });

                model_step_has_function_call = true;
            }
            ot::ResponseInputItem::FunctionCallOutput(tool_result) => {
                let output_text = openai_function_call_output_content_to_text(&tool_result.output);
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

                pending_function_call_signature = None;
                model_step_has_function_call = false;
            }
            ot::ResponseInputItem::ReasoningItem(reasoning) => {
                let mut text = openai_reasoning_summary_to_text(&reasoning.summary);
                if text.is_empty() {
                    text = reasoning
                        .encrypted_content
                        .unwrap_or_else(|| "[reasoning]".to_string());
                }

                let thought_signature = Some(thought_signature_or_dummy(
                    reasoning.id.filter(|id| !id.is_empty()),
                ));
                pending_function_call_signature = thought_signature.clone();
                model_step_has_function_call = false;

                contents.push(gt::GeminiContent {
                    parts: vec![gt::GeminiPart {
                        thought: Some(true),
                        thought_signature,
                        text: Some(text),
                        ..gt::GeminiPart::default()
                    }],
                    role: Some(gt::GeminiContentRole::Model),
                });
            }
            other => {
                pending_function_call_signature = None;
                model_step_has_function_call = false;

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

    contents
}

pub fn openai_tool_to_gemini(tool: ot::ResponseTool) -> Option<gt::GeminiTool> {
    match tool {
        ot::ResponseTool::Function(tool) => Some(gt::GeminiTool {
            function_declarations: Some(vec![gt::GeminiFunctionDeclaration {
                name: tool.name,
                description: tool.description.unwrap_or_default(),
                behavior: None,
                parameters: None,
                parameters_json_schema: serde_json::to_value(tool.parameters).ok(),
                response: None,
                response_json_schema: None,
            }]),
            ..gt::GeminiTool::default()
        }),
        ot::ResponseTool::Custom(tool) => Some(gt::GeminiTool {
            function_declarations: Some(vec![gt::GeminiFunctionDeclaration {
                name: tool.name,
                description: tool.description.unwrap_or_default(),
                behavior: None,
                parameters: None,
                parameters_json_schema: None,
                response: None,
                response_json_schema: None,
            }]),
            ..gt::GeminiTool::default()
        }),
        ot::ResponseTool::FileSearch(tool) => Some(gt::GeminiTool {
            file_search: Some(gt::GeminiFileSearch {
                file_search_store_names: tool.vector_store_ids,
                metadata_filter: None,
                top_k: tool.max_num_results.map(u64::from),
            }),
            ..gt::GeminiTool::default()
        }),
        ot::ResponseTool::Computer(_) => Some(gt::GeminiTool {
            computer_use: Some(gt::GeminiComputerUse {
                environment: gt::GeminiEnvironment::EnvironmentBrowser,
                excluded_predefined_functions: None,
            }),
            ..gt::GeminiTool::default()
        }),
        ot::ResponseTool::WebSearch(_) | ot::ResponseTool::WebSearchPreview(_) => {
            Some(gt::GeminiTool {
                google_search: Some(gt::GeminiGoogleSearch::default()),
                ..gt::GeminiTool::default()
            })
        }
        ot::ResponseTool::CodeInterpreter(_)
        | ot::ResponseTool::LocalShell(_)
        | ot::ResponseTool::Shell(_)
        | ot::ResponseTool::ApplyPatch(_) => Some(gt::GeminiTool {
            code_execution: Some(gt::GeminiCodeExecution {}),
            ..gt::GeminiTool::default()
        }),
        ot::ResponseTool::Mcp(_)
        | ot::ResponseTool::ImageGeneration(_)
        | ot::ResponseTool::Namespace(_)
        | ot::ResponseTool::ToolSearch(_) => None,
    }
}

fn openai_tool_uses_gemini_function_calling(tool: &ot::ResponseTool) -> bool {
    matches!(
        tool,
        ot::ResponseTool::Function(_) | ot::ResponseTool::Custom(_)
    )
}

fn openai_tool_uses_gemini_builtin_search(tool: &ot::ResponseTool) -> bool {
    matches!(
        tool,
        ot::ResponseTool::WebSearch(_) | ot::ResponseTool::WebSearchPreview(_)
    )
}

pub fn openai_tools_to_gemini(tools: Vec<ot::ResponseTool>) -> (Option<Vec<gt::GeminiTool>>, bool) {
    let has_function_calling_tools = tools.iter().any(openai_tool_uses_gemini_function_calling);
    let has_builtin_search_tools = tools.iter().any(openai_tool_uses_gemini_builtin_search);

    let converted = tools
        .into_iter()
        .filter(|tool| {
            !(has_function_calling_tools
                && has_builtin_search_tools
                && openai_tool_uses_gemini_builtin_search(tool))
        })
        .filter_map(openai_tool_to_gemini)
        .collect::<Vec<_>>();

    let converted = if converted.is_empty() {
        None
    } else {
        Some(converted)
    };

    (converted, has_function_calling_tools)
}

pub fn openai_tool_choice_to_gemini(
    choice: Option<ot::ResponseToolChoice>,
    has_function_calling_tools: bool,
) -> Option<gt::GeminiToolConfig> {
    if !has_function_calling_tools {
        return None;
    }

    let config = match choice {
        Some(ot::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::Auto)) => {
            Some(gt::GeminiFunctionCallingConfig {
                mode: Some(gt::GeminiFunctionCallingMode::Auto),
                allowed_function_names: None,
            })
        }
        Some(ot::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::Required)) => {
            Some(gt::GeminiFunctionCallingConfig {
                mode: Some(gt::GeminiFunctionCallingMode::Any),
                allowed_function_names: None,
            })
        }
        Some(ot::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::None)) => {
            Some(gt::GeminiFunctionCallingConfig {
                mode: Some(gt::GeminiFunctionCallingMode::None),
                allowed_function_names: None,
            })
        }
        Some(ot::ResponseToolChoice::Function(tool)) => Some(gt::GeminiFunctionCallingConfig {
            mode: Some(gt::GeminiFunctionCallingMode::Any),
            allowed_function_names: Some(vec![tool.name]),
        }),
        Some(ot::ResponseToolChoice::Custom(tool)) => Some(gt::GeminiFunctionCallingConfig {
            mode: Some(gt::GeminiFunctionCallingMode::Any),
            allowed_function_names: Some(vec![tool.name]),
        }),
        Some(ot::ResponseToolChoice::Mcp(tool)) => Some(gt::GeminiFunctionCallingConfig {
            mode: Some(gt::GeminiFunctionCallingMode::Any),
            allowed_function_names: tool.name.map(|name| vec![name]),
        }),
        Some(ot::ResponseToolChoice::Allowed(choice)) => Some(gt::GeminiFunctionCallingConfig {
            mode: Some(match choice.mode {
                ot::ResponseToolChoiceAllowedMode::Auto => gt::GeminiFunctionCallingMode::Auto,
                ot::ResponseToolChoiceAllowedMode::Required => gt::GeminiFunctionCallingMode::Any,
            }),
            allowed_function_names: None,
        }),
        Some(ot::ResponseToolChoice::Types(_))
        | Some(ot::ResponseToolChoice::ApplyPatch(_))
        | Some(ot::ResponseToolChoice::Shell(_)) => None,
        None => None,
    }?;

    Some(gt::GeminiToolConfig {
        function_calling_config: Some(config),
        retrieval_config: None,
    })
}

fn openai_reasoning_to_gemini(
    reasoning: Option<ot::ResponseReasoning>,
) -> Option<gt::GeminiThinkingConfig> {
    let effort = reasoning.and_then(|reasoning| reasoning.effort)?;
    Some(match effort {
        ot::ResponseReasoningEffort::None => gt::GeminiThinkingConfig {
            include_thoughts: Some(false),
            ..gt::GeminiThinkingConfig::default()
        },
        ot::ResponseReasoningEffort::Minimal => gt::GeminiThinkingConfig {
            include_thoughts: Some(true),
            thinking_level: Some(gt::GeminiThinkingLevel::Minimal),
            ..gt::GeminiThinkingConfig::default()
        },
        ot::ResponseReasoningEffort::Low => gt::GeminiThinkingConfig {
            include_thoughts: Some(true),
            thinking_level: Some(gt::GeminiThinkingLevel::Low),
            ..gt::GeminiThinkingConfig::default()
        },
        ot::ResponseReasoningEffort::Medium => gt::GeminiThinkingConfig {
            include_thoughts: Some(true),
            thinking_level: Some(gt::GeminiThinkingLevel::Medium),
            ..gt::GeminiThinkingConfig::default()
        },
        ot::ResponseReasoningEffort::High | ot::ResponseReasoningEffort::XHigh => {
            gt::GeminiThinkingConfig {
                include_thoughts: Some(true),
                thinking_level: Some(gt::GeminiThinkingLevel::High),
                ..gt::GeminiThinkingConfig::default()
            }
        }
    })
}

pub fn openai_generation_config(
    reasoning: Option<ot::ResponseReasoning>,
    text: Option<ot::ResponseTextConfig>,
    max_output_tokens: Option<u64>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    top_logprobs: Option<u32>,
) -> Option<gt::GeminiGenerationConfig> {
    let mut config = gt::GeminiGenerationConfig::default();
    let mut has_config = false;

    if let Some(thinking_config) = openai_reasoning_to_gemini(reasoning) {
        config.thinking_config = Some(thinking_config);
        has_config = true;
    }

    if let Some(text_config) = text
        && let Some(format) = text_config.format
    {
        match format {
            ot::ResponseTextFormatConfig::JsonSchema(schema) => {
                config.response_mime_type = Some("application/json".to_string());
                config.response_json_schema = serde_json::to_value(schema.schema).ok();
                has_config = true;
            }
            ot::ResponseTextFormatConfig::JsonObject(_) => {
                config.response_mime_type = Some("application/json".to_string());
                has_config = true;
            }
            ot::ResponseTextFormatConfig::Text(_) => {
                config.response_mime_type = Some("text/plain".to_string());
                has_config = true;
            }
        }
    }

    if let Some(value) = max_output_tokens {
        config.max_output_tokens = Some(value.min(u32::MAX as u64) as u32);
        has_config = true;
    }
    if let Some(value) = temperature {
        config.temperature = Some(value);
        has_config = true;
    }
    if let Some(value) = top_p {
        config.top_p = Some(value);
        has_config = true;
    }
    if let Some(value) = top_logprobs {
        config.response_logprobs = Some(true);
        config.logprobs = Some(value);
        has_config = true;
    }

    if has_config { Some(config) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mixed_function_and_web_search_prefers_function_tools() {
        let (tools, has_function_calling_tools) = openai_tools_to_gemini(vec![
            ot::ResponseTool::Function(ot::ResponseFunctionTool {
                name: "exec_command".to_string(),
                parameters: serde_json::from_value(json!({
                    "type": "object",
                    "properties": {
                        "cmd": {"type": "string"}
                    }
                }))
                .expect("json object"),
                strict: Some(true),
                type_: ot::ResponseFunctionToolType::Function,
                defer_loading: None,
                description: Some("Run a command".to_string()),
            }),
            ot::ResponseTool::WebSearch(ot::ResponseWebSearchTool {
                type_: ot::ResponseWebSearchToolType::WebSearch,
                filters: None,
                search_context_size: None,
                user_location: None,
            }),
        ]);

        assert!(has_function_calling_tools);
        let tools = tools.expect("converted tools");
        assert_eq!(tools.len(), 1);
        assert!(tools[0].function_declarations.is_some());
        assert!(tools[0].google_search.is_none());
    }

    #[test]
    fn web_search_only_skips_function_calling_config() {
        let tool_config = openai_tool_choice_to_gemini(
            Some(ot::ResponseToolChoice::Options(
                ot::ResponseToolChoiceOptions::Auto,
            )),
            false,
        );

        assert!(tool_config.is_none());
    }

    #[test]
    fn standalone_function_call_gets_dummy_thought_signature() {
        let contents =
            openai_input_items_to_gemini_contents(vec![ot::ResponseInputItem::FunctionToolCall(
                ot::ResponseFunctionToolCall {
                    arguments: "{\"cmd\":\"ls\"}".to_string(),
                    call_id: "call_1".to_string(),
                    name: "exec_command".to_string(),
                    type_: ot::ResponseFunctionToolCallType::FunctionCall,
                    id: None,
                    status: None,
                },
            )]);

        let part = contents
            .first()
            .and_then(|content| content.parts.first())
            .expect("function call part");
        assert_eq!(
            part.thought_signature.as_deref(),
            Some(GEMINI_SKIP_THOUGHT_SIGNATURE)
        );
    }

    #[test]
    fn reasoning_signature_is_reused_by_first_function_call() {
        let contents = openai_input_items_to_gemini_contents(vec![
            ot::ResponseInputItem::ReasoningItem(ot::ResponseReasoningItem {
                id: Some("reasoning_sig".to_string()),
                summary: vec![ot::ResponseSummaryTextContent {
                    text: "plan".to_string(),
                    type_: ot::ResponseSummaryTextContentType::SummaryText,
                }],
                type_: ot::ResponseReasoningItemType::Reasoning,
                content: None,
                encrypted_content: None,
                status: None,
            }),
            ot::ResponseInputItem::FunctionToolCall(ot::ResponseFunctionToolCall {
                arguments: "{}".to_string(),
                call_id: "call_1".to_string(),
                name: "exec_command".to_string(),
                type_: ot::ResponseFunctionToolCallType::FunctionCall,
                id: None,
                status: None,
            }),
            ot::ResponseInputItem::FunctionToolCall(ot::ResponseFunctionToolCall {
                arguments: "{}".to_string(),
                call_id: "call_2".to_string(),
                name: "write_stdin".to_string(),
                type_: ot::ResponseFunctionToolCallType::FunctionCall,
                id: None,
                status: None,
            }),
        ]);

        let first_call = contents
            .get(1)
            .and_then(|content| content.parts.first())
            .expect("first call part");
        let second_call = contents
            .get(2)
            .and_then(|content| content.parts.first())
            .expect("second call part");

        assert_eq!(
            first_call.thought_signature.as_deref(),
            Some("reasoning_sig")
        );
        assert!(second_call.thought_signature.is_none());
    }
}
