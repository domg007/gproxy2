use gproxy_protocol::claude::create_message::stream::{
    BetaServerToolUseBlockStream, BetaStreamContentBlock, BetaStreamContentBlockDelta,
    BetaStreamEvent, BetaStreamEventKnown, BetaStreamMessage, BetaStreamMessageDelta,
    BetaStreamUsage, BetaThinkingBlockStream,
};
use gproxy_protocol::claude::create_message::types::{BetaContentBlock, BetaMessage, JsonObject};
use gproxy_protocol::openai::create_chat_completions::response::CreateChatCompletionResponse as OpenAIChatCompletionResponse;
use gproxy_protocol::openai::create_chat_completions::stream::{
    ChatCompletionChunkObjectType, ChatCompletionStreamChoice, CreateChatCompletionStreamResponse,
};
use gproxy_protocol::openai::create_chat_completions::types::{
    ChatCompletionFunctionCallDelta, ChatCompletionMessageToolCall,
    ChatCompletionMessageToolCallChunk, ChatCompletionMessageToolCallChunkFunction,
    ChatCompletionResponseMessage, ChatCompletionResponseRole, ChatCompletionRole,
    ChatCompletionStreamResponseDelta, ChatCompletionToolCallChunkType, CompletionUsage,
};
use gproxy_protocol::openai::create_response::response::Response as OpenAIResponse;
use gproxy_protocol::openai::create_response::stream::{
    ResponseCompletedEvent, ResponseCreatedEvent, ResponseCustomToolCallInputDeltaEvent,
    ResponseCustomToolCallInputDoneEvent, ResponseFunctionCallArgumentsDeltaEvent,
    ResponseFunctionCallArgumentsDoneEvent, ResponseMCPCallArgumentsDeltaEvent,
    ResponseMCPCallArgumentsDoneEvent, ResponseOutputItemAddedEvent, ResponseOutputItemDoneEvent,
    ResponseRefusalDeltaEvent, ResponseRefusalDoneEvent, ResponseStreamEvent,
    ResponseTextDeltaEvent, ResponseTextDoneEvent,
};
use gproxy_protocol::openai::create_response::types::{
    CustomToolCall, FunctionToolCall, MCPToolCall, OutputItem, OutputMessage, OutputMessageContent,
};

use super::helpers::ensure_generate_proto;
use super::ops::transform_response;
use super::types::{
    GenerateContentResponse, Op, Proto, Response, StreamEvent, TransformContext, TransformError,
};

use crate::generate_content;
use crate::stream2nostream;

pub enum StreamTransformer {
    Passthrough(Proto),
    ClaudeToOpenAIChat(generate_content::claude2openai_chat_completions::stream::ClaudeToOpenAIChatCompletionStreamState),
    ClaudeToOpenAIResponse(generate_content::claude2openai_response::stream::ClaudeToOpenAIResponseStreamState),
    ClaudeToGemini(generate_content::gemini2claude::stream::ClaudeToGeminiStreamState),
    OpenAIChatToClaude(generate_content::openai_chat_completions2claude::stream::OpenAIToClaudeChatCompletionStreamState),
    OpenAIChatToOpenAIResponse(generate_content::openai_chat_completions2openai_response::stream::OpenAIChatCompletionToResponseStreamState),
    OpenAIChatToGemini(generate_content::openai_chat_completions2gemini::stream::OpenAIChatCompletionToGeminiStreamState),
    OpenAIResponseToClaude(generate_content::openai_response2claude::stream::OpenAIResponseToClaudeStreamState),
    OpenAIResponseToOpenAIChat(generate_content::openai_response2openai_chat_completions::stream::OpenAIResponseToChatCompletionStreamState),
    OpenAIResponseToGemini(generate_content::openai_response2gemini::stream::OpenAIResponseToGeminiStreamState),
    GeminiToClaude(generate_content::claude2gemini::stream::GeminiToClaudeStreamState),
    GeminiToOpenAIChat(generate_content::gemini2openai_chat_completions::stream::GeminiToOpenAIChatCompletionStreamState),
    GeminiToOpenAIResponse(generate_content::gemini2openai_response::stream::GeminiToOpenAIResponseStreamState),
}

