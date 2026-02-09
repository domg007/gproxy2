use super::*;
use gproxy_protocol::claude::count_tokens::response::CountTokensResponse as ClaudeCountTokensResponse;
use gproxy_protocol::claude::count_tokens::types::Model as ClaudeModel;
use gproxy_protocol::claude::create_message::types::{
    BetaCacheCreation, BetaMessage, BetaMessageRole, BetaMessageType, BetaServiceTierUsed,
    BetaUsage,
};
use gproxy_protocol::claude::list_models::request::ListModelsRequest as ClaudeListModelsRequest;
use gproxy_protocol::gemini::count_tokens::response::CountTokensResponse as GeminiCountTokensResponse;
use gproxy_protocol::gemini::generate_content::response::GenerateContentResponse as GeminiGenerateContentResponse;
use gproxy_protocol::gemini::generate_content::types::UsageMetadata;
use gproxy_protocol::openai::count_tokens::types::{InputTokenCount, InputTokenObjectType};
use gproxy_protocol::openai::create_chat_completions::request::{
    CreateChatCompletionRequest, CreateChatCompletionRequestBody,
};
use gproxy_protocol::openai::create_chat_completions::response::{
    ChatCompletionChoice, ChatCompletionObjectType, CreateChatCompletionResponse,
};
use gproxy_protocol::openai::create_chat_completions::types::{
    ChatCompletionFinishReason, ChatCompletionRequestMessage, ChatCompletionRequestUserMessage,
    ChatCompletionResponseMessage, ChatCompletionResponseRole, ChatCompletionUserContent,
    CompletionUsage, PromptTokensDetails,
};
use gproxy_protocol::openai::create_response::response::{
    Response as OpenAIResponse, ResponseObjectType,
};
use gproxy_protocol::openai::create_response::types::{
    ResponseUsage, ResponseUsageInputTokensDetails, ResponseUsageOutputTokensDetails,
};

#[test]
fn stream_format_basic() {
    assert_eq!(
        stream_format(Proto::Claude),
        Some(StreamFormat::SseNamedEvent)
    );
    assert_eq!(
        stream_format(Proto::OpenAIChat),
        Some(StreamFormat::SseDataOnly)
    );
    assert_eq!(
        stream_format(Proto::OpenAIResponse),
        Some(StreamFormat::SseNamedEvent)
    );
    assert_eq!(stream_format(Proto::Gemini), Some(StreamFormat::JsonStream));
    assert_eq!(stream_format(Proto::OpenAI), None);
}

#[test]
fn model_list_transform() {
    let ctx = TransformContext {
        src: Proto::Claude,
        dst: Proto::OpenAI,
        src_op: Op::ModelList,
        dst_op: Op::ModelList,
    };
    let req = Request::ModelList(ModelListRequest::Claude(ClaudeListModelsRequest::default()));
    let out = transform_request(&ctx, req).unwrap();
    match out {
        Request::ModelList(ModelListRequest::OpenAI(_)) => {}
        _ => panic!("unexpected output"),
    }
}

fn make_openai_chat_request(stream: Option<bool>) -> CreateChatCompletionRequest {
    let message = ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
        content: ChatCompletionUserContent::Text("hi".to_string()),
        name: None,
    });

    CreateChatCompletionRequest {
        body: CreateChatCompletionRequestBody {
            messages: vec![message],
            model: "gpt-test".to_string(),
            modalities: None,
            verbosity: None,
            reasoning_effort: None,
            max_completion_tokens: None,
            frequency_penalty: None,
            presence_penalty: None,
            web_search_options: None,
            top_logprobs: None,
            response_format: None,
            audio: None,
            store: None,
            stream,
            stop: None,
            logit_bias: None,
            logprobs: None,
            max_tokens: None,
            n: None,
            prediction: None,
            seed: None,
            stream_options: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            function_call: None,
            functions: None,
            metadata: None,
            extra_body: None,
            temperature: None,
            top_p: None,
            user: None,
            safety_identifier: None,
            prompt_cache_key: None,
            service_tier: None,
            prompt_cache_retention: None,
        },
    }
}

