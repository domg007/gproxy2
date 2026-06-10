//! Bytes-level dispatch from a resolved [`TransformPair`] to its typed pair
//! functions. M2 wires the 12 content-generation pairs; other groups
//! (count_tokens/models/embeddings/images/compact) return `InvalidInput`
//! until their gateway paths exist in `pipeline/classify`.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::generate_content as gc;
use super::{TransformContext, TransformError, TransformPair};

/// Whether the bytes dispatch has arms for this pair.
pub fn is_wired(pair: TransformPair) -> bool {
    use TransformPair as P;
    matches!(
        pair,
        P::ClaudeMessagesToGeminiGenerateContent
            | P::ClaudeMessagesToOpenAiChat
            | P::ClaudeMessagesToOpenAiResponses
            | P::GeminiGenerateContentToClaudeMessages
            | P::GeminiGenerateContentToOpenAiChat
            | P::GeminiGenerateContentToOpenAiResponses
            | P::OpenAiChatToClaudeMessages
            | P::OpenAiChatToGeminiGenerateContent
            | P::OpenAiChatToOpenAiResponses
            | P::OpenAiResponsesToClaudeMessages
            | P::OpenAiResponsesToGeminiGenerateContent
            | P::OpenAiResponsesToOpenAiChat
    )
}

/// Convert a request body (inbound wire JSON → upstream wire JSON).
pub fn request_bytes(
    pair: TransformPair,
    ctx: &TransformContext,
    body: &[u8],
) -> Result<Vec<u8>, TransformError> {
    use TransformPair as P;
    match pair {
        P::ClaudeMessagesToGeminiGenerateContent => run(
            gc::claude_messages_to_gemini_generate_content::request,
            ctx,
            body,
        ),
        P::ClaudeMessagesToOpenAiChat => {
            run(gc::claude_messages_to_openai_chat::request, ctx, body)
        }
        P::ClaudeMessagesToOpenAiResponses => {
            run(gc::claude_messages_to_openai_responses::request, ctx, body)
        }
        P::GeminiGenerateContentToClaudeMessages => run(
            gc::gemini_generate_content_to_claude_messages::request,
            ctx,
            body,
        ),
        P::GeminiGenerateContentToOpenAiChat => run(
            gc::gemini_generate_content_to_openai_chat::request,
            ctx,
            body,
        ),
        P::GeminiGenerateContentToOpenAiResponses => run(
            gc::gemini_generate_content_to_openai_responses::request,
            ctx,
            body,
        ),
        P::OpenAiChatToClaudeMessages => {
            run(gc::openai_chat_to_claude_messages::request, ctx, body)
        }
        P::OpenAiChatToGeminiGenerateContent => run(
            gc::openai_chat_to_gemini_generate_content::request,
            ctx,
            body,
        ),
        P::OpenAiChatToOpenAiResponses => {
            run(gc::openai_chat_to_openai_responses::request, ctx, body)
        }
        P::OpenAiResponsesToClaudeMessages => {
            run(gc::openai_responses_to_claude_messages::request, ctx, body)
        }
        P::OpenAiResponsesToGeminiGenerateContent => run(
            gc::openai_responses_to_gemini_generate_content::request,
            ctx,
            body,
        ),
        P::OpenAiResponsesToOpenAiChat => {
            run(gc::openai_responses_to_openai_chat::request, ctx, body)
        }
        other => Err(not_wired(other)),
    }
}

/// Convert a response body (upstream wire JSON → inbound wire JSON). The pair
/// here is the REVERSE pair (`resolve(upstream_key, inbound_key)`).
pub fn response_bytes(
    pair: TransformPair,
    ctx: &TransformContext,
    body: &[u8],
) -> Result<Vec<u8>, TransformError> {
    use TransformPair as P;
    match pair {
        P::ClaudeMessagesToGeminiGenerateContent => run(
            gc::claude_messages_to_gemini_generate_content::response,
            ctx,
            body,
        ),
        P::ClaudeMessagesToOpenAiChat => {
            run(gc::claude_messages_to_openai_chat::response, ctx, body)
        }
        P::ClaudeMessagesToOpenAiResponses => {
            run(gc::claude_messages_to_openai_responses::response, ctx, body)
        }
        P::GeminiGenerateContentToClaudeMessages => run(
            gc::gemini_generate_content_to_claude_messages::response,
            ctx,
            body,
        ),
        P::GeminiGenerateContentToOpenAiChat => run(
            gc::gemini_generate_content_to_openai_chat::response,
            ctx,
            body,
        ),
        P::GeminiGenerateContentToOpenAiResponses => run(
            gc::gemini_generate_content_to_openai_responses::response,
            ctx,
            body,
        ),
        P::OpenAiChatToClaudeMessages => {
            run(gc::openai_chat_to_claude_messages::response, ctx, body)
        }
        P::OpenAiChatToGeminiGenerateContent => run(
            gc::openai_chat_to_gemini_generate_content::response,
            ctx,
            body,
        ),
        P::OpenAiChatToOpenAiResponses => {
            run(gc::openai_chat_to_openai_responses::response, ctx, body)
        }
        P::OpenAiResponsesToClaudeMessages => {
            run(gc::openai_responses_to_claude_messages::response, ctx, body)
        }
        P::OpenAiResponsesToGeminiGenerateContent => run(
            gc::openai_responses_to_gemini_generate_content::response,
            ctx,
            body,
        ),
        P::OpenAiResponsesToOpenAiChat => {
            run(gc::openai_responses_to_openai_chat::response, ctx, body)
        }
        other => Err(not_wired(other)),
    }
}

