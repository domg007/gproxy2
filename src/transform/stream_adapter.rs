//! Runtime SSE adapter for cross-protocol streaming: decode upstream frames,
//! convert each event through the resolved (reverse) pair, re-encode in the
//! inbound wire format. Sync core — shared by the native stream wrapper
//! (`pipeline/stream.rs`) and the buffered path (wasm / buffered attempts).
//!
//! Pair `stream_event` fns are 1:1 and stateless; any future cross-event
//! aggregation (block indexes, tool-call identity, final usage) lives HERE
//! (see transform/README.md).

use serde_json::Value;

use super::common::sse::{SseDecoder, SseFrame};
use super::{TransformContext, TransformPair, dispatch};
use crate::protocol::ContentGenerationKind;

pub struct SseTransformer {
    decoder: SseDecoder,
    /// Reverse pair: upstream kind → inbound kind.
    pair: TransformPair,
    ctx: TransformContext,
    inbound: ContentGenerationKind,
    skipped: u64,
}

impl SseTransformer {
    pub fn new(pair: TransformPair, ctx: TransformContext, inbound: ContentGenerationKind) -> Self {
        Self {
            decoder: SseDecoder::new(),
            pair,
            ctx,
            inbound,
            skipped: 0,
        }
    }

    /// Feed one upstream chunk; returns encoded inbound bytes (possibly empty).
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for frame in self.decoder.push(chunk) {
            self.convert_into(frame, &mut out);
        }
        out
    }

    /// Flush the trailing frame and emit the inbound terminator.
    pub fn finish(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(frame) = self.decoder.finish() {
            self.convert_into(frame, &mut out);
        }
        if self.inbound == ContentGenerationKind::OpenAiChatCompletions {
            out.extend_from_slice(b"data: [DONE]\n\n");
        }
        if self.skipped > 0 {
            tracing::warn!(
                skipped = self.skipped,
                "stream transform skipped unconvertible frames"
            );
        }
        out
    }

    fn convert_into(&mut self, frame: SseFrame, out: &mut Vec<u8>) {
        // upstream openai-chat terminator — represented by finish() inbound-side
        if frame.data.trim() == "[DONE]" {
            return;
        }
        let event: Value = match serde_json::from_str(&frame.data) {
            Ok(v) => v,
            Err(_) => {
                self.skipped += 1;
                return;
            }
        };
        match dispatch::stream_event_value(self.pair, &self.ctx, event) {
            Ok(converted) => {
                out.extend_from_slice(encode_frame(self.inbound, &converted).as_bytes())
            }
            Err(_) => {
                self.skipped += 1;
            }
        }
    }
}

/// Encode one converted event in the inbound wire format. Claude and OpenAI
/// Responses streams carry `event:` names equal to the payload `type`; chat
/// completions and gemini (`alt=sse`) are data-only.
fn encode_frame(kind: ContentGenerationKind, v: &Value) -> String {
    use ContentGenerationKind as K;
    let data = v.to_string();
    match kind {
        K::ClaudeMessages | K::OpenAiResponses => {
            let name = v.get("type").and_then(|t| t.as_str()).unwrap_or("message");
            SseFrame::event(name, data).encode()
        }
        K::OpenAiChatCompletions | K::GeminiGenerateContent => SseFrame::data(data).encode(),
    }
}

/// Convert a fully-buffered SSE body (wasm, or any buffered streaming attempt).
pub fn convert_buffered(mut t: SseTransformer, body: &[u8]) -> Vec<u8> {
    let mut out = t.push(body);
    out.extend(t.finish());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Operation, OperationKey};

    /// openai-chat upstream chunks → claude inbound events, across a chunk
    /// boundary, with [DONE] swallowed and claude event names emitted.
    #[test]
    fn chat_chunks_to_claude_events() {
        let upstream = OperationKey::content_generation(
            Operation::GenerateContent,
            ContentGenerationKind::OpenAiChatCompletions,
        );
        let inbound = OperationKey::content_generation(
            Operation::GenerateContent,
            ContentGenerationKind::ClaudeMessages,
        );
        let pair = crate::transform::resolve(upstream, inbound).unwrap();
        let mut t = SseTransformer::new(
            pair,
            TransformContext::new(upstream, inbound),
            ContentGenerationKind::ClaudeMessages,
        );
        let chunk = br#"data: {"id":"c1","object":"chat.completion.chunk","created":0,"model":"m","choices":[{"index":0,"delta":{"role":"assistant","content":"he"},"finish_reason":null}]}"#;
        let mut out = t.push(chunk);
        out.extend(t.push(b"\n\ndata: [DONE]\n\n"));
        out.extend(t.finish());
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("event: "),
            "claude frames carry event names: {text}"
        );
        assert!(
            !text.contains("[DONE]"),
            "claude streams have no DONE: {text}"
        );
        // every data line parses as JSON with a type field
        for line in text.lines().filter(|l| l.starts_with("data: ")) {
            let v: Value = serde_json::from_str(&line[6..]).unwrap();
            assert!(v.get("type").is_some());
        }
    }
}