fn make_openai_chat_response_with_usage(usage: CompletionUsage) -> CreateChatCompletionResponse {
    let message = ChatCompletionResponseMessage {
        role: ChatCompletionResponseRole::Assistant,
        content: Some("ok".to_string()),
        refusal: None,
        tool_calls: None,
        annotations: None,
        function_call: None,
        audio: None,
    };
    let choice = ChatCompletionChoice {
        index: 0,
        message,
        finish_reason: ChatCompletionFinishReason::Stop,
        logprobs: None,
    };

    CreateChatCompletionResponse {
        id: "chatcmpl-test".to_string(),
        object: ChatCompletionObjectType::ChatCompletion,
        created: 0,
        model: "gpt-test".to_string(),
        choices: vec![choice],
        usage: Some(usage),
        service_tier: None,
        system_fingerprint: None,
    }
}

fn make_openai_response_with_usage(usage: ResponseUsage) -> OpenAIResponse {
    OpenAIResponse {
        id: "resp-test".to_string(),
        object: ResponseObjectType::Response,
        created_at: 0,
        status: None,
        completed_at: None,
        error: None,
        incomplete_details: None,
        instructions: None,
        model: "gpt-test".to_string(),
        output: Vec::new(),
        output_text: None,
        usage: Some(usage),
        parallel_tool_calls: None,
        conversation: None,
        previous_response_id: None,
        reasoning: None,
        background: None,
        max_output_tokens: None,
        max_tool_calls: None,
        text: None,
        tools: None,
        tool_choice: None,
        prompt: None,
        truncation: None,
        metadata: None,
        temperature: None,
        top_p: None,
        top_logprobs: None,
        user: None,
        safety_identifier: None,
        prompt_cache_key: None,
        service_tier: None,
        prompt_cache_retention: None,
        store: None,
    }
}

fn make_claude_response_with_usage(
    usage: BetaUsage,
) -> gproxy_protocol::claude::create_message::response::CreateMessageResponse {
    BetaMessage {
        id: "claude-test".to_string(),
        container: None,
        content: Vec::new(),
        context_management: None,
        model: ClaudeModel::Custom("claude-test".to_string()),
        role: BetaMessageRole::Assistant,
        stop_reason: None,
        stop_sequence: None,
        r#type: BetaMessageType::Message,
        usage,
    }
}

fn make_gemini_response_with_usage(usage: UsageMetadata) -> GeminiGenerateContentResponse {
    GeminiGenerateContentResponse {
        candidates: Vec::new(),
        prompt_feedback: None,
        usage_metadata: Some(usage),
        model_version: None,
        response_id: None,
        model_status: None,
    }
}

#[test]
fn openai_chat_stream_include_usage_default() {
    let ctx = TransformContext {
        src: Proto::OpenAIChat,
        dst: Proto::OpenAIChat,
        src_op: Op::GenerateContent,
        dst_op: Op::StreamGenerateContent,
    };
    let req = make_openai_chat_request(None);
    let out = transform_request(
        &ctx,
        Request::GenerateContent(GenerateContentRequest::OpenAIChat(req)),
    )
    .unwrap();
    let out_req = match out {
        Request::GenerateContent(GenerateContentRequest::OpenAIChat(req)) => req,
        _ => panic!("unexpected output"),
    };
    assert_eq!(out_req.body.stream, Some(true));
    assert_eq!(
        out_req
            .body
            .stream_options
            .as_ref()
            .and_then(|opts| opts.include_usage),
        Some(true)
    );
}

#[test]
fn usage_cache_mapping_claude() {
    let usage = BetaUsage {
        cache_creation: BetaCacheCreation {
            ephemeral_1h_input_tokens: 0,
            ephemeral_5m_input_tokens: 0,
        },
        cache_creation_input_tokens: 4,
        cache_read_input_tokens: 3,
        inference_geo: None,
        input_tokens: 1,
        iterations: None,
        output_tokens: 2,
        server_tool_use: None,
        service_tier: BetaServiceTierUsed::Standard,
        speed: None,
    };
    let resp = make_claude_response_with_usage(usage);
    let summary =
        usage_from_response(Proto::Claude, &GenerateContentResponse::Claude(resp)).unwrap();
    assert_eq!(summary.input_tokens, Some(1));
    assert_eq!(summary.output_tokens, Some(2));
    assert_eq!(summary.cache_read_input_tokens, Some(3));
    assert_eq!(summary.cache_creation_input_tokens, Some(4));
}

