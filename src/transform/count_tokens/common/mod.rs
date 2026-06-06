use std::collections::BTreeMap;

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

pub(in crate::transform::count_tokens) fn claude_speed_to_gemini_service_tier(
    speed: Option<claude::Speed>,
) -> Option<gemini::ServiceTier> {
    let tier = match speed? {
        claude::Speed::Known(claude::SpeedKnown::Standard) => gemini::ServiceTierKnown::Standard,
        claude::Speed::Known(claude::SpeedKnown::Fast) => gemini::ServiceTierKnown::Priority,
        claude::Speed::Unknown(_) => gemini::ServiceTierKnown::Standard,
    };
    Some(gemini::ServiceTier::Known(tier))
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

pub(in crate::transform::count_tokens) struct GeminiCountTokenParts {
    pub model: Option<String>,
    pub contents: Vec<gemini::Content>,
    pub system_instruction: Option<gemini::Content>,
    pub tools: Vec<gemini::Tool>,
    pub tool_config: Option<gemini::ToolConfig>,
    pub generation_config: Option<gemini::GenerationConfig>,
    pub service_tier: Option<gemini::ServiceTier>,
}

pub(in crate::transform::count_tokens) fn split_gemini_count_token_request(
    input: gemini::CountTokensRequest,
) -> GeminiCountTokenParts {
    let mut model = input.model;
    let mut contents = input.contents;
    let mut system_instruction = None;
    let mut tools = Vec::new();
    let mut tool_config = None;
    let mut generation_config = None;
    let mut service_tier = None;

    if let Some(request) = input.generate_content_request {
        if model.is_none() {
            model = request.model;
        }
        if contents.is_empty() {
            contents = request.contents;
        }
        system_instruction = request.system_instruction;
        tools = request.tools;
        tool_config = request.tool_config;
        generation_config = request.generation_config;
        service_tier = request.service_tier;
    }

    GeminiCountTokenParts {
        model,
        contents,
        system_instruction,
        tools,
        tool_config,
        generation_config,
        service_tier,
    }
}

pub(in crate::transform::count_tokens) fn claude_tools_to_openai(
    tools: Option<Vec<claude::Tool>>,
    mcp_servers: Option<Vec<claude::McpServer>>,
) -> Option<Vec<openai::ResponseTool>> {
    let mut output = Vec::new();

    for tool in tools.into_iter().flatten() {
        match tool {
            claude::Tool::Custom(tool) => output.push(openai::ResponseTool::Function {
                name: tool.name,
                parameters: json_object(json_value(tool.input_schema)),
                strict: tool.common.strict.unwrap_or_default(),
                defer_loading: tool.common.defer_loading,
                description: tool.description,
                extra: Default::default(),
            }),
            claude::Tool::WebSearch(_) => output.push(openai::ResponseTool::WebSearchPreview {
                search_content_types: None,
                search_context_size: None,
                user_location: None,
                extra: Default::default(),
            }),
            claude::Tool::WebFetch(_) => output.push(openai::ResponseTool::WebSearch {
                filters: None,
                search_context_size: None,
                user_location: None,
                extra: Default::default(),
            }),
            claude::Tool::Computer(_) => output.push(openai::ResponseTool::Computer {
                extra: Default::default(),
            }),
            claude::Tool::Command(
                claude::CommandTool::CodeExecution20250522(_)
                | claude::CommandTool::CodeExecution20250825(_)
                | claude::CommandTool::CodeExecution20260120(_),
            ) => output.push(openai::ResponseTool::CodeInterpreter {
                container: openai::CodeInterpreterContainer::Auto(
                    openai::CodeInterpreterAutoContainer {
                        type_: openai::CodeInterpreterContainerType::Auto,
                        file_ids: None,
                        memory_limit: None,
                        network_policy: None,
                        extra: Default::default(),
                    },
                ),
                extra: Default::default(),
            }),
            claude::Tool::Command(_) => {}
            claude::Tool::McpToolset(toolset) => output.push(openai::ResponseTool::Mcp {
                server_label: toolset.mcp_server_name,
                allowed_tools: None,
                authorization: None,
                connector_id: None,
                defer_loading: None,
                headers: None,
                require_approval: None,
                server_description: None,
                server_url: None,
                extra: Default::default(),
            }),
            _ => {}
        }
    }

    output.extend(mcp_servers.into_iter().flatten().map(|server| {
        openai::ResponseTool::Mcp {
            server_label: server.name,
            allowed_tools: server
                .tool_configuration
                .and_then(|config| config.allowed_tools)
                .map(openai::McpAllowedTools::Names),
            authorization: server.authorization_token,
            connector_id: None,
            defer_loading: None,
            headers: None,
            require_approval: None,
            server_description: None,
            server_url: Some(server.url),
            extra: Default::default(),
        }
    }));

    non_empty_vec(output)
}

pub(in crate::transform::count_tokens) fn gemini_tools_to_openai(
    tools: Vec<gemini::Tool>,
) -> Option<Vec<openai::ResponseTool>> {
    let mut output = Vec::new();

    for tool in tools {
        output.extend(tool.function_declarations.into_iter().map(|function| {
            openai::ResponseTool::Function {
                name: function.name,
                parameters: function
                    .parameters_json_schema
                    .or_else(|| function.parameters.map(json_value))
                    .map(json_object)
                    .unwrap_or_default(),
                strict: false,
                defer_loading: None,
                description: empty_string_to_none(function.description),
                extra: Default::default(),
            }
        }));

        if let Some(file_search) = tool.file_search {
            output.push(openai::ResponseTool::FileSearch {
                vector_store_ids: file_search.file_search_store_names,
                filters: None,
                max_num_results: file_search.top_k.map(i32_to_u32),
                ranking_options: None,
                extra: Default::default(),
            });
        }
        if tool.google_search.is_some() || tool.google_search_retrieval.is_some() {
            output.push(openai::ResponseTool::WebSearchPreview {
                search_content_types: None,
                search_context_size: None,
                user_location: None,
                extra: Default::default(),
            });
        }
        if tool.code_execution.is_some() {
            output.push(openai::ResponseTool::CodeInterpreter {
                container: openai::CodeInterpreterContainer::Auto(
                    openai::CodeInterpreterAutoContainer {
                        type_: openai::CodeInterpreterContainerType::Auto,
                        file_ids: None,
                        memory_limit: None,
                        network_policy: None,
                        extra: Default::default(),
                    },
                ),
                extra: Default::default(),
            });
        }
        if tool.computer_use.is_some() {
            output.push(openai::ResponseTool::Computer {
                extra: Default::default(),
            });
        }
        output.extend(tool.mcp_servers.into_iter().map(|server| {
            let transport = server.streamable_http_transport;
            openai::ResponseTool::Mcp {
                server_label: server.name.unwrap_or_default(),
                allowed_tools: None,
                authorization: None,
                connector_id: None,
                defer_loading: None,
                headers: transport
                    .as_ref()
                    .map(|transport| transport.headers.clone()),
                require_approval: None,
                server_description: None,
                server_url: transport.and_then(|transport| transport.url),
                extra: Default::default(),
            }
        }));
    }

    non_empty_vec(output)
}

pub(in crate::transform::count_tokens) struct ClaudeToolParts {
    pub tools: Option<Vec<claude::Tool>>,
    pub mcp_servers: Option<Vec<claude::McpServer>>,
}

pub(in crate::transform::count_tokens) fn gemini_tools_to_claude(
    tools: Vec<gemini::Tool>,
) -> ClaudeToolParts {
    let mut output_tools = Vec::new();
    let mut mcp_servers = Vec::new();

    for tool in tools {
        output_tools.extend(tool.function_declarations.into_iter().map(|function| {
            claude::Tool::Custom(claude::CustomTool {
                input_schema: function
                    .parameters_json_schema
                    .or_else(|| function.parameters.map(json_value))
                    .map(json_object)
                    .map(claude_json_schema)
                    .unwrap_or_else(|| claude_json_schema(Default::default())),
                name: function.name,
                type_: Some(claude::CustomToolType::Custom),
                description: empty_string_to_none(function.description),
                eager_input_streaming: None,
                common: Default::default(),
            })
        }));

        if tool.google_search.is_some() || tool.google_search_retrieval.is_some() {
            output_tools.push(claude::Tool::WebSearch(
                claude::WebSearchTool::WebSearch20260209(claude::WebSearchTool20260209 {
                    name: claude::WebSearchToolName::WebSearch,
                    type_: claude::WebSearchTool20260209Type::WebSearch20260209,
                    params: claude::WebSearchToolParams {
                        allowed_domains: None,
                        blocked_domains: None,
                        max_uses: None,
                        user_location: None,
                    },
                    common: Default::default(),
                }),
            ));
        }

        if tool.url_context.is_some() {
            output_tools.push(claude::Tool::WebFetch(
                claude::WebFetchTool::WebFetch20260309(claude::WebFetchTool20260309 {
                    name: claude::WebFetchToolName::WebFetch,
                    type_: claude::WebFetchTool20260309Type::WebFetch20260309,
                    params: claude::WebFetchToolParams {
                        allowed_domains: None,
                        blocked_domains: None,
                        citations: None,
                        max_content_tokens: None,
                        max_uses: None,
                    },
                    use_cache: None,
                    common: Default::default(),
                }),
            ));
        }

        if tool.code_execution.is_some() {
            output_tools.push(claude::Tool::Command(
                claude::CommandTool::CodeExecution20260120(claude::CodeExecutionTool20260120 {
                    name: claude::CodeExecutionToolName::CodeExecution,
                    type_: claude::CodeExecutionTool20260120Type::CodeExecution20260120,
                    common: Default::default(),
                }),
            ));
        }

        for server in tool.mcp_servers {
            if let Some(server) = gemini_mcp_server_to_claude_server(server.clone()) {
                mcp_servers.push(server);
            } else if let Some(toolset) = gemini_mcp_server_to_claude_toolset(server) {
                output_tools.push(claude::Tool::McpToolset(toolset));
            }
        }
    }

    ClaudeToolParts {
        tools: non_empty_vec(output_tools),
        mcp_servers: non_empty_vec(mcp_servers),
    }
}

fn gemini_mcp_server_to_claude_server(server: gemini::McpServer) -> Option<claude::McpServer> {
    let transport = server.streamable_http_transport?;
    Some(claude::McpServer {
        name: server.name.unwrap_or_default(),
        type_: claude::McpServerType::Known(claude::McpServerTypeKnown::Url),
        url: transport.url?,
        authorization_token: None,
        tool_configuration: None,
        extra: Default::default(),
    })
}

fn gemini_mcp_server_to_claude_toolset(server: gemini::McpServer) -> Option<claude::McpToolset> {
    Some(claude::McpToolset {
        mcp_server_name: server.name?,
        type_: claude::McpToolsetType::McpToolset,
        cache_control: None,
        configs: Default::default(),
        default_config: None,
    })
}

pub(in crate::transform::count_tokens) fn openai_tools_to_claude(
    tools: Option<Vec<openai::ResponseTool>>,
) -> Option<Vec<claude::Tool>> {
    let mut output = Vec::new();

    for tool in tools.into_iter().flatten() {
        match tool {
            openai::ResponseTool::Function {
                name,
                parameters,
                strict,
                defer_loading,
                description,
                ..
            } => output.push(claude::Tool::Custom(claude::CustomTool {
                input_schema: claude_json_schema(parameters),
                name,
                type_: Some(claude::CustomToolType::Custom),
                description,
                eager_input_streaming: None,
                common: claude::ToolCommon {
                    defer_loading,
                    strict: Some(strict),
                    ..Default::default()
                },
            })),
            openai::ResponseTool::Namespace { tools, .. } => {
                output.extend(
                    tools
                        .into_iter()
                        .filter_map(openai_namespace_tool_to_claude),
                );
            }
            _ => {}
        }
    }

    non_empty_vec(output)
}

pub(in crate::transform::count_tokens) fn openai_mcp_servers_to_claude(
    tools: Option<&[openai::ResponseTool]>,
) -> Option<Vec<claude::McpServer>> {
    let output = tools
        .into_iter()
        .flatten()
        .filter_map(|tool| match tool {
            openai::ResponseTool::Mcp {
                server_label,
                allowed_tools,
                authorization,
                server_url: Some(server_url),
                ..
            } => Some(claude::McpServer {
                name: server_label.clone(),
                type_: claude::McpServerType::Known(claude::McpServerTypeKnown::Url),
                url: server_url.clone(),
                authorization_token: authorization.clone(),
                tool_configuration: allowed_tools
                    .as_ref()
                    .and_then(openai_mcp_allowed_tools_to_claude),
                extra: Default::default(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    non_empty_vec(output)
}

fn openai_mcp_allowed_tools_to_claude(
    allowed_tools: &openai::McpAllowedTools,
) -> Option<claude::McpToolConfiguration> {
    let openai::McpAllowedTools::Names(names) = allowed_tools else {
        return None;
    };
    Some(claude::McpToolConfiguration {
        allowed_tools: Some(names.clone()),
        enabled: None,
        extra: Default::default(),
    })
}

fn openai_namespace_tool_to_claude(tool: openai::ResponseNamespaceTool) -> Option<claude::Tool> {
    match tool {
        openai::ResponseNamespaceTool::Function {
            name,
            description,
            parameters,
            strict,
            defer_loading,
            ..
        } => Some(claude::Tool::Custom(claude::CustomTool {
            input_schema: parameters
                .map(json_object)
                .map(claude_json_schema)
                .unwrap_or_else(|| claude_json_schema(Default::default())),
            name,
            type_: Some(claude::CustomToolType::Custom),
            description,
            eager_input_streaming: None,
            common: claude::ToolCommon {
                defer_loading,
                strict,
                ..Default::default()
            },
        })),
        _ => None,
    }
}

pub(in crate::transform::count_tokens) fn claude_tool_choice_to_openai(
    tool_choice: Option<claude::ToolChoice>,
) -> Option<openai::ResponseToolChoice> {
    match tool_choice? {
        claude::ToolChoice::Auto(_) => Some(openai::ResponseToolChoice::Mode(
            openai::ToolChoiceMode::Auto,
        )),
        claude::ToolChoice::Any(_) => Some(openai::ResponseToolChoice::Mode(
            openai::ToolChoiceMode::Required,
        )),
        claude::ToolChoice::Tool(choice) => Some(openai::ResponseToolChoice::Function(
            openai::ResponseFunctionToolChoice {
                type_: openai::FunctionToolChoiceType::Function,
                name: choice.name,
                extra: Default::default(),
            },
        )),
        claude::ToolChoice::None(_) => Some(openai::ResponseToolChoice::Mode(
            openai::ToolChoiceMode::None,
        )),
        claude::ToolChoice::Unknown(_) => None,
    }
}

pub(in crate::transform::count_tokens) fn gemini_tool_config_to_openai(
    tool_config: Option<gemini::ToolConfig>,
) -> Option<openai::ResponseToolChoice> {
    let config = tool_config?.function_calling_config?;
    let names = config.allowed_function_names;
    match config.mode? {
        gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::None) => Some(
            openai::ResponseToolChoice::Mode(openai::ToolChoiceMode::None),
        ),
        gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Auto) => {
            if names.is_empty() {
                Some(openai::ResponseToolChoice::Mode(
                    openai::ToolChoiceMode::Auto,
                ))
            } else {
                Some(openai::ResponseToolChoice::Allowed(
                    openai_allowed_function_choice(openai::AllowedToolsMode::Auto, names),
                ))
            }
        }
        gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Any)
        | gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Validated) => {
            if names.len() == 1 {
                Some(openai::ResponseToolChoice::Function(
                    openai::ResponseFunctionToolChoice {
                        type_: openai::FunctionToolChoiceType::Function,
                        name: names.into_iter().next().unwrap_or_default(),
                        extra: Default::default(),
                    },
                ))
            } else if names.is_empty() {
                Some(openai::ResponseToolChoice::Mode(
                    openai::ToolChoiceMode::Required,
                ))
            } else {
                Some(openai::ResponseToolChoice::Allowed(
                    openai_allowed_function_choice(openai::AllowedToolsMode::Required, names),
                ))
            }
        }
        gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::ModeUnspecified)
        | gemini::FunctionCallingMode::Unknown(_) => None,
    }
}

pub(in crate::transform::count_tokens) fn gemini_tool_config_to_claude(
    tool_config: Option<gemini::ToolConfig>,
) -> Option<claude::ToolChoice> {
    let config = tool_config?.function_calling_config?;
    let names = config.allowed_function_names;
    match config.mode? {
        gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::None) => {
            Some(claude::ToolChoice::None(claude::ToolChoiceNone {
                type_: claude::ToolChoiceNoneType::None,
                extra: Default::default(),
            }))
        }
        gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Auto) => {
            Some(claude::ToolChoice::Auto(claude::ToolChoiceAuto {
                type_: claude::ToolChoiceAutoType::Auto,
                disable_parallel_tool_use: None,
                extra: Default::default(),
            }))
        }
        gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Any)
        | gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::Validated) => {
            if names.len() == 1 {
                Some(claude::ToolChoice::Tool(claude::ToolChoiceTool {
                    name: names.into_iter().next().unwrap_or_default(),
                    type_: claude::ToolChoiceToolType::Tool,
                    disable_parallel_tool_use: None,
                    extra: Default::default(),
                }))
            } else {
                Some(claude::ToolChoice::Any(claude::ToolChoiceAny {
                    type_: claude::ToolChoiceAnyType::Any,
                    disable_parallel_tool_use: None,
                    extra: Default::default(),
                }))
            }
        }
        gemini::FunctionCallingMode::Known(gemini::FunctionCallingModeKnown::ModeUnspecified)
        | gemini::FunctionCallingMode::Unknown(_) => None,
    }
}

