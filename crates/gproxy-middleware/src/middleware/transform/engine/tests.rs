use super::*;

use gproxy_protocol::gemini::count_tokens::types::{GeminiContent, GeminiContentRole, GeminiPart};
use gproxy_protocol::gemini::generate_content::response::ResponseBody as GeminiGenerateContentResponseBody;
use gproxy_protocol::gemini::live::types::{
    GeminiBidiGenerateContentClientMessage, GeminiBidiGenerateContentClientMessageType,
    GeminiBidiGenerateContentServerContent, GeminiBidiGenerateContentServerMessage,
    GeminiBidiGenerateContentServerMessageType, GeminiBidiGenerateContentSetup,
};
use gproxy_protocol::openai::create_response::request::RequestBody as OpenAiCreateResponseRequestBody;
use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
use gproxy_protocol::openai::create_response::types as rt;
use gproxy_protocol::openai::create_response::websocket::types::{
    OpenAiCreateResponseCreateWebSocketRequestBody, OpenAiCreateResponseWebSocketClientMessage,
    OpenAiCreateResponseWebSocketServerMessage,
};
use gproxy_protocol::transform::openai::stream_generate_content::openai_response::utils::response_snapshot;

#[test]
fn encode_gemini_sse_event_filters_done_marker() {
    let encoded = stream::encode_gemini_sse_event(GeminiSseEvent {
        event: None,
        data: GeminiSseEventData::Done("[DONE]".to_string()),
    });
    assert!(encoded.is_none());
}

#[test]
fn encode_gemini_sse_event_keeps_json_chunk() {
    let encoded = stream::encode_gemini_sse_event(GeminiSseEvent {
        event: None,
        data: GeminiSseEventData::Chunk(GeminiGenerateContentResponseBody::default()),
    })
    .expect("chunk should be encoded")
    .expect("chunk should serialize");
    let text = std::str::from_utf8(encoded.as_ref()).expect("valid utf8");
    assert_eq!(text, "data: {}\n\n");
}

#[test]
fn stream_output_converter_chat_routes_directly() {
    let from_openai = stream::stream_output_converter_route_kind(
        ProtocolKind::OpenAi,
        ProtocolKind::OpenAiChatCompletion,
    )
    .expect("openai -> chat converter");
    assert_eq!(from_openai, "openai_response_to_chat");

    let from_claude = stream::stream_output_converter_route_kind(
        ProtocolKind::Claude,
        ProtocolKind::OpenAiChatCompletion,
    )
    .expect("claude -> chat converter");
    assert_eq!(from_claude, "claude_to_chat");

    let from_gemini = stream::stream_output_converter_route_kind(
        ProtocolKind::Gemini,
        ProtocolKind::OpenAiChatCompletion,
    )
    .expect("gemini -> chat converter");
    assert_eq!(from_gemini, "gemini_to_chat");
}

#[test]
fn transform_stream_response_non_stream_input_is_unsupported() {
    let response = OpenAiCreateResponseResponse::Success {
        stats_code: StatusCode::OK,
        headers: Default::default(),
        body: response_snapshot(
            "resp_1",
            "gpt-5",
            Some(rt::ResponseStatus::Completed),
            None,
            None,
            None,
            None,
        ),
    };

    let err = transform_stream_response(
        TransformResponse::GenerateContentOpenAiResponse(response),
        ProtocolKind::OpenAiChatCompletion,
    )
    .expect_err("non-stream payload should be rejected");

    assert!(matches!(
        err,
        MiddlewareTransformError::Unsupported(
            "stream response transform requires stream_generate_content destination payload"
        )
    ));
}

