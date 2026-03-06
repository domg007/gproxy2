use crate::gemini::count_tokens::types as gt;
use crate::gemini::generate_content::request::{
    GeminiGenerateContentRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::gemini::generate_content::types::HttpMethod as GeminiHttpMethod;
use crate::openai::create_response::request::OpenAiCreateResponseRequest;
use crate::transform::gemini::model_get::utils::ensure_models_prefix;
use crate::transform::openai::count_tokens::gemini::utils::{
    openai_generation_config, openai_input_items_to_gemini_contents, openai_tool_choice_to_gemini,
    openai_tools_to_gemini,
};
use crate::transform::openai::count_tokens::openai::utils::openai_input_to_items;
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiCreateResponseRequest> for GeminiGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseRequest) -> Result<Self, TransformError> {
        let body = value.body;
        let model = ensure_models_prefix(&body.model.unwrap_or_default());

        let contents = openai_input_items_to_gemini_contents(openai_input_to_items(body.input));

        let (tools, has_function_calling_tools) = body
            .tools
            .map(openai_tools_to_gemini)
            .unwrap_or((None, false));

        let tool_config =
            openai_tool_choice_to_gemini(body.tool_choice, has_function_calling_tools);
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
    use crate::openai::count_tokens::types as ot;
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
        assert_eq!(
            function_calling.mode,
            Some(gt::GeminiFunctionCallingMode::Auto)
        );
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