fn openai_allowed_function_choice(
    mode: openai::AllowedToolsMode,
    names: Vec<String>,
) -> openai::ResponseAllowedToolChoice {
    openai::ResponseAllowedToolChoice {
        mode,
        tools: names
            .into_iter()
            .map(|name| openai::ResponseAllowedTool::Function {
                name,
                extra: Default::default(),
            })
            .collect(),
        type_: openai::AllowedToolsType::AllowedTools,
        extra: Default::default(),
    }
}

pub(in crate::transform::count_tokens) fn openai_tool_choice_to_claude(
    tool_choice: Option<openai::ResponseToolChoice>,
    parallel_tool_calls: Option<bool>,
) -> Option<claude::ToolChoice> {
    let disable_parallel_tool_use = parallel_tool_calls.map(|value| !value);
    match tool_choice {
        Some(openai::ResponseToolChoice::Mode(openai::ToolChoiceMode::None)) => {
            Some(claude::ToolChoice::None(claude::ToolChoiceNone {
                type_: claude::ToolChoiceNoneType::None,
                extra: Default::default(),
            }))
        }
        Some(openai::ResponseToolChoice::Mode(openai::ToolChoiceMode::Auto)) | None => {
            disable_parallel_tool_use.map(|disable_parallel_tool_use| {
                claude::ToolChoice::Auto(claude::ToolChoiceAuto {
                    type_: claude::ToolChoiceAutoType::Auto,
                    disable_parallel_tool_use: Some(disable_parallel_tool_use),
                    extra: Default::default(),
                })
            })
        }
        Some(openai::ResponseToolChoice::Mode(openai::ToolChoiceMode::Required)) => {
            Some(claude::ToolChoice::Any(claude::ToolChoiceAny {
                type_: claude::ToolChoiceAnyType::Any,
                disable_parallel_tool_use,
                extra: Default::default(),
            }))
        }
        Some(openai::ResponseToolChoice::Function(choice)) => {
            Some(claude::ToolChoice::Tool(claude::ToolChoiceTool {
                name: choice.name,
                type_: claude::ToolChoiceToolType::Tool,
                disable_parallel_tool_use,
                extra: Default::default(),
            }))
        }
        Some(openai::ResponseToolChoice::Allowed(choice)) => {
            let names = choice
                .tools
                .into_iter()
                .filter_map(openai_allowed_tool_name)
                .collect::<Vec<_>>();
            if names.len() == 1 {
                Some(claude::ToolChoice::Tool(claude::ToolChoiceTool {
                    name: names.into_iter().next().unwrap_or_default(),
                    type_: claude::ToolChoiceToolType::Tool,
                    disable_parallel_tool_use,
                    extra: Default::default(),
                }))
            } else {
                Some(claude::ToolChoice::Any(claude::ToolChoiceAny {
                    type_: claude::ToolChoiceAnyType::Any,
                    disable_parallel_tool_use,
                    extra: Default::default(),
                }))
            }
        }
        _ => None,
    }
}

