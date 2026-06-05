use crate::protocol::{claude, gemini, openai};
use serde_json::Value;

pub(in crate::transform::count_tokens) const DEFAULT_MODEL: &str = "unknown";

pub(in crate::transform::count_tokens) fn u64_to_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(in crate::transform::count_tokens) fn u64_to_i32(value: u64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

pub(in crate::transform::count_tokens) fn u32_to_u64(value: u32) -> u64 {
    u64::from(value)
}

pub(in crate::transform::count_tokens) fn u32_to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

pub(in crate::transform::count_tokens) fn i32_to_u32(value: i32) -> u32 {
    u32::try_from(value).unwrap_or_default()
}

pub(in crate::transform::count_tokens) fn i32_to_u64(value: i32) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

pub(in crate::transform::count_tokens) fn openai_model_string(
    model: Option<openai::OpenAiModelId>,
) -> String {
    model
        .as_ref()
        .map(model_to_string)
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned())
}

pub(in crate::transform::count_tokens) fn claude_model_string(
    model: &claude::ClaudeModel,
) -> String {
    model_to_string(model)
}

pub(in crate::transform::count_tokens) fn gemini_model_string(model: Option<String>) -> String {
    model.unwrap_or_else(|| DEFAULT_MODEL.to_owned())
}

fn model_to_string<T: serde::Serialize>(model: &T) -> String {
    let Ok(value) = serde_json::to_value(model) else {
        return DEFAULT_MODEL.to_owned();
    };
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned())
}

pub(in crate::transform::count_tokens) fn openai_input_to_text(
    input: Option<openai::ResponseInput>,
) -> String {
    match input {
        Some(openai::ResponseInput::Text(text)) => text,
        Some(openai::ResponseInput::Items(items)) => items
            .into_iter()
            .map(openai_item_text)
            .collect::<Vec<_>>()
            .join("\n"),
        None => String::new(),
    }
}

fn openai_item_text(item: openai::ResponseItem) -> String {
    match item {
        openai::ResponseItem::Message(openai::ResponseMessageItem::EasyInput(message)) => {
            openai_easy_content_text(message.content)
        }
        openai::ResponseItem::Message(openai::ResponseMessageItem::Input(message)) => {
            response_input_parts_text(message.content)
        }
        openai::ResponseItem::Message(openai::ResponseMessageItem::Output(message)) => message
            .content
            .into_iter()
            .map(|part| match part {
                openai::ResponseMessageOutputContentPart::OutputText { text, .. } => text,
                openai::ResponseMessageOutputContentPart::Refusal { refusal, .. } => refusal,
            })
            .collect::<Vec<_>>()
            .join(""),
        openai::ResponseItem::Typed(_) | openai::ResponseItem::Unknown(_) => String::new(),
    }
}

fn openai_easy_content_text(content: openai::ResponseEasyInputContent) -> String {
    match content {
        openai::ResponseEasyInputContent::Text(text) => text,
        openai::ResponseEasyInputContent::Parts(parts) => response_input_parts_text(parts),
    }
}

