use std::collections::BTreeMap;

use crate::openai::create_response::request::{OpenAiCreateResponseRequest, RequestBody};
use crate::openai::create_response::types::Metadata;
use crate::openai::create_response::websocket::request::OpenAiCreateResponseWebSocketConnectRequest;
use crate::openai::create_response::websocket::types::{
    OpenAiCreateResponseCreateWebSocketRequestBody, OpenAiCreateResponseWebSocketClientMessage,
};
use crate::transform::openai::websocket::context::OpenAiWebsocketTransformContext;
use crate::transform::utils::TransformError;

pub const OPENAI_CLIENT_METADATA_TUNNEL_PREFIX: &str = "gproxy.ws.client_metadata.";

fn extract_client_metadata_from_request_body(
    body: &mut RequestBody,
    ctx: &mut OpenAiWebsocketTransformContext,
) -> Option<Metadata> {
    let metadata = body.metadata.as_mut()?;

    let mut tunnel = BTreeMap::new();
    let keys = metadata.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        if let Some(tunnel_key) = key.strip_prefix(OPENAI_CLIENT_METADATA_TUNNEL_PREFIX) {
            let Some(value) = metadata.remove(&key) else {
                continue;
            };
            tunnel.insert(tunnel_key.to_string(), value);
        }
    }

    if metadata.is_empty() {
        body.metadata = None;
    }

    if !tunnel.is_empty() {
        ctx.push_warning(
            "openai websocket from_http request: restored client_metadata from metadata tunnel",
        );
        Some(tunnel)
    } else {
        None
    }
}

pub fn openai_create_response_request_to_websocket_message_with_context(
    value: &OpenAiCreateResponseRequest,
) -> Result<
    (
        OpenAiCreateResponseWebSocketClientMessage,
        OpenAiWebsocketTransformContext,
    ),
    TransformError,
> {
    let mut ctx = OpenAiWebsocketTransformContext::default();
    let mut request_body = value.body.clone();
    let client_metadata = extract_client_metadata_from_request_body(&mut request_body, &mut ctx);

    Ok((
        OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(
            OpenAiCreateResponseCreateWebSocketRequestBody {
                request: request_body,
                generate: None,
                client_metadata,
            },
        ),
        ctx,
    ))
}

pub fn openai_create_response_request_to_websocket_connect_with_context(
    value: &OpenAiCreateResponseRequest,
) -> Result<
    (
        OpenAiCreateResponseWebSocketConnectRequest,
        OpenAiWebsocketTransformContext,
    ),
    TransformError,
> {
    let (message, ctx) = openai_create_response_request_to_websocket_message_with_context(value)?;
    Ok((
        OpenAiCreateResponseWebSocketConnectRequest {
            body: Some(message),
            ..OpenAiCreateResponseWebSocketConnectRequest::default()
        },
        ctx,
    ))
}

impl TryFrom<&OpenAiCreateResponseRequest> for OpenAiCreateResponseWebSocketClientMessage {
    type Error = TransformError;

    fn try_from(value: &OpenAiCreateResponseRequest) -> Result<Self, TransformError> {
        Ok(openai_create_response_request_to_websocket_message_with_context(value)?.0)
    }
}

impl TryFrom<&OpenAiCreateResponseRequest> for OpenAiCreateResponseWebSocketConnectRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiCreateResponseRequest) -> Result<Self, TransformError> {
        Ok(openai_create_response_request_to_websocket_connect_with_context(value)?.0)
    }
}

impl TryFrom<OpenAiCreateResponseRequest> for OpenAiCreateResponseWebSocketConnectRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseRequest) -> Result<Self, TransformError> {
        OpenAiCreateResponseWebSocketConnectRequest::try_from(&value)
    }
}

#[cfg(test)]
mod tests {
    use crate::openai::create_response::request::{OpenAiCreateResponseRequest, RequestBody};
    use crate::openai::create_response::types::Metadata;

    use super::*;

    #[test]
    fn create_response_request_maps_to_websocket_connect_request() {
        let request = OpenAiCreateResponseRequest {
            body: RequestBody {
                model: Some("gpt-5.3-codex".to_string()),
                stream: Some(true),
                ..RequestBody::default()
            },
            ..OpenAiCreateResponseRequest::default()
        };

        let ws = OpenAiCreateResponseWebSocketConnectRequest::try_from(request)
            .expect("conversion should succeed");
        let Some(OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(payload)) = ws.body
        else {
            panic!("expected response.create websocket frame");
        };

        assert_eq!(payload.request.model.as_deref(), Some("gpt-5.3-codex"));
        assert_eq!(payload.request.stream, Some(true));
    }

    #[test]
    fn metadata_tunnel_restores_client_metadata() {
        let mut metadata = Metadata::new();
        metadata.insert(
            format!("{OPENAI_CLIENT_METADATA_TUNNEL_PREFIX}x-codex-turn-metadata"),
            "turn-1".to_string(),
        );
        metadata.insert("normal-key".to_string(), "normal-value".to_string());

        let request = OpenAiCreateResponseRequest {
            body: RequestBody {
                model: Some("gpt-5.3-codex".to_string()),
                metadata: Some(metadata),
                ..RequestBody::default()
            },
            ..OpenAiCreateResponseRequest::default()
        };

        let (message, ctx) =
            openai_create_response_request_to_websocket_message_with_context(&request)
                .expect("conversion should succeed");
        let OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(payload) = message else {
            panic!("expected response.create");
        };

        assert_eq!(
            payload
                .client_metadata
                .as_ref()
                .and_then(|m| m.get("x-codex-turn-metadata")),
            Some(&"turn-1".to_string())
        );
        assert_eq!(
            payload
                .request
                .metadata
                .as_ref()
                .and_then(|m| m.get("normal-key")),
            Some(&"normal-value".to_string())
        );
        assert_eq!(ctx.warnings.len(), 1);
    }
}