impl StreamTransformer {
    pub fn new(ctx: &TransformContext) -> Result<Self, TransformError> {
        if !matches!(ctx.src_op, Op::GenerateContent | Op::StreamGenerateContent)
            || !matches!(ctx.dst_op, Op::GenerateContent | Op::StreamGenerateContent)
        {
            return Err(TransformError::OpMismatch);
        }
        if ctx.src_op != Op::StreamGenerateContent || ctx.dst_op != Op::StreamGenerateContent {
            return Err(TransformError::StreamMismatch);
        }
        ensure_generate_proto(ctx.src)?;
        ensure_generate_proto(ctx.dst)?;

        if ctx.src == ctx.dst {
            return Ok(StreamTransformer::Passthrough(ctx.src));
        }

        let transformer = match (ctx.src, ctx.dst) {
            (Proto::Claude, Proto::OpenAIChat) => {
                StreamTransformer::ClaudeToOpenAIChat(
                    generate_content::claude2openai_chat_completions::stream::ClaudeToOpenAIChatCompletionStreamState::new(now_unix()),
                )
            }
            (Proto::Claude, Proto::OpenAIResponse) => {
                StreamTransformer::ClaudeToOpenAIResponse(
                    generate_content::claude2openai_response::stream::ClaudeToOpenAIResponseStreamState::new(now_unix()),
                )
            }
            (Proto::Claude, Proto::Gemini) => {
                StreamTransformer::ClaudeToGemini(
                    generate_content::gemini2claude::stream::ClaudeToGeminiStreamState::new(),
                )
            }
            (Proto::OpenAIChat, Proto::Claude) => {
                StreamTransformer::OpenAIChatToClaude(
                    generate_content::openai_chat_completions2claude::stream::OpenAIToClaudeChatCompletionStreamState::new(),
                )
            }
            (Proto::OpenAIChat, Proto::OpenAIResponse) => {
                StreamTransformer::OpenAIChatToOpenAIResponse(
                    generate_content::openai_chat_completions2openai_response::stream::OpenAIChatCompletionToResponseStreamState::new(),
                )
            }
            (Proto::OpenAIChat, Proto::Gemini) => {
                StreamTransformer::OpenAIChatToGemini(
                    generate_content::openai_chat_completions2gemini::stream::OpenAIChatCompletionToGeminiStreamState::new(),
                )
            }
            (Proto::OpenAIResponse, Proto::Claude) => {
                StreamTransformer::OpenAIResponseToClaude(
                    generate_content::openai_response2claude::stream::OpenAIResponseToClaudeStreamState::new(),
                )
            }
            (Proto::OpenAIResponse, Proto::OpenAIChat) => {
                StreamTransformer::OpenAIResponseToOpenAIChat(
                    generate_content::openai_response2openai_chat_completions::stream::OpenAIResponseToChatCompletionStreamState::new(),
                )
            }
            (Proto::OpenAIResponse, Proto::Gemini) => {
                StreamTransformer::OpenAIResponseToGemini(
                    generate_content::openai_response2gemini::stream::OpenAIResponseToGeminiStreamState::new(),
                )
            }
            (Proto::Gemini, Proto::Claude) => {
                StreamTransformer::GeminiToClaude(
                    generate_content::claude2gemini::stream::GeminiToClaudeStreamState::new(),
                )
            }
            (Proto::Gemini, Proto::OpenAIChat) => {
                StreamTransformer::GeminiToOpenAIChat(
                    generate_content::gemini2openai_chat_completions::stream::GeminiToOpenAIChatCompletionStreamState::new(),
                )
            }
            (Proto::Gemini, Proto::OpenAIResponse) => {
                StreamTransformer::GeminiToOpenAIResponse(
                    generate_content::gemini2openai_response::stream::GeminiToOpenAIResponseStreamState::new(),
                )
            }
            _ => {
                return Err(TransformError::UnsupportedPair {
                    src: ctx.src,
                    dst: ctx.dst,
                    src_op: ctx.src_op,
                    dst_op: ctx.dst_op,
                });
            }
        };

        Ok(transformer)
    }

