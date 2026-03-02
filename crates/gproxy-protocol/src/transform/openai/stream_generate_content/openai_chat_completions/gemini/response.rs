use std::collections::{BTreeMap, BTreeSet};

use crate::gemini::generate_content::response::ResponseBody as GeminiGenerateContentResponseBody;
use crate::gemini::generate_content::types::{
    GeminiBlockReason, GeminiCandidate, GeminiFinishReason,
};
use crate::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
use crate::gemini::stream_generate_content::stream::{
    GeminiNdjsonStreamBody, GeminiSseEvent, GeminiSseEventData, GeminiSseStreamBody,
};
use crate::openai::create_chat_completions::stream::{
    ChatCompletionChunk, ChatCompletionChunkChoice, ChatCompletionChunkDelta,
    ChatCompletionChunkDeltaToolCall, ChatCompletionChunkDeltaToolCallType,
    ChatCompletionFunctionCallDelta, OpenAiChatCompletionsSseData, OpenAiChatCompletionsSseEvent,
    OpenAiChatCompletionsSseStreamBody,
};
use crate::openai::create_chat_completions::types as ct;
use crate::transform::openai::generate_content::openai_chat_completions::gemini::utils::{
    gemini_citation_annotations, gemini_function_response_to_text, gemini_logprobs,
    json_object_to_string, prompt_feedback_refusal_text,
};
use crate::transform::openai::model_list::gemini::utils::strip_models_prefix;
use crate::transform::utils::TransformError;

#[derive(Debug, Clone)]
struct OpenAiChatToolState {
    choice_index: u32,
    tool_index: u32,
    call_id: String,
    name: String,
    name_emitted: bool,
    arguments_snapshot: String,
}

#[derive(Debug, Default, Clone)]
pub struct GeminiToOpenAiChatCompletionsStream {
    response_id: String,
    model: String,
    created: u64,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
    incomplete_finish_reason: Option<ct::ChatCompletionFinishReason>,
    choice_finish_reasons: BTreeMap<u32, ct::ChatCompletionFinishReason>,
    output_choice_map: BTreeMap<u64, u32>,
    role_emitted: BTreeSet<u32>,
    choice_tool_counts: BTreeMap<u32, u32>,
    choice_has_tool_calls: BTreeSet<u32>,
    tool_states: BTreeMap<String, OpenAiChatToolState>,
    chunk_sequence: u64,
    finished: bool,
}

