use crate::response::UpstreamError;

/// Transform a request body from one (operation, protocol) to another.
///
/// This dispatches to the appropriate `TryFrom` implementation in `gproxy_protocol::transform`.
pub fn transform_request(
    src_operation: &str,
    src_protocol: &str,
    dst_operation: &str,
    dst_protocol: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, UpstreamError> {
    tracing::debug!(
        src_operation,
        src_protocol,
        dst_operation,
        dst_protocol,
        "transforming request"
    );
    let key = (src_operation, src_protocol, dst_operation, dst_protocol);

    match key {
        // =====================================================================
        // generate_content
        // =====================================================================

        // === Claude source → OpenAI targets ===
        ("generate_content", "claude", "generate_content", "openai_chat_completions") => {
            transform_json::<
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
                gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest,
            >(&body)
        }
        ("generate_content", "claude", "generate_content", "openai_response") => {
            transform_json::<
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
                gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest,
            >(&body)
        }

        // === Claude source → Gemini targets ===
        ("generate_content", "claude", "generate_content", "gemini") => {
            transform_json::<
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
                gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest,
            >(&body)
        }

        // === OpenAI ChatCompletions source → Claude ===
        ("generate_content", "openai_chat_completions", "generate_content", "claude") => {
            transform_json::<
                gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest,
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
            >(&body)
        }

        // === OpenAI ChatCompletions source → Gemini ===
        ("generate_content", "openai_chat_completions", "generate_content", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest,
                gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest,
            >(&body)
        }

        // === OpenAI Response source → Claude ===
        ("generate_content", "openai_response", "generate_content", "claude") => {
            transform_json::<
                gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest,
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
            >(&body)
        }

        // === OpenAI Response source → Gemini ===
        ("generate_content", "openai_response", "generate_content", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest,
                gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest,
            >(&body)
        }

        // === Gemini source → Claude ===
        ("generate_content", "gemini", "generate_content", "claude") => {
            transform_json::<
                gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest,
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
            >(&body)
        }

        // === Gemini source → OpenAI ChatCompletions ===
        ("generate_content", "gemini", "generate_content", "openai_chat_completions") => {
            transform_json::<
                gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest,
                gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest,
            >(&body)
        }

        // === Gemini source → OpenAI Response ===
        ("generate_content", "gemini", "generate_content", "openai_response") => {
            transform_json::<
                gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest,
                gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest,
            >(&body)
        }

        // =====================================================================
        // stream_generate_content (request transforms only)
        // =====================================================================

        // --- Claude source ---
        ("stream_generate_content", "claude", "stream_generate_content", "gemini") => {
            transform_json_ref::<
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
                gproxy_protocol::gemini::stream_generate_content::request::GeminiStreamGenerateContentRequest,
            >(&body)
        }
        ("stream_generate_content", "claude", "stream_generate_content", "openai_chat_completions") => {
            transform_json_ref::<
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
                gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest,
            >(&body)
        }
        ("stream_generate_content", "claude", "stream_generate_content", "openai_response") => {
            transform_json_ref::<
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
                gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest,
            >(&body)
        }

        // --- Gemini source ---
        ("stream_generate_content", "gemini", "stream_generate_content", "claude") => {
            transform_json::<
                gproxy_protocol::gemini::stream_generate_content::request::GeminiStreamGenerateContentRequest,
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
            >(&body)
        }
        ("stream_generate_content", "gemini", "stream_generate_content", "openai_chat_completions") => {
            transform_json::<
                gproxy_protocol::gemini::stream_generate_content::request::GeminiStreamGenerateContentRequest,
                gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest,
            >(&body)
        }
        ("stream_generate_content", "gemini", "stream_generate_content", "openai_response") => {
            transform_json::<
                gproxy_protocol::gemini::stream_generate_content::request::GeminiStreamGenerateContentRequest,
                gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest,
            >(&body)
        }

        // --- OpenAI ChatCompletions source ---
        ("stream_generate_content", "openai_chat_completions", "stream_generate_content", "claude") => {
            transform_json_ref::<
                gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest,
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
            >(&body)
        }
        ("stream_generate_content", "openai_chat_completions", "stream_generate_content", "gemini") => {
            transform_json_ref::<
                gproxy_protocol::openai::create_chat_completions::request::OpenAiChatCompletionsRequest,
                gproxy_protocol::gemini::stream_generate_content::request::GeminiStreamGenerateContentRequest,
            >(&body)
        }

        // --- OpenAI Response source ---
        ("stream_generate_content", "openai_response", "stream_generate_content", "claude") => {
            transform_json_ref::<
                gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest,
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
            >(&body)
        }
        ("stream_generate_content", "openai_response", "stream_generate_content", "gemini") => {
            transform_json_ref::<
                gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest,
                gproxy_protocol::gemini::stream_generate_content::request::GeminiStreamGenerateContentRequest,
            >(&body)
        }

        // =====================================================================
        // count_tokens
        // =====================================================================

        // --- Claude source ---
        ("count_tokens", "claude", "count_tokens", "gemini") => {
            transform_json::<
                gproxy_protocol::claude::count_tokens::request::ClaudeCountTokensRequest,
                gproxy_protocol::gemini::count_tokens::request::GeminiCountTokensRequest,
            >(&body)
        }
        ("count_tokens", "claude", "count_tokens", "openai") => {
            transform_json::<
                gproxy_protocol::claude::count_tokens::request::ClaudeCountTokensRequest,
                gproxy_protocol::openai::count_tokens::request::OpenAiCountTokensRequest,
            >(&body)
        }

        // --- OpenAI source ---
        ("count_tokens", "openai", "count_tokens", "claude") => {
            transform_json::<
                gproxy_protocol::openai::count_tokens::request::OpenAiCountTokensRequest,
                gproxy_protocol::claude::count_tokens::request::ClaudeCountTokensRequest,
            >(&body)
        }
        ("count_tokens", "openai", "count_tokens", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::count_tokens::request::OpenAiCountTokensRequest,
                gproxy_protocol::gemini::count_tokens::request::GeminiCountTokensRequest,
            >(&body)
        }

        // --- Gemini source ---
        ("count_tokens", "gemini", "count_tokens", "claude") => {
            transform_json::<
                gproxy_protocol::gemini::count_tokens::request::GeminiCountTokensRequest,
                gproxy_protocol::claude::count_tokens::request::ClaudeCountTokensRequest,
            >(&body)
        }
        ("count_tokens", "gemini", "count_tokens", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::count_tokens::request::GeminiCountTokensRequest,
                gproxy_protocol::openai::count_tokens::request::OpenAiCountTokensRequest,
            >(&body)
        }

        // =====================================================================
        // model_get
        // =====================================================================

        // --- Claude source ---
        ("model_get", "claude", "model_get", "gemini") => {
            transform_json::<
                gproxy_protocol::claude::model_get::request::ClaudeModelGetRequest,
                gproxy_protocol::gemini::model_get::request::GeminiModelGetRequest,
            >(&body)
        }
        ("model_get", "claude", "model_get", "openai") => {
            transform_json::<
                gproxy_protocol::claude::model_get::request::ClaudeModelGetRequest,
                gproxy_protocol::openai::model_get::request::OpenAiModelGetRequest,
            >(&body)
        }

        // --- OpenAI source ---
        ("model_get", "openai", "model_get", "claude") => {
            transform_json::<
                gproxy_protocol::openai::model_get::request::OpenAiModelGetRequest,
                gproxy_protocol::claude::model_get::request::ClaudeModelGetRequest,
            >(&body)
        }
        ("model_get", "openai", "model_get", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::model_get::request::OpenAiModelGetRequest,
                gproxy_protocol::gemini::model_get::request::GeminiModelGetRequest,
            >(&body)
        }

        // --- Gemini source ---
        ("model_get", "gemini", "model_get", "claude") => {
            transform_json::<
                gproxy_protocol::gemini::model_get::request::GeminiModelGetRequest,
                gproxy_protocol::claude::model_get::request::ClaudeModelGetRequest,
            >(&body)
        }
        ("model_get", "gemini", "model_get", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::model_get::request::GeminiModelGetRequest,
                gproxy_protocol::openai::model_get::request::OpenAiModelGetRequest,
            >(&body)
        }

        // =====================================================================
        // model_list
        // =====================================================================

        // --- Claude source ---
        ("model_list", "claude", "model_list", "gemini") => {
            transform_json::<
                gproxy_protocol::claude::model_list::request::ClaudeModelListRequest,
                gproxy_protocol::gemini::model_list::request::GeminiModelListRequest,
            >(&body)
        }
        ("model_list", "claude", "model_list", "openai") => {
            transform_json::<
                gproxy_protocol::claude::model_list::request::ClaudeModelListRequest,
                gproxy_protocol::openai::model_list::request::OpenAiModelListRequest,
            >(&body)
        }

        // --- OpenAI source ---
        ("model_list", "openai", "model_list", "claude") => {
            transform_json::<
                gproxy_protocol::openai::model_list::request::OpenAiModelListRequest,
                gproxy_protocol::claude::model_list::request::ClaudeModelListRequest,
            >(&body)
        }
        ("model_list", "openai", "model_list", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::model_list::request::OpenAiModelListRequest,
                gproxy_protocol::gemini::model_list::request::GeminiModelListRequest,
            >(&body)
        }

        // --- Gemini source ---
        ("model_list", "gemini", "model_list", "claude") => {
            transform_json::<
                gproxy_protocol::gemini::model_list::request::GeminiModelListRequest,
                gproxy_protocol::claude::model_list::request::ClaudeModelListRequest,
            >(&body)
        }
        ("model_list", "gemini", "model_list", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::model_list::request::GeminiModelListRequest,
                gproxy_protocol::openai::model_list::request::OpenAiModelListRequest,
            >(&body)
        }

        // =====================================================================
        // embeddings
        // =====================================================================

        ("embeddings", "openai", "embeddings", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::embeddings::request::OpenAiEmbeddingsRequest,
                gproxy_protocol::gemini::embeddings::request::GeminiEmbedContentRequest,
            >(&body)
        }
        ("embeddings", "gemini", "embeddings", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::embeddings::request::GeminiEmbedContentRequest,
                gproxy_protocol::openai::embeddings::request::OpenAiEmbeddingsRequest,
            >(&body)
        }

        // =====================================================================
        // create_image
        // =====================================================================

        ("create_image", "openai", "generate_content", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::create_image::request::OpenAiCreateImageRequest,
                gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest,
            >(&body)
        }

        // =====================================================================
        // create_image_edit
        // =====================================================================

        ("create_image_edit", "openai", "generate_content", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::create_image_edit::request::OpenAiCreateImageEditRequest,
                gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest,
            >(&body)
        }

        // =====================================================================
        // compact
        // =====================================================================

        ("compact", "openai", "generate_content", "claude") => {
            transform_json::<
                gproxy_protocol::openai::compact_response::request::OpenAiCompactRequest,
                gproxy_protocol::claude::create_message::request::ClaudeCreateMessageRequest,
            >(&body)
        }
        ("compact", "openai", "generate_content", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::compact_response::request::OpenAiCompactRequest,
                gproxy_protocol::gemini::generate_content::request::GeminiGenerateContentRequest,
            >(&body)
        }

        _ => Err(UpstreamError::Channel(format!(
            "no request transform for ({}, {}) -> ({}, {})",
            src_operation, src_protocol, dst_operation, dst_protocol
        ))),
    }
}

/// Transform a response body from upstream protocol back to client protocol.
///
/// When a client sends request in (src_op, src_proto) and upstream responds in
/// (dst_op, dst_proto), we need to convert the upstream response back to the
/// client's protocol. The key is looked up as (dst_op, dst_proto, src_op, src_proto)
/// because we're converting FROM the upstream format TO the client format.
pub fn transform_response(
    src_operation: &str,
    src_protocol: &str,
    dst_operation: &str,
    dst_protocol: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, UpstreamError> {
    tracing::debug!(
        src_operation,
        src_protocol,
        dst_operation,
        dst_protocol,
        "transforming response"
    );
    // Response direction: upstream responded in (dst_op, dst_proto),
    // client expects (src_op, src_proto).
    let key = (dst_operation, dst_protocol, src_operation, src_protocol);

    match key {
        // =====================================================================
        // generate_content responses
        // =====================================================================

        // Gemini response → Claude
        ("generate_content", "gemini", "generate_content", "claude") => {
            transform_json::<
                gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse,
                gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse,
            >(&body)
        }
        // OpenAI ChatCompletions response → Claude
        ("generate_content", "openai_chat_completions", "generate_content", "claude") => {
            transform_json::<
                gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse,
                gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse,
            >(&body)
        }
        // OpenAI Response response → Claude
        ("generate_content", "openai_response", "generate_content", "claude") => {
            transform_json::<
                gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse,
                gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse,
            >(&body)
        }

        // Claude response → Gemini
        ("generate_content", "claude", "generate_content", "gemini") => {
            transform_json::<
                gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse,
                gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse,
            >(&body)
        }
        // OpenAI ChatCompletions response → Gemini
        ("generate_content", "openai_chat_completions", "generate_content", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse,
                gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse,
            >(&body)
        }
        // OpenAI Response response → Gemini
        ("generate_content", "openai_response", "generate_content", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse,
                gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse,
            >(&body)
        }

        // Claude response → OpenAI ChatCompletions
        ("generate_content", "claude", "generate_content", "openai_chat_completions") => {
            transform_json::<
                gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse,
                gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse,
            >(&body)
        }
        // Gemini response → OpenAI ChatCompletions
        ("generate_content", "gemini", "generate_content", "openai_chat_completions") => {
            transform_json::<
                gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse,
                gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse,
            >(&body)
        }

        // Claude response → OpenAI Response
        ("generate_content", "claude", "generate_content", "openai_response") => {
            transform_json::<
                gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse,
                gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse,
            >(&body)
        }
        // Gemini response → OpenAI Response
        ("generate_content", "gemini", "generate_content", "openai_response") => {
            transform_json::<
                gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse,
                gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse,
            >(&body)
        }

        // =====================================================================
        // count_tokens responses
        // =====================================================================

        // Gemini response → Claude
        ("count_tokens", "gemini", "count_tokens", "claude") => {
            transform_json::<
                gproxy_protocol::gemini::count_tokens::response::GeminiCountTokensResponse,
                gproxy_protocol::claude::count_tokens::response::ClaudeCountTokensResponse,
            >(&body)
        }
        // OpenAI response → Claude
        ("count_tokens", "openai", "count_tokens", "claude") => {
            transform_json::<
                gproxy_protocol::openai::count_tokens::response::OpenAiCountTokensResponse,
                gproxy_protocol::claude::count_tokens::response::ClaudeCountTokensResponse,
            >(&body)
        }

        // Claude response → OpenAI
        ("count_tokens", "claude", "count_tokens", "openai") => {
            transform_json::<
                gproxy_protocol::claude::count_tokens::response::ClaudeCountTokensResponse,
                gproxy_protocol::openai::count_tokens::response::OpenAiCountTokensResponse,
            >(&body)
        }
        // Gemini response → OpenAI
        ("count_tokens", "gemini", "count_tokens", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::count_tokens::response::GeminiCountTokensResponse,
                gproxy_protocol::openai::count_tokens::response::OpenAiCountTokensResponse,
            >(&body)
        }

        // Claude response → Gemini
        ("count_tokens", "claude", "count_tokens", "gemini") => {
            transform_json::<
                gproxy_protocol::claude::count_tokens::response::ClaudeCountTokensResponse,
                gproxy_protocol::gemini::count_tokens::response::GeminiCountTokensResponse,
            >(&body)
        }
        // OpenAI response → Gemini
        ("count_tokens", "openai", "count_tokens", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::count_tokens::response::OpenAiCountTokensResponse,
                gproxy_protocol::gemini::count_tokens::response::GeminiCountTokensResponse,
            >(&body)
        }

        // =====================================================================
        // model_get responses
        // =====================================================================

        // Gemini response → Claude
        ("model_get", "gemini", "model_get", "claude") => {
            transform_json::<
                gproxy_protocol::gemini::model_get::response::GeminiModelGetResponse,
                gproxy_protocol::claude::model_get::response::ClaudeModelGetResponse,
            >(&body)
        }
        // OpenAI response → Claude
        ("model_get", "openai", "model_get", "claude") => {
            transform_json::<
                gproxy_protocol::openai::model_get::response::OpenAiModelGetResponse,
                gproxy_protocol::claude::model_get::response::ClaudeModelGetResponse,
            >(&body)
        }

        // Claude response → OpenAI
        ("model_get", "claude", "model_get", "openai") => {
            transform_json::<
                gproxy_protocol::claude::model_get::response::ClaudeModelGetResponse,
                gproxy_protocol::openai::model_get::response::OpenAiModelGetResponse,
            >(&body)
        }
        // Gemini response → OpenAI
        ("model_get", "gemini", "model_get", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::model_get::response::GeminiModelGetResponse,
                gproxy_protocol::openai::model_get::response::OpenAiModelGetResponse,
            >(&body)
        }

        // Claude response → Gemini
        ("model_get", "claude", "model_get", "gemini") => {
            transform_json::<
                gproxy_protocol::claude::model_get::response::ClaudeModelGetResponse,
                gproxy_protocol::gemini::model_get::response::GeminiModelGetResponse,
            >(&body)
        }
        // OpenAI response → Gemini
        ("model_get", "openai", "model_get", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::model_get::response::OpenAiModelGetResponse,
                gproxy_protocol::gemini::model_get::response::GeminiModelGetResponse,
            >(&body)
        }

        // =====================================================================
        // model_list responses
        // =====================================================================

        // Gemini response → Claude
        ("model_list", "gemini", "model_list", "claude") => {
            transform_json::<
                gproxy_protocol::gemini::model_list::response::GeminiModelListResponse,
                gproxy_protocol::claude::model_list::response::ClaudeModelListResponse,
            >(&body)
        }
        // OpenAI response → Claude
        ("model_list", "openai", "model_list", "claude") => {
            transform_json::<
                gproxy_protocol::openai::model_list::response::OpenAiModelListResponse,
                gproxy_protocol::claude::model_list::response::ClaudeModelListResponse,
            >(&body)
        }

        // Claude response → OpenAI
        ("model_list", "claude", "model_list", "openai") => {
            transform_json::<
                gproxy_protocol::claude::model_list::response::ClaudeModelListResponse,
                gproxy_protocol::openai::model_list::response::OpenAiModelListResponse,
            >(&body)
        }
        // Gemini response → OpenAI
        ("model_list", "gemini", "model_list", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::model_list::response::GeminiModelListResponse,
                gproxy_protocol::openai::model_list::response::OpenAiModelListResponse,
            >(&body)
        }

        // Claude response → Gemini
        ("model_list", "claude", "model_list", "gemini") => {
            transform_json::<
                gproxy_protocol::claude::model_list::response::ClaudeModelListResponse,
                gproxy_protocol::gemini::model_list::response::GeminiModelListResponse,
            >(&body)
        }
        // OpenAI response → Gemini
        ("model_list", "openai", "model_list", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::model_list::response::OpenAiModelListResponse,
                gproxy_protocol::gemini::model_list::response::GeminiModelListResponse,
            >(&body)
        }

        // =====================================================================
        // embeddings responses
        // =====================================================================

        ("embeddings", "gemini", "embeddings", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::embeddings::response::GeminiEmbedContentResponse,
                gproxy_protocol::openai::embeddings::response::OpenAiEmbeddingsResponse,
            >(&body)
        }
        ("embeddings", "openai", "embeddings", "gemini") => {
            transform_json::<
                gproxy_protocol::openai::embeddings::response::OpenAiEmbeddingsResponse,
                gproxy_protocol::gemini::embeddings::response::GeminiEmbedContentResponse,
            >(&body)
        }

        // =====================================================================
        // create_image responses
        // =====================================================================

        ("generate_content", "gemini", "create_image", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse,
                gproxy_protocol::openai::create_image::response::OpenAiCreateImageResponse,
            >(&body)
        }

        // =====================================================================
        // create_image_edit responses
        // =====================================================================

        ("generate_content", "gemini", "create_image_edit", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse,
                gproxy_protocol::openai::create_image_edit::response::OpenAiCreateImageEditResponse,
            >(&body)
        }

        // =====================================================================
        // compact responses
        // =====================================================================

        ("generate_content", "claude", "compact", "openai") => {
            transform_json::<
                gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse,
                gproxy_protocol::openai::compact_response::response::OpenAiCompactResponse,
            >(&body)
        }
        ("generate_content", "gemini", "compact", "openai") => {
            transform_json::<
                gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse,
                gproxy_protocol::openai::compact_response::response::OpenAiCompactResponse,
            >(&body)
        }

        _ => Err(UpstreamError::Channel(format!(
            "no response transform for ({}, {}) -> ({}, {})",
            dst_operation, dst_protocol, src_operation, src_protocol
        ))),
    }
}

/// Generic JSON transform: deserialize as Src, TryFrom into Dst, serialize back.
fn transform_json<Src, Dst>(body: &[u8]) -> Result<Vec<u8>, UpstreamError>
where
    Src: serde::de::DeserializeOwned,
    Dst: TryFrom<Src> + serde::Serialize,
    Dst::Error: std::fmt::Display,
{
    let src: Src = serde_json::from_slice(body)
        .map_err(|e| UpstreamError::Channel(format!("request deserialize: {}", e)))?;

    let dst =
        Dst::try_from(src).map_err(|e| UpstreamError::Channel(format!("transform: {}", e)))?;

    serde_json::to_vec(&dst)
        .map_err(|e| UpstreamError::Channel(format!("response serialize: {}", e)))
}

/// Generic JSON transform for reference-based TryFrom (used by stream request transforms).
/// Deserializes as Src, converts via `TryFrom<&Src>` into Dst, then serializes.
fn transform_json_ref<Src, Dst>(body: &[u8]) -> Result<Vec<u8>, UpstreamError>
where
    Src: serde::de::DeserializeOwned,
    for<'a> Dst: TryFrom<&'a Src> + serde::Serialize,
    for<'a> <Dst as TryFrom<&'a Src>>::Error: std::fmt::Display,
{
    let src: Src = serde_json::from_slice(body)
        .map_err(|e| UpstreamError::Channel(format!("request deserialize: {}", e)))?;

    let dst =
        Dst::try_from(&src).map_err(|e| UpstreamError::Channel(format!("transform: {}", e)))?;

    serde_json::to_vec(&dst)
        .map_err(|e| UpstreamError::Channel(format!("response serialize: {}", e)))
}

// =====================================================================
// Nonstream ↔ Stream conversions (same protocol, format change)
// =====================================================================

/// Convert a non-streaming response to stream events (same protocol).
/// Output is NDJSON (one JSON line per event) written into `out`.
pub fn nonstream_to_stream(
    protocol: &str,
    body: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), UpstreamError> {
    match protocol {
        "claude" => {
            use gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse;
            use gproxy_protocol::claude::create_message::stream::ClaudeStreamEvent;
            use gproxy_protocol::transform::claude::nonstream_to_stream::nonstream_to_stream;

            let response: ClaudeCreateMessageResponse = serde_json::from_slice(body)
                .map_err(|e| UpstreamError::Channel(format!("deserialize: {e}")))?;

            let mut events: Vec<ClaudeStreamEvent> = Vec::new();
            nonstream_to_stream(response, &mut events)
                .map_err(|e| UpstreamError::Channel(format!("nonstream_to_stream: {e}")))?;

            for event in &events {
                let json = serde_json::to_vec(event)
                    .map_err(|e| UpstreamError::Channel(format!("serialize event: {e}")))?;
                out.extend_from_slice(&json);
                out.push(b'\n');
            }
            Ok(())
        }
        "openai_chat_completions" => {
            use gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
            use gproxy_protocol::openai::create_chat_completions::stream::ChatCompletionChunk;

            let response: OpenAiChatCompletionsResponse = serde_json::from_slice(body)
                .map_err(|e| UpstreamError::Channel(format!("deserialize: {e}")))?;

            let chunks = Vec::<ChatCompletionChunk>::try_from(response)
                .map_err(|e| UpstreamError::Channel(format!("nonstream_to_stream: {e}")))?;

            for chunk in &chunks {
                let json = serde_json::to_vec(chunk)
                    .map_err(|e| UpstreamError::Channel(format!("serialize chunk: {e}")))?;
                out.extend_from_slice(&json);
                out.push(b'\n');
            }
            Ok(())
        }
        "openai_response" => {
            use gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse;
            use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;

            let response: OpenAiCreateResponseResponse = serde_json::from_slice(body)
                .map_err(|e| UpstreamError::Channel(format!("deserialize: {e}")))?;

            let events = Vec::<ResponseStreamEvent>::try_from(response)
                .map_err(|e| UpstreamError::Channel(format!("nonstream_to_stream: {e}")))?;

            for event in &events {
                let json = serde_json::to_vec(event)
                    .map_err(|e| UpstreamError::Channel(format!("serialize event: {e}")))?;
                out.extend_from_slice(&json);
                out.push(b'\n');
            }
            Ok(())
        }
        "gemini" => {
            use gproxy_protocol::gemini::generate_content::response::GeminiGenerateContentResponse;

            let response: GeminiGenerateContentResponse = serde_json::from_slice(body)
                .map_err(|e| UpstreamError::Channel(format!("deserialize: {e}")))?;

            // Gemini non-stream and stream share the same chunk body shape
            if let GeminiGenerateContentResponse::Success { body: resp, .. } = response {
                let json = serde_json::to_vec(&resp)
                    .map_err(|e| UpstreamError::Channel(format!("serialize chunk: {e}")))?;
                out.extend_from_slice(&json);
                out.push(b'\n');
            }
            Ok(())
        }
        _ => Err(UpstreamError::Channel(format!(
            "no nonstream_to_stream for protocol: {protocol}"
        ))),
    }
}

/// Convert stream events (NDJSON lines) to a non-streaming response (same protocol).
pub fn stream_to_nonstream(protocol: &str, chunks: &[&[u8]]) -> Result<Vec<u8>, UpstreamError> {
    match protocol {
        "claude" => {
            use gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse;
            use gproxy_protocol::claude::create_message::stream::ClaudeStreamEvent;

            let events: Vec<ClaudeStreamEvent> = chunks
                .iter()
                .map(|c| serde_json::from_slice(c))
                .collect::<Result<_, _>>()
                .map_err(|e| UpstreamError::Channel(format!("deserialize events: {e}")))?;

            let response = ClaudeCreateMessageResponse::try_from(events)
                .map_err(|e| UpstreamError::Channel(format!("stream_to_nonstream: {e}")))?;

            serde_json::to_vec(&response)
                .map_err(|e| UpstreamError::Channel(format!("serialize: {e}")))
        }
        "openai_chat_completions" => {
            use gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
            use gproxy_protocol::openai::create_chat_completions::stream::ChatCompletionChunk;

            let chunks_parsed: Vec<ChatCompletionChunk> = chunks
                .iter()
                .map(|c| serde_json::from_slice(c))
                .collect::<Result<_, _>>()
                .map_err(|e| UpstreamError::Channel(format!("deserialize chunks: {e}")))?;

            let response = OpenAiChatCompletionsResponse::try_from(chunks_parsed)
                .map_err(|e| UpstreamError::Channel(format!("stream_to_nonstream: {e}")))?;

            serde_json::to_vec(&response)
                .map_err(|e| UpstreamError::Channel(format!("serialize: {e}")))
        }
        "openai_response" => {
            use gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse;
            use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;

            let events: Vec<ResponseStreamEvent> = chunks
                .iter()
                .map(|c| serde_json::from_slice(c))
                .collect::<Result<_, _>>()
                .map_err(|e| UpstreamError::Channel(format!("deserialize events: {e}")))?;

            let response = OpenAiCreateResponseResponse::try_from(events)
                .map_err(|e| UpstreamError::Channel(format!("stream_to_nonstream: {e}")))?;

            serde_json::to_vec(&response)
                .map_err(|e| UpstreamError::Channel(format!("serialize: {e}")))
        }
        "gemini" => {
            use gproxy_protocol::gemini::generate_content::response::ResponseBody;
            use gproxy_protocol::gemini::generate_content::types::GeminiCandidate;
            use std::collections::BTreeMap;

            let mut merged = ResponseBody::default();
            let mut candidate_map: BTreeMap<u32, GeminiCandidate> = BTreeMap::new();

            for chunk in chunks {
                let body: ResponseBody = serde_json::from_slice(chunk)
                    .map_err(|e| UpstreamError::Channel(format!("deserialize chunk: {e}")))?;
                gproxy_protocol::transform::gemini::stream_to_nonstream::merge_chunk(
                    &mut merged,
                    &mut candidate_map,
                    body,
                );
            }

            let body = gproxy_protocol::transform::gemini::stream_to_nonstream::finalize_body(
                merged,
                candidate_map,
            );

            serde_json::to_vec(&body).map_err(|e| UpstreamError::Channel(format!("serialize: {e}")))
        }
        _ => Err(UpstreamError::Channel(format!(
            "no stream_to_nonstream for protocol: {protocol}"
        ))),
    }
}
