use std::collections::BTreeMap;

use crate::claude::create_message::stream::{
    BetaRawContentBlockDelta, ClaudeCreateMessageSseStreamBody, ClaudeCreateMessageStreamEvent,
};
use crate::claude::create_message::types::{BetaContentBlock, BetaStopReason};
use crate::claude::types::BetaError;
use crate::gemini::count_tokens::types::{GeminiContentRole, GeminiFunctionCall, GeminiPart};
use crate::gemini::generate_content::response::ResponseBody as GeminiGenerateContentResponseBody;
use crate::gemini::generate_content::types::{
    GeminiBlockReason, GeminiCandidate, GeminiContent, GeminiFinishReason, GeminiPromptFeedback,
    GeminiUsageMetadata,
};
use crate::gemini::stream_generate_content::stream::{GeminiNdjsonStreamBody, GeminiSseStreamBody};
use crate::transform::claude::utils::claude_model_to_string;
use crate::transform::gemini::stream_generate_content::utils::{
    chunk_event, done_event, parse_json_object_or_empty, sse_body_to_ndjson_body,
};
use crate::transform::utils::TransformError;

#[derive(Debug, Clone)]
enum ClaudeBlockState {
    Thinking {
        signature: String,
    },
    ToolUse {
        id: String,
        name: String,
        partial_json: String,
    },
    Other,
}

#[derive(Debug, Default, Clone)]
pub struct ClaudeToGeminiStream {
    response_id: Option<String>,
    model_version: Option<String>,
    usage_metadata: Option<GeminiUsageMetadata>,
    blocks: BTreeMap<u64, ClaudeBlockState>,
    finished: bool,
}

impl ClaudeToGeminiStream {
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn usage_from_counts(
        input_tokens: u64,
        cached_tokens: u64,
        output_tokens: u64,
    ) -> GeminiUsageMetadata {
        GeminiUsageMetadata {
            prompt_token_count: Some(input_tokens),
            cached_content_token_count: Some(cached_tokens),
            candidates_token_count: Some(output_tokens),
            total_token_count: Some(input_tokens.saturating_add(output_tokens)),
            ..GeminiUsageMetadata::default()
        }
    }

    fn finish_reason_from_stop_reason(stop_reason: Option<BetaStopReason>) -> GeminiFinishReason {
        match stop_reason {
            Some(BetaStopReason::MaxTokens) | Some(BetaStopReason::ModelContextWindowExceeded) => {
                GeminiFinishReason::MaxTokens
            }
            Some(BetaStopReason::ToolUse) => GeminiFinishReason::UnexpectedToolCall,
            Some(BetaStopReason::Refusal) => GeminiFinishReason::Safety,
            Some(BetaStopReason::Compaction) | Some(BetaStopReason::PauseTurn) => {
                GeminiFinishReason::Other
            }
            Some(BetaStopReason::EndTurn) | Some(BetaStopReason::StopSequence) | None => {
                GeminiFinishReason::Stop
            }
        }
    }

    fn error_message(error: BetaError) -> String {
        match error {
            BetaError::InvalidRequest(error) => error.message,
            BetaError::Authentication(error) => error.message,
            BetaError::Billing(error) => error.message,
            BetaError::Permission(error) => error.message,
            BetaError::NotFound(error) => error.message,
            BetaError::RateLimit(error) => error.message,
            BetaError::GatewayTimeout(error) => error.message,
            BetaError::Api(error) => error.message,
            BetaError::Overloaded(error) => error.message,
        }
    }

    fn chunk_from_parts(
        &self,
        parts: Vec<GeminiPart>,
        finish_reason: Option<GeminiFinishReason>,
        prompt_feedback: Option<GeminiPromptFeedback>,
    ) -> GeminiGenerateContentResponseBody {
        GeminiGenerateContentResponseBody {
            candidates: Some(vec![GeminiCandidate {
                content: Some(GeminiContent {
                    parts,
                    role: Some(GeminiContentRole::Model),
                }),
                finish_reason,
                index: Some(0),
                ..GeminiCandidate::default()
            }]),
            prompt_feedback,
            usage_metadata: self.usage_metadata.clone(),
            model_version: self.model_version.clone(),
            response_id: self.response_id.clone(),
            model_status: None,
        }
    }

