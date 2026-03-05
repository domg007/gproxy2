use crate::openai::create_response::request::{OpenAiCreateResponseRequest, RequestBody};
use crate::openai::create_response::types::Metadata;
use crate::openai::create_response::websocket::request::OpenAiCreateResponseWebSocketConnectRequest;
use crate::openai::create_response::websocket::types::OpenAiCreateResponseWebSocketClientMessage;
use crate::transform::openai::websocket::context::OpenAiWebsocketTransformContext;
use crate::transform::openai::websocket::from_http::request::OPENAI_CLIENT_METADATA_TUNNEL_PREFIX;
use crate::transform::utils::TransformError;

fn inject_client_metadata_into_request_body(
    body: &mut RequestBody,
    client_metadata: Option<&Metadata>,
    ctx: &mut OpenAiWebsocketTransformContext,
) {
    let Some(client_metadata) = client_metadata else {
        return;
    };
    if client_metadata.is_empty() {
        return;
    }

    let metadata = body.metadata.get_or_insert_with(Metadata::new);
    for (key, value) in client_metadata {
        let tunnel_key = format!("{OPENAI_CLIENT_METADATA_TUNNEL_PREFIX}{key}");
        if metadata.contains_key(&tunnel_key) {
            ctx.push_warning(format!(
                "openai websocket to_http request: metadata tunnel key conflict overwritten `{tunnel_key}`"
            ));
        }
        metadata.insert(tunnel_key, value.clone());
    }
    ctx.push_warning(
        "openai websocket to_http request: client_metadata tunneled through metadata".to_string(),
    );
}

pub fn websocket_client_message_to_openai_request_with_context(
    value: &OpenAiCreateResponseWebSocketClientMessage,
) -> Result<(OpenAiCreateResponseRequest, OpenAiWebsocketTransformContext), TransformError> {
    let mut ctx = OpenAiWebsocketTransformContext::default();

    let request = match value {
        OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(payload) => {
            let mut body = payload.request.clone();
            inject_client_metadata_into_request_body(
                &mut body,
                payload.client_metadata.as_ref(),
                &mut ctx,
            );
            if payload.generate == Some(false) {
                ctx.push_warning(
                    "openai websocket to_http request: dropped generate=false flag".to_string(),
                );
            }
            OpenAiCreateResponseRequest {
                body,
                ..OpenAiCreateResponseRequest::default()
            }
        }
        OpenAiCreateResponseWebSocketClientMessage::ResponseAppend(payload) => {
            let mut body = RequestBody {
                input: Some(payload.input.clone()),
                stream: Some(true),
                ..RequestBody::default()
            };
            inject_client_metadata_into_request_body(
                &mut body,
                payload.client_metadata.as_ref(),
                &mut ctx,
            );
            OpenAiCreateResponseRequest {
                body,
                ..OpenAiCreateResponseRequest::default()
            }
        }
    };

    Ok((request, ctx))
}

pub fn websocket_connect_to_openai_request_with_context(
    value: &OpenAiCreateResponseWebSocketConnectRequest,
) -> Result<(OpenAiCreateResponseRequest, OpenAiWebsocketTransformContext), TransformError> {
    let Some(message) = value.body.as_ref() else {
        let mut ctx = OpenAiWebsocketTransformContext::default();
        ctx.push_warning(
            "openai websocket to_http request: missing initial body, downgraded to empty request"
                .to_string(),
        );
        return Ok((OpenAiCreateResponseRequest::default(), ctx));
    };
    websocket_client_message_to_openai_request_with_context(message)
}

impl TryFrom<&OpenAiCreateResponseWebSocketClientMessage> for OpenAiCreateResponseRequest {
    type Error = TransformError;

    fn try_from(
        value: &OpenAiCreateResponseWebSocketClientMessage,
    ) -> Result<Self, TransformError> {
        Ok(websocket_client_message_to_openai_request_with_context(value)?.0)
    }
}

impl TryFrom<&OpenAiCreateResponseWebSocketConnectRequest> for OpenAiCreateResponseRequest {
    type Error = TransformError;

    fn try_from(
        value: &OpenAiCreateResponseWebSocketConnectRequest,
    ) -> Result<Self, TransformError> {
        Ok(websocket_connect_to_openai_request_with_context(value)?.0)
    }
}

impl TryFrom<OpenAiCreateResponseWebSocketConnectRequest> for OpenAiCreateResponseRequest {
    type Error = TransformError;

    fn try_from(
        value: OpenAiCreateResponseWebSocketConnectRequest,
    ) -> Result<Self, TransformError> {
        OpenAiCreateResponseRequest::try_from(&value)
    }
}

#[cfg(test)]
mod tests {
    use crate::openai::count_tokens::types::ResponseInput;
    use crate::openai::create_response::request::RequestBody;
    use crate::openai::create_response::websocket::types::{
        OpenAiCreateResponseAppendWebSocketRequestBody,
        OpenAiCreateResponseCreateWebSocketRequestBody, OpenAiCreateResponseWebSocketClientMessage,
    };

    use super::*;

    #[test]
    fn websocket_append_maps_to_http_stream_request() {
        let message = OpenAiCreateResponseWebSocketClientMessage::ResponseAppend(
            OpenAiCreateResponseAppendWebSocketRequestBody {
                input: ResponseInput::Text("hello".to_string()),
                client_metadata: None,
            },
        );

        let request =
            OpenAiCreateResponseRequest::try_from(&message).expect("conversion should succeed");
        assert_eq!(request.body.stream, Some(true));
        assert!(matches!(request.body.input, Some(ResponseInput::Text(_))));
    }

    #[test]
    fn websocket_generate_false_is_downgraded_to_warning() {
        let message = OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(
            OpenAiCreateResponseCreateWebSocketRequestBody {
                request: RequestBody {
                    model: Some("gpt-5.3-codex".to_string()),
                    stream: Some(true),
                    ..RequestBody::default()
                },
                generate: Some(false),
                client_metadata: None,
            },
        );

        let (request, ctx) = websocket_client_message_to_openai_request_with_context(&message)
            .expect("conversion should succeed");
        assert_eq!(request.body.model.as_deref(), Some("gpt-5.3-codex"));
        assert_eq!(ctx.warnings.len(), 1);
    }

    #[test]
    fn websocket_client_metadata_is_tunneled_to_metadata() {
        let message = OpenAiCreateResponseWebSocketClientMessage::ResponseAppend(
            OpenAiCreateResponseAppendWebSocketRequestBody {
                input: ResponseInput::Text("hello".to_string()),
                client_metadata: Some(
                    [("x-codex-turn-metadata".to_string(), "turn-1".to_string())]
                        .into_iter()
                        .collect(),
                ),
            },
        );

        let (request, ctx) = websocket_client_message_to_openai_request_with_context(&message)
            .expect("conversion should succeed");
        assert_eq!(
            request.body.metadata.as_ref().and_then(|m| m.get(&format!(
                "{OPENAI_CLIENT_METADATA_TUNNEL_PREFIX}x-codex-turn-metadata"
            ))),
            Some(&"turn-1".to_string())
        );
        assert_eq!(ctx.warnings.len(), 1);
    }

    #[test]
    fn websocket_connect_without_body_downgrades_to_empty_request() {
        let (request, ctx) = websocket_connect_to_openai_request_with_context(
            &OpenAiCreateResponseWebSocketConnectRequest::default(),
        )
        .expect("conversion should succeed");

        assert_eq!(request, OpenAiCreateResponseRequest::default());
        assert_eq!(ctx.warnings.len(), 1);
    }
}
