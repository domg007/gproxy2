use crate::AppState;
use gproxy_provider::{ChannelId, CredentialRef, CredentialStateManager, ProviderDefinition};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, http as tungstenite_http};

pub(super) type UpstreamWebSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

enum WsFailureKind {
    AuthDead,
    RateLimited,
    Transient,
    NonRetryable,
}

struct WsConnectError {
    status: Option<u16>,
    message: String,
}

pub(super) async fn connect_upstream_websocket_with_credential_retry<Prepare>(
    state: &AppState,
    channel: &ChannelId,
    provider: &ProviderDefinition,
    model_hint: Option<&str>,
    now_unix_ms: u64,
    mut prepare_request: Prepare,
) -> Result<UpstreamWebSocket, String>
where
    Prepare: FnMut(&CredentialRef) -> Result<(String, Vec<(String, String)>), String>,
{
    let model_hint = model_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let eligible = state.credential_states().eligible_credentials(
        channel,
        provider.credentials.list_credentials(),
        model_hint.as_deref(),
        now_unix_ms,
    );
    if eligible.is_empty() {
        return Err(format!(
            "provider {} has no eligible credential for websocket",
            channel.as_str()
        ));
    }

    let mut last_error: Option<String> = None;
    for credential in eligible {
        let prepared = match prepare_request(credential) {
            Ok(prepared) => prepared,
            Err(err) => {
                last_error = Some(format!(
                    "credential {} websocket request prepare failed: {err}",
                    credential.id
                ));
                continue;
            }
        };
        match connect_websocket_once(prepared.0.as_str(), prepared.1).await {
            Ok(socket) => {
                state_manager.mark_success(state.credential_states(), channel, credential.id);
                return Ok(socket);
            }
            Err(err) => {
                let message = format!(
                    "credential {} websocket connect failed: {}",
                    credential.id, err.message
                );
                last_error = Some(message.clone());
                match classify_ws_failure(err.status) {
                    WsFailureKind::AuthDead => {
                        state_manager.mark_auth_dead(
                            state.credential_states(),
                            channel,
                            credential.id,
                            Some(message),
                        );
                        continue;
                    }
                    WsFailureKind::RateLimited => {
                        state_manager.mark_rate_limited(
                            state.credential_states(),
                            channel,
                            credential.id,
                            model_hint.as_deref(),
                            None,
                            Some(message),
                        );
                        continue;
                    }
                    WsFailureKind::Transient => {
                        state_manager.mark_transient_failure(
                            state.credential_states(),
                            channel,
                            credential.id,
                            model_hint.as_deref(),
                            None,
                            Some(message),
                        );
                        continue;
                    }
                    WsFailureKind::NonRetryable => {
                        return Err(message);
                    }
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        format!(
            "provider {} websocket connect failed for all eligible credentials",
            channel.as_str()
        )
    }))
}

async fn connect_websocket_once(
    url: &str,
    headers: Vec<(String, String)>,
) -> Result<UpstreamWebSocket, WsConnectError> {
    let mut request = url.into_client_request().map_err(|err| WsConnectError {
        status: None,
        message: format!("invalid upstream websocket request URL: {err}"),
    })?;
    {
        let request_headers = request.headers_mut();
        for (name, value) in headers {
            let header_name =
                tungstenite_http::HeaderName::from_bytes(name.as_bytes()).map_err(|err| {
                    WsConnectError {
                        status: None,
                        message: format!("invalid upstream websocket header name `{name}`: {err}"),
                    }
                })?;
            let header_value =
                tungstenite_http::HeaderValue::from_str(value.as_str()).map_err(|err| {
                    WsConnectError {
                        status: None,
                        message: format!(
                            "invalid upstream websocket header value for `{name}`: {err}"
                        ),
                    }
                })?;
            request_headers.insert(header_name, header_value);
        }
    }

    connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .map_err(map_connect_ws_error)
}

fn map_connect_ws_error(err: WsError) -> WsConnectError {
    if let WsError::Http(response) = &err {
        let status = response.status().as_u16();
        return WsConnectError {
            status: Some(status),
            message: format!("upstream websocket connect failed with status {status}"),
        };
    }
    WsConnectError {
        status: None,
        message: format!("upstream websocket connect failed: {err}"),
    }
}

fn classify_ws_failure(status: Option<u16>) -> WsFailureKind {
    match status {
        Some(401 | 403) => WsFailureKind::AuthDead,
        Some(429) => WsFailureKind::RateLimited,
        Some(426) => WsFailureKind::NonRetryable,
        Some(408 | 409 | 425 | 500 | 502 | 503 | 504) => WsFailureKind::Transient,
        Some(code) if (400..500).contains(&code) => WsFailureKind::NonRetryable,
        Some(_) => WsFailureKind::Transient,
        None => WsFailureKind::Transient,
    }
}