    fn text_chunk(&self, text: String) -> Option<GeminiGenerateContentResponseBody> {
        if text.is_empty() {
            None
        } else {
            Some(self.chunk_from_parts(
                vec![GeminiPart {
                    text: Some(text),
                    ..GeminiPart::default()
                }],
                None,
                None,
            ))
        }
    }

    fn thinking_chunk(
        &self,
        signature: String,
        thinking: String,
    ) -> Option<GeminiGenerateContentResponseBody> {
        if thinking.is_empty() {
            None
        } else {
            Some(self.chunk_from_parts(
                vec![GeminiPart {
                    thought: Some(true),
                    thought_signature: Some(signature),
                    text: Some(thinking),
                    ..GeminiPart::default()
                }],
                None,
                None,
            ))
        }
    }

    fn function_call_chunk(
        &self,
        id: String,
        name: String,
        arguments: String,
    ) -> GeminiGenerateContentResponseBody {
        self.chunk_from_parts(
            vec![GeminiPart {
                function_call: Some(GeminiFunctionCall {
                    id: Some(id),
                    name,
                    args: Some(parse_json_object_or_empty(&arguments)),
                }),
                ..GeminiPart::default()
            }],
            None,
            None,
        )
    }

    pub fn on_event(
        &mut self,
        event: ClaudeCreateMessageStreamEvent,
    ) -> Result<Vec<crate::gemini::stream_generate_content::stream::GeminiSseEvent>, TransformError>
    {
        if self.finished {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        match event {
            ClaudeCreateMessageStreamEvent::MessageStart(event) => {
                self.response_id = Some(event.message.id);
                self.model_version = Some(claude_model_to_string(&event.message.model));
                self.usage_metadata = Some(Self::usage_from_counts(
                    event.message.usage.input_tokens,
                    event.message.usage.cache_read_input_tokens,
                    event.message.usage.output_tokens,
                ));
            }
            ClaudeCreateMessageStreamEvent::ContentBlockStart(event) => {
                let state = match event.content_block {
                    BetaContentBlock::Thinking(block) => ClaudeBlockState::Thinking {
                        signature: block.signature,
                    },
                    BetaContentBlock::ToolUse(block) => ClaudeBlockState::ToolUse {
                        id: block.id,
                        name: block.name,
                        partial_json: String::new(),
                    },
                    _ => ClaudeBlockState::Other,
                };
                self.blocks.insert(event.index, state);
            }
            ClaudeCreateMessageStreamEvent::ContentBlockDelta(event) => match event.delta {
                BetaRawContentBlockDelta::Text(delta) => {
                    if let Some(chunk) = self.text_chunk(delta.text) {
                        out.push(chunk_event(chunk));
                    }
                }
                BetaRawContentBlockDelta::Thinking(delta) => {
                    let signature = match self.blocks.get(&event.index) {
                        Some(ClaudeBlockState::Thinking { signature }) => signature.clone(),
                        _ => format!("thought_{}", event.index),
                    };
                    if let Some(chunk) = self.thinking_chunk(signature, delta.thinking) {
                        out.push(chunk_event(chunk));
                    }
                }
                BetaRawContentBlockDelta::InputJson(delta) => {
                    let mut tool_snapshot = None;
                    if let Some(ClaudeBlockState::ToolUse {
                        id,
                        name,
                        partial_json,
                    }) = self.blocks.get_mut(&event.index)
                    {
                        partial_json.push_str(&delta.partial_json);
                        tool_snapshot = Some((id.clone(), name.clone(), partial_json.clone()));
                    }
                    if let Some((id, name, arguments)) = tool_snapshot {
                        out.push(chunk_event(self.function_call_chunk(id, name, arguments)));
                    }
                }
                BetaRawContentBlockDelta::Signature(delta) => {
                    if let Some(ClaudeBlockState::Thinking { signature }) =
                        self.blocks.get_mut(&event.index)
                    {
                        *signature = delta.signature;
                    }
                }
                BetaRawContentBlockDelta::Compaction(delta) => {
                    if let Some(content) = delta.content
                        && let Some(chunk) = self.text_chunk(content)
                    {
                        out.push(chunk_event(chunk));
                    }
                }
                BetaRawContentBlockDelta::Citations(_) => {}
            },
            ClaudeCreateMessageStreamEvent::ContentBlockStop(event) => {
                self.blocks.remove(&event.index);
            }
            ClaudeCreateMessageStreamEvent::MessageDelta(event) => {
                self.usage_metadata = Some(Self::usage_from_counts(
                    event.usage.input_tokens.unwrap_or(0),
                    event.usage.cache_read_input_tokens.unwrap_or(0),
                    event.usage.output_tokens,
                ));

                let finish_reason = Self::finish_reason_from_stop_reason(event.delta.stop_reason);
                let prompt_feedback = if matches!(finish_reason, GeminiFinishReason::Safety) {
                    Some(GeminiPromptFeedback {
                        block_reason: Some(GeminiBlockReason::Safety),
                        safety_ratings: None,
                    })
                } else {
                    None
                };

                out.push(chunk_event(self.chunk_from_parts(
                    Vec::new(),
                    Some(finish_reason),
                    prompt_feedback,
                )));
            }
            ClaudeCreateMessageStreamEvent::MessageStop(_) => {
                out.push(done_event());
                self.finished = true;
            }
            ClaudeCreateMessageStreamEvent::Error(event) => {
                let message = Self::error_message(event.error);
                if let Some(chunk) = self.text_chunk(message) {
                    out.push(chunk_event(chunk));
                }
                out.push(done_event());
                self.finished = true;
            }
            ClaudeCreateMessageStreamEvent::Ping(_)
            | ClaudeCreateMessageStreamEvent::Unknown(_) => {}
        }

        Ok(out)
    }
}

impl TryFrom<Vec<ClaudeCreateMessageStreamEvent>> for GeminiSseStreamBody {
    type Error = TransformError;