pub(in crate::transform::count_tokens) fn claude_parallel_tool_calls(
    tool_choice: Option<&claude::ToolChoice>,
) -> Option<bool> {
    match tool_choice? {
        claude::ToolChoice::Auto(choice) => choice.disable_parallel_tool_use.map(|value| !value),
        claude::ToolChoice::Any(choice) => choice.disable_parallel_tool_use.map(|value| !value),
        claude::ToolChoice::Tool(choice) => choice.disable_parallel_tool_use.map(|value| !value),
        claude::ToolChoice::None(_) | claude::ToolChoice::Unknown(_) => None,
    }
}

pub(in crate::transform::count_tokens) fn claude_generation_to_openai_reasoning(
    thinking: Option<claude::ThinkingConfig>,
    output_config: Option<&claude::OutputConfig>,
) -> Option<openai::ReasoningConfig> {
    let effort = output_config
        .and_then(|config| config.effort.clone())
        .map(claude_output_effort_to_openai)
        .or_else(|| thinking.map(claude_thinking_to_openai_effort));

    effort.map(|effort| openai::ReasoningConfig {
        effort: Some(effort),
        summary: None,
        generate_summary: None,
        extra: Default::default(),
    })
}

fn claude_output_effort_to_openai(effort: claude::OutputEffort) -> openai::ReasoningEffort {
    match effort {
        claude::OutputEffort::Known(claude::OutputEffortKnown::Low) => openai::ReasoningEffort::Low,
        claude::OutputEffort::Known(claude::OutputEffortKnown::Medium) => {
            openai::ReasoningEffort::Medium
        }
        claude::OutputEffort::Known(claude::OutputEffortKnown::High) => {
            openai::ReasoningEffort::High
        }
        claude::OutputEffort::Known(claude::OutputEffortKnown::XHigh)
        | claude::OutputEffort::Known(claude::OutputEffortKnown::Max)
        | claude::OutputEffort::Unknown(_) => openai::ReasoningEffort::XHigh,
    }
}

