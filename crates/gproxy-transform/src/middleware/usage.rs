use serde::{Deserialize, Serialize};

use gproxy_protocol::claude::count_tokens::request::{
    CountTokensHeaders as ClaudeCountTokensHeaders, CountTokensRequest as ClaudeCountTokensRequest,
    CountTokensRequestBody as ClaudeCountTokensRequestBody,
};
use gproxy_protocol::claude::count_tokens::types::{
    BetaMessageContent as ClaudeMessageContent, BetaMessageParam as ClaudeMessageParam,
    BetaMessageRole as ClaudeMessageRole, Model as ClaudeModel,
};
use gproxy_protocol::claude::create_message::request::CreateMessageRequest as ClaudeCreateMessageRequest;
use gproxy_protocol::claude::create_message::response::CreateMessageResponse as ClaudeCreateMessageResponse;
use gproxy_protocol::claude::create_message::stream::{
    BetaStreamContentBlockDelta, BetaStreamEvent, BetaStreamEventKnown, BetaStreamUsage,
};

use gproxy_protocol::gemini::count_tokens::request::{
    CountTokensPath as GeminiCountTokensPath, CountTokensRequest as GeminiCountTokensRequest,
    CountTokensRequestBody as GeminiCountTokensRequestBody,
};
use gproxy_protocol::gemini::count_tokens::types::{
    Content as GeminiContent, ContentRole as GeminiContentRole, Part as GeminiPart,
};
use gproxy_protocol::gemini::generate_content::request::GenerateContentRequest as GeminiGenerateContentRequest;
use gproxy_protocol::gemini::generate_content::response::GenerateContentResponse as GeminiGenerateContentResponse;
use gproxy_protocol::gemini::stream_content::response::StreamGenerateContentResponse;

use gproxy_protocol::openai::count_tokens::request::{
    InputTokenCountRequest as OpenAIInputTokenCountRequest,
    InputTokenCountRequestBody as OpenAIInputTokenCountRequestBody,
};
use gproxy_protocol::openai::create_chat_completions::request::CreateChatCompletionRequest as OpenAIChatCompletionRequest;
use gproxy_protocol::openai::create_chat_completions::response::CreateChatCompletionResponse as OpenAIChatCompletionResponse;
use gproxy_protocol::openai::create_chat_completions::types::CompletionUsage;
use gproxy_protocol::openai::create_response::request::CreateResponseRequest as OpenAIResponseRequest;
use gproxy_protocol::openai::create_response::response::Response as OpenAIResponse;
use gproxy_protocol::openai::create_response::stream::{
    ResponseCompletedEvent, ResponseCreatedEvent, ResponseCustomToolCallInputDeltaEvent,
    ResponseFailedEvent, ResponseFunctionCallArgumentsDeltaEvent, ResponseInProgressEvent,
    ResponseIncompleteEvent, ResponseMCPCallArgumentsDeltaEvent, ResponseRefusalDeltaEvent,
    ResponseStreamEvent, ResponseTextDeltaEvent,
};
use gproxy_protocol::openai::create_response::types::{
    InputParam, OutputItem, OutputMessage, OutputMessageContent, ResponseUsage,
};

