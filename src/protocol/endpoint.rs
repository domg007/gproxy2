//! Target endpoint synthesis (M2): the provider-relative path/query a
//! transformed request must hit for a given upstream wire kind. Passthrough
//! keeps the inbound path and never calls this.

use crate::protocol::ContentGenerationKind;

/// Provider-relative request target for a content-generation call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestTarget {
    pub path: String,
    /// Extra query the wire format requires (e.g. gemini `alt=sse`).
    pub query: Option<String>,
}

/// Build the request target for a content-generation kind. `model` is the
/// upstream model id (path-templated providers embed it); `stream` selects the
/// streaming variant where the wire format distinguishes it by endpoint.
pub fn content_request_target(
    kind: ContentGenerationKind,
    model: &str,
    stream: bool,
) -> RequestTarget {
    use ContentGenerationKind as K;
    match kind {
        K::OpenAiChatCompletions => RequestTarget {
            path: "/v1/chat/completions".to_owned(),
            query: None,
        },
        K::OpenAiResponses => RequestTarget {
            path: "/v1/responses".to_owned(),
            query: None,
        },
        K::ClaudeMessages => RequestTarget {
            path: "/v1/messages".to_owned(),
            query: None,
        },
        K::GeminiGenerateContent => {
            let verb = if stream {
                "streamGenerateContent"
            } else {
                "generateContent"
            };
            RequestTarget {
                path: format!("/v1beta/models/{model}:{verb}"),
                query: stream.then(|| "alt=sse".to_owned()),
            }
        }
    }
}
