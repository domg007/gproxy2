use crate::claude::create_message::response::ClaudeCreateMessageResponse;
use crate::claude::create_message::stream::{
    BetaCitationsDelta, BetaCitationsDeltaType, BetaCompactionContentBlockDelta,
    BetaCompactionContentBlockDeltaType, BetaInputJsonDelta, BetaInputJsonDeltaType,
    BetaMessageDeltaUsage, BetaRawContentBlockDelta, BetaRawContentBlockDeltaEvent,
    BetaRawContentBlockDeltaEventType, BetaRawContentBlockStartEvent,
    BetaRawContentBlockStartEventType, BetaRawContentBlockStopEvent,
    BetaRawContentBlockStopEventType, BetaRawMessageDelta, BetaRawMessageDeltaEvent,
    BetaRawMessageDeltaEventType, BetaRawMessageStartEvent, BetaRawMessageStartEventType,
    BetaRawMessageStopEvent, BetaRawMessageStopEventType, BetaSignatureDelta,
    BetaSignatureDeltaType, BetaStreamErrorEvent, BetaStreamErrorEventType, BetaTextDelta,
    BetaTextDeltaType, BetaThinkingDelta, BetaThinkingDeltaType, ClaudeCreateMessageSseStreamBody,
    ClaudeCreateMessageStreamEvent,
};
use crate::claude::create_message::types::BetaContentBlock;
use crate::transform::utils::TransformError;

fn stream_start_content_block(content_block: &BetaContentBlock) -> BetaContentBlock {
    match content_block {
        BetaContentBlock::Text(block) => {
            BetaContentBlock::Text(crate::claude::create_message::types::BetaTextBlock {
                citations: None,
                text: String::new(),
                type_: block.type_.clone(),
            })
        }
        BetaContentBlock::Thinking(block) => {
            BetaContentBlock::Thinking(crate::claude::create_message::types::BetaThinkingBlock {
                signature: block.signature.clone(),
                thinking: String::new(),
                type_: block.type_.clone(),
            })
        }
        BetaContentBlock::ToolUse(block) => {
            BetaContentBlock::ToolUse(crate::claude::create_message::types::BetaToolUseBlock {
                id: block.id.clone(),
                input: Default::default(),
                name: block.name.clone(),
                type_: block.type_.clone(),
                cache_control: block.cache_control.clone(),
                caller: block.caller.clone(),
            })
        }
        BetaContentBlock::Compaction(block) => BetaContentBlock::Compaction(
            crate::claude::create_message::types::BetaCompactionBlock {
                content: None,
                type_: block.type_.clone(),
                cache_control: block.cache_control.clone(),
            },
        ),
        _ => content_block.clone(),
    }
}

fn push_content_block_delta_events(
    events: &mut Vec<ClaudeCreateMessageStreamEvent>,
    index: u64,
    content_block: &BetaContentBlock,
) {
    match content_block {
        BetaContentBlock::Text(block) => {
            if !block.text.is_empty() {
                events.push(ClaudeCreateMessageStreamEvent::ContentBlockDelta(
                    BetaRawContentBlockDeltaEvent {
                        delta: BetaRawContentBlockDelta::Text(BetaTextDelta {
                            text: block.text.clone(),
                            type_: BetaTextDeltaType::TextDelta,
                        }),
                        index,
                        type_: BetaRawContentBlockDeltaEventType::ContentBlockDelta,
                    },
                ));
            }
            if let Some(citations) = block.citations.as_ref() {
                for citation in citations {
                    events.push(ClaudeCreateMessageStreamEvent::ContentBlockDelta(
                        BetaRawContentBlockDeltaEvent {
                            delta: BetaRawContentBlockDelta::Citations(BetaCitationsDelta {
                                citation: citation.clone(),
                                type_: BetaCitationsDeltaType::CitationsDelta,
                            }),
                            index,
                            type_: BetaRawContentBlockDeltaEventType::ContentBlockDelta,
                        },
                    ));
                }
            }
        }
        BetaContentBlock::Thinking(block) => {
            if !block.thinking.is_empty() {
                events.push(ClaudeCreateMessageStreamEvent::ContentBlockDelta(
                    BetaRawContentBlockDeltaEvent {
                        delta: BetaRawContentBlockDelta::Thinking(BetaThinkingDelta {
                            thinking: block.thinking.clone(),
                            type_: BetaThinkingDeltaType::ThinkingDelta,
                        }),
                        index,
                        type_: BetaRawContentBlockDeltaEventType::ContentBlockDelta,
                    },
                ));
            }
            if !block.signature.is_empty() {
                events.push(ClaudeCreateMessageStreamEvent::ContentBlockDelta(
                    BetaRawContentBlockDeltaEvent {
                        delta: BetaRawContentBlockDelta::Signature(BetaSignatureDelta {
                            signature: block.signature.clone(),
                            type_: BetaSignatureDeltaType::SignatureDelta,
                        }),
                        index,
                        type_: BetaRawContentBlockDeltaEventType::ContentBlockDelta,
                    },
                ));
            }
        }
        BetaContentBlock::ToolUse(block) => {
            if !block.input.is_empty()
                && let Ok(input_json) = serde_json::to_string(&block.input)
                && !input_json.is_empty()
                && input_json != "{}"
            {
                events.push(ClaudeCreateMessageStreamEvent::ContentBlockDelta(
                    BetaRawContentBlockDeltaEvent {
                        delta: BetaRawContentBlockDelta::InputJson(BetaInputJsonDelta {
                            partial_json: input_json,
                            type_: BetaInputJsonDeltaType::InputJsonDelta,
                        }),
                        index,
                        type_: BetaRawContentBlockDeltaEventType::ContentBlockDelta,
                    },
                ));
            }
        }
        BetaContentBlock::Compaction(block) => {
            if block.content.is_some() {
                events.push(ClaudeCreateMessageStreamEvent::ContentBlockDelta(
                    BetaRawContentBlockDeltaEvent {
                        delta: BetaRawContentBlockDelta::Compaction(
                            BetaCompactionContentBlockDelta {
                                content: block.content.clone(),
                                type_: BetaCompactionContentBlockDeltaType::CompactionDelta,
                            },
                        ),
                        index,
                        type_: BetaRawContentBlockDeltaEventType::ContentBlockDelta,
                    },
                ));
            }
        }
        _ => {}
    }
}