fn claude_thinking_to_openai_effort(thinking: claude::ThinkingConfig) -> openai::ReasoningEffort {
    match thinking {
        claude::ThinkingConfig::Disabled(_) => openai::ReasoningEffort::None,
        claude::ThinkingConfig::Enabled(_) => openai::ReasoningEffort::Medium,
        claude::ThinkingConfig::Adaptive(_) => openai::ReasoningEffort::Medium,
        claude::ThinkingConfig::Unknown(_) => openai::ReasoningEffort::Medium,
    }
}

pub(in crate::transform::count_tokens) fn claude_previous_message_id_to_openai(
    diagnostics: Option<claude::DiagnosticsParam>,
) -> Option<String> {
    diagnostics?.previous_message_id?
}

pub(in crate::transform::count_tokens) fn openai_previous_response_id_to_claude(
    previous_response_id: Option<String>,
) -> Option<claude::DiagnosticsParam> {
    Some(claude::DiagnosticsParam {
        previous_message_id: Some(Some(previous_response_id?)),
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_openai_reasoning(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<openai::ReasoningConfig> {
    let thinking = generation_config?.thinking_config.as_ref()?;
    let effort = if thinking.include_thoughts == Some(false) {
        openai::ReasoningEffort::None
    } else {
        thinking
            .thinking_level
            .clone()
            .map(gemini_thinking_level_to_openai)
            .unwrap_or(openai::ReasoningEffort::Medium)
    };
    Some(openai::ReasoningConfig {
        effort: Some(effort),
        summary: None,
        generate_summary: None,
        extra: Default::default(),
    })
}

fn gemini_thinking_level_to_openai(level: gemini::ThinkingLevel) -> openai::ReasoningEffort {
    match level {
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Minimal) => {
            openai::ReasoningEffort::Minimal
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Low) => {
            openai::ReasoningEffort::Low
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Medium)
        | gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::ThinkingLevelUnspecified) => {
            openai::ReasoningEffort::Medium
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::High)
        | gemini::ThinkingLevel::Unknown(_) => openai::ReasoningEffort::High,
    }
}

pub(in crate::transform::count_tokens) fn claude_generation_to_openai_text(
    output_config: Option<&claude::OutputConfig>,
    output_format: Option<claude::JsonSchemaFormat>,
) -> Option<openai::TextConfig> {
    let format = output_config
        .and_then(|config| config.format.clone())
        .or(output_format)?;
    Some(openai::TextConfig {
        format: Some(openai::ResponseFormat::JsonSchema(
            openai::JsonSchemaResponseFormat {
                type_: openai::JsonSchemaResponseFormatType::JsonSchema,
                name: "response".to_owned(),
                schema: json_object(json_value(format.schema)),
                description: None,
                strict: None,
                extra: Default::default(),
            },
        )),
        verbosity: None,
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_openai_text(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<openai::TextConfig> {
    let config = generation_config?;
    let format = if let Some(schema) = config
        .response_json_schema
        .clone()
        .or_else(|| config.response_schema.clone().map(json_value))
    {
        openai::ResponseFormat::JsonSchema(openai::JsonSchemaResponseFormat {
            type_: openai::JsonSchemaResponseFormatType::JsonSchema,
            name: "response".to_owned(),
            schema: json_object(schema),
            description: None,
            strict: None,
            extra: Default::default(),
        })
    } else if matches!(
        config.response_mime_type,
        Some(gemini::ResponseMimeType::Known(
            gemini::ResponseMimeTypeKnown::ApplicationJson
        ))
    ) {
        openai::ResponseFormat::JsonObject(openai::JsonObjectResponseFormat {
            type_: openai::JsonObjectResponseFormatType::JsonObject,
            extra: Default::default(),
        })
    } else if matches!(
        config.response_mime_type,
        Some(gemini::ResponseMimeType::Known(
            gemini::ResponseMimeTypeKnown::TextPlain
        ))
    ) {
        openai::ResponseFormat::Text(openai::TextResponseFormat {
            type_: openai::TextResponseFormatType::Text,
            extra: Default::default(),
        })
    } else {
        return None;
    };

    Some(openai::TextConfig {
        format: Some(format),
        verbosity: None,
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_claude_output_config(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<claude::OutputConfig> {
    let config = generation_config?;
    let effort = config
        .thinking_config
        .as_ref()
        .and_then(gemini_thinking_to_claude_output_effort);
    let format = gemini_generation_to_claude_output_format(Some(config));
    let task_budget = config
        .max_output_tokens
        .map(|total| claude::TokenTaskBudget {
            total: i32_to_u64(total),
            type_: claude::TaskBudgetType::Known(claude::TaskBudgetTypeKnown::Tokens),
            remaining: None,
            extra: Default::default(),
        });

    if effort.is_none() && format.is_none() && task_budget.is_none() {
        return None;
    }

    Some(claude::OutputConfig {
        effort,
        format,
        task_budget,
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_claude_output_format(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<claude::JsonSchemaFormat> {
    let config = generation_config?;
    let schema = config
        .response_json_schema
        .clone()
        .or_else(|| config.private_response_json_schema.clone())
        .or_else(|| {
            config
                .response_format
                .as_ref()
                .and_then(|format| format.text.as_ref())
                .and_then(|format| format.schema.clone())
        })
        .or_else(|| config.response_schema.clone().map(json_value))?;

    Some(claude::JsonSchemaFormat {
        type_: claude::JsonSchemaFormatType::Known(claude::JsonSchemaFormatTypeKnown::JsonSchema),
        schema: json_object(schema),
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_claude_thinking(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<claude::ThinkingConfig> {
    let thinking = generation_config?.thinking_config.as_ref()?;
    if thinking.include_thoughts == Some(false) {
        return Some(claude::ThinkingConfig::Disabled(claude::ThinkingDisabled {
            type_: claude::ThinkingDisabledType::Disabled,
            extra: Default::default(),
        }));
    }
    if let Some(budget) = thinking.thinking_budget {
        return Some(claude::ThinkingConfig::Enabled(claude::ThinkingEnabled {
            budget_tokens: i32_to_u64(budget),
            type_: claude::ThinkingEnabledType::Enabled,
            display: None,
            extra: Default::default(),
        }));
    }
    Some(claude::ThinkingConfig::Adaptive(claude::ThinkingAdaptive {
        type_: claude::ThinkingAdaptiveType::Adaptive,
        display: None,
        extra: Default::default(),
    }))
}

fn gemini_thinking_to_claude_output_effort(
    thinking: &gemini::ThinkingConfig,
) -> Option<claude::OutputEffort> {
    let effort = match thinking.thinking_level.as_ref()? {
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Minimal)
        | gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Low) => {
            claude::OutputEffortKnown::Low
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Medium)
        | gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::ThinkingLevelUnspecified) => {
            claude::OutputEffortKnown::Medium
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::High)
        | gemini::ThinkingLevel::Unknown(_) => claude::OutputEffortKnown::High,
    };
    Some(claude::OutputEffort::Known(effort))
}

pub(in crate::transform::count_tokens) fn gemini_service_tier_to_claude_speed(
    service_tier: Option<gemini::ServiceTier>,
) -> Option<claude::Speed> {
    let speed = match service_tier? {
        gemini::ServiceTier::Known(gemini::ServiceTierKnown::Priority)
        | gemini::ServiceTier::Known(gemini::ServiceTierKnown::Flex) => claude::SpeedKnown::Fast,
        gemini::ServiceTier::Known(gemini::ServiceTierKnown::Standard)
        | gemini::ServiceTier::Known(gemini::ServiceTierKnown::Unspecified)
        | gemini::ServiceTier::Unknown(_) => claude::SpeedKnown::Standard,
    };
    Some(claude::Speed::Known(speed))
}

pub(in crate::transform::count_tokens) fn openai_reasoning_to_claude(
    reasoning: Option<openai::ReasoningConfig>,
) -> Option<claude::ThinkingConfig> {
    match reasoning?.effort? {
        openai::ReasoningEffort::None => {
            Some(claude::ThinkingConfig::Disabled(claude::ThinkingDisabled {
                type_: claude::ThinkingDisabledType::Disabled,
                extra: Default::default(),
            }))
        }
        _ => Some(claude::ThinkingConfig::Adaptive(claude::ThinkingAdaptive {
            type_: claude::ThinkingAdaptiveType::Adaptive,
            display: None,
            extra: Default::default(),
        })),
    }
}

pub(in crate::transform::count_tokens) fn openai_text_to_claude_output_format(
    text: Option<openai::TextConfig>,
) -> Option<claude::JsonSchemaFormat> {
    openai_response_format_to_claude(&text?.format?)
}

pub(in crate::transform::count_tokens) fn openai_generation_to_claude_output_config(
    reasoning: Option<&openai::ReasoningConfig>,
    text: Option<&openai::TextConfig>,
) -> Option<claude::OutputConfig> {
    let effort = reasoning
        .and_then(|reasoning| reasoning.effort.as_ref())
        .map(openai_reasoning_effort_to_claude_output);
    let format = text
        .and_then(|text| text.format.as_ref())
        .and_then(openai_response_format_to_claude);

    if effort.is_none() && format.is_none() {
        return None;
    }

    Some(claude::OutputConfig {
        effort,
        format,
        task_budget: None,
        extra: Default::default(),
    })
}

fn openai_reasoning_effort_to_claude_output(
    effort: &openai::ReasoningEffort,
) -> claude::OutputEffort {
    let effort = match effort {
        openai::ReasoningEffort::None
        | openai::ReasoningEffort::Minimal
        | openai::ReasoningEffort::Low => claude::OutputEffortKnown::Low,
        openai::ReasoningEffort::Medium => claude::OutputEffortKnown::Medium,
        openai::ReasoningEffort::High => claude::OutputEffortKnown::High,
        openai::ReasoningEffort::XHigh => claude::OutputEffortKnown::XHigh,
    };
    claude::OutputEffort::Known(effort)
}

fn openai_response_format_to_claude(
    format: &openai::ResponseFormat,
) -> Option<claude::JsonSchemaFormat> {
    let openai::ResponseFormat::JsonSchema(format) = format else {
        return None;
    };
    Some(claude::JsonSchemaFormat {
        type_: claude::JsonSchemaFormatType::Known(claude::JsonSchemaFormatTypeKnown::JsonSchema),
        schema: format.schema.clone(),
        extra: Default::default(),
    })
}

fn json_object(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

fn claude_json_schema(schema: BTreeMap<String, Value>) -> claude::JsonSchema {
    let mut properties = BTreeMap::new();
    let mut required = Vec::new();
    let mut extra = schema;

    if let Some(Value::Object(values)) = extra.remove("properties") {
        properties = values.into_iter().collect();
    }
    if let Some(Value::Array(values)) = extra.remove("required") {
        required = values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect();
    }

    claude::JsonSchema {
        type_: claude::JsonSchemaObjectType::Known(claude::JsonSchemaObjectTypeKnown::Object),
        properties,
        required,
        extra,
    }
}

fn empty_string_to_none(value: String) -> Option<String> {
    if value.is_empty() { None } else { Some(value) }
}

fn non_empty_vec<T>(value: Vec<T>) -> Option<Vec<T>> {
    if value.is_empty() { None } else { Some(value) }
}