/// Convert one decoded stream event (upstream wire JSON value → inbound wire
/// JSON value). Same reverse-pair convention as [`response_bytes`].
pub fn stream_event_value(
    pair: TransformPair,
    ctx: &TransformContext,
    event: Value,
) -> Result<Value, TransformError> {
    use TransformPair as P;
    match pair {
        P::ClaudeMessagesToGeminiGenerateContent => run_value(
            gc::claude_messages_to_gemini_generate_content::stream_event,
            ctx,
            event,
        ),
        P::ClaudeMessagesToOpenAiChat => {
            run_value(gc::claude_messages_to_openai_chat::stream_event, ctx, event)
        }
        P::ClaudeMessagesToOpenAiResponses => run_value(
            gc::claude_messages_to_openai_responses::stream_event,
            ctx,
            event,
        ),
        P::GeminiGenerateContentToClaudeMessages => run_value(
            gc::gemini_generate_content_to_claude_messages::stream_event,
            ctx,
            event,
        ),
        P::GeminiGenerateContentToOpenAiChat => run_value(
            gc::gemini_generate_content_to_openai_chat::stream_event,
            ctx,
            event,
        ),
        P::GeminiGenerateContentToOpenAiResponses => run_value(
            gc::gemini_generate_content_to_openai_responses::stream_event,
            ctx,
            event,
        ),
        P::OpenAiChatToClaudeMessages => {
            run_value(gc::openai_chat_to_claude_messages::stream_event, ctx, event)
        }
        P::OpenAiChatToGeminiGenerateContent => run_value(
            gc::openai_chat_to_gemini_generate_content::stream_event,
            ctx,
            event,
        ),
        P::OpenAiChatToOpenAiResponses => run_value(
            gc::openai_chat_to_openai_responses::stream_event,
            ctx,
            event,
        ),
        P::OpenAiResponsesToClaudeMessages => run_value(
            gc::openai_responses_to_claude_messages::stream_event,
            ctx,
            event,
        ),
        P::OpenAiResponsesToGeminiGenerateContent => run_value(
            gc::openai_responses_to_gemini_generate_content::stream_event,
            ctx,
            event,
        ),
        P::OpenAiResponsesToOpenAiChat => run_value(
            gc::openai_responses_to_openai_chat::stream_event,
            ctx,
            event,
        ),
        other => Err(not_wired(other)),
    }
}

fn run<S, T>(
    f: impl Fn(S, &TransformContext) -> Result<T, TransformError>,
    ctx: &TransformContext,
    body: &[u8],
) -> Result<Vec<u8>, TransformError>
where
    S: DeserializeOwned,
    T: Serialize,
{
    let input: S = serde_json::from_slice(body).map_err(|e| TransformError::InvalidInput {
        reason: format!("decode source body: {e}"),
    })?;
    let out = f(input, ctx)?;
    serde_json::to_vec(&out).map_err(|e| TransformError::Serialization {
        reason: e.to_string(),
    })
}

fn run_value<S, T>(
    f: impl Fn(S, &TransformContext) -> Result<T, TransformError>,
    ctx: &TransformContext,
    event: Value,
) -> Result<Value, TransformError>
where
    S: DeserializeOwned,
    T: Serialize,
{
    let input: S = serde_json::from_value(event).map_err(|e| TransformError::InvalidInput {
        reason: format!("decode stream event: {e}"),
    })?;
    let out = f(input, ctx)?;
    serde_json::to_value(&out).map_err(|e| TransformError::Serialization {
        reason: e.to_string(),
    })
}

fn not_wired(pair: TransformPair) -> TransformError {
    TransformError::InvalidInput {
        reason: format!("bytes dispatch not wired for {pair:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ContentGenerationKind, Operation, OperationKey};

    #[test]
    fn claude_to_openai_chat_request_roundtrip() {
        let source = OperationKey::content_generation(
            Operation::GenerateContent,
            ContentGenerationKind::ClaudeMessages,
        );
        let target = OperationKey::content_generation(
            Operation::GenerateContent,
            ContentGenerationKind::OpenAiChatCompletions,
        );
        let ctx = TransformContext::new(source, target);
        let body = br#"{"model":"m","max_tokens":16,"messages":[{"role":"user","content":"hi"}]}"#;
        let out = request_bytes(TransformPair::ClaudeMessagesToOpenAiChat, &ctx, body).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["messages"][0]["role"], "user");
        assert!(v.get("max_tokens").is_some() || v.get("max_completion_tokens").is_some());
    }
}
