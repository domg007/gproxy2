use std::collections::{BTreeMap, BTreeSet};

use crate::claude::count_tokens::types::BetaServerToolUseName;
use crate::claude::create_message::stream::{
    BetaRawContentBlockDelta, ClaudeCreateMessageSseStreamBody, ClaudeCreateMessageStreamEvent,
};
use crate::claude::create_message::types::{BetaContentBlock, BetaStopReason};
use crate::openai::create_chat_completions::stream::{
    ChatCompletionChunk, ChatCompletionChunkChoice, ChatCompletionChunkDelta,
    ChatCompletionChunkDeltaToolCall, ChatCompletionChunkDeltaToolCallType,
    ChatCompletionFunctionCallDelta, OpenAiChatCompletionsSseData, OpenAiChatCompletionsSseEvent,
    OpenAiChatCompletionsSseStreamBody,
};
use crate::openai::create_chat_completions::types as ct;
use crate::transform::claude::utils::claude_model_to_string;
use crate::transform::utils::TransformError;

#[derive(Debug, Clone)]
struct OpenAiChatToolState {
    choice_index: u32,
    tool_index: u32,
    call_id: String,
    name: String,
    name_emitted: bool,
}

#[derive(Debug, Default, Clone)]
pub struct ClaudeToOpenAiChatCompletionsStream {
    response_id: String,
    model: String,
    created: u64,
    input_tokens: u64,
    cache_creation_input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    incomplete_finish_reason: Option<ct::ChatCompletionFinishReason>,
    output_choice_map: BTreeMap<u64, u32>,
    role_emitted: BTreeSet<u32>,
    choice_tool_counts: BTreeMap<u32, u32>,
    choice_has_tool_calls: BTreeSet<u32>,
    text_blocks: BTreeSet<u64>,
    thinking_blocks: BTreeSet<u64>,
    tool_blocks: BTreeMap<u64, String>,
    tool_states: BTreeMap<String, OpenAiChatToolState>,
    started: bool,
    finished: bool,
}

impl ClaudeToOpenAiChatCompletionsStream {
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn stop_reason_to_finish_reason(
        stop_reason: Option<BetaStopReason>,
    ) -> Option<ct::ChatCompletionFinishReason> {
        match stop_reason {
            Some(BetaStopReason::MaxTokens) | Some(BetaStopReason::ModelContextWindowExceeded) => {
                Some(ct::ChatCompletionFinishReason::Length)
            }
            Some(BetaStopReason::Refusal) => Some(ct::ChatCompletionFinishReason::ContentFilter),
            _ => None,
        }
    }

    fn fallback_response_id(&self) -> String {
        if self.response_id.is_empty() {
            "response".to_string()
        } else {
            self.response_id.clone()
        }
    }

    fn fallback_model(&self) -> String {
        if self.model.is_empty() {
            "claude".to_string()
        } else {
            self.model.clone()
        }
    }

    fn usage(&self) -> Option<ct::CompletionUsage> {
        if !self.started {
            return None;
        }

        let prompt_tokens = self
            .input_tokens
            .saturating_add(self.cache_creation_input_tokens)
            .saturating_add(self.cached_input_tokens);

        Some(ct::CompletionUsage {
            completion_tokens: self.output_tokens,
            prompt_tokens,
            total_tokens: prompt_tokens.saturating_add(self.output_tokens),
            completion_tokens_details: Some(ct::CompletionTokensDetails {
                accepted_prediction_tokens: None,
                audio_tokens: None,
                reasoning_tokens: Some(0),
                rejected_prediction_tokens: None,
            }),
            prompt_tokens_details: Some(ct::PromptTokensDetails {
                audio_tokens: None,
                cached_tokens: Some(self.cached_input_tokens),
            }),
        })
    }

