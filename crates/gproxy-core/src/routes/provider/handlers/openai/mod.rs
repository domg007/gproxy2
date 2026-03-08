use std::sync::Arc;

use axum::body::{Bytes, to_bytes};
use axum::extract::ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{OriginalUri, Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use gproxy_middleware::{
    OperationFamily, ProtocolKind, TransformRequest, TransformRequestPayload, TransformResponse,
    TransformRoute,
};
use gproxy_protocol::claude::model_get::request as claude_model_get_request;
use gproxy_protocol::claude::model_list::request as claude_model_list_request;
use gproxy_protocol::gemini::live::request::GeminiLiveConnectRequest;
use gproxy_protocol::gemini::live::response::GeminiLiveMessageResponse;
use gproxy_protocol::gemini::model_get::request as gemini_model_get_request;
use gproxy_protocol::gemini::model_list::request as gemini_model_list_request;
use gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse;
use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
use gproxy_protocol::openai::create_response::websocket::request::{
    OpenAiCreateResponseWebSocketConnectRequest,
    QueryParameters as OpenAiCreateResponseWebSocketQueryParameters,
    RequestHeaders as OpenAiCreateResponseWebSocketRequestHeaders,
};
use gproxy_protocol::openai::create_response::websocket::response::OpenAiCreateResponseWebSocketMessageResponse;
use gproxy_protocol::openai::create_response::websocket::types::{
    OpenAiCreateResponseWebSocketClientMessage, OpenAiCreateResponseWebSocketDoneMarker,
    OpenAiCreateResponseWebSocketServerMessage, OpenAiCreateResponseWebSocketWrappedError,
    OpenAiCreateResponseWebSocketWrappedErrorEvent,
    OpenAiCreateResponseWebSocketWrappedErrorEventType,
};
use gproxy_protocol::openai::create_video::request as openai_create_video_request;
use gproxy_protocol::openai::model_get::request as openai_model_get_request;
use gproxy_protocol::openai::model_list::request as openai_model_list_request;
use gproxy_protocol::openai::video_content_get::request as openai_video_content_get_request;
use gproxy_protocol::openai::video_content_get::types as openai_video_content_get_types;
use gproxy_protocol::openai::video_get::request as openai_video_get_request;
use gproxy_provider::{
    BuiltinChannel, BuiltinChannelCredential, ChannelCredential, ChannelId, CredentialRef,
    ProviderDefinition, RouteImplementation, RouteKey, UpstreamOAuthRequest, parse_query_value,
};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
use url::form_urlencoded;

use super::websocket_retry::{UpstreamWebSocket, connect_upstream_websocket_with_credential_retry};
use crate::AppState;

use super::super::{
    HttpError, ModelProtocolPreference, RequestAuthContext, UpstreamResponseMeta,
    anthropic_headers_from_request, apply_credential_update_and_persist, authorize_provider_access,
    bad_request, capture_tracked_http_events, collect_headers, collect_passthrough_headers,
    collect_unscoped_model_ids, collect_websocket_passthrough_headers,
    enqueue_credential_status_updates_for_request, enqueue_internal_tracked_http_events,
    enqueue_upstream_request_event_from_meta, execute_transform_candidates,
    execute_transform_request, execute_transform_request_payload, internal_error,
    model_protocol_preference, normalize_gemini_model_path, now_unix_ms,
    oauth_callback_response_to_axum, oauth_response_to_axum, parse_json_body,
    parse_optional_query_value, persist_provider_and_credential, resolve_credential_id,
    resolve_provider, resolve_provider_id, response_from_status_headers_and_bytes,
    restrict_provider_to_credential, split_provider_prefixed_plain_model,
    upstream_error_credential_id, upstream_error_request_meta, upstream_error_status,
};

mod http_fallback;
mod request;
mod websocket;

use request::*;
use websocket::*;

mod content;
mod models;
mod oauth;
#[cfg(test)]
mod tests;
mod websocket_api;

pub(in crate::routes::provider) use content::*;
pub(in crate::routes::provider) use models::*;
pub(in crate::routes::provider) use oauth::*;
pub(in crate::routes::provider) use websocket_api::*;