use super::types::{
    CountTokensRequest, CountTokensResponse, GenerateContentRequest, GenerateContentResponse,
    Proto, StreamEvent,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageSummary {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
    pub cache_creation_input_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct UsageAccumulator {
    proto: Proto,
    latest: UsageSummary,
    seen: bool,
}

impl UsageAccumulator {
    pub fn new(proto: Proto) -> Self {
        Self {
            proto,
            latest: UsageSummary::default(),
            seen: false,
        }
    }

    pub fn push(&mut self, event: &StreamEvent) -> Option<UsageSummary> {
        let incoming = match (self.proto, event) {
            (Proto::Claude, StreamEvent::Claude(event)) => usage_from_claude_stream(event),
            (Proto::OpenAIChat, StreamEvent::OpenAIChat(event)) => {
                event.usage.as_ref().map(usage_from_openai_chat_usage)
            }
            (Proto::OpenAIResponse, StreamEvent::OpenAIResponse(event)) => {
                usage_from_openai_response_stream(event)
            }
            (Proto::Gemini, StreamEvent::Gemini(event)) => {
                event.usage_metadata.as_ref().map(usage_from_gemini_usage)
            }
            _ => None,
        };

        if let Some(incoming) = incoming {
            merge_usage(&mut self.latest, incoming);
            self.seen = true;
            return Some(self.latest.clone());
        }
        None
    }

    pub fn finalize(&self) -> Option<UsageSummary> {
        if self.seen {
            Some(self.latest.clone())
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutputAccumulator {
    proto: Proto,
    buffer: String,
}

impl OutputAccumulator {
    pub fn new(proto: Proto) -> Self {
        Self {
            proto,
            buffer: String::new(),
        }
    }

    pub fn push(&mut self, event: &StreamEvent) {
        match (self.proto, event) {
            (
                Proto::Claude,
                StreamEvent::Claude(BetaStreamEvent::Known(
                    BetaStreamEventKnown::ContentBlockDelta { delta, .. },
                )),
            ) => match delta {
                BetaStreamContentBlockDelta::TextDelta { text } => {
                    self.buffer.push_str(text);
                }
                BetaStreamContentBlockDelta::InputJsonDelta { partial_json } => {
                    self.buffer.push_str(partial_json);
                }
                _ => {}
            },
            (Proto::Claude, _) => {}
            (Proto::OpenAIChat, StreamEvent::OpenAIChat(event)) => {
                for choice in &event.choices {
                    if let Some(content) = &choice.delta.content {
                        self.buffer.push_str(content);
                    }
                    if let Some(refusal) = &choice.delta.refusal {
                        self.buffer.push_str(refusal);
                    }
                    if let Some(tool_calls) = &choice.delta.tool_calls
                        && let Ok(json) = serde_json::to_string(tool_calls)
                    {
                        self.buffer.push_str(&json);
                    }
                    if let Some(function_call) = &choice.delta.function_call
                        && let Ok(json) = serde_json::to_string(function_call)
                    {
                        self.buffer.push_str(&json);
                    }
                }
            }
            (Proto::OpenAIResponse, StreamEvent::OpenAIResponse(event)) => match event {
                ResponseStreamEvent::OutputTextDelta(ResponseTextDeltaEvent { delta, .. }) => {
                    self.buffer.push_str(delta);
                }
                ResponseStreamEvent::RefusalDelta(ResponseRefusalDeltaEvent { delta, .. }) => {
                    self.buffer.push_str(delta);
                }
                ResponseStreamEvent::FunctionCallArgumentsDelta(
                    ResponseFunctionCallArgumentsDeltaEvent { delta, .. },
                ) => {
                    self.buffer.push_str(delta);
                }
                ResponseStreamEvent::MCPCallArgumentsDelta(
                    ResponseMCPCallArgumentsDeltaEvent { delta, .. },
                ) => {
                    self.buffer.push_str(delta);
                }
                ResponseStreamEvent::CustomToolCallInputDelta(
                    ResponseCustomToolCallInputDeltaEvent { delta, .. },
                ) => {
                    self.buffer.push_str(delta);
                }
                _ => {}
            },
            (Proto::Gemini, StreamEvent::Gemini(event)) => {
                append_gemini_response_text(&mut self.buffer, event);
            }
            _ => {}
        }
    }

    pub fn extend_from_response(&mut self, resp: &GenerateContentResponse) {
        self.buffer.push_str(&output_for_counting(self.proto, resp));
    }

    pub fn as_str(&self) -> &str {
        &self.buffer
    }

    pub fn into_string(self) -> String {
        self.buffer
    }
}

pub trait CountTokensFn {
    type Error;

    fn count_tokens(
        &self,
        proto: Proto,
        req: CountTokensRequest,
    ) -> Result<CountTokensResponse, Self::Error>;
}

#[derive(Debug, Clone)]
pub enum UsageError<E> {
    CountTokens(E),
    BuildRequest,
}

pub fn usage_from_response(proto: Proto, resp: &GenerateContentResponse) -> Option<UsageSummary> {
    match (proto, resp) {
        (Proto::Claude, GenerateContentResponse::Claude(resp)) => {
            Some(usage_from_claude_response(resp))
        }
        (Proto::OpenAIChat, GenerateContentResponse::OpenAIChat(resp)) => {
            resp.usage.as_ref().map(usage_from_openai_chat_usage)
        }
        (Proto::OpenAIResponse, GenerateContentResponse::OpenAIResponse(resp)) => {
            resp.usage.as_ref().map(usage_from_openai_response_usage)
        }
        (Proto::Gemini, GenerateContentResponse::Gemini(resp)) => {
            resp.usage_metadata.as_ref().map(usage_from_gemini_usage)
        }
        _ => None,
    }
}

pub fn output_for_counting(proto: Proto, resp: &GenerateContentResponse) -> String {
    match (proto, resp) {
        (Proto::Claude, GenerateContentResponse::Claude(resp)) => render_claude_output(resp),
        (Proto::OpenAIChat, GenerateContentResponse::OpenAIChat(resp)) => {
            render_openai_chat_output(resp)
        }
        (Proto::OpenAIResponse, GenerateContentResponse::OpenAIResponse(resp)) => {
            render_openai_response_output(resp)
        }
        (Proto::Gemini, GenerateContentResponse::Gemini(resp)) => render_gemini_output(resp),
        _ => String::new(),
    }
}

pub fn fallback_usage_with_count_tokens<E>(
    proto: Proto,
    input_req: &GenerateContentRequest,
    output_text: &str,
    count_fn: &impl CountTokensFn<Error = E>,
) -> Result<UsageSummary, UsageError<E>> {
    let input_req = build_input_count_request(proto, input_req).ok_or(UsageError::BuildRequest)?;
    let input_model = input_req_model(proto, &input_req);
    let input_resp = count_fn
        .count_tokens(proto, input_req)
        .map_err(UsageError::CountTokens)?;
    let input_tokens = count_tokens_value(&input_resp);

    let output_tokens = if output_text.is_empty() {
        Some(0)
    } else {
        let output_req = build_output_count_request(proto, input_model, output_text)
            .ok_or(UsageError::BuildRequest)?;
        let output_resp = count_fn
            .count_tokens(proto, output_req)
            .map_err(UsageError::CountTokens)?;
        count_tokens_value(&output_resp)
    };

    Ok(UsageSummary {
        input_tokens,
        output_tokens,
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
    })
}

fn usage_from_claude_response(resp: &ClaudeCreateMessageResponse) -> UsageSummary {
    UsageSummary {
        input_tokens: Some(resp.usage.input_tokens),
        output_tokens: Some(resp.usage.output_tokens),
        cache_read_input_tokens: Some(resp.usage.cache_read_input_tokens),
        cache_creation_input_tokens: Some(resp.usage.cache_creation_input_tokens),
    }
}

fn usage_from_claude_stream(event: &BetaStreamEvent) -> Option<UsageSummary> {
    match event {
        BetaStreamEvent::Known(BetaStreamEventKnown::MessageStart { message }) => {
            Some(usage_from_claude_stream_usage(&message.usage))
        }
        BetaStreamEvent::Known(BetaStreamEventKnown::MessageDelta { usage, .. }) => {
            Some(usage_from_claude_stream_usage(usage))
        }
        _ => None,
    }
}

fn usage_from_claude_stream_usage(usage: &BetaStreamUsage) -> UsageSummary {
    UsageSummary {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_input_tokens: usage.cache_read_input_tokens,
        cache_creation_input_tokens: usage.cache_creation_input_tokens,
    }
}

fn usage_from_openai_chat_usage(usage: &CompletionUsage) -> UsageSummary {
    UsageSummary {
        input_tokens: Some(clamp_i64_to_u32(usage.prompt_tokens)),
        output_tokens: Some(clamp_i64_to_u32(usage.completion_tokens)),
        cache_read_input_tokens: usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens)
            .map(clamp_i64_to_u32),
        cache_creation_input_tokens: None,
    }
}

fn usage_from_openai_response_usage(usage: &ResponseUsage) -> UsageSummary {
    UsageSummary {
        input_tokens: Some(clamp_i64_to_u32(usage.input_tokens)),
        output_tokens: Some(clamp_i64_to_u32(usage.output_tokens)),
        cache_read_input_tokens: Some(clamp_i64_to_u32(usage.input_tokens_details.cached_tokens)),
        cache_creation_input_tokens: None,
    }
}

fn usage_from_openai_response_stream(event: &ResponseStreamEvent) -> Option<UsageSummary> {
    let response = match event {
        ResponseStreamEvent::Created(ResponseCreatedEvent { response, .. }) => Some(response),
        ResponseStreamEvent::InProgress(ResponseInProgressEvent { response, .. }) => Some(response),
        ResponseStreamEvent::Completed(ResponseCompletedEvent { response, .. }) => Some(response),
        ResponseStreamEvent::Failed(ResponseFailedEvent { response, .. }) => Some(response),
        ResponseStreamEvent::Incomplete(ResponseIncompleteEvent { response, .. }) => Some(response),
        _ => None,
    };

    response
        .and_then(|resp| resp.usage.as_ref())
        .map(usage_from_openai_response_usage)
}

fn usage_from_gemini_usage(
    usage: &gproxy_protocol::gemini::generate_content::types::UsageMetadata,
) -> UsageSummary {
    UsageSummary {
        input_tokens: usage.prompt_token_count,
        output_tokens: usage.candidates_token_count,
        cache_read_input_tokens: usage.cached_content_token_count,
        cache_creation_input_tokens: None,
    }
}

fn build_input_count_request(
    proto: Proto,
    req: &GenerateContentRequest,
) -> Option<CountTokensRequest> {
    match (proto, req) {
        (Proto::Claude, GenerateContentRequest::Claude(req)) => {
            Some(CountTokensRequest::Claude(build_claude_count_request(req)))
        }
        (Proto::OpenAIChat, GenerateContentRequest::OpenAIChat(req)) => Some(
            CountTokensRequest::OpenAI(build_openai_chat_count_request(req)),
        ),
        (Proto::OpenAIResponse, GenerateContentRequest::OpenAIResponse(req)) => Some(
            CountTokensRequest::OpenAI(build_openai_response_count_request(req)),
        ),
        (Proto::Gemini, GenerateContentRequest::Gemini(req)) => {
            Some(CountTokensRequest::Gemini(build_gemini_count_request(req)))
        }
        (Proto::Gemini, GenerateContentRequest::GeminiStream(req)) => {
            let req = GeminiGenerateContentRequest {
                path: req.path.clone(),
                body: req.body.clone(),
            };
            Some(CountTokensRequest::Gemini(build_gemini_count_request(&req)))
        }
        _ => None,
    }
}

fn build_output_count_request(
    proto: Proto,
    model: Option<String>,
    output_text: &str,
) -> Option<CountTokensRequest> {
    match proto {
        Proto::Claude => {
            let model = model.map(ClaudeModel::Custom)?;
            let msg = ClaudeMessageParam {
                role: ClaudeMessageRole::Assistant,
                content: ClaudeMessageContent::Text(output_text.to_string()),
            };
            let body = ClaudeCountTokensRequestBody {
                messages: vec![msg],
                model,
                system: None,
                tools: None,
                tool_choice: None,
                thinking: None,
                output_config: None,
                output_format: None,
                context_management: None,
                mcp_servers: None,
            };
            Some(CountTokensRequest::Claude(ClaudeCountTokensRequest {
                headers: ClaudeCountTokensHeaders::default(),
                body,
            }))
        }
        Proto::OpenAIChat | Proto::OpenAIResponse => {
            let model = model?;
            let body = OpenAIInputTokenCountRequestBody {
                model,
                input: Some(InputParam::Text(output_text.to_string())),
                previous_response_id: None,
                tools: None,
                text: None,
                reasoning: None,
                truncation: None,
                instructions: None,
                conversation: None,
                tool_choice: None,
                parallel_tool_calls: None,
            };
            Some(CountTokensRequest::OpenAI(OpenAIInputTokenCountRequest {
                body,
            }))
        }
        Proto::Gemini => {
            let model = model?;
            let content = GeminiContent {
                parts: vec![GeminiPart {
                    text: Some(output_text.to_string()),
                    inline_data: None,
                    function_call: None,
                    function_response: None,
                    file_data: None,
                    executable_code: None,
                    code_execution_result: None,
                    thought: None,
                    thought_signature: None,
                    part_metadata: None,
                    video_metadata: None,
                }],
                role: Some(GeminiContentRole::Model),
            };
            let body = GeminiCountTokensRequestBody {
                contents: Some(vec![content]),
                generate_content_request: None,
            };
            Some(CountTokensRequest::Gemini(GeminiCountTokensRequest {
                path: GeminiCountTokensPath { model },
                body,
            }))
        }
        _ => None,
    }
}

fn input_req_model(proto: Proto, req: &CountTokensRequest) -> Option<String> {
    match (proto, req) {
        (Proto::Claude, CountTokensRequest::Claude(req)) => match &req.body.model {
            ClaudeModel::Custom(value) => Some(value.clone()),
            ClaudeModel::Known(known) => serde_json::to_value(known)
                .ok()?
                .as_str()
                .map(|s| s.to_string()),
        },
        (Proto::OpenAIChat, CountTokensRequest::OpenAI(req)) => Some(req.body.model.clone()),
        (Proto::OpenAIResponse, CountTokensRequest::OpenAI(req)) => Some(req.body.model.clone()),
        (Proto::Gemini, CountTokensRequest::Gemini(req)) => Some(req.path.model.clone()),
        _ => None,
    }
}

fn build_claude_count_request(req: &ClaudeCreateMessageRequest) -> ClaudeCountTokensRequest {
    ClaudeCountTokensRequest {
        headers: ClaudeCountTokensHeaders::default(),
        body: ClaudeCountTokensRequestBody {
            messages: req.body.messages.clone(),
            model: req.body.model.clone(),
            system: req.body.system.clone(),
            tools: req.body.tools.clone(),
            tool_choice: req.body.tool_choice.clone(),
            thinking: req.body.thinking.clone(),
            output_config: req.body.output_config.clone(),
            output_format: req.body.output_format.clone(),
            context_management: req.body.context_management.clone(),
            mcp_servers: req.body.mcp_servers.clone(),
        },
    }
}

fn build_openai_chat_count_request(
    req: &OpenAIChatCompletionRequest,
) -> OpenAIInputTokenCountRequest {
    let messages_json = serde_json::to_string(&req.body.messages).unwrap_or_default();
    let body = OpenAIInputTokenCountRequestBody {
        model: req.body.model.clone(),
        input: Some(InputParam::Text(messages_json)),
        previous_response_id: None,
        tools: None,
        text: None,
        reasoning: None,
        truncation: None,
        instructions: None,
        conversation: None,
        tool_choice: None,
        parallel_tool_calls: None,
    };
    OpenAIInputTokenCountRequest { body }
}

fn build_openai_response_count_request(
    req: &OpenAIResponseRequest,
) -> OpenAIInputTokenCountRequest {
    let body = OpenAIInputTokenCountRequestBody {
        model: req.body.model.clone(),
        input: req.body.input.clone(),
        previous_response_id: req.body.previous_response_id.clone(),
        tools: req.body.tools.clone(),
        text: req.body.text.clone(),
        reasoning: req.body.reasoning.clone(),
        truncation: req.body.truncation,
        instructions: req.body.instructions.clone(),
        conversation: req.body.conversation.clone(),
        tool_choice: req.body.tool_choice.clone(),
        parallel_tool_calls: req.body.parallel_tool_calls,
    };
    OpenAIInputTokenCountRequest { body }
}

fn build_gemini_count_request(req: &GeminiGenerateContentRequest) -> GeminiCountTokensRequest {
    let generate_content_request = serde_json::to_value(&req.body).ok();
    let body = GeminiCountTokensRequestBody {
        contents: None,
        generate_content_request,
    };

    GeminiCountTokensRequest {
        path: GeminiCountTokensPath {
            model: req.path.model.clone(),
        },
        body,
    }
}

fn count_tokens_value(resp: &CountTokensResponse) -> Option<u32> {
    match resp {
        CountTokensResponse::Claude(resp) => Some(resp.input_tokens),
        CountTokensResponse::OpenAI(resp) => Some(clamp_i64_to_u32(resp.input_tokens)),
        CountTokensResponse::Gemini(resp) => Some(resp.total_tokens),
    }
}

fn clamp_i64_to_u32(value: i64) -> u32 {
    if value <= 0 {
        0
    } else if value > i64::from(u32::MAX) {
        u32::MAX
    } else {
        value as u32
    }
}

fn render_claude_output(resp: &ClaudeCreateMessageResponse) -> String {
    let mut out = String::new();
    for block in &resp.content {
        match block {
            gproxy_protocol::claude::create_message::types::BetaContentBlock::Text(text) => {
                out.push_str(&text.text);
            }
            _ => {
                if let Ok(json) = serde_json::to_string(block) {
                    out.push_str(&json);
                }
            }
        }
    }
    out
}

fn render_openai_chat_output(resp: &OpenAIChatCompletionResponse) -> String {
    let mut out = String::new();
    for choice in &resp.choices {
        let message = &choice.message;
        if let Some(content) = &message.content {
            out.push_str(content);
        }
        if let Some(refusal) = &message.refusal {
            out.push_str(refusal);
        }
        if let Some(tool_calls) = &message.tool_calls
            && let Ok(json) = serde_json::to_string(tool_calls)
        {
            out.push_str(&json);
        }
        if let Some(function_call) = &message.function_call
            && let Ok(json) = serde_json::to_string(function_call)
        {
            out.push_str(&json);
        }
    }
    out
}

fn render_openai_response_output(resp: &OpenAIResponse) -> String {
    let mut out = String::new();
    if resp.output.is_empty() {
        if let Some(output_text) = &resp.output_text {
            out.push_str(output_text);
        }
        return out;
    }
    for item in &resp.output {
        match item {
            OutputItem::Message(message) => {
                append_openai_message_output(&mut out, message);
            }
            _ => {
                if let Ok(json) = serde_json::to_string(item) {
                    out.push_str(&json);
                }
            }
        }
    }
    out
}

fn append_openai_message_output(out: &mut String, message: &OutputMessage) {
    for content in &message.content {
        match content {
            OutputMessageContent::OutputText(text) => {
                out.push_str(&text.text);
            }
            OutputMessageContent::Refusal(refusal) => {
                out.push_str(&refusal.refusal);
            }
        }
    }
}

fn render_gemini_output(resp: &GeminiGenerateContentResponse) -> String {
    let mut out = String::new();
    for candidate in &resp.candidates {
        for part in &candidate.content.parts {
            append_gemini_part(&mut out, part);
        }
    }
    out
}

fn append_gemini_response_text(out: &mut String, resp: &StreamGenerateContentResponse) {
    for candidate in &resp.candidates {
        for part in &candidate.content.parts {
            append_gemini_part(out, part);
        }
    }
}

fn append_gemini_part(out: &mut String, part: &gproxy_protocol::gemini::count_tokens::types::Part) {
    if let Some(text) = &part.text {
        out.push_str(text);
        return;
    }
    if let Some(value) = &part.inline_data {
        if let Ok(json) = serde_json::to_string(value) {
            out.push_str(&json);
        }
        return;
    }
    if let Some(value) = &part.function_call {
        if let Ok(json) = serde_json::to_string(value) {
            out.push_str(&json);
        }
        return;
    }
    if let Some(value) = &part.function_response {
        if let Ok(json) = serde_json::to_string(value) {
            out.push_str(&json);
        }
        return;
    }
    if let Some(value) = &part.file_data {
        if let Ok(json) = serde_json::to_string(value) {
            out.push_str(&json);
        }
        return;
    }
    if let Some(value) = &part.executable_code {
        if let Ok(json) = serde_json::to_string(value) {
            out.push_str(&json);
        }
        return;
    }
    if let Some(value) = &part.code_execution_result
        && let Ok(json) = serde_json::to_string(value)
    {
        out.push_str(&json);
    }
}

fn merge_usage(base: &mut UsageSummary, incoming: UsageSummary) {
    if incoming.input_tokens.is_some() {
        base.input_tokens = incoming.input_tokens;
    }
    if incoming.output_tokens.is_some() {
        base.output_tokens = incoming.output_tokens;
    }
    if incoming.cache_read_input_tokens.is_some() {
        base.cache_read_input_tokens = incoming.cache_read_input_tokens;
    }
    if incoming.cache_creation_input_tokens.is_some() {
        base.cache_creation_input_tokens = incoming.cache_creation_input_tokens;
    }
}