    fn chunk_event(
        &self,
        index: u32,
        delta: ChatCompletionChunkDelta,
        finish_reason: Option<ct::ChatCompletionFinishReason>,
        usage: Option<ct::CompletionUsage>,
    ) -> OpenAiChatCompletionsSseEvent {
        OpenAiChatCompletionsSseEvent {
            event: None,
            data: OpenAiChatCompletionsSseData::Chunk(ChatCompletionChunk {
                id: self.fallback_response_id(),
                choices: vec![ChatCompletionChunkChoice {
                    delta,
                    finish_reason,
                    index,
                    logprobs: None,
                }],
                created: self.created,
                model: self.fallback_model(),
                object: crate::openai::create_chat_completions::stream::ChatCompletionChunkObject::ChatCompletionChunk,
                service_tier: None,
                system_fingerprint: None,
                usage,
            }),
        }
    }

    fn ensure_choice_index(&mut self, output_index: u64) -> u32 {
        // Claude stream content block index is not Chat choice index.
        // Keep a single assistant choice for chat-completions compatibility.
        self.output_choice_map.insert(output_index, 0);
        0
    }

    fn maybe_emit_role(&mut self, out: &mut Vec<OpenAiChatCompletionsSseEvent>, choice_index: u32) {
        if self.role_emitted.insert(choice_index) {
            out.push(self.chunk_event(
                choice_index,
                ChatCompletionChunkDelta {
                    role: Some(ct::ChatCompletionDeltaRole::Assistant),
                    ..Default::default()
                },
                None,
                None,
            ));
        }
    }

    fn emit_content(
        &mut self,
        output_index: u64,
        text: String,
        refusal: bool,
        out: &mut Vec<OpenAiChatCompletionsSseEvent>,
    ) {
        let choice_index = self.ensure_choice_index(output_index);
        self.maybe_emit_role(out, choice_index);

        if text.is_empty() {
            return;
        }

        out.push(self.chunk_event(
            choice_index,
            ChatCompletionChunkDelta {
                content: if refusal { None } else { Some(text.clone()) },
                refusal: if refusal { Some(text) } else { None },
                ..Default::default()
            },
            None,
            None,
        ));
    }

    fn emit_reasoning_content(
        &mut self,
        output_index: u64,
        text: String,
        out: &mut Vec<OpenAiChatCompletionsSseEvent>,
    ) {
        if text.is_empty() {
            return;
        }

        let choice_index = self.ensure_choice_index(output_index);
        self.maybe_emit_role(out, choice_index);

        out.push(self.chunk_event(
            choice_index,
            ChatCompletionChunkDelta {
                reasoning_content: Some(text),
                ..Default::default()
            },
            None,
            None,
        ));
    }

    fn emit_reasoning_signature(
        &mut self,
        output_index: u64,
        signature: String,
        out: &mut Vec<OpenAiChatCompletionsSseEvent>,
    ) {
        if signature.is_empty() {
            return;
        }

        let choice_index = self.ensure_choice_index(output_index);
        self.maybe_emit_role(out, choice_index);
        let reasoning_id = format!("reasoning_{output_index}");

        out.push(self.chunk_event(
            choice_index,
            ChatCompletionChunkDelta {
                reasoning_details: Some(vec![ct::ChatCompletionReasoningDetail {
                    type_: ct::ChatCompletionReasoningDetailType::ReasoningEncrypted,
                    id: Some(reasoning_id),
                    data: Some(signature),
                }]),
                ..Default::default()
            },
            None,
            None,
        ));
    }

    fn emit_tool_call_arguments_delta(
        &mut self,
        call_id: &str,
        delta: String,
        out: &mut Vec<OpenAiChatCompletionsSseEvent>,
    ) {
        if delta.is_empty() {
            return;
        }

        if let Some(tool) = self.tool_states.get(call_id).cloned() {
            self.maybe_emit_role(out, tool.choice_index);
            out.push(self.chunk_event(
                tool.choice_index,
                ChatCompletionChunkDelta {
                    tool_calls: Some(vec![ChatCompletionChunkDeltaToolCall {
                        index: tool.tool_index,
                        id: Some(tool.call_id.clone()),
                        function: Some(ChatCompletionFunctionCallDelta {
                            name: if tool.name_emitted {
                                None
                            } else {
                                Some(tool.name.clone())
                            },
                            arguments: Some(delta),
                        }),
                        type_: Some(ChatCompletionChunkDeltaToolCallType::Function),
                    }]),
                    ..Default::default()
                },
                None,
                None,
            ));

            if let Some(tool_state) = self.tool_states.get_mut(call_id) {
                tool_state.name_emitted = true;
            }
        }
    }

