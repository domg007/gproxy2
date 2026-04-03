//! WebSocket protocol bridge: transparent or cross-protocol duplex relay with usage tracking.

use gproxy_sdk::provider::engine::Usage;
use gproxy_sdk::provider::response::UpstreamError;

// ---------------------------------------------------------------------------
// WsProtocolBridge trait
// ---------------------------------------------------------------------------

/// Abstraction for bidirectional WS message conversion with usage tracking.
///
/// Implementations handle:
/// - **PassthroughBridge**: same-protocol transparent relay + usage extraction
/// - **OpenAiToGeminiBridge**: OpenAI Responses WS ↔ Gemini Live WS
/// - **GeminiToOpenAiBridge**: Gemini Live WS ↔ OpenAI Responses WS
pub(crate) trait WsProtocolBridge: Send {
    /// Convert a downstream (client) text message into zero or more upstream messages.
    fn convert_client_message(&mut self, msg: &str) -> Result<Vec<String>, UpstreamError>;

    /// Convert an upstream (server) text message into zero or more downstream messages.
    /// Returns extracted usage from this message, if any.
    fn convert_server_message(
        &mut self,
        msg: &str,
    ) -> Result<(Vec<String>, Option<Usage>), UpstreamError>;

    /// Accumulated usage over the entire connection lifetime.
    fn final_usage(&self) -> Option<Usage>;
}

// ---------------------------------------------------------------------------
// PassthroughBridge — same protocol, usage tracking only
// ---------------------------------------------------------------------------

pub(crate) struct PassthroughBridge {
    protocol: String,
    accumulated_usage: Usage,
    has_usage: bool,
}

impl PassthroughBridge {
    pub fn new(protocol: impl Into<String>) -> Self {
        Self {
            protocol: protocol.into(),
            accumulated_usage: Usage::default(),
            has_usage: false,
        }
    }
}

impl WsProtocolBridge for PassthroughBridge {
    fn convert_client_message(&mut self, msg: &str) -> Result<Vec<String>, UpstreamError> {
        Ok(vec![msg.to_string()])
    }

    fn convert_server_message(
        &mut self,
        msg: &str,
    ) -> Result<(Vec<String>, Option<Usage>), UpstreamError> {
        let usage = extract_ws_usage(&self.protocol, msg.as_bytes());
        if let Some(ref u) = usage {
            merge_usage(&mut self.accumulated_usage, u);
            self.has_usage = true;
        }
        Ok((vec![msg.to_string()], usage))
    }

    fn final_usage(&self) -> Option<Usage> {
        if self.has_usage {
            Some(self.accumulated_usage.clone())
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Usage extraction from WS messages
// ---------------------------------------------------------------------------

fn extract_ws_usage(protocol: &str, msg: &[u8]) -> Option<Usage> {
    match protocol {
        "openai" | "openai_response" => extract_openai_ws_usage(msg),
        "gemini" => extract_gemini_ws_usage(msg),
        _ => None,
    }
}

fn extract_openai_ws_usage(msg: &[u8]) -> Option<Usage> {
    use gproxy_sdk::protocol::openai::create_response::stream::ResponseStreamEvent;

    // OpenAI WS server messages can be stream events, errors, etc.
    // Try to parse as ResponseStreamEvent (most common)
    let event: ResponseStreamEvent = serde_json::from_slice(msg).ok()?;
    match event {
        ResponseStreamEvent::Created { response, .. }
        | ResponseStreamEvent::Queued { response, .. }
        | ResponseStreamEvent::InProgress { response, .. }
        | ResponseStreamEvent::Completed { response, .. }
        | ResponseStreamEvent::Incomplete { response, .. }
        | ResponseStreamEvent::Failed { response, .. } => {
            let u = response.usage?;
            Some(Usage {
                input_tokens: i64::try_from(u.input_tokens).ok(),
                output_tokens: i64::try_from(u.output_tokens).ok(),
                cache_read_input_tokens: i64::try_from(u.input_tokens_details.cached_tokens).ok(),
                cache_creation_input_tokens: None,
                cache_creation_input_tokens_5min: None,
                cache_creation_input_tokens_1h: None,
            })
        }
        _ => None,
    }
}

fn extract_gemini_ws_usage(msg: &[u8]) -> Option<Usage> {
    use gproxy_sdk::protocol::gemini::live::types::GeminiBidiGenerateContentServerMessage;

    let server_msg: GeminiBidiGenerateContentServerMessage = serde_json::from_slice(msg).ok()?;
    let u = server_msg.usage_metadata?;
    Some(Usage {
        input_tokens: u.prompt_token_count.and_then(|v| i64::try_from(v).ok()),
        output_tokens: u.response_token_count.and_then(|v| i64::try_from(v).ok()),
        cache_read_input_tokens: u
            .cached_content_token_count
            .and_then(|v| i64::try_from(v).ok()),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    })
}

fn merge_usage(dst: &mut Usage, src: &Usage) {
    if src.input_tokens.is_some() {
        dst.input_tokens = src.input_tokens;
    }
    if src.output_tokens.is_some() {
        dst.output_tokens = src.output_tokens;
    }
    if src.cache_read_input_tokens.is_some() {
        dst.cache_read_input_tokens = src.cache_read_input_tokens;
    }
    if src.cache_creation_input_tokens.is_some() {
        dst.cache_creation_input_tokens = src.cache_creation_input_tokens;
    }
    if src.cache_creation_input_tokens_5min.is_some() {
        dst.cache_creation_input_tokens_5min = src.cache_creation_input_tokens_5min;
    }
    if src.cache_creation_input_tokens_1h.is_some() {
        dst.cache_creation_input_tokens_1h = src.cache_creation_input_tokens_1h;
    }
}