impl TryFrom<ClaudeCreateMessageResponse> for Vec<ClaudeCreateMessageStreamEvent> {
    type Error = TransformError;

    fn try_from(value: ClaudeCreateMessageResponse) -> Result<Self, TransformError> {
        Ok(match value {
            ClaudeCreateMessageResponse::Success { body, .. } => {
                let mut events = Vec::new();

                let mut start_message = body.clone();
                start_message.content = Vec::new();
                start_message.context_management = None;
                start_message.stop_reason = None;
                start_message.stop_sequence = None;
                start_message.usage.output_tokens = 0;

                events.push(ClaudeCreateMessageStreamEvent::MessageStart(
                    BetaRawMessageStartEvent {
                        message: start_message,
                        type_: BetaRawMessageStartEventType::MessageStart,
                    },
                ));

                for (index, content_block) in body.content.iter().enumerate() {
                    let index = index as u64;
                    events.push(ClaudeCreateMessageStreamEvent::ContentBlockStart(
                        BetaRawContentBlockStartEvent {
                            content_block: stream_start_content_block(content_block),
                            index,
                            type_: BetaRawContentBlockStartEventType::ContentBlockStart,
                        },
                    ));

                    push_content_block_delta_events(&mut events, index, content_block);

                    events.push(ClaudeCreateMessageStreamEvent::ContentBlockStop(
                        BetaRawContentBlockStopEvent {
                            index,
                            type_: BetaRawContentBlockStopEventType::ContentBlockStop,
                        },
                    ));
                }

                events.push(ClaudeCreateMessageStreamEvent::MessageDelta(
                    BetaRawMessageDeltaEvent {
                        context_management: body.context_management.clone(),
                        delta: BetaRawMessageDelta {
                            container: body.container.clone(),
                            stop_reason: body.stop_reason.clone(),
                            stop_sequence: body.stop_sequence.clone(),
                        },
                        type_: BetaRawMessageDeltaEventType::MessageDelta,
                        usage: BetaMessageDeltaUsage {
                            cache_creation_input_tokens: Some(
                                body.usage.cache_creation_input_tokens,
                            ),
                            cache_read_input_tokens: Some(body.usage.cache_read_input_tokens),
                            input_tokens: Some(body.usage.input_tokens),
                            iterations: Some(body.usage.iterations.clone()),
                            output_tokens: body.usage.output_tokens,
                            server_tool_use: Some(body.usage.server_tool_use.clone()),
                        },
                    },
                ));

                events.push(ClaudeCreateMessageStreamEvent::MessageStop(
                    BetaRawMessageStopEvent {
                        type_: BetaRawMessageStopEventType::MessageStop,
                    },
                ));

                events
            }
            ClaudeCreateMessageResponse::Error { body, .. } => {
                vec![ClaudeCreateMessageStreamEvent::Error(
                    BetaStreamErrorEvent {
                        error: body.error,
                        type_: BetaStreamErrorEventType::Error,
                    },
                )]
            }
        })
    }
}

impl TryFrom<ClaudeCreateMessageResponse> for ClaudeCreateMessageSseStreamBody {
    type Error = TransformError;

    fn try_from(value: ClaudeCreateMessageResponse) -> Result<Self, TransformError> {
        Ok(Self {
            events: Vec::<ClaudeCreateMessageStreamEvent>::try_from(value)?,
        })
    }
}