fn response_input_parts_text(parts: Vec<openai::ResponseInputContentPart>) -> String {
    parts
        .into_iter()
        .filter_map(|part| match part {
            openai::ResponseInputContentPart::InputText { text, .. } => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(in crate::transform::count_tokens) fn text_to_openai_input(
    text: String,
) -> Option<openai::ResponseInput> {
    if text.is_empty() {
        None
    } else {
        Some(openai::ResponseInput::Text(text))
    }
}

pub(in crate::transform::count_tokens) fn claude_messages_to_text(
    messages: Vec<claude::MessageParam>,
) -> String {
    messages
        .into_iter()
        .map(claude_message_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn claude_message_text(message: claude::MessageParam) -> String {
    match message.content {
        claude::StringOrArray::String(text) => text,
        claude::StringOrArray::Array(blocks) => blocks
            .into_iter()
            .filter_map(|block| match block {
                claude::ContentBlockParam::Text(text) => Some(text.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}

pub(in crate::transform::count_tokens) fn text_to_claude_messages(
    text: String,
) -> Vec<claude::MessageParam> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![claude::MessageParam {
        role: claude::MessageRole::Known(claude::MessageRoleKnown::User),
        content: claude::StringOrArray::String(text),
        extra: Default::default(),
    }]
}

pub(in crate::transform::count_tokens) fn claude_system_to_text(
    system: Option<claude::SystemPrompt>,
) -> Option<String> {
    let system = system?;
    match system {
        claude::StringOrArray::String(text) => Some(text),
        claude::StringOrArray::Array(blocks) => {
            let text = blocks
                .into_iter()
                .map(|block| block.text)
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() { None } else { Some(text) }
        }
    }
}

pub(in crate::transform::count_tokens) fn text_to_claude_system(
    text: Option<String>,
) -> Option<claude::SystemPrompt> {
    text.filter(|value| !value.is_empty())
        .map(claude::StringOrArray::String)
}

pub(in crate::transform::count_tokens) fn gemini_contents_to_text(
    contents: Vec<gemini::Content>,
) -> String {
    contents
        .into_iter()
        .map(gemini_content_text)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(in crate::transform::count_tokens) fn gemini_content_text(content: gemini::Content) -> String {
    content
        .parts
        .into_iter()
        .filter_map(|part| match part.data {
            Some(gemini::PartData::Text { text }) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(in crate::transform::count_tokens) fn text_to_gemini_contents(
    text: String,
) -> Vec<gemini::Content> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![text_to_gemini_content(
        text,
        Some(gemini::ContentRole::Known(gemini::ContentRoleKnown::User)),
    )]
}

pub(in crate::transform::count_tokens) fn text_to_gemini_content(
    text: String,
    role: Option<gemini::ContentRole>,
) -> gemini::Content {
    gemini::Content {
        parts: vec![gemini::Part {
            thought: None,
            thought_signature: None,
            part_metadata: None,
            media_resolution: None,
            data: Some(gemini::PartData::Text { text }),
            metadata: None,
            extra: Default::default(),
        }],
        role,
        extra: Default::default(),
    }
}

pub(in crate::transform::count_tokens) fn openai_tools_to_gemini(
    tools: Option<Vec<openai::ResponseTool>>,
) -> Vec<gemini::Tool> {
    let mut declarations = Vec::new();
    let mut gemini_tools = Vec::new();

    for tool in tools.into_iter().flatten() {
        match tool {
            openai::ResponseTool::Function {
                name,
                parameters,
                description,
                ..
            } => declarations.push(gemini::FunctionDeclaration {
                name,
                description: description.unwrap_or_default(),
                behavior: None,
                parameters: None,
                parameters_json_schema: Some(json_value(parameters)),
                response: None,
                response_json_schema: None,
                extra: Default::default(),
            }),
            openai::ResponseTool::Namespace { tools, .. } => {
                declarations.extend(tools.into_iter().filter_map(openai_namespace_function));
            }
            openai::ResponseTool::FileSearch {
                vector_store_ids,
                max_num_results,
                ..
            } => gemini_tools.push(gemini::Tool {
                file_search: Some(gemini::FileSearch {
                    file_search_store_names: vector_store_ids,
                    metadata_filter: None,
                    top_k: max_num_results.map(u32_to_i32),
                    extra: Default::default(),
                }),
                ..Default::default()
            }),
            openai::ResponseTool::WebSearch { .. }
            | openai::ResponseTool::WebSearch20250826 { .. }
            | openai::ResponseTool::WebSearchPreview { .. }
            | openai::ResponseTool::WebSearchPreview20250311 { .. } => {
                gemini_tools.push(gemini::Tool {
                    google_search: Some(gemini::GoogleSearch::default()),
                    ..Default::default()
                });
            }
            openai::ResponseTool::CodeInterpreter { .. } => gemini_tools.push(gemini::Tool {
                code_execution: Some(gemini::CodeExecution::default()),
                ..Default::default()
            }),
            openai::ResponseTool::Computer { .. }
            | openai::ResponseTool::ComputerUsePreview { .. } => gemini_tools.push(gemini::Tool {
                computer_use: Some(gemini::ComputerUse::default()),
                ..Default::default()
            }),
            openai::ResponseTool::Mcp {
                server_label,
                server_url,
                headers,
                ..
            } => gemini_tools.push(gemini::Tool {
                mcp_servers: vec![gemini::McpServer {
                    name: Some(server_label),
                    streamable_http_transport: server_url.map(|url| {
                        gemini::StreamableHttpTransport {
                            url: Some(url),
                            headers: headers.unwrap_or_default(),
                            timeout: None,
                            sse_read_timeout: None,
                            terminate_on_close: None,
                            extra: Default::default(),
                        }
                    }),
                    extra: Default::default(),
                }],
                ..Default::default()
            }),
            _ => {}
        }
    }

    if !declarations.is_empty() {
        gemini_tools.insert(
            0,
            gemini::Tool {
                function_declarations: declarations,
                ..Default::default()
            },
        );
    }

    gemini_tools
}

fn openai_namespace_function(
    tool: openai::ResponseNamespaceTool,
) -> Option<gemini::FunctionDeclaration> {
    match tool {
        openai::ResponseNamespaceTool::Function {
            name,
            description,
            parameters,
            ..
        } => Some(gemini::FunctionDeclaration {
            name,
            description: description.unwrap_or_default(),
            behavior: None,
            parameters: None,
            parameters_json_schema: parameters,
            response: None,
            response_json_schema: None,
            extra: Default::default(),
        }),
        _ => None,
    }
}

pub(in crate::transform::count_tokens) fn claude_tools_to_gemini(
    tools: Option<Vec<claude::Tool>>,
    mcp_servers: Option<Vec<claude::McpServer>>,
) -> Vec<gemini::Tool> {
    let mut declarations = Vec::new();
    let mut gemini_tools = Vec::new();

    for tool in tools.into_iter().flatten() {
        match tool {
            claude::Tool::Custom(tool) => declarations.push(gemini::FunctionDeclaration {
                name: tool.name,
                description: tool.description.unwrap_or_default(),
                behavior: None,
                parameters: None,
                parameters_json_schema: Some(json_value(tool.input_schema)),
                response: None,
                response_json_schema: None,
                extra: Default::default(),
            }),
            claude::Tool::WebSearch(_) => gemini_tools.push(gemini::Tool {
                google_search: Some(gemini::GoogleSearch::default()),
                ..Default::default()
            }),
            claude::Tool::WebFetch(_) => gemini_tools.push(gemini::Tool {
                url_context: Some(gemini::UrlContext::default()),
                ..Default::default()
            }),
            claude::Tool::Computer(_) => gemini_tools.push(gemini::Tool {
                computer_use: Some(gemini::ComputerUse::default()),
                ..Default::default()
            }),
            claude::Tool::Command(
                claude::CommandTool::CodeExecution20250522(_)
                | claude::CommandTool::CodeExecution20250825(_)
                | claude::CommandTool::CodeExecution20260120(_),
            ) => gemini_tools.push(gemini::Tool {
                code_execution: Some(gemini::CodeExecution::default()),
                ..Default::default()
            }),
            claude::Tool::Command(_) => {}
            claude::Tool::McpToolset(toolset) => gemini_tools.push(gemini::Tool {
                mcp_servers: vec![gemini::McpServer {
                    name: Some(toolset.mcp_server_name),
                    streamable_http_transport: None,
                    extra: Default::default(),
                }],
                ..Default::default()
            }),
            _ => {}
        }
    }

    let mcp_servers = mcp_servers
        .into_iter()
        .flatten()
        .map(|server| gemini::McpServer {
            name: Some(server.name),
            streamable_http_transport: Some(gemini::StreamableHttpTransport {
                url: Some(server.url),
                headers: Default::default(),
                timeout: None,
                sse_read_timeout: None,
                terminate_on_close: None,
                extra: Default::default(),
            }),
            extra: Default::default(),
        })
        .collect::<Vec<_>>();
    if !mcp_servers.is_empty() {
        gemini_tools.push(gemini::Tool {
            mcp_servers,
            ..Default::default()
        });
    }

    if !declarations.is_empty() {
        gemini_tools.insert(
            0,
            gemini::Tool {
                function_declarations: declarations,
                ..Default::default()
            },
        );
    }

    gemini_tools
}

pub(in crate::transform::count_tokens) fn openai_tool_config_to_gemini(
    tool_choice: Option<openai::ResponseToolChoice>,
) -> Option<gemini::ToolConfig> {
    let function_calling_config = match tool_choice? {
        openai::ResponseToolChoice::Mode(mode) => Some(gemini::FunctionCallingConfig {
            mode: Some(openai_tool_choice_mode_to_gemini(mode)),
            allowed_function_names: Vec::new(),
            extra: Default::default(),
        }),
        openai::ResponseToolChoice::Allowed(choice) => Some(gemini::FunctionCallingConfig {
            mode: Some(openai_allowed_tools_mode_to_gemini(choice.mode)),
            allowed_function_names: choice
                .tools
                .into_iter()
                .filter_map(openai_allowed_tool_name)
                .collect(),
            extra: Default::default(),
        }),
        openai::ResponseToolChoice::Function(choice) => Some(gemini::FunctionCallingConfig {
            mode: Some(gemini::FunctionCallingMode::Known(
                gemini::FunctionCallingModeKnown::Any,
            )),
            allowed_function_names: vec![choice.name],
            extra: Default::default(),
        }),
        _ => None,
    }?;

    Some(gemini::ToolConfig {
        function_calling_config: Some(function_calling_config),
        retrieval_config: None,
        include_server_side_tool_invocations: None,
        extra: Default::default(),
    })
}

fn openai_tool_choice_mode_to_gemini(mode: openai::ToolChoiceMode) -> gemini::FunctionCallingMode {
    match mode {
        openai::ToolChoiceMode::None => {
            gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::None)
        }
        openai::ToolChoiceMode::Auto => {
            gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Auto)
        }
        openai::ToolChoiceMode::Required => {
            gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Any)
        }
    }
}

fn openai_allowed_tools_mode_to_gemini(
    mode: openai::AllowedToolsMode,
) -> gemini::FunctionCallingMode {
    match mode {
        openai::AllowedToolsMode::Auto => {
            gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Auto)
        }
        openai::AllowedToolsMode::Required => {
            gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Any)
        }
    }
}

fn openai_allowed_tool_name(tool: openai::ResponseAllowedTool) -> Option<String> {
    match tool {
        openai::ResponseAllowedTool::Function { name, .. } => Some(name),
        _ => None,
    }
}

pub(in crate::transform::count_tokens) fn claude_tool_config_to_gemini(
    tool_choice: Option<claude::ToolChoice>,
) -> Option<gemini::ToolConfig> {
    let function_calling_config = match tool_choice? {
        claude::ToolChoice::Auto(_) => Some(gemini::FunctionCallingConfig {
            mode: Some(gemini::FunctionCallingMode::Known(
                gemini::FunctionCallingModeKnown::Auto,
            )),
            allowed_function_names: Vec::new(),
            extra: Default::default(),
        }),
        claude::ToolChoice::Any(_) => Some(gemini::FunctionCallingConfig {
            mode: Some(gemini::FunctionCallingMode::Known(
                gemini::FunctionCallingModeKnown::Any,
            )),
            allowed_function_names: Vec::new(),
            extra: Default::default(),
        }),
        claude::ToolChoice::Tool(choice) => Some(gemini::FunctionCallingConfig {
            mode: Some(gemini::FunctionCallingMode::Known(
                gemini::FunctionCallingModeKnown::Any,
            )),
            allowed_function_names: vec![choice.name],
            extra: Default::default(),
        }),
        claude::ToolChoice::None(_) => Some(gemini::FunctionCallingConfig {
            mode: Some(gemini::FunctionCallingMode::Known(
                gemini::FunctionCallingModeKnown::None,
            )),
            allowed_function_names: Vec::new(),
            extra: Default::default(),
        }),
        claude::ToolChoice::Unknown(_) => None,
    }?;

    Some(gemini::ToolConfig {
        function_calling_config: Some(function_calling_config),
        retrieval_config: None,
        include_server_side_tool_invocations: None,
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn openai_generation_config_to_gemini(
    reasoning: Option<openai::ReasoningConfig>,
    text: Option<openai::TextConfig>,
) -> Option<gemini::GenerationConfig> {
    let mut config = gemini::GenerationConfig::default();

    if let Some(reasoning) = reasoning {
        config.thinking_config = Some(gemini::ThinkingConfig {
            include_thoughts: None,
            thinking_budget: None,
            thinking_level: reasoning.effort.map(openai_reasoning_effort_to_gemini),
            extra: Default::default(),
        });
    }

    if let Some(text) = text.and_then(|text| text.format) {
        apply_openai_response_format(&mut config, text);
    }

    non_empty_generation_config(config)
}

fn openai_reasoning_effort_to_gemini(effort: openai::ReasoningEffort) -> gemini::ThinkingLevel {
    let level = match effort {
        openai::ReasoningEffort::None | openai::ReasoningEffort::Minimal => {
            gemini::ThinkingLevelKnown::Minimal
        }
        openai::ReasoningEffort::Low => gemini::ThinkingLevelKnown::Low,
        openai::ReasoningEffort::Medium => gemini::ThinkingLevelKnown::Medium,
        openai::ReasoningEffort::High | openai::ReasoningEffort::XHigh => {
            gemini::ThinkingLevelKnown::High
        }
    };
    gemini::ThinkingLevel::Known(level)
}

fn apply_openai_response_format(
    config: &mut gemini::GenerationConfig,
    format: openai::ResponseFormat,
) {
    match format {
        openai::ResponseFormat::Text(_) => {
            config.response_mime_type = Some(gemini::ResponseMimeType::Known(
                gemini::ResponseMimeTypeKnown::TextPlain,
            ));
        }
        openai::ResponseFormat::JsonObject(_) => {
            config.response_mime_type = Some(gemini::ResponseMimeType::Known(
                gemini::ResponseMimeTypeKnown::ApplicationJson,
            ));
        }
        openai::ResponseFormat::JsonSchema(format) => {
            config.response_mime_type = Some(gemini::ResponseMimeType::Known(
                gemini::ResponseMimeTypeKnown::ApplicationJson,
            ));
            config.response_json_schema = Some(json_value(format.schema));
        }
    }
}

pub(in crate::transform::count_tokens) fn claude_generation_config_to_gemini(
    output_config: Option<claude::OutputConfig>,
    output_format: Option<claude::JsonSchemaFormat>,
    thinking: Option<claude::ThinkingConfig>,
) -> Option<gemini::GenerationConfig> {
    let mut config = gemini::GenerationConfig::default();

    let output_format = output_config
        .as_ref()
        .and_then(|config| config.format.clone())
        .or(output_format);
    if let Some(format) = output_format {
        config.response_mime_type = Some(gemini::ResponseMimeType::Known(
            gemini::ResponseMimeTypeKnown::ApplicationJson,
        ));
        config.response_json_schema = Some(json_value(format.schema));
    }

    if let Some(task_budget) = output_config.and_then(|config| config.task_budget) {
        config.max_output_tokens = Some(u64_to_i32(task_budget.total));
    }

    if let Some(thinking) = thinking {
        config.thinking_config = Some(claude_thinking_to_gemini(thinking));
    }

    non_empty_generation_config(config)
}

fn claude_thinking_to_gemini(thinking: claude::ThinkingConfig) -> gemini::ThinkingConfig {
    match thinking {
        claude::ThinkingConfig::Enabled(config) => gemini::ThinkingConfig {
            include_thoughts: Some(true),
            thinking_budget: Some(u64_to_i32(config.budget_tokens)),
            thinking_level: None,
            extra: Default::default(),
        },
        claude::ThinkingConfig::Disabled(_) => gemini::ThinkingConfig {
            include_thoughts: Some(false),
            thinking_budget: None,
            thinking_level: None,
            extra: Default::default(),
        },
        claude::ThinkingConfig::Adaptive(_) => gemini::ThinkingConfig {
            include_thoughts: Some(true),
            thinking_budget: None,
            thinking_level: None,
            extra: Default::default(),
        },
        claude::ThinkingConfig::Unknown(_) => gemini::ThinkingConfig::default(),
    }
}

fn non_empty_generation_config(
    config: gemini::GenerationConfig,
) -> Option<gemini::GenerationConfig> {
    if config == gemini::GenerationConfig::default() {
        None
    } else {
        Some(config)
    }
}

fn json_value<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}