    fn start_tool_call(
        &mut self,
        output_index: u64,
        call_id: String,
        name: String,
        initial_arguments: String,
        count_for_finish_reason: bool,
        out: &mut Vec<OpenAiChatCompletionsSseEvent>,
    ) {
        let choice_index = self.ensure_choice_index(output_index);
        self.maybe_emit_role(out, choice_index);

        let tool_index_ref = self.choice_tool_counts.entry(choice_index).or_insert(0);
        let tool_index = *tool_index_ref;
        *tool_index_ref = tool_index.saturating_add(1);

        if count_for_finish_reason {
            self.choice_has_tool_calls.insert(choice_index);
        }

        let state = OpenAiChatToolState {
            choice_index,
            tool_index,
            call_id: call_id.clone(),
            name,
            name_emitted: false,
        };
        self.tool_blocks.insert(output_index, call_id.clone());
        self.tool_states.insert(call_id.clone(), state.clone());

        out.push(self.chunk_event(
            choice_index,
            ChatCompletionChunkDelta {
                tool_calls: Some(vec![ChatCompletionChunkDeltaToolCall {
                    index: state.tool_index,
                    id: Some(state.call_id.clone()),
                    function: Some(ChatCompletionFunctionCallDelta {
                        name: Some(state.name.clone()),
                        arguments: None,
                    }),
                    type_: Some(ChatCompletionChunkDeltaToolCallType::Function),
                }]),
                ..Default::default()
            },
            None,
            None,
        ));

        if let Some(tool) = self.tool_states.get_mut(&call_id) {
            tool.name_emitted = true;
        }

        if !initial_arguments.is_empty() && initial_arguments != "{}" {
            self.emit_tool_call_arguments_delta(&call_id, initial_arguments, out);
        }
    }

    fn default_finish_reason(&self) -> ct::ChatCompletionFinishReason {
        if let Some(reason) = self.incomplete_finish_reason.clone() {
            return reason;
        }

        if self.choice_has_tool_calls.is_empty() {
            ct::ChatCompletionFinishReason::Stop
        } else {
            ct::ChatCompletionFinishReason::ToolCalls
        }
    }

    fn sorted_choice_indexes(&self) -> Vec<u32> {
        let mut indexes = self.output_choice_map.values().copied().collect::<Vec<_>>();
        indexes.sort_unstable();
        indexes.dedup();
        indexes
    }

