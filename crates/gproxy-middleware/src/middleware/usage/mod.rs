use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures_util::{StreamExt, stream};
use gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse;
use gproxy_protocol::claude::create_message::stream::{
    BetaMessageDeltaUsage, ClaudeCreateMessageStreamEvent,
};
use gproxy_protocol::claude::create_message::types::{BetaIterationUsage, BetaUsage};
use gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse;
use gproxy_protocol::gemini::generate_content::types::GeminiUsageMetadata;
use gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
use gproxy_protocol::openai::create_chat_completions::stream::ChatCompletionChunk;
use gproxy_protocol::openai::create_chat_completions::types::CompletionUsage;
use gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse;
use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
use gproxy_protocol::openai::create_response::types::ResponseUsage;
use tower::{Layer, Service};

use crate::middleware::transform::kinds::{OperationFamily, ProtocolKind};
use crate::middleware::transform::message::{TransformRequestPayload, TransformResponsePayload};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UsageSnapshot {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_creation_input_tokens_5min: Option<u64>,
    pub cache_creation_input_tokens_1h: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub thoughts_tokens: Option<u64>,
    pub tool_use_prompt_tokens: Option<u64>,
}

impl UsageSnapshot {
    fn with_derived_totals(mut self) -> Self {
        if self.total_tokens.is_none()
            && let (Some(input), Some(output)) = (self.input_tokens, self.output_tokens)
        {
            self.total_tokens = Some(input.saturating_add(output));
        }
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct UsageHandle {
    inner: Arc<Mutex<Option<UsageSnapshot>>>,
}

impl UsageHandle {
    pub fn latest(&self) -> Option<UsageSnapshot> {
        self.inner.lock().ok().and_then(|guard| guard.clone())
    }

    fn set_latest(&self, usage: Option<UsageSnapshot>) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = usage;
        }
    }
}

pub struct UsageExtractedResponse {
    pub response: TransformResponsePayload,
    pub usage: UsageHandle,
}

struct UsageStreamState {
    input: crate::middleware::transform::message::TransformBodyStream,
    parser: UsageParser,
    usage: UsageHandle,
}

pub fn attach_usage_extractor(payload: TransformResponsePayload) -> UsageExtractedResponse {
    let usage = UsageHandle::default();
    let parser = UsageParser::new(payload.operation, payload.protocol);
    let state = UsageStreamState {
        input: payload.body,
        parser,
        usage: usage.clone(),
    };

    let body = stream::try_unfold(state, |mut state| async move {
        match state.input.next().await {
            Some(Ok(chunk)) => {
                state.parser.feed(chunk.as_ref());
                Ok(Some((chunk, state)))
            }
            Some(Err(err)) => {
                state.usage.set_latest(state.parser.snapshot());
                Err(err)
            }
            None => {
                state.usage.set_latest(state.parser.finish());
                Ok(None)
            }
        }
    });

    UsageExtractedResponse {
        response: TransformResponsePayload::new(
            payload.operation,
            payload.protocol,
            Box::pin(body),
        ),
        usage,
    }
}

mod parser;
mod service;

use parser::UsageParser;
pub use service::*;