impl GeminiToOpenAiChatCompletionsStream {
    pub fn is_finished(&self) -> bool {
        self.finished
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
            "gemini".to_string()
        } else {
            self.model.clone()
        }
    }

    fn usage(&self) -> Option<ct::CompletionUsage> {
        if self.input_tokens == 0
            && self.cached_input_tokens == 0
            && self.output_tokens == 0
            && self.reasoning_tokens == 0
        {
            return None;
        }

        Some(ct::CompletionUsage {
            completion_tokens: self.output_tokens,
            prompt_tokens: self.input_tokens,
            total_tokens: self.input_tokens.saturating_add(self.output_tokens),
            completion_tokens_details: Some(ct::CompletionTokensDetails {
                accepted_prediction_tokens: None,
                audio_tokens: None,
                reasoning_tokens: Some(self.reasoning_tokens),
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
        logprobs: Option<ct::ChatCompletionLogprobs>,
    ) -> OpenAiChatCompletionsSseEvent {
        OpenAiChatCompletionsSseEvent {
            event: None,
            data: OpenAiChatCompletionsSseData::Chunk(ChatCompletionChunk {
                id: self.fallback_response_id(),
                choices: vec![ChatCompletionChunkChoice {
                    delta,
                    finish_reason,
                    index,
                    logprobs,
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
        if let Some(choice_index) = self.output_choice_map.get(&output_index) {
            return *choice_index;
        }

        let choice_index = u32::try_from(self.output_choice_map.len()).unwrap_or(u32::MAX);
        self.output_choice_map.insert(output_index, choice_index);
        choice_index
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
            None,
        ));
    }

    fn emit_annotations(
        &mut self,
        output_index: u64,
        annotations: Vec<ct::ChatCompletionAnnotation>,
        out: &mut Vec<OpenAiChatCompletionsSseEvent>,
    ) {
        if annotations.is_empty() {
            return;
        }

        let choice_index = self.ensure_choice_index(output_index);
        self.maybe_emit_role(out, choice_index);
        out.push(self.chunk_event(
            choice_index,
            ChatCompletionChunkDelta {
                annotations: Some(annotations),
                ..Default::default()
            },
            None,
            None,
            None,
        ));
    }

    fn emit_logprobs(
        &mut self,
        output_index: u64,
        logprobs: ct::ChatCompletionLogprobs,
        out: &mut Vec<OpenAiChatCompletionsSseEvent>,
    ) {
        let choice_index = self.ensure_choice_index(output_index);
        self.maybe_emit_role(out, choice_index);
        out.push(self.chunk_event(choice_index, Default::default(), None, None, Some(logprobs)));
    }

    fn emit_error_refusal(&mut self, text: String, out: &mut Vec<OpenAiChatCompletionsSseEvent>) {
        self.emit_content(0, text, true, out);
    }

    fn emit_function_call_snapshot(
        &mut self,
        output_index: u64,
        call_id: String,
        name: String,
        arguments_snapshot: String,
        out: &mut Vec<OpenAiChatCompletionsSseEvent>,
    ) {
        if let Some(state) = self.tool_states.get_mut(&call_id) {
            if !name.is_empty() {
                state.name = name;
            }

            let delta = if arguments_snapshot.starts_with(&state.arguments_snapshot) {
                arguments_snapshot[state.arguments_snapshot.len()..].to_string()
            } else {
                arguments_snapshot.clone()
            };
            state.arguments_snapshot = arguments_snapshot;

            if delta.is_empty() {
                return;
            }

            let state_snapshot = state.clone();
            self.maybe_emit_role(out, state_snapshot.choice_index);
            out.push(self.chunk_event(
                state_snapshot.choice_index,
                ChatCompletionChunkDelta {
                    tool_calls: Some(vec![ChatCompletionChunkDeltaToolCall {
                        index: state_snapshot.tool_index,
                        id: Some(state_snapshot.call_id.clone()),
                        function: Some(ChatCompletionFunctionCallDelta {
                            name: if state_snapshot.name_emitted {
                                None
                            } else {
                                Some(state_snapshot.name.clone())
                            },
                            arguments: Some(delta),
                        }),
                        type_: Some(ChatCompletionChunkDeltaToolCallType::Function),
                    }]),
                    ..Default::default()
                },
                None,
                None,
                None,
            ));

            if let Some(tool_state) = self.tool_states.get_mut(&call_id) {
                tool_state.name_emitted = true;
            }
            return;
        }

        let choice_index = self.ensure_choice_index(output_index);
        self.maybe_emit_role(out, choice_index);

        let tool_index_ref = self.choice_tool_counts.entry(choice_index).or_insert(0);
        let tool_index = *tool_index_ref;
        *tool_index_ref = tool_index.saturating_add(1);
        self.choice_has_tool_calls.insert(choice_index);

        let state = OpenAiChatToolState {
            choice_index,
            tool_index,
            call_id: call_id.clone(),
            name,
            name_emitted: false,
            arguments_snapshot: arguments_snapshot.clone(),
        };
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
            None,
        ));

        if let Some(tool_state) = self.tool_states.get_mut(&call_id) {
            tool_state.name_emitted = true;
        }

        if !arguments_snapshot.is_empty() && arguments_snapshot != "{}" {
            out.push(self.chunk_event(
                choice_index,
                ChatCompletionChunkDelta {
                    tool_calls: Some(vec![ChatCompletionChunkDeltaToolCall {
                        index: state.tool_index,
                        id: Some(state.call_id.clone()),
                        function: Some(ChatCompletionFunctionCallDelta {
                            name: None,
                            arguments: Some(arguments_snapshot),
                        }),
                        type_: Some(ChatCompletionChunkDeltaToolCallType::Function),
                    }]),
                    ..Default::default()
                },
                None,
                None,
                None,
            ));
        }
    }

    fn update_envelope_from_chunk(&mut self, chunk: &GeminiGenerateContentResponseBody) {
        if let Some(response_id) = chunk.response_id.as_ref() {
            self.response_id = response_id.clone();
        }
        if let Some(model_version) = chunk.model_version.as_ref() {
            self.model = strip_models_prefix(model_version);
        }
        if let Some(usage) = chunk.usage_metadata.as_ref() {
            self.input_tokens = usage
                .prompt_token_count
                .unwrap_or(0)
                .saturating_add(usage.tool_use_prompt_token_count.unwrap_or(0));
            self.cached_input_tokens = usage.cached_content_token_count.unwrap_or(0);
            self.output_tokens = usage
                .candidates_token_count
                .unwrap_or(0)
                .saturating_add(usage.thoughts_token_count.unwrap_or(0));
            self.reasoning_tokens = usage.thoughts_token_count.unwrap_or(0);
        }
    }

    fn map_finish_reason(reason: &GeminiFinishReason) -> ct::ChatCompletionFinishReason {
        match reason {
            GeminiFinishReason::MaxTokens => ct::ChatCompletionFinishReason::Length,
            GeminiFinishReason::Safety
            | GeminiFinishReason::Recitation
            | GeminiFinishReason::Blocklist
            | GeminiFinishReason::ProhibitedContent
            | GeminiFinishReason::Spii
            | GeminiFinishReason::ImageSafety
            | GeminiFinishReason::ImageProhibitedContent
            | GeminiFinishReason::ImageRecitation => ct::ChatCompletionFinishReason::ContentFilter,
            GeminiFinishReason::MalformedFunctionCall
            | GeminiFinishReason::UnexpectedToolCall
            | GeminiFinishReason::TooManyToolCalls => ct::ChatCompletionFinishReason::ToolCalls,
            GeminiFinishReason::Stop
            | GeminiFinishReason::FinishReasonUnspecified
            | GeminiFinishReason::Language
            | GeminiFinishReason::Other
            | GeminiFinishReason::ImageOther
            | GeminiFinishReason::NoImage
            | GeminiFinishReason::MissingThoughtSignature => ct::ChatCompletionFinishReason::Stop,
        }
    }

    fn map_block_reason(reason: &GeminiBlockReason) -> Option<ct::ChatCompletionFinishReason> {
        match reason {
            GeminiBlockReason::Safety
            | GeminiBlockReason::Blocklist
            | GeminiBlockReason::ProhibitedContent
            | GeminiBlockReason::ImageSafety => Some(ct::ChatCompletionFinishReason::ContentFilter),
            _ => None,
        }
    }

    fn on_chunk(
        &mut self,
        chunk: GeminiGenerateContentResponseBody,
    ) -> Result<Vec<OpenAiChatCompletionsSseEvent>, TransformError> {
        if self.finished {
            return Ok(Vec::new());
        }

        self.update_envelope_from_chunk(&chunk);
        let mut out = Vec::new();

        if let Some(reason) = chunk
            .prompt_feedback
            .as_ref()
            .and_then(|feedback| feedback.block_reason.as_ref())
            .and_then(Self::map_block_reason)
        {
            self.incomplete_finish_reason = Some(reason);
        }

        if let Some(refusal_text) = prompt_feedback_refusal_text(chunk.prompt_feedback.as_ref())
            && !refusal_text.is_empty()
        {
            self.emit_content(0, refusal_text, true, &mut out);
        }

        if let Some(model_status_message) = chunk
            .model_status
            .as_ref()
            .and_then(|status| status.message.as_ref())
            && !model_status_message.is_empty()
        {
            self.emit_content(
                0,
                format!("model_status: {model_status_message}"),
                false,
                &mut out,
            );
        }

        if let Some(candidates) = chunk.candidates {
            for (candidate_pos, candidate) in candidates.into_iter().enumerate() {
                let output_index = candidate.index.unwrap_or(candidate_pos as u32) as u64;
                self.process_candidate(output_index, candidate, &mut out);
            }
        }

        self.chunk_sequence = self.chunk_sequence.saturating_add(1);
        Ok(out)
    }

    fn process_candidate(
        &mut self,
        output_index: u64,
        candidate: GeminiCandidate,
        out: &mut Vec<OpenAiChatCompletionsSseEvent>,
    ) {
        let choice_index = self.ensure_choice_index(output_index);
        let GeminiCandidate {
            content,
            finish_reason,
            citation_metadata,
            logprobs_result,
            finish_message,
            ..
        } = candidate;

        if let Some(content) = content {
            for (part_index, part) in content.parts.into_iter().enumerate() {
                if part.thought.unwrap_or(false) {
                    continue;
                }

                if let Some(function_call) = part.function_call {
                    let call_id = function_call.id.unwrap_or_else(|| {
                        format!(
                            "tool_call_{}_{}_{}",
                            output_index, self.chunk_sequence, part_index
                        )
                    });
                    let arguments_snapshot = function_call
                        .args
                        .as_ref()
                        .map(json_object_to_string)
                        .unwrap_or_else(|| "{}".to_string());
                    self.emit_function_call_snapshot(
                        output_index,
                        call_id,
                        function_call.name,
                        arguments_snapshot,
                        out,
                    );
                    continue;
                }

                if let Some(function_response) = part.function_response {
                    self.emit_content(
                        output_index,
                        gemini_function_response_to_text(function_response),
                        false,
                        out,
                    );
                    continue;
                }

                if let Some(executable_code) = part.executable_code {
                    self.emit_content(output_index, executable_code.code, false, out);
                    continue;
                }

                if let Some(code_execution_result) = part.code_execution_result {
                    if let Some(output_text) = code_execution_result.output {
                        self.emit_content(output_index, output_text, false, out);
                    }
                    continue;
                }

                if let Some(text) = part.text {
                    self.emit_content(output_index, text, false, out);
                    continue;
                }

                if let Some(inline_data) = part.inline_data {
                    self.emit_content(
                        output_index,
                        format!("data:{};base64,{}", inline_data.mime_type, inline_data.data),
                        false,
                        out,
                    );
                    continue;
                }

                if let Some(file_data) = part.file_data {
                    self.emit_content(output_index, file_data.file_uri, false, out);
                }
            }
        }

        if let Some(finish_message) = finish_message
            && !finish_message.is_empty()
        {
            self.emit_content(output_index, finish_message, false, out);
        }

        let annotations = gemini_citation_annotations(citation_metadata.as_ref());
        self.emit_annotations(output_index, annotations, out);

        if let Some(logprobs) = gemini_logprobs(logprobs_result.as_ref()) {
            self.emit_logprobs(output_index, logprobs, out);
        }

        if let Some(finish_reason) = finish_reason.as_ref() {
            self.choice_finish_reasons
                .insert(choice_index, Self::map_finish_reason(finish_reason));
        }
    }

    pub fn on_sse_event(
        &mut self,
        event: GeminiSseEvent,
    ) -> Result<Vec<OpenAiChatCompletionsSseEvent>, TransformError> {
        if self.finished {
            return Ok(Vec::new());
        }

        match event.data {
            GeminiSseEventData::Chunk(chunk) => self.on_chunk(chunk),
            GeminiSseEventData::Done(_) => Ok(self.finish()),
        }
    }

    pub fn finish(&mut self) -> Vec<OpenAiChatCompletionsSseEvent> {
        if self.finished {
            return Vec::new();
        }

        let mut out = Vec::new();
        let default_reason = self
            .incomplete_finish_reason
            .clone()
            .unwrap_or(ct::ChatCompletionFinishReason::Stop);

        let mut choices = self.output_choice_map.values().copied().collect::<Vec<_>>();
        choices.sort_unstable();
        choices.dedup();
        if choices.is_empty() {
            choices.push(0);
        }

        for choice_index in &choices {
            let finish_reason = self
                .choice_finish_reasons
                .get(choice_index)
                .cloned()
                .or_else(|| {
                    if self.choice_has_tool_calls.contains(choice_index) {
                        Some(ct::ChatCompletionFinishReason::ToolCalls)
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| default_reason.clone());
            out.push(self.chunk_event(
                *choice_index,
                Default::default(),
                Some(finish_reason),
                None,
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

impl TryFrom<GeminiStreamGenerateContentResponse> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: GeminiStreamGenerateContentResponse) -> Result<Self, TransformError> {
        let mut converter = GeminiToOpenAiChatCompletionsStream::default();
        let mut events = Vec::new();

        match value {
            GeminiStreamGenerateContentResponse::NdjsonSuccess { body, .. } => {
                for chunk in body.chunks {
                    events.extend(converter.on_chunk(chunk)?);
                }
            }
            GeminiStreamGenerateContentResponse::SseSuccess { body, .. } => {
                for event in body.events {
                    events.extend(converter.on_sse_event(event)?);
                }
            }
            GeminiStreamGenerateContentResponse::Error {
                stats_code, body, ..
            } => {
                let status = body
                    .error
                    .status
                    .clone()
                    .unwrap_or_else(|| stats_code.as_str().to_string());
                let message = format!(
                    "gemini_error status={status} code={} message={}",
                    body.error.code, body.error.message
                );
                converter.emit_error_refusal(message, &mut events);
                events.extend(converter.finish());
            }
        }

        if !converter.is_finished() {
            events.extend(converter.finish());
        }

        Ok(Self { events })
    }
}

impl TryFrom<GeminiSseStreamBody> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: GeminiSseStreamBody) -> Result<Self, TransformError> {
        let mut converter = GeminiToOpenAiChatCompletionsStream::default();
        let mut events = Vec::new();

        for event in value.events {
            events.extend(converter.on_sse_event(event)?);
        }

        if !converter.is_finished() {
            events.extend(converter.finish());
        }

        Ok(Self { events })
    }
}

impl TryFrom<GeminiNdjsonStreamBody> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: GeminiNdjsonStreamBody) -> Result<Self, TransformError> {
        let mut converter = GeminiToOpenAiChatCompletionsStream::default();
        let mut events = Vec::new();

        for chunk in value.chunks {
            events.extend(converter.on_chunk(chunk)?);
        }

        if !converter.is_finished() {
            events.extend(converter.finish());
        }

        Ok(Self { events })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gemini::count_tokens::types::{GeminiContent, GeminiContentRole, GeminiPart};
    use crate::gemini::generate_content::types::{
        GeminiCandidate, GeminiCitationMetadata, GeminiCitationSource, GeminiFinishReason,
        GeminiLogprobsCandidate, GeminiLogprobsResult, GeminiTopCandidates, GeminiUsageMetadata,
    };
    use crate::gemini::types::{GeminiApiError, GeminiApiErrorResponse, GeminiResponseHeaders};
    use crate::transform::gemini::stream_generate_content::utils::{chunk_event, done_event};

    #[test]
    fn gemini_stream_to_chat_stream_is_direct() {
        let chunk = GeminiGenerateContentResponseBody {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    parts: vec![GeminiPart {
                        text: Some("hello".to_string()),
                        ..GeminiPart::default()
                    }],
                    role: Some(GeminiContentRole::Model),
                }),
                finish_reason: Some(GeminiFinishReason::Stop),
                index: Some(0),
                ..GeminiCandidate::default()
            }]),
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: Some(6),
                cached_content_token_count: Some(1),
                candidates_token_count: Some(2),
                thoughts_token_count: Some(0),
                total_token_count: Some(8),
                ..GeminiUsageMetadata::default()
            }),
            model_version: Some("models/gemini-2.0-flash".to_string()),
            response_id: Some("resp_2".to_string()),
            ..GeminiGenerateContentResponseBody::default()
        };

        let stream = GeminiStreamGenerateContentResponse::SseSuccess {
            stats_code: http::StatusCode::OK,
            headers: Default::default(),
            body: GeminiSseStreamBody {
                events: vec![chunk_event(chunk), done_event()],
            },
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
                    chunk.usage.as_ref().map(|usage| usage.total_tokens),
                    Some(8)
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
    fn gemini_stream_citation_maps_to_chat_annotations() {
        let chunk = GeminiGenerateContentResponseBody {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    parts: vec![GeminiPart {
                        text: Some("hello".to_string()),
                        ..GeminiPart::default()
                    }],
                    role: Some(GeminiContentRole::Model),
                }),
                citation_metadata: Some(GeminiCitationMetadata {
                    citation_sources: Some(vec![GeminiCitationSource {
                        start_index: Some(0),
                        end_index: Some(5),
                        uri: Some("https://example.com/doc".to_string()),
                        license: None,
                    }]),
                }),
                finish_reason: Some(GeminiFinishReason::Stop),
                index: Some(0),
                ..GeminiCandidate::default()
            }]),
            ..GeminiGenerateContentResponseBody::default()
        };

        let stream = GeminiStreamGenerateContentResponse::SseSuccess {
            stats_code: http::StatusCode::OK,
            headers: Default::default(),
            body: GeminiSseStreamBody {
                events: vec![chunk_event(chunk), done_event()],
            },
        };

        let converted = OpenAiChatCompletionsSseStreamBody::try_from(stream).unwrap();
        let annotations = converted.events.iter().find_map(|event| match &event.data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                chunk.choices.first()?.delta.annotations.as_ref()
            }
            OpenAiChatCompletionsSseData::Done(_) => None,
        });

        let annotations = annotations.expect("expected annotations chunk");
        assert_eq!(annotations.len(), 1);
        assert_eq!(
            annotations[0].url_citation.url,
            "https://example.com/doc".to_string()
        );
        assert_eq!(annotations[0].url_citation.start_index, 0);
        assert_eq!(annotations[0].url_citation.end_index, 5);
    }

    #[test]
    fn gemini_stream_logprobs_maps_to_chat_logprobs() {
        let chunk = GeminiGenerateContentResponseBody {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    parts: vec![GeminiPart {
                        text: Some("h".to_string()),
                        ..GeminiPart::default()
                    }],
                    role: Some(GeminiContentRole::Model),
                }),
                logprobs_result: Some(GeminiLogprobsResult {
                    chosen_candidates: Some(vec![GeminiLogprobsCandidate {
                        token: Some("h".to_string()),
                        token_id: Some(1),
                        log_probability: Some(-0.1),
                    }]),
                    top_candidates: Some(vec![GeminiTopCandidates {
                        candidates: Some(vec![
                            GeminiLogprobsCandidate {
                                token: Some("h".to_string()),
                                token_id: Some(1),
                                log_probability: Some(-0.1),
                            },
                            GeminiLogprobsCandidate {
                                token: Some("e".to_string()),
                                token_id: Some(2),
                                log_probability: Some(-0.4),
                            },
                        ]),
                    }]),
                    ..GeminiLogprobsResult::default()
                }),
                finish_reason: Some(GeminiFinishReason::Stop),
                index: Some(0),
                ..GeminiCandidate::default()
            }]),
            ..GeminiGenerateContentResponseBody::default()
        };

        let stream = GeminiStreamGenerateContentResponse::SseSuccess {
            stats_code: http::StatusCode::OK,
            headers: Default::default(),
            body: GeminiSseStreamBody {
                events: vec![chunk_event(chunk), done_event()],
            },
        };

        let converted = OpenAiChatCompletionsSseStreamBody::try_from(stream).unwrap();
        let logprobs = converted.events.iter().find_map(|event| match &event.data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => chunk
                .choices
                .first()
                .and_then(|choice| choice.logprobs.as_ref()),
            OpenAiChatCompletionsSseData::Done(_) => None,
        });

        let logprobs = logprobs.expect("expected logprobs chunk");
        let content = logprobs
            .content
            .as_ref()
            .expect("expected content logprobs");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0].token, "h");
        assert_eq!(content[0].top_logprobs.len(), 2);
    }

    #[test]
    fn gemini_stream_unexpected_tool_call_maps_finish_reason_to_tool_calls() {
        let chunk = GeminiGenerateContentResponseBody {
            candidates: Some(vec![GeminiCandidate {
                finish_reason: Some(GeminiFinishReason::UnexpectedToolCall),
                index: Some(0),
                ..GeminiCandidate::default()
            }]),
            ..GeminiGenerateContentResponseBody::default()
        };

        let stream = GeminiStreamGenerateContentResponse::SseSuccess {
            stats_code: http::StatusCode::OK,
            headers: Default::default(),
            body: GeminiSseStreamBody {
                events: vec![chunk_event(chunk), done_event()],
            },
        };

        let converted = OpenAiChatCompletionsSseStreamBody::try_from(stream).unwrap();
        let finish_reason = converted.events.iter().find_map(|event| match &event.data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => chunk
                .choices
                .first()
                .and_then(|choice| choice.finish_reason.clone()),
            OpenAiChatCompletionsSseData::Done(_) => None,
        });

        assert_eq!(
            finish_reason,
            Some(ct::ChatCompletionFinishReason::ToolCalls)
        );
    }

    #[test]
    fn gemini_error_stream_emits_error_details_before_done() {
        let stream = GeminiStreamGenerateContentResponse::Error {
            stats_code: http::StatusCode::BAD_REQUEST,
            headers: GeminiResponseHeaders::default(),
            body: GeminiApiErrorResponse {
                error: GeminiApiError {
                    code: 400,
                    message: "bad prompt".to_string(),
                    status: Some("INVALID_ARGUMENT".to_string()),
                    details: None,
                },
            },
        };

        let converted = OpenAiChatCompletionsSseStreamBody::try_from(stream).unwrap();
        let refusal_text = converted.events.iter().find_map(|event| match &event.data {
            OpenAiChatCompletionsSseData::Chunk(chunk) => {
                chunk.choices.first()?.delta.refusal.clone()
            }
            OpenAiChatCompletionsSseData::Done(_) => None,
        });

        let refusal_text = refusal_text.expect("expected error refusal chunk");
        assert!(refusal_text.contains("INVALID_ARGUMENT"));
        assert!(refusal_text.contains("bad prompt"));
        assert!(matches!(
            converted.events.last().map(|event| &event.data),
            Some(OpenAiChatCompletionsSseData::Done(_))
        ));
    }
}