    pub fn on_event(
        &mut self,
        event: ClaudeCreateMessageStreamEvent,
    ) -> Result<Vec<OpenAiChatCompletionsSseEvent>, TransformError> {
        if self.finished {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        match event {
            ClaudeCreateMessageStreamEvent::MessageStart(event) => {
                self.response_id = event.message.id;
                self.model = claude_model_to_string(&event.message.model);
                self.input_tokens = event.message.usage.input_tokens;
                self.cache_creation_input_tokens = event.message.usage.cache_creation_input_tokens;
                self.cached_input_tokens = event.message.usage.cache_read_input_tokens;
                self.output_tokens = event.message.usage.output_tokens;
                self.incomplete_finish_reason =
                    Self::stop_reason_to_finish_reason(event.message.stop_reason);
                self.started = true;
            }
            ClaudeCreateMessageStreamEvent::ContentBlockStart(event) => {
                let output_index = event.index;
                match event.content_block {
                    BetaContentBlock::Text(block) => {
                        self.text_blocks.insert(output_index);
                        self.emit_content(output_index, block.text, false, &mut out);
                    }
                    BetaContentBlock::Thinking(_) | BetaContentBlock::RedactedThinking(_) => {
                        self.thinking_blocks.insert(output_index);
                    }
                    BetaContentBlock::ToolUse(block) => {
                        let arguments = serde_json::to_string(&block.input)
                            .unwrap_or_else(|_| "{}".to_string());
                        self.start_tool_call(
                            output_index,
                            block.id,
                            block.name,
                            arguments,
                            true,
                            &mut out,
                        );
                    }
                    BetaContentBlock::ServerToolUse(block) => {
                        let arguments = serde_json::to_string(&block.input)
                            .unwrap_or_else(|_| "{}".to_string());
                        self.start_tool_call(
                            output_index,
                            block.id,
                            server_tool_name(&block.name).to_string(),
                            arguments,
                            true,
                            &mut out,
                        );
                    }
                    BetaContentBlock::McpToolUse(block) => {
                        let arguments = serde_json::to_string(&block.input)
                            .unwrap_or_else(|_| "{}".to_string());
                        self.start_tool_call(
                            output_index,
                            block.id,
                            block.name,
                            arguments,
                            true,
                            &mut out,
                        );
                    }
                    other => {
                        if let Ok(text) = serde_json::to_string(&other) {
                            self.text_blocks.insert(output_index);
                            self.emit_content(output_index, text, false, &mut out);
                        }
                    }
                }
            }
            ClaudeCreateMessageStreamEvent::ContentBlockDelta(event) => match event.delta {
                BetaRawContentBlockDelta::Text(delta) => {
                    if self.text_blocks.contains(&event.index) {
                        self.emit_content(event.index, delta.text, false, &mut out);
                    }
                }
                BetaRawContentBlockDelta::Thinking(delta) => {
                    if self.thinking_blocks.contains(&event.index) {
                        self.emit_reasoning_content(event.index, delta.thinking, &mut out);
                    }
                }
                BetaRawContentBlockDelta::Signature(delta) => {
                    if self.thinking_blocks.contains(&event.index) {
                        self.emit_reasoning_signature(event.index, delta.signature, &mut out);
                    }
                }
                BetaRawContentBlockDelta::InputJson(delta) => {
                    if let Some(call_id) = self.tool_blocks.get(&event.index).cloned() {
                        self.emit_tool_call_arguments_delta(&call_id, delta.partial_json, &mut out);
                    }
                }
                _ => {}
            },
            ClaudeCreateMessageStreamEvent::ContentBlockStop(event) => {
                self.text_blocks.remove(&event.index);
                self.thinking_blocks.remove(&event.index);
                if let Some(call_id) = self.tool_blocks.remove(&event.index) {
                    self.tool_states.remove(&call_id);
                }
            }
            ClaudeCreateMessageStreamEvent::MessageDelta(event) => {
                if let Some(input_tokens) = event.usage.input_tokens {
                    self.input_tokens = input_tokens;
                }
                if let Some(cache_creation_input_tokens) = event.usage.cache_creation_input_tokens {
                    self.cache_creation_input_tokens = cache_creation_input_tokens;
                }
                if let Some(cached_input_tokens) = event.usage.cache_read_input_tokens {
                    self.cached_input_tokens = cached_input_tokens;
                }
                self.output_tokens = event.usage.output_tokens;

                if event.delta.stop_reason.is_some() {
                    self.incomplete_finish_reason =
                        Self::stop_reason_to_finish_reason(event.delta.stop_reason);
                }
            }
            ClaudeCreateMessageStreamEvent::MessageStop(_) => {
                out.extend(self.finish());
            }
            ClaudeCreateMessageStreamEvent::Error(_) => {
                self.finished = true;
                out.push(OpenAiChatCompletionsSseEvent {
                    event: None,
                    data: OpenAiChatCompletionsSseData::Done("[DONE]".to_string()),
                });
            }
            ClaudeCreateMessageStreamEvent::Ping(_)
            | ClaudeCreateMessageStreamEvent::Unknown(_) => {}
        }

        Ok(out)
    }

    pub fn finish(&mut self) -> Vec<OpenAiChatCompletionsSseEvent> {
        if self.finished {
            return Vec::new();
        }

        let mut out = Vec::new();
        let default_reason = self.default_finish_reason();

        let mut choices = self.sorted_choice_indexes();
        if choices.is_empty() {
            choices.push(0);
        }

        for choice_index in &choices {
            let finish_reason = if self.choice_has_tool_calls.contains(choice_index) {
                ct::ChatCompletionFinishReason::ToolCalls
            } else {
                default_reason.clone()
            };
            out.push(self.chunk_event(
                *choice_index,
                Default::default(),
                Some(finish_reason),
                None,
            ));
        }

        if let Some(last) = out.last_mut()
            && let OpenAiChatCompletionsSseData::Chunk(chunk) = &mut last.data
        {
            chunk.usage = self.usage();
        }

        out.push(OpenAiChatCompletionsSseEvent {
            event: None,
            data: OpenAiChatCompletionsSseData::Done("[DONE]".to_string()),
        });
        self.finished = true;
        out
    }
}

impl TryFrom<Vec<ClaudeCreateMessageStreamEvent>> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: Vec<ClaudeCreateMessageStreamEvent>) -> Result<Self, TransformError> {
        let mut converter = ClaudeToOpenAiChatCompletionsStream::default();
        let mut events = Vec::new();