    fn try_from(value: Vec<ClaudeCreateMessageStreamEvent>) -> Result<Self, TransformError> {
        let mut converter = ClaudeToGeminiStream::default();
        let mut events = Vec::new();
        for event in value {
            events.extend(converter.on_event(event)?);
        }
        if !converter.is_finished() {
            events.push(done_event());
        }
        Ok(GeminiSseStreamBody { events })
    }
}

impl TryFrom<ClaudeCreateMessageSseStreamBody> for GeminiSseStreamBody {
    type Error = TransformError;

    fn try_from(value: ClaudeCreateMessageSseStreamBody) -> Result<Self, TransformError> {
        Self::try_from(value.events)
    }
}

impl TryFrom<Vec<ClaudeCreateMessageStreamEvent>> for GeminiNdjsonStreamBody {
    type Error = TransformError;

    fn try_from(value: Vec<ClaudeCreateMessageStreamEvent>) -> Result<Self, TransformError> {
        let sse_body = GeminiSseStreamBody::try_from(value)?;
        Ok(sse_body_to_ndjson_body(&sse_body))
    }
}

impl TryFrom<ClaudeCreateMessageSseStreamBody> for GeminiNdjsonStreamBody {
    type Error = TransformError;

    fn try_from(value: ClaudeCreateMessageSseStreamBody) -> Result<Self, TransformError> {
        let sse_body = GeminiSseStreamBody::try_from(value)?;
        Ok(sse_body_to_ndjson_body(&sse_body))
    }
}