#[test]
fn transform_request_openai_ws_to_gemini_live_direct() {
    let input =
        TransformRequest::OpenAiResponseWebSocket(OpenAiCreateResponseWebSocketConnectRequest {
            body: Some(OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(
                OpenAiCreateResponseCreateWebSocketRequestBody {
                    request: OpenAiCreateResponseRequestBody {
                        model: Some("gpt-5.3-codex".to_string()),
                        stream: Some(true),
                        ..OpenAiCreateResponseRequestBody::default()
                    },
                    generate: None,
                    client_metadata: None,
                },
            )),
            ..OpenAiCreateResponseWebSocketConnectRequest::default()
        });
    let route = TransformRoute {
        src_operation: OperationFamily::OpenAiResponseWebSocket,
        src_protocol: ProtocolKind::OpenAi,
        dst_operation: OperationFamily::GeminiLive,
        dst_protocol: ProtocolKind::Gemini,
    };

    let transformed = transform_request(input, route).expect("conversion should succeed");
    let TransformRequest::GeminiLive(request) = transformed else {
        panic!("expected gemini live request");
    };

    let Some(GeminiBidiGenerateContentClientMessage {
        message_type: GeminiBidiGenerateContentClientMessageType::Setup { setup },
    }) = request.body
    else {
        panic!("expected setup frame");
    };
    assert!(setup.model.starts_with("models/"));
}

#[test]
fn transform_request_gemini_live_to_openai_ws_direct() {
    let input = TransformRequest::GeminiLive(GeminiLiveConnectRequest {
        body: Some(GeminiBidiGenerateContentClientMessage {
            message_type: GeminiBidiGenerateContentClientMessageType::Setup {
                setup: GeminiBidiGenerateContentSetup {
                    model: "models/gemini-2.5-flash".to_string(),
                    ..GeminiBidiGenerateContentSetup::default()
                },
            },
        }),
        ..GeminiLiveConnectRequest::default()
    });
    let route = TransformRoute {
        src_operation: OperationFamily::GeminiLive,
        src_protocol: ProtocolKind::Gemini,
        dst_operation: OperationFamily::OpenAiResponseWebSocket,
        dst_protocol: ProtocolKind::OpenAi,
    };

    let transformed = transform_request(input, route).expect("conversion should succeed");
    let TransformRequest::OpenAiResponseWebSocket(request) = transformed else {
        panic!("expected openai websocket request");
    };

    let Some(OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(create)) = request.body
    else {
        panic!("expected response.create frame");
    };
    assert_eq!(
        create.request.model.as_deref(),
        Some("gemini-2.5-flash"),
        "gemini model should map to openai model id"
    );
}

#[test]
fn transform_response_openai_ws_to_gemini_live_direct() {
    let input = TransformResponse::OpenAiResponseWebSocket(vec![
        OpenAiCreateResponseWebSocketServerMessage::StreamEvent(ResponseStreamEvent::Error {
            code: "invalid_prompt".to_string(),
            message: "bad prompt".to_string(),
            param: None,
            sequence_number: 1,
        }),
    ]);
    let route = TransformRoute {
        src_operation: OperationFamily::GeminiLive,
        src_protocol: ProtocolKind::Gemini,
        dst_operation: OperationFamily::OpenAiResponseWebSocket,
        dst_protocol: ProtocolKind::OpenAi,
    };

    let transformed = transform_response(input, route).expect("conversion should succeed");
    let TransformResponse::GeminiLive(messages) = transformed else {
        panic!("expected gemini live response");
    };
    assert!(!messages.is_empty());
}

#[test]
fn transform_response_gemini_live_to_openai_ws_direct() {
    let input = TransformResponse::GeminiLive(vec![GeminiLiveMessageResponse::Message(
        GeminiBidiGenerateContentServerMessage {
            usage_metadata: None,
            message_type: GeminiBidiGenerateContentServerMessageType::ServerContent {
                server_content: GeminiBidiGenerateContentServerContent {
                    model_turn: Some(GeminiContent {
                        parts: vec![GeminiPart {
                            text: Some("hello".to_string()),
                            ..GeminiPart::default()
                        }],
                        role: Some(GeminiContentRole::Model),
                    }),
                    generation_complete: Some(true),
                    turn_complete: Some(true),
                    ..GeminiBidiGenerateContentServerContent::default()
                },
            },
        },
    )]);
    let route = TransformRoute {
        src_operation: OperationFamily::OpenAiResponseWebSocket,
        src_protocol: ProtocolKind::OpenAi,
        dst_operation: OperationFamily::GeminiLive,
        dst_protocol: ProtocolKind::Gemini,
    };

    let transformed = transform_response(input, route).expect("conversion should succeed");
    let TransformResponse::OpenAiResponseWebSocket(messages) = transformed else {
        panic!("expected openai websocket response");
    };
    assert!(!messages.is_empty());
}