    pub fn push(&mut self, event: StreamEvent) -> Result<Vec<StreamEvent>, TransformError> {
        match self {
            StreamTransformer::Passthrough(proto) => match (proto, event) {
                (Proto::Claude, StreamEvent::Claude(event)) => Ok(vec![StreamEvent::Claude(event)]),
                (Proto::OpenAIChat, StreamEvent::OpenAIChat(event)) => {
                    Ok(vec![StreamEvent::OpenAIChat(event)])
                }
                (Proto::OpenAIResponse, StreamEvent::OpenAIResponse(event)) => {
                    Ok(vec![StreamEvent::OpenAIResponse(event)])
                }
                (Proto::Gemini, StreamEvent::Gemini(event)) => Ok(vec![StreamEvent::Gemini(event)]),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::ClaudeToOpenAIChat(state) => match event {
                StreamEvent::Claude(event) => Ok(state
                    .transform_event(event)
                    .into_iter()
                    .map(StreamEvent::OpenAIChat)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::ClaudeToOpenAIResponse(state) => match event {
                StreamEvent::Claude(event) => Ok(state
                    .transform_event(event)
                    .into_iter()
                    .map(StreamEvent::OpenAIResponse)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::ClaudeToGemini(state) => match event {
                StreamEvent::Claude(event) => Ok(state
                    .transform_event(event)
                    .into_iter()
                    .map(StreamEvent::Gemini)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::OpenAIChatToClaude(state) => match event {
                StreamEvent::OpenAIChat(event) => Ok(state
                    .transform_chunk(event)
                    .into_iter()
                    .map(StreamEvent::Claude)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::OpenAIChatToOpenAIResponse(state) => match event {
                StreamEvent::OpenAIChat(event) => Ok(state
                    .transform_event(event)
                    .into_iter()
                    .map(StreamEvent::OpenAIResponse)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::OpenAIChatToGemini(state) => match event {
                StreamEvent::OpenAIChat(event) => Ok(state
                    .transform_event(event)
                    .into_iter()
                    .map(StreamEvent::Gemini)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::OpenAIResponseToClaude(state) => match event {
                StreamEvent::OpenAIResponse(event) => Ok(state
                    .transform_event(event)
                    .into_iter()
                    .map(StreamEvent::Claude)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::OpenAIResponseToOpenAIChat(state) => match event {
                StreamEvent::OpenAIResponse(event) => Ok(state
                    .transform_event(event)
                    .into_iter()
                    .map(StreamEvent::OpenAIChat)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::OpenAIResponseToGemini(state) => match event {
                StreamEvent::OpenAIResponse(event) => Ok(state
                    .transform_event(event)
                    .into_iter()
                    .map(StreamEvent::Gemini)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::GeminiToClaude(state) => match event {
                StreamEvent::Gemini(event) => Ok(state
                    .transform_response(event)
                    .into_iter()
                    .map(StreamEvent::Claude)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::GeminiToOpenAIChat(state) => match event {
                StreamEvent::Gemini(event) => Ok(state
                    .transform_response(event)
                    .into_iter()
                    .map(StreamEvent::OpenAIChat)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
            StreamTransformer::GeminiToOpenAIResponse(state) => match event {
                StreamEvent::Gemini(event) => Ok(state
                    .transform_response(event)
                    .into_iter()
                    .map(StreamEvent::OpenAIResponse)
                    .collect()),
                _ => Err(TransformError::ProtoMismatch),
            },
        }
    }
}

pub struct StreamToNostream {
    transformer: StreamTransformer,
    accumulator: TargetAccumulator,
}

impl StreamToNostream {
    pub fn new(ctx: &TransformContext) -> Result<Self, TransformError> {
        if ctx.src_op != Op::StreamGenerateContent || ctx.dst_op != Op::GenerateContent {
            return Err(TransformError::StreamMismatch);
        }
        let mut stream_ctx = *ctx;
        stream_ctx.dst_op = Op::StreamGenerateContent;
        let transformer = StreamTransformer::new(&stream_ctx)?;
        let accumulator = TargetAccumulator::new(ctx.dst)?;
        Ok(Self {
            transformer,
            accumulator,
        })
    }

    pub fn push(&mut self, event: StreamEvent) -> Result<Option<Response>, TransformError> {
        let events = self.transformer.push(event)?;
        let mut output = None;
        for event in events {
            if let Some(resp) = self.accumulator.push(event)? {
                output = Some(Response::GenerateContent(resp));
            }
        }
        Ok(output)
    }

    pub fn finalize(&mut self) -> Result<Option<Response>, TransformError> {
        Ok(self.accumulator.finalize().map(Response::GenerateContent))
    }

    pub fn finalize_on_eof(&mut self) -> Result<Option<Response>, TransformError> {
        Ok(self
            .accumulator
            .finalize_on_eof()
            .map(Response::GenerateContent))
    }
}

pub struct NostreamToStream {
    ctx: TransformContext,
}

impl NostreamToStream {
    pub fn new(ctx: &TransformContext) -> Result<Self, TransformError> {
        if ctx.src_op != Op::GenerateContent || ctx.dst_op != Op::StreamGenerateContent {
            return Err(TransformError::StreamMismatch);
        }
        Ok(Self { ctx: *ctx })
    }

    pub fn transform_response(
        &mut self,
        resp: Response,
    ) -> Result<Vec<StreamEvent>, TransformError> {
        let ctx = TransformContext {
            src: self.ctx.src,
            dst: self.ctx.dst,
            src_op: Op::GenerateContent,
            dst_op: Op::GenerateContent,
        };
        let resp = transform_response(&ctx, resp)?;
        let resp = match resp {
            Response::GenerateContent(resp) => resp,
            _ => return Err(TransformError::OpMismatch),
        };
        Ok(streamify_response(self.ctx.dst, resp))
    }
}

#[allow(clippy::large_enum_variant)]
enum TargetAccumulator {
    Claude(stream2nostream::claude::ClaudeStreamToMessageState),
    OpenAIChat(stream2nostream::openai_chat_completions::OpenAIChatCompletionStreamToResponseState),
    OpenAIResponse(stream2nostream::openai_response::OpenAIResponseStreamToResponseState),
    Gemini(stream2nostream::gemini::GeminiStreamToResponseState),
}

impl TargetAccumulator {
    fn new(proto: Proto) -> Result<Self, TransformError> {
        match proto {
            Proto::Claude => Ok(TargetAccumulator::Claude(
                stream2nostream::claude::ClaudeStreamToMessageState::new(),
            )),
            Proto::OpenAIChat => Ok(TargetAccumulator::OpenAIChat(
                stream2nostream::openai_chat_completions::OpenAIChatCompletionStreamToResponseState::new(),
            )),
            Proto::OpenAIResponse => Ok(TargetAccumulator::OpenAIResponse(
                stream2nostream::openai_response::OpenAIResponseStreamToResponseState::new(),
            )),
            Proto::Gemini => Ok(TargetAccumulator::Gemini(
                stream2nostream::gemini::GeminiStreamToResponseState::new(),
            )),
            _ => Err(TransformError::ProtoMismatch),
        }
    }

    fn push(
        &mut self,
        event: StreamEvent,
    ) -> Result<Option<GenerateContentResponse>, TransformError> {
        match (self, event) {
            (TargetAccumulator::Claude(state), StreamEvent::Claude(event)) => {
                Ok(state.push_event(event).map(GenerateContentResponse::Claude))
            }
            (TargetAccumulator::OpenAIChat(state), StreamEvent::OpenAIChat(event)) => Ok(state
                .push_chunk(event)
                .map(GenerateContentResponse::OpenAIChat)),
            (TargetAccumulator::OpenAIResponse(state), StreamEvent::OpenAIResponse(event)) => {
                Ok(state
                    .push_event(event)
                    .map(GenerateContentResponse::OpenAIResponse))
            }
            (TargetAccumulator::Gemini(state), StreamEvent::Gemini(event)) => {
                Ok(state.push_chunk(event).map(GenerateContentResponse::Gemini))
            }
            _ => Err(TransformError::ProtoMismatch),
        }
    }

    fn finalize(&mut self) -> Option<GenerateContentResponse> {
        match self {
            TargetAccumulator::Claude(state) => {
                state.finalize().map(GenerateContentResponse::Claude)
            }
            TargetAccumulator::OpenAIChat(state) => {
                Some(GenerateContentResponse::OpenAIChat(state.finalize()))
            }
            TargetAccumulator::OpenAIResponse(state) => {
                let old = std::mem::take(state);
                old.finalize().map(GenerateContentResponse::OpenAIResponse)
            }
            TargetAccumulator::Gemini(state) => {
                Some(GenerateContentResponse::Gemini(state.finalize()))
            }
        }
    }

    fn finalize_on_eof(&mut self) -> Option<GenerateContentResponse> {
        match self {
            TargetAccumulator::Claude(state) => {
                state.finalize_on_eof().map(GenerateContentResponse::Claude)
            }
            TargetAccumulator::OpenAIChat(state) => {
                Some(GenerateContentResponse::OpenAIChat(state.finalize_on_eof()))
            }
            TargetAccumulator::OpenAIResponse(state) => state
                .finalize_on_eof()
                .map(GenerateContentResponse::OpenAIResponse),
            TargetAccumulator::Gemini(state) => {
                Some(GenerateContentResponse::Gemini(state.finalize_on_eof()))
            }
        }
    }
}

fn streamify_response(proto: Proto, resp: GenerateContentResponse) -> Vec<StreamEvent> {
    match (proto, resp) {
        (Proto::Claude, GenerateContentResponse::Claude(resp)) => streamify_claude_message(resp)
            .into_iter()
            .map(StreamEvent::Claude)
            .collect(),
        (Proto::OpenAIChat, GenerateContentResponse::OpenAIChat(resp)) => {
            streamify_openai_chat(resp)
                .into_iter()
                .map(StreamEvent::OpenAIChat)
                .collect()
        }
        (Proto::OpenAIResponse, GenerateContentResponse::OpenAIResponse(resp)) => {
            streamify_openai_response(resp)
                .into_iter()
                .map(StreamEvent::OpenAIResponse)
                .collect()
        }
        (Proto::Gemini, GenerateContentResponse::Gemini(resp)) => {
            vec![StreamEvent::Gemini(resp)]
        }
        _ => Vec::new(),
    }
}

fn streamify_claude_message(message: BetaMessage) -> Vec<BetaStreamEvent> {
    let mut events = Vec::new();

    let usage = BetaStreamUsage {
        input_tokens: Some(message.usage.input_tokens),
        output_tokens: Some(message.usage.output_tokens),
        cache_creation_input_tokens: Some(message.usage.cache_creation_input_tokens),
        cache_read_input_tokens: Some(message.usage.cache_read_input_tokens),
        cache_creation: Some(message.usage.cache_creation.clone()),
        server_tool_use: message.usage.server_tool_use.clone(),
    };

    let stream_message = BetaStreamMessage {
        id: message.id.clone(),
        container: message.container.clone(),
        content: Vec::new(),
        context_management: None,
        model: message.model.clone(),
        role: message.role,
        stop_reason: None,
        stop_sequence: None,
        r#type: message.r#type,
        usage,
    };

    events.push(BetaStreamEvent::Known(BetaStreamEventKnown::MessageStart {
        message: stream_message,
    }));

    for (index, block) in message.content.iter().enumerate() {
        let (start_block, deltas) = streamify_block(block);
        events.push(BetaStreamEvent::Known(
            BetaStreamEventKnown::ContentBlockStart {
                index: index as u32,
                content_block: start_block,
            },
        ));
        for delta in deltas {
            events.push(BetaStreamEvent::Known(
                BetaStreamEventKnown::ContentBlockDelta {
                    index: index as u32,
                    delta,
                },
            ));
        }
        events.push(BetaStreamEvent::Known(
            BetaStreamEventKnown::ContentBlockStop {
                index: index as u32,
            },
        ));
    }

    events.push(BetaStreamEvent::Known(BetaStreamEventKnown::MessageDelta {
        delta: BetaStreamMessageDelta {
            stop_reason: message.stop_reason,
            stop_sequence: message.stop_sequence.clone(),
        },
        usage: BetaStreamUsage {
            input_tokens: Some(message.usage.input_tokens),
            output_tokens: Some(message.usage.output_tokens),
            cache_creation_input_tokens: Some(message.usage.cache_creation_input_tokens),
            cache_read_input_tokens: Some(message.usage.cache_read_input_tokens),
            cache_creation: Some(message.usage.cache_creation.clone()),
            server_tool_use: message.usage.server_tool_use.clone(),
        },
        context_management: message.context_management.clone(),
    }));

    events.push(BetaStreamEvent::Known(BetaStreamEventKnown::MessageStop));
    events
}

fn streamify_block(
    block: &BetaContentBlock,
) -> (BetaStreamContentBlock, Vec<BetaStreamContentBlockDelta>) {
    match block {
        BetaContentBlock::Text(text) => {
            let mut start = text.clone();
            let mut deltas = Vec::new();
            start.text.clear();
            start.citations = None;
            if !text.text.is_empty() {
                deltas.push(BetaStreamContentBlockDelta::TextDelta {
                    text: text.text.clone(),
                });
            }
            if let Some(citations) = &text.citations {
                for citation in citations {
                    deltas.push(BetaStreamContentBlockDelta::CitationsDelta {
                        citation: citation.clone(),
                    });
                }
            }
            (BetaStreamContentBlock::Text(start), deltas)
        }
        BetaContentBlock::Thinking(thinking) => {
            let start = BetaThinkingBlockStream {
                signature: None,
                thinking: String::new(),
                r#type: thinking.r#type,
            };
            let mut deltas = vec![BetaStreamContentBlockDelta::ThinkingDelta {
                thinking: thinking.thinking.clone(),
            }];
            if !thinking.signature.is_empty() {
                deltas.push(BetaStreamContentBlockDelta::SignatureDelta {
                    signature: thinking.signature.clone(),
                });
            }
            (BetaStreamContentBlock::Thinking(start), deltas)
        }
        BetaContentBlock::ToolUse(tool) => {
            let mut start = tool.clone();
            let delta = serde_json::to_string(&tool.input).unwrap_or_else(|_| "{}".to_string());
            start.input = JsonObject::new();
            (
                BetaStreamContentBlock::ToolUse(start),
                vec![BetaStreamContentBlockDelta::InputJsonDelta {
                    partial_json: delta,
                }],
            )
        }
        BetaContentBlock::ServerToolUse(tool) => {
            let start = BetaStreamContentBlock::ServerToolUse(BetaServerToolUseBlockStream {
                id: tool.id.clone(),
                caller: Some(tool.caller.clone()),
                input: JsonObject::new(),
                name: tool.name,
                r#type: tool.r#type,
            });
            let delta = serde_json::to_string(&tool.input).unwrap_or_else(|_| "{}".to_string());
            (
                start,
                vec![BetaStreamContentBlockDelta::InputJsonDelta {
                    partial_json: delta,
                }],
            )
        }
        BetaContentBlock::McpToolUse(tool) => {
            let mut start = tool.clone();
            let delta = serde_json::to_string(&tool.input).unwrap_or_else(|_| "{}".to_string());
            start.input = JsonObject::new();
            (
                BetaStreamContentBlock::McpToolUse(start),
                vec![BetaStreamContentBlockDelta::InputJsonDelta {
                    partial_json: delta,
                }],
            )
        }
        _ => (map_block_passthrough(block.clone()), Vec::new()),
    }
}

fn map_block_passthrough(block: BetaContentBlock) -> BetaStreamContentBlock {
    match block {
        BetaContentBlock::Text(text) => BetaStreamContentBlock::Text(text),
        BetaContentBlock::Thinking(block) => {
            BetaStreamContentBlock::Thinking(BetaThinkingBlockStream {
                signature: Some(block.signature),
                thinking: block.thinking,
                r#type: block.r#type,
            })
        }
        BetaContentBlock::RedactedThinking(block) => {
            BetaStreamContentBlock::RedactedThinking(block)
        }
        BetaContentBlock::ToolUse(block) => BetaStreamContentBlock::ToolUse(block),
        BetaContentBlock::ServerToolUse(block) => {
            BetaStreamContentBlock::ServerToolUse(BetaServerToolUseBlockStream {
                id: block.id,
                caller: Some(block.caller),
                input: block.input,
                name: block.name,
                r#type: block.r#type,
            })
        }
        BetaContentBlock::WebSearchToolResult(block) => {
            BetaStreamContentBlock::WebSearchToolResult(block)
        }
        BetaContentBlock::WebFetchToolResult(block) => {
            BetaStreamContentBlock::WebFetchToolResult(block)
        }
        BetaContentBlock::CodeExecutionToolResult(block) => {
            BetaStreamContentBlock::CodeExecutionToolResult(block)
        }
        BetaContentBlock::BashCodeExecutionToolResult(block) => {
            BetaStreamContentBlock::BashCodeExecutionToolResult(block)
        }
        BetaContentBlock::TextEditorCodeExecutionToolResult(block) => {
            BetaStreamContentBlock::TextEditorCodeExecutionToolResult(block)
        }
        BetaContentBlock::ToolSearchToolResult(block) => {
            BetaStreamContentBlock::ToolSearchToolResult(block)
        }
        BetaContentBlock::McpToolUse(block) => BetaStreamContentBlock::McpToolUse(block),
        BetaContentBlock::McpToolResult(block) => BetaStreamContentBlock::McpToolResult(block),
        BetaContentBlock::ContainerUpload(block) => BetaStreamContentBlock::ContainerUpload(block),
        BetaContentBlock::Compaction(block) => BetaStreamContentBlock::Compaction(block),
    }
}

fn streamify_openai_chat(
    response: OpenAIChatCompletionResponse,
) -> Vec<CreateChatCompletionStreamResponse> {
    let mut events = Vec::new();

    for choice in &response.choices {
        let delta = stream_delta_from_message(&choice.message);
        let stream_choice = ChatCompletionStreamChoice {
            index: choice.index,
            delta,
            logprobs: choice.logprobs.clone(),
            finish_reason: None,
        };

        events.push(make_chat_stream_response(
            &response,
            vec![stream_choice],
            None,
        ));

        let finish_choice = ChatCompletionStreamChoice {
            index: choice.index,
            delta: ChatCompletionStreamResponseDelta {
                role: None,
                content: None,
                reasoning_content: None,
                function_call: None,
                tool_calls: None,
                refusal: None,
                obfuscation: None,
            },
            logprobs: choice.logprobs.clone(),
            finish_reason: Some(choice.finish_reason),
        };

        events.push(make_chat_stream_response(
            &response,
            vec![finish_choice],
            response.usage.clone(),
        ));
    }

    events
}

fn make_chat_stream_response(
    response: &OpenAIChatCompletionResponse,
    choices: Vec<ChatCompletionStreamChoice>,
    usage: Option<CompletionUsage>,
) -> CreateChatCompletionStreamResponse {
    CreateChatCompletionStreamResponse {
        id: response.id.clone(),
        object: ChatCompletionChunkObjectType::ChatCompletionChunk,
        created: response.created,
        model: response.model.clone(),
        choices,
        usage,
        service_tier: response.service_tier,
        system_fingerprint: response.system_fingerprint.clone(),
    }
}

fn stream_delta_from_message(
    message: &ChatCompletionResponseMessage,
) -> ChatCompletionStreamResponseDelta {
    ChatCompletionStreamResponseDelta {
        role: Some(map_chat_role(message.role)),
        content: message.content.clone(),
        reasoning_content: None,
        function_call: message
            .function_call
            .as_ref()
            .map(|call| ChatCompletionFunctionCallDelta {
                name: Some(call.name.clone()),
                arguments: Some(call.arguments.clone()),
            }),
        tool_calls: message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .enumerate()
                .map(|(idx, call)| map_tool_call_chunk(idx as i64, call))
                .collect()
        }),
        refusal: message.refusal.clone(),
        obfuscation: None,
    }
}

fn map_tool_call_chunk(
    index: i64,
    call: &ChatCompletionMessageToolCall,
) -> ChatCompletionMessageToolCallChunk {
    match call {
        ChatCompletionMessageToolCall::Function { id, function } => {
            ChatCompletionMessageToolCallChunk {
                index,
                id: Some(id.clone()),
                r#type: Some(ChatCompletionToolCallChunkType::Function),
                function: Some(ChatCompletionMessageToolCallChunkFunction {
                    name: Some(function.name.clone()),
                    arguments: Some(function.arguments.clone()),
                }),
            }
        }
        ChatCompletionMessageToolCall::Custom { id, custom } => {
            ChatCompletionMessageToolCallChunk {
                index,
                id: Some(id.clone()),
                r#type: Some(ChatCompletionToolCallChunkType::Function),
                function: Some(ChatCompletionMessageToolCallChunkFunction {
                    name: Some(custom.name.clone()),
                    arguments: Some(custom.input.clone()),
                }),
            }
        }
    }
}

fn map_chat_role(role: ChatCompletionResponseRole) -> ChatCompletionRole {
    match role {
        ChatCompletionResponseRole::Assistant => ChatCompletionRole::Assistant,
    }
}

fn streamify_openai_response(response: OpenAIResponse) -> Vec<ResponseStreamEvent> {
    let mut events = Vec::new();
    let mut sequence = 0;

    events.push(ResponseStreamEvent::Created(ResponseCreatedEvent {
        response: response.clone(),
        sequence_number: next_seq(&mut sequence),
    }));

    for (index, item) in response.output.iter().enumerate() {
        let output_index = index as i64;
        events.push(ResponseStreamEvent::OutputItemAdded(
            ResponseOutputItemAddedEvent {
                output_index,
                item: item.clone(),
                sequence_number: next_seq(&mut sequence),
            },
        ));

        if let OutputItem::Message(message) = item {
            events.extend(streamify_openai_message(
                message,
                output_index,
                &mut sequence,
            ));
        }

        if let OutputItem::Function(call) = item {
            events.extend(streamify_function_call(call, output_index, &mut sequence));
        }

        if let OutputItem::MCPCall(call) = item {
            events.extend(streamify_mcp_call(call, output_index, &mut sequence));
        }

        if let OutputItem::CustomToolCall(call) = item {
            events.extend(streamify_custom_call(call, output_index, &mut sequence));
        }

        events.push(ResponseStreamEvent::OutputItemDone(
            ResponseOutputItemDoneEvent {
                output_index,
                item: item.clone(),
                sequence_number: next_seq(&mut sequence),
            },
        ));
    }

    events.push(ResponseStreamEvent::Completed(ResponseCompletedEvent {
        response,
        sequence_number: next_seq(&mut sequence),
    }));

    events
}

fn streamify_openai_message(
    message: &OutputMessage,
    output_index: i64,
    sequence: &mut i64,
) -> Vec<ResponseStreamEvent> {
    let mut events = Vec::new();
    for (content_index, content) in message.content.iter().enumerate() {
        let content_index = content_index as i64;
        match content {
            OutputMessageContent::OutputText(text) => {
                events.push(ResponseStreamEvent::OutputTextDelta(
                    ResponseTextDeltaEvent {
                        item_id: message.id.clone(),
                        output_index,
                        content_index,
                        delta: text.text.clone(),
                        sequence_number: next_seq(sequence),
                        logprobs: Vec::new(),
                    },
                ));
                events.push(ResponseStreamEvent::OutputTextDone(ResponseTextDoneEvent {
                    item_id: message.id.clone(),
                    output_index,
                    content_index,
                    text: text.text.clone(),
                    sequence_number: next_seq(sequence),
                    logprobs: Vec::new(),
                }));
            }
            OutputMessageContent::Refusal(refusal) => {
                events.push(ResponseStreamEvent::RefusalDelta(
                    ResponseRefusalDeltaEvent {
                        item_id: message.id.clone(),
                        output_index,
                        content_index,
                        delta: refusal.refusal.clone(),
                        sequence_number: next_seq(sequence),
                    },
                ));
                events.push(ResponseStreamEvent::RefusalDone(ResponseRefusalDoneEvent {
                    item_id: message.id.clone(),
                    output_index,
                    content_index,
                    refusal: refusal.refusal.clone(),
                    sequence_number: next_seq(sequence),
                }));
            }
        }
    }
    events
}

fn streamify_function_call(
    call: &FunctionToolCall,
    output_index: i64,
    sequence: &mut i64,
) -> Vec<ResponseStreamEvent> {
    vec![
        ResponseStreamEvent::FunctionCallArgumentsDelta(ResponseFunctionCallArgumentsDeltaEvent {
            item_id: call.id.clone().unwrap_or_else(|| "tool".to_string()),
            output_index,
            delta: call.arguments.clone(),
            sequence_number: next_seq(sequence),
        }),
        ResponseStreamEvent::FunctionCallArgumentsDone(ResponseFunctionCallArgumentsDoneEvent {
            item_id: call.id.clone().unwrap_or_else(|| "tool".to_string()),
            name: call.name.clone(),
            output_index,
            arguments: call.arguments.clone(),
            sequence_number: next_seq(sequence),
        }),
    ]
}

fn streamify_mcp_call(
    call: &MCPToolCall,
    output_index: i64,
    sequence: &mut i64,
) -> Vec<ResponseStreamEvent> {
    vec![
        ResponseStreamEvent::MCPCallArgumentsDelta(ResponseMCPCallArgumentsDeltaEvent {
            output_index,
            item_id: call.id.clone(),
            delta: call.arguments.clone(),
            sequence_number: next_seq(sequence),
        }),
        ResponseStreamEvent::MCPCallArgumentsDone(ResponseMCPCallArgumentsDoneEvent {
            output_index,
            item_id: call.id.clone(),
            arguments: call.arguments.clone(),
            sequence_number: next_seq(sequence),
        }),
    ]
}

fn streamify_custom_call(
    call: &CustomToolCall,
    output_index: i64,
    sequence: &mut i64,
) -> Vec<ResponseStreamEvent> {
    let item_id = call
        .id
        .clone()
        .unwrap_or_else(|| format!("custom-{}", call.call_id));
    vec![
        ResponseStreamEvent::CustomToolCallInputDelta(ResponseCustomToolCallInputDeltaEvent {
            sequence_number: next_seq(sequence),
            output_index,
            item_id: item_id.clone(),
            delta: call.input.clone(),
        }),
        ResponseStreamEvent::CustomToolCallInputDone(ResponseCustomToolCallInputDoneEvent {
            sequence_number: next_seq(sequence),
            output_index,
            item_id,
            input: call.input.clone(),
        }),
    ]
}

fn next_seq(seq: &mut i64) -> i64 {
    let value = *seq;
    *seq += 1;
    value
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