#[test]
fn usage_cache_mapping_openai_chat() {
    let usage = CompletionUsage {
        prompt_tokens: 10,
        completion_tokens: 5,
        total_tokens: 15,
        completion_tokens_details: None,
        prompt_tokens_details: Some(PromptTokensDetails {
            audio_tokens: None,
            cached_tokens: Some(7),
        }),
    };
    let resp = make_openai_chat_response_with_usage(usage);
    let summary = usage_from_response(
        Proto::OpenAIChat,
        &GenerateContentResponse::OpenAIChat(resp),
    )
    .unwrap();
    assert_eq!(summary.input_tokens, Some(10));
    assert_eq!(summary.output_tokens, Some(5));
    assert_eq!(summary.cache_read_input_tokens, Some(7));
    assert_eq!(summary.cache_creation_input_tokens, None);
}

#[test]
fn usage_cache_mapping_openai_response() {
    let usage = ResponseUsage {
        input_tokens: 11,
        input_tokens_details: ResponseUsageInputTokensDetails { cached_tokens: 9 },
        output_tokens: 22,
        output_tokens_details: ResponseUsageOutputTokensDetails {
            reasoning_tokens: 0,
        },
        total_tokens: 33,
    };
    let resp = make_openai_response_with_usage(usage);
    let summary = usage_from_response(
        Proto::OpenAIResponse,
        &GenerateContentResponse::OpenAIResponse(resp),
    )
    .unwrap();
    assert_eq!(summary.input_tokens, Some(11));
    assert_eq!(summary.output_tokens, Some(22));
    assert_eq!(summary.cache_read_input_tokens, Some(9));
    assert_eq!(summary.cache_creation_input_tokens, None);
}

#[test]
fn usage_cache_mapping_gemini() {
    let usage = UsageMetadata {
        prompt_token_count: Some(1),
        cached_content_token_count: Some(2),
        candidates_token_count: Some(3),
        tool_use_prompt_token_count: None,
        thoughts_token_count: None,
        total_token_count: None,
        prompt_tokens_details: None,
        cache_tokens_details: None,
        candidates_tokens_details: None,
        tool_use_prompt_tokens_details: None,
    };
    let resp = make_gemini_response_with_usage(usage);
    let summary =
        usage_from_response(Proto::Gemini, &GenerateContentResponse::Gemini(resp)).unwrap();
    assert_eq!(summary.input_tokens, Some(1));
    assert_eq!(summary.output_tokens, Some(3));
    assert_eq!(summary.cache_read_input_tokens, Some(2));
    assert_eq!(summary.cache_creation_input_tokens, None);
}

#[test]
fn fallback_usage_does_not_set_cache_fields() {
    struct FixedCounter {
        value: u32,
    }

    impl CountTokensFn for FixedCounter {
        type Error = ();

        fn count_tokens(
            &self,
            proto: Proto,
            _req: CountTokensRequest,
        ) -> Result<CountTokensResponse, Self::Error> {
            match proto {
                Proto::Claude => Ok(CountTokensResponse::Claude(ClaudeCountTokensResponse {
                    context_management: None,
                    input_tokens: self.value,
                })),
                Proto::OpenAI | Proto::OpenAIChat | Proto::OpenAIResponse => {
                    Ok(CountTokensResponse::OpenAI(InputTokenCount {
                        object: InputTokenObjectType::ResponseInputTokens,
                        input_tokens: self.value as i64,
                    }))
                }
                Proto::Gemini => Ok(CountTokensResponse::Gemini(GeminiCountTokensResponse {
                    total_tokens: self.value,
                    cached_content_token_count: None,
                    prompt_tokens_details: None,
                    cache_tokens_details: None,
                })),
            }
        }
    }

    let req = GenerateContentRequest::OpenAIChat(make_openai_chat_request(Some(false)));
    let summary = fallback_usage_with_count_tokens(
        Proto::OpenAIChat,
        &req,
        "hello",
        &FixedCounter { value: 42 },
    )
    .unwrap();
    assert_eq!(summary.input_tokens, Some(42));
    assert_eq!(summary.output_tokens, Some(42));
    assert_eq!(summary.cache_read_input_tokens, None);
    assert_eq!(summary.cache_creation_input_tokens, None);
}