        for event in value {
            events.extend(converter.on_event(event)?);
        }

        if !converter.is_finished() {
            events.extend(converter.finish());
        }

        Ok(Self { events })
    }
}

impl TryFrom<ClaudeCreateMessageSseStreamBody> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: ClaudeCreateMessageSseStreamBody) -> Result<Self, TransformError> {
        Self::try_from(value.events)
    }
}

fn server_tool_name(name: &BetaServerToolUseName) -> &'static str {
    match name {
        BetaServerToolUseName::WebSearch => "web_search",
        BetaServerToolUseName::WebFetch => "web_fetch",
        BetaServerToolUseName::CodeExecution => "code_execution",
        BetaServerToolUseName::BashCodeExecution => "bash_code_execution",
        BetaServerToolUseName::TextEditorCodeExecution => "text_editor_code_execution",
        BetaServerToolUseName::ToolSearchToolRegex => "tool_search_tool_regex",
        BetaServerToolUseName::ToolSearchToolBm25 => "tool_search_tool_bm25",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::count_tokens::types as cct;
    use crate::claude::create_message::stream::{
        BetaMessageDeltaUsage, BetaRawContentBlockDelta, BetaRawContentBlockDeltaEvent,
        BetaRawContentBlockDeltaEventType, BetaRawContentBlockStartEvent,
        BetaRawContentBlockStartEventType, BetaRawMessageDelta, BetaRawMessageDeltaEvent,
        BetaRawMessageDeltaEventType, BetaSignatureDelta, BetaSignatureDeltaType,
        BetaThinkingDelta, BetaThinkingDeltaType, ClaudeCreateMessageStreamEvent,
    };
    use crate::claude::create_message::types::{
        BetaServiceTier, BetaStopReason, BetaThinkingBlock,
    };
    use crate::transform::claude::stream_generate_content::utils::{
        message_delta_event, message_start_event, message_stop_event, start_text_block_event,
        stop_block_event, text_delta_event,
    };

    #[test]
    fn claude_stream_to_chat_stream_is_direct() {
        let stream = ClaudeCreateMessageSseStreamBody {
            events: vec![
                message_start_event(
                    "msg_1".to_string(),
                    "claude-sonnet".to_string(),
                    BetaServiceTier::Standard,
                    11,
                    2,
                ),
                start_text_block_event(0),
                text_delta_event(0, "hello".to_string()),
                stop_block_event(0),
                message_delta_event(Some(BetaStopReason::EndTurn), 11, 2, 3),
                message_stop_event(),
            ],
        };

        let converted = OpenAiChatCompletionsSseStreamBody::try_from(stream).unwrap();
        assert_eq!(converted.events.len(), 4);

        match &converted.events[0].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                assert_eq!(
                    chunk.choices[0].delta.role,
                    Some(ct::ChatCompletionDeltaRole::Assistant)
                );
            }
            other => panic!("unexpected first event: {other:?}"),
        }

        match &converted.events[1].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("hello"));
            }
            other => panic!("unexpected second event: {other:?}"),
        }

        match &converted.events[2].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                assert_eq!(
                    chunk.choices[0].finish_reason,
                    Some(ct::ChatCompletionFinishReason::Stop)
                );
                assert_eq!(
                    chunk.usage.as_ref().map(|usage| usage.prompt_tokens),
                    Some(13)
                );
                assert_eq!(
                    chunk.usage.as_ref().map(|usage| usage.total_tokens),
                    Some(16)
                );
                assert_eq!(
                    chunk
                        .usage
                        .as_ref()
                        .and_then(|usage| usage.prompt_tokens_details.as_ref())
                        .and_then(|details| details.cached_tokens),
                    Some(2)
                );
            }
            other => panic!("unexpected third event: {other:?}"),
        }

        assert!(matches!(
            converted.events[3].data,
            OpenAiChatCompletionsSseData::Done(_)
        ));
    }

    #[test]
    fn claude_mcp_tool_use_stream_maps_to_chat_tool_calls() {
        let mut input = cct::JsonObject::new();
        input.insert(
            "foo".to_string(),
            serde_json::Value::String("bar".to_string()),
        );

        let stream = ClaudeCreateMessageSseStreamBody {
            events: vec![
                message_start_event(
                    "msg_2".to_string(),
                    "claude-sonnet".to_string(),
                    BetaServiceTier::Standard,
                    9,
                    1,
                ),
                ClaudeCreateMessageStreamEvent::ContentBlockStart(BetaRawContentBlockStartEvent {
                    content_block: BetaContentBlock::McpToolUse(cct::BetaMcpToolUseBlockParam {
                        id: "mcp_call_1".to_string(),
                        input: input.clone(),
                        name: "list_tools".to_string(),
                        server_name: "local".to_string(),
                        type_: cct::BetaMcpToolUseBlockType::McpToolUse,
                        cache_control: None,
                    }),
                    index: 0,
                    type_: BetaRawContentBlockStartEventType::ContentBlockStart,
                }),
                stop_block_event(0),
                message_delta_event(Some(BetaStopReason::ToolUse), 9, 1, 2),
                message_stop_event(),
            ],
        };

        let converted = OpenAiChatCompletionsSseStreamBody::try_from(stream).unwrap();
        assert_eq!(converted.events.len(), 5);

        match &converted.events[1].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                let tool_calls = chunk.choices[0]
                    .delta
                    .tool_calls
                    .as_ref()
                    .expect("tool call start chunk");
                assert_eq!(tool_calls[0].id.as_deref(), Some("mcp_call_1"));
                assert_eq!(
                    tool_calls[0]
                        .function
                        .as_ref()
                        .and_then(|function| function.name.as_deref()),
                    Some("list_tools")
                );
                assert_eq!(
                    tool_calls[0]
                        .function
                        .as_ref()
                        .and_then(|function| function.arguments.as_deref()),
                    None
                );
            }
            other => panic!("unexpected tool call start event: {other:?}"),
        }

        match &converted.events[2].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                let tool_calls = chunk.choices[0]
                    .delta
                    .tool_calls
                    .as_ref()
                    .expect("tool call args chunk");
                assert_eq!(
                    tool_calls[0]
                        .function
                        .as_ref()
                        .and_then(|function| function.name.as_deref()),
                    None
                );
                assert_eq!(
                    tool_calls[0]
                        .function
                        .as_ref()
                        .and_then(|function| function.arguments.as_deref()),
                    Some(serde_json::to_string(&input).unwrap().as_str())
                );
            }
            other => panic!("unexpected tool call args event: {other:?}"),
        }

        match &converted.events[3].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                assert_eq!(
                    chunk.choices[0].finish_reason,
                    Some(ct::ChatCompletionFinishReason::ToolCalls)
                );
            }
            other => panic!("unexpected finish event: {other:?}"),
        }
    }

    #[test]
    fn claude_stream_usage_includes_cache_creation_tokens() {
        let stream = ClaudeCreateMessageSseStreamBody {
            events: vec![
                message_start_event(
                    "msg_3".to_string(),
                    "claude-sonnet".to_string(),
                    BetaServiceTier::Standard,
                    10,
                    20,
                ),
                start_text_block_event(0),
                text_delta_event(0, "ok".to_string()),
                stop_block_event(0),
                ClaudeCreateMessageStreamEvent::MessageDelta(BetaRawMessageDeltaEvent {
                    context_management: None,
                    delta: BetaRawMessageDelta {
                        container: None,
                        stop_reason: Some(BetaStopReason::EndTurn),
                        stop_sequence: None,
                    },
                    type_: BetaRawMessageDeltaEventType::MessageDelta,
                    usage: BetaMessageDeltaUsage {
                        cache_creation_input_tokens: Some(5),
                        cache_read_input_tokens: Some(20),
                        input_tokens: Some(10),
                        iterations: None,
                        output_tokens: 3,
                        server_tool_use: None,
                    },
                }),
                message_stop_event(),
            ],
        };

        let converted = OpenAiChatCompletionsSseStreamBody::try_from(stream).unwrap();
        assert_eq!(converted.events.len(), 4);

        match &converted.events[2].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                let usage = chunk.usage.as_ref().expect("usage should be present");
                assert_eq!(usage.prompt_tokens, 35);
                assert_eq!(usage.completion_tokens, 3);
                assert_eq!(usage.total_tokens, 38);
            }
            other => panic!("unexpected third event: {other:?}"),
        }
    }

    #[test]
    fn claude_thinking_stream_maps_to_chat_reasoning_content() {
        let stream = ClaudeCreateMessageSseStreamBody {
            events: vec![
                message_start_event(
                    "msg_4".to_string(),
                    "claude-sonnet".to_string(),
                    BetaServiceTier::Standard,
                    7,
                    0,
                ),
                ClaudeCreateMessageStreamEvent::ContentBlockStart(BetaRawContentBlockStartEvent {
                    content_block: BetaContentBlock::Thinking(BetaThinkingBlock {
                        signature: String::new(),
                        thinking: String::new(),
                        type_: cct::BetaThinkingBlockType::Thinking,
                    }),
                    index: 0,
                    type_: BetaRawContentBlockStartEventType::ContentBlockStart,
                }),
                ClaudeCreateMessageStreamEvent::ContentBlockDelta(BetaRawContentBlockDeltaEvent {
                    delta: BetaRawContentBlockDelta::Thinking(BetaThinkingDelta {
                        thinking: "plan".to_string(),
                        type_: BetaThinkingDeltaType::ThinkingDelta,
                    }),
                    index: 0,
                    type_: BetaRawContentBlockDeltaEventType::ContentBlockDelta,
                }),
                ClaudeCreateMessageStreamEvent::ContentBlockDelta(BetaRawContentBlockDeltaEvent {
                    delta: BetaRawContentBlockDelta::Signature(BetaSignatureDelta {
                        signature: "sig_1".to_string(),
                        type_: BetaSignatureDeltaType::SignatureDelta,
                    }),
                    index: 0,
                    type_: BetaRawContentBlockDeltaEventType::ContentBlockDelta,
                }),
                stop_block_event(0),
                message_delta_event(Some(BetaStopReason::EndTurn), 7, 0, 1),
                message_stop_event(),
            ],
        };

        let converted = OpenAiChatCompletionsSseStreamBody::try_from(stream).unwrap();
        assert_eq!(converted.events.len(), 5);

        match &converted.events[0].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                assert_eq!(
                    chunk.choices[0].delta.role,
                    Some(ct::ChatCompletionDeltaRole::Assistant)
                );
            }
            other => panic!("unexpected first event: {other:?}"),
        }

        match &converted.events[1].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                assert_eq!(
                    chunk.choices[0].delta.reasoning_content.as_deref(),
                    Some("plan")
                );
                assert!(chunk.choices[0].delta.content.is_none());
            }
            other => panic!("unexpected second event: {other:?}"),
        }

        match &converted.events[2].data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                let details = chunk.choices[0]
                    .delta
                    .reasoning_details
                    .as_ref()
                    .expect("reasoning details chunk");
                assert_eq!(details[0].id.as_deref(), Some("reasoning_0"));
                assert_eq!(details[0].data.as_deref(), Some("sig_1"));
                assert_eq!(
                    details[0].type_,
                    ct::ChatCompletionReasoningDetailType::ReasoningEncrypted
                );
            }
            other => panic!("unexpected third event: {other:?}"),
        }
    }
}
