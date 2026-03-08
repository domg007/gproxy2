use std::collections::{HashMap, HashSet};
use std::time::Instant;

use bytes::Bytes;
use futures_util::{SinkExt as _, StreamExt as _};
use http::StatusCode;
use serde_json::{Value, json};
use tokio::time::{Duration, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message, http as ws_http};

use crate::channels::retry::{
    CredentialRetryDecision, credential_pick_mode, retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{UpstreamRequestMeta, tracked_request_meta};
use crate::channels::{
    BuiltinChannelCredential, BuiltinChannelSettings, ChannelCredential, ChannelSettings,
};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

use super::response::{
    build_http_stream_response, build_json_http_response, build_openai_error_json_response,
};
use super::stream::{random_hex, unix_timestamp_secs};
use super::web::{
    build_grok_imagine_request_message, build_grok_websocket_headers, build_grok_ws_url,
};
use super::*;
use super::cf::{invalidate_grok_session, resolve_grok_session};

const IMAGINE_TIMEOUT_SECS: u64 = 120;
const IMAGINE_IDLE_WAIT_SECS: u64 = 5;
const IMAGINE_MEDIUM_MIN_BYTES: usize = 30_000;
const IMAGINE_FINAL_MIN_BYTES: usize = 100_000;

#[derive(Debug, Clone)]
struct GrokImageCandidate {
    image_id: String,
    ext: String,
    blob: String,
    blob_size: usize,
    stage: GrokImageStage,
    is_final: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrokImageStage {
    Preview,
    Medium,
    Final,
}

#[derive(Debug)]
struct WsConnectError {
    status: Option<u16>,
    message: String,
}

#[derive(Debug)]
struct ImageExecutionError {
    status: Option<StatusCode>,
    message: String,
    retryable: bool,
}

pub(super) async fn execute_grok_image_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: GrokPreparedImageRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }

    let settings = image_settings(provider);
    let url = build_grok_ws_url(base_url, super::super::constants::IMAGINE_WS_PATH);
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let model_hint = Some(prepared.request_model.clone());
    let pick_mode = credential_pick_mode(provider.credential_pick_mode, None);

    retry_with_eligible_credentials_with_affinity(
        crate::channels::retry::CredentialRetryContext {
            provider,
            credential_states,
            model: model_hint.as_deref(),
            now_unix_ms,
            pick_mode,
            cache_affinity_hint: None,
        },
        |credential| match &credential.credential {
            ChannelCredential::Builtin(BuiltinChannelCredential::Grok(value)) => {
                Some(value.clone())
            }
            _ => None,
        },
        |attempt| {
            let settings = settings.clone();
            let url = url.clone();
            let prepared = prepared.clone();
            let model_hint = model_hint.clone();

            async move {
                let request_body = match build_grok_imagine_request_message(
                    &prepared.prompt,
                    &prepared.aspect_ratio,
                ) {
                    Ok(body) => body,
                    Err(err) => {
                        return CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(err.to_string()),
                            last_request_meta: None,
                        };
                    }
                };
                let session = match resolve_grok_session(
                    client,
                    &settings,
                    base_url,
                    attempt.material.sso.as_str(),
                )
                .await
                {
                    Ok(value) => value,
                    Err(err) => {
                        return CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(err.to_string()),
                            last_request_meta: None,
                        };
                    }
                };
                let headers = match build_grok_websocket_headers(
                    session.user_agent.as_deref().or(settings.user_agent.as_deref()),
                    session.extra_cookie_header.as_deref(),
                    attempt.material.sso.as_str(),
                    prepared.extra_headers.as_slice(),
                    base_url,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        return CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(err.to_string()),
                            last_request_meta: None,
                        };
                    }
                };
                let request_meta = tracked_request_meta(
                    "GET",
                    url.as_str(),
                    headers.clone(),
                    Some(request_body.clone()),
                );

                let socket = match connect_grok_image_socket(url.as_str(), headers.clone()).await {
                    Ok(socket) => socket,
                    Err(err) => {
                        if err.status == Some(StatusCode::FORBIDDEN.as_u16()) {
                            invalidate_grok_session(
                                &settings,
                                base_url,
                                attempt.material.sso.as_str(),
                            );
                        }
                        return handle_image_retry_error(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            err.status,
                            err.message,
                            None,
                        );
                    }
                };

                if prepared.stream {
                    let response = match build_image_stream_http_response(
                        socket,
                        prepared.clone(),
                        request_body,
                    )
                    .await
                    {
                        Ok(response) => response,
                        Err(err) => {
                            return handle_image_retry_error(
                                provider,
                                credential_states,
                                &state_manager,
                                attempt.credential_id,
                                model_hint.as_deref(),
                                Some(StatusCode::BAD_GATEWAY.as_u16()),
                                err.to_string(),
                                Some(request_meta.clone()),
                            );
                        }
                    };
                    state_manager.mark_success(
                        credential_states,
                        &provider.channel,
                        attempt.credential_id,
                    );
                    return CredentialRetryDecision::Return(
                        UpstreamResponse::from_http(
                            attempt.credential_id,
                            attempt.attempts,
                            response,
                        )
                        .with_request_meta(request_meta),
                    );
                }

                match collect_image_response(socket, prepared.clone(), request_body).await {
                    Ok(response) => {
                        state_manager.mark_success(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                        );
                        CredentialRetryDecision::Return(
                            UpstreamResponse::from_http(
                                attempt.credential_id,
                                attempt.attempts,
                                response,
                            )
                            .with_request_meta(request_meta),
                        )
                    }
                    Err(err) if err.retryable => handle_image_retry_error(
                        provider,
                        credential_states,
                        &state_manager,
                        attempt.credential_id,
                        model_hint.as_deref(),
                        err.status.map(|status| status.as_u16()),
                        err.message,
                        Some(request_meta),
                    ),
                    Err(err) => {
                        let status = err.status.unwrap_or(StatusCode::BAD_GATEWAY);
                        let response = match build_openai_error_json_response(
                            status,
                            err.message,
                            if status == StatusCode::TOO_MANY_REQUESTS {
                                "rate_limit_error"
                            } else {
                                "invalid_request_error"
                            },
                            None,
                            None,
                        ) {
                            Ok(response) => response,
                            Err(build_err) => {
                                return handle_image_retry_error(
                                    provider,
                                    credential_states,
                                    &state_manager,
                                    attempt.credential_id,
                                    model_hint.as_deref(),
                                    Some(status.as_u16()),
                                    build_err.to_string(),
                                    Some(request_meta),
                                );
                            }
                        };
                        state_manager.mark_success(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                        );
                        CredentialRetryDecision::Return(
                            UpstreamResponse::from_http(
                                attempt.credential_id,
                                attempt.attempts,
                                response,
                            )
                            .with_request_meta(request_meta),
                        )
                    }
                }
            }
        },
    )
    .await
}

fn image_settings(provider: &ProviderDefinition) -> GrokSettings {
    match &provider.settings {
        ChannelSettings::Builtin(BuiltinChannelSettings::Grok(value)) => value.clone(),
        _ => GrokSettings::default(),
    }
}

async fn connect_grok_image_socket(
    url: &str,
    headers: Vec<(String, String)>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsConnectError,
> {
    let mut request = url.into_client_request().map_err(|err| WsConnectError {
        status: None,
        message: format!("invalid grok imagine websocket url: {err}"),
    })?;
    {
        let request_headers = request.headers_mut();
        for (name, value) in headers {
            let header_name =
                ws_http::HeaderName::from_bytes(name.as_bytes()).map_err(|err| WsConnectError {
                    status: None,
                    message: format!("invalid websocket header name `{name}`: {err}"),
                })?;
            let header_value =
                ws_http::HeaderValue::from_str(value.as_str()).map_err(|err| WsConnectError {
                    status: None,
                    message: format!("invalid websocket header value for `{name}`: {err}"),
                })?;
            request_headers.insert(header_name, header_value);
        }
    }

    connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .map_err(map_connect_ws_error)
}

async fn build_image_stream_http_response(
    mut socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    prepared: GrokPreparedImageRequest,
    request_body: Vec<u8>,
) -> Result<wreq::Response, UpstreamError> {
    socket
        .send(Message::Text(
            String::from_utf8_lossy(&request_body).to_string().into(),
        ))
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;

    let stream = async_stream::try_stream! {
        let started_at = Instant::now();
        let mut emitted_partial = HashSet::<String>::new();
        let mut emitted_final = HashSet::<String>::new();
        let mut fallback = HashMap::<String, GrokImageCandidate>::new();
        let mut completed = 0usize;

        while started_at.elapsed() < Duration::from_secs(IMAGINE_TIMEOUT_SECS) {
            let next = timeout(Duration::from_secs(IMAGINE_IDLE_WAIT_SECS), socket.next()).await;
            let message = match next {
                Ok(Some(Ok(message))) => message,
                Ok(Some(Err(err))) => {
                    yield sse_error_frame(err.to_string().as_str());
                    break;
                }
                Ok(None) => break,
                Err(_) => {
                    if completed > 0 {
                        break;
                    }
                    continue;
                }
            };

            let Some(text) = websocket_text(message) else {
                continue;
            };
            match parse_image_socket_message(text.as_str()) {
                Some(ParsedImageSocketMessage::Image(candidate)) => {
                    upsert_best_candidate(&mut fallback, candidate.clone());
                    if candidate.stage == GrokImageStage::Medium
                        && emitted_partial.insert(candidate.image_id.clone())
                    {
                        yield sse_named_json(
                            "image_generation.partial_image",
                            &json!({
                                "type": "image_generation.partial_image",
                                "b64_json": strip_base64(candidate.blob.as_str()),
                                "background": prepared.background,
                                "created_at": unix_timestamp_secs(),
                                "output_format": prepared.output_format,
                                "partial_image_index": 0,
                                "quality": prepared.quality,
                                "size": prepared.request_size,
                            }),
                        )
                        .map_err(|err| std::io::Error::other(err.to_string()))?;
                    }
                    if candidate.is_final && emitted_final.insert(candidate.image_id.clone()) {
                        completed += 1;
                        yield sse_named_json(
                            "image_generation.completed",
                            &json!({
                                "type": "image_generation.completed",
                                "b64_json": strip_base64(candidate.blob.as_str()),
                                "background": prepared.background,
                                "created_at": unix_timestamp_secs(),
                                "output_format": prepared.output_format,
                                "quality": prepared.quality,
                                "size": prepared.request_size,
                                "usage": {
                                    "total_tokens": 0,
                                    "input_tokens": 0,
                                    "output_tokens": 0,
                                    "input_tokens_details": {
                                        "text_tokens": 0,
                                        "image_tokens": 0,
                                    }
                                }
                            }),
                        )
                        .map_err(|err| std::io::Error::other(err.to_string()))?;
                    }
                    if completed >= prepared.n as usize {
                        break;
                    }
                }
                Some(ParsedImageSocketMessage::Error(message)) => {
                    yield sse_error_frame(message.as_str());
                    break;
                }
                None => {}
            }
        }

        if completed == 0 {
            for candidate in select_image_candidates(&fallback, prepared.n as usize) {
                yield sse_named_json(
                    "image_generation.completed",
                    &json!({
                        "type": "image_generation.completed",
                        "b64_json": strip_base64(candidate.blob.as_str()),
                        "background": prepared.background,
                        "created_at": unix_timestamp_secs(),
                        "output_format": prepared.output_format,
                        "quality": prepared.quality,
                        "size": prepared.request_size,
                        "usage": {
                            "total_tokens": 0,
                            "input_tokens": 0,
                            "output_tokens": 0,
                            "input_tokens_details": {
                                "text_tokens": 0,
                                "image_tokens": 0,
                            }
                        }
                    }),
                )
                .map_err(|err| std::io::Error::other(err.to_string()))?;
            }
        }

        yield Bytes::from("data: [DONE]\n\n");
    };

    build_http_stream_response(stream)
}

async fn collect_image_response(
    mut socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    prepared: GrokPreparedImageRequest,
    request_body: Vec<u8>,
) -> Result<wreq::Response, ImageExecutionError> {
    socket
        .send(Message::Text(
            String::from_utf8_lossy(&request_body).to_string().into(),
        ))
        .await
        .map_err(|err| ImageExecutionError {
            status: Some(StatusCode::BAD_GATEWAY),
            message: err.to_string(),
            retryable: true,
        })?;

    let started_at = Instant::now();
    let mut images = HashMap::<String, GrokImageCandidate>::new();
    let mut final_ids = HashSet::<String>::new();

    while started_at.elapsed() < Duration::from_secs(IMAGINE_TIMEOUT_SECS) {
        let next = timeout(Duration::from_secs(IMAGINE_IDLE_WAIT_SECS), socket.next()).await;
        let message = match next {
            Ok(Some(Ok(message))) => message,
            Ok(Some(Err(err))) => {
                return Err(ImageExecutionError {
                    status: Some(StatusCode::BAD_GATEWAY),
                    message: err.to_string(),
                    retryable: true,
                });
            }
            Ok(None) => break,
            Err(_) => {
                if !final_ids.is_empty() {
                    break;
                }
                continue;
            }
        };

        let Some(text) = websocket_text(message) else {
            continue;
        };
        match parse_image_socket_message(text.as_str()) {
            Some(ParsedImageSocketMessage::Image(candidate)) => {
                if candidate.is_final {
                    final_ids.insert(candidate.image_id.clone());
                }
                upsert_best_candidate(&mut images, candidate);
                if final_ids.len() >= prepared.n as usize {
                    break;
                }
            }
            Some(ParsedImageSocketMessage::Error(message)) => {
                let status = if message.to_ascii_lowercase().contains("rate") {
                    Some(StatusCode::TOO_MANY_REQUESTS)
                } else {
                    Some(StatusCode::BAD_GATEWAY)
                };
                return Err(ImageExecutionError {
                    status,
                    retryable: status == Some(StatusCode::TOO_MANY_REQUESTS),
                    message,
                });
            }
            None => {}
        }
    }

    let selected = select_image_candidates(&images, prepared.n as usize);
    if selected.is_empty() {
        return Err(ImageExecutionError {
            status: Some(StatusCode::BAD_GATEWAY),
            message: "grok image stream returned no images".to_string(),
            retryable: true,
        });
    }

    let data = selected
        .into_iter()
        .map(|candidate| match prepared.response_format {
            GrokImageResponseFormat::B64Json => json!({
                "b64_json": strip_base64(candidate.blob.as_str()),
            }),
            GrokImageResponseFormat::Url => json!({
                "url": to_data_uri(candidate.blob.as_str(), candidate.ext.as_str()),
            }),
        })
        .collect::<Vec<_>>();
    build_json_http_response(
        StatusCode::OK,
        &json!({
            "created": unix_timestamp_secs(),
            "data": data,
        }),
    )
    .map_err(|err| ImageExecutionError {
        status: Some(StatusCode::BAD_GATEWAY),
        message: err.to_string(),
        retryable: true,
    })
}

fn select_image_candidates(
    images: &HashMap<String, GrokImageCandidate>,
    limit: usize,
) -> Vec<GrokImageCandidate> {
    let mut items = images.values().cloned().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .is_final
            .cmp(&left.is_final)
            .then_with(|| right.blob_size.cmp(&left.blob_size))
    });
    items.truncate(limit.max(1));
    items
}

fn upsert_best_candidate(
    images: &mut HashMap<String, GrokImageCandidate>,
    candidate: GrokImageCandidate,
) {
    match images.get(candidate.image_id.as_str()) {
        Some(existing)
            if existing.is_final && !candidate.is_final
                || existing.blob_size >= candidate.blob_size =>
        {
            return;
        }
        _ => {}
    }
    images.insert(candidate.image_id.clone(), candidate);
}

fn parse_image_socket_message(text: &str) -> Option<ParsedImageSocketMessage> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    match value.get("type").and_then(Value::as_str) {
        Some("image") => parse_image_candidate(&value).map(ParsedImageSocketMessage::Image),
        Some("error") => Some(ParsedImageSocketMessage::Error(
            value
                .get("err_msg")
                .or_else(|| value.get("error"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("grok imagine websocket error")
                .to_string(),
        )),
        _ => None,
    }
}

fn parse_image_candidate(value: &Value) -> Option<GrokImageCandidate> {
    let url = value.get("url")?.as_str()?.trim();
    let blob = value.get("blob")?.as_str()?.trim();
    if url.is_empty() || blob.is_empty() {
        return None;
    }
    let (image_id, ext) = parse_image_id_and_ext(url);
    let blob_size = blob.len();
    let stage = if blob_size >= IMAGINE_FINAL_MIN_BYTES {
        GrokImageStage::Final
    } else if blob_size >= IMAGINE_MEDIUM_MIN_BYTES {
        GrokImageStage::Medium
    } else {
        GrokImageStage::Preview
    };

    Some(GrokImageCandidate {
        image_id,
        ext,
        blob: blob.to_string(),
        blob_size,
        stage,
        is_final: stage == GrokImageStage::Final,
    })
}

fn parse_image_id_and_ext(url: &str) -> (String, String) {
    let trimmed = url.trim();
    let default_id = format!("img_{}", random_hex(24));
    let Some(images_pos) = trimmed.find("/images/") else {
        return (default_id, "png".to_string());
    };
    let rest = &trimmed[images_pos + "/images/".len()..];
    let Some(dot_pos) = rest.rfind('.') else {
        return (default_id, "png".to_string());
    };
    let image_id = rest[..dot_pos].trim_matches('/').trim();
    let ext = rest[dot_pos + 1..]
        .split(['?', '#'])
        .next()
        .unwrap_or("png")
        .trim()
        .to_ascii_lowercase();
    (
        if image_id.is_empty() {
            default_id
        } else {
            image_id.to_string()
        },
        if ext.is_empty() {
            "png".to_string()
        } else {
            ext
        },
    )
}

fn websocket_text(message: Message) -> Option<String> {
    match message {
        Message::Text(text) => Some(text.to_string()),
        Message::Binary(bytes) => String::from_utf8(bytes.to_vec()).ok(),
        _ => None,
    }
}

fn strip_base64(blob: &str) -> String {
    blob.strip_prefix("data:image/png;base64,")
        .or_else(|| blob.strip_prefix("data:image/jpeg;base64,"))
        .or_else(|| blob.strip_prefix("data:image/jpg;base64,"))
        .or_else(|| blob.strip_prefix("data:image/webp;base64,"))
        .unwrap_or(blob)
        .trim()
        .to_string()
}

fn to_data_uri(blob: &str, ext: &str) -> String {
    if blob.starts_with("data:") {
        blob.to_string()
    } else {
        let mime = match ext {
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            _ => "image/png",
        };
        format!("data:{mime};base64,{}", strip_base64(blob))
    }
}

fn sse_named_json(event: &str, value: &Value) -> Result<Bytes, UpstreamError> {
    let data = serde_json::to_string(value)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok(Bytes::from(format!("event: {event}\ndata: {data}\n\n")))
}

fn sse_error_frame(message: &str) -> Bytes {
    let payload = json!({
        "type": "error",
        "error": {
            "message": message,
            "type": "server_error",
        }
    });
    Bytes::from(format!(
        "event: error\ndata: {}\n\n",
        serde_json::to_string(&payload).unwrap_or_else(|_| "{\"type\":\"error\"}".to_string())
    ))
}

fn handle_image_retry_error(
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    state_manager: &CredentialStateManager,
    credential_id: i64,
    model: Option<&str>,
    status: Option<u16>,
    message: String,
    request_meta: Option<UpstreamRequestMeta>,
) -> CredentialRetryDecision<UpstreamResponse> {
    match status {
        Some(401 | 403) => {
            state_manager.mark_auth_dead(
                credential_states,
                &provider.channel,
                credential_id,
                Some(message.clone()),
            );
        }
        Some(429) => {
            state_manager.mark_rate_limited(
                credential_states,
                &provider.channel,
                credential_id,
                model,
                None,
                Some(message.clone()),
            );
        }
        _ => {
            state_manager.mark_transient_failure(
                credential_states,
                &provider.channel,
                credential_id,
                model,
                None,
                Some(message.clone()),
            );
        }
    }
    CredentialRetryDecision::Retry {
        last_status: status,
        last_error: Some(message),
        last_request_meta: request_meta,
    }
}

fn map_connect_ws_error(err: WsError) -> WsConnectError {
    if let WsError::Http(response) = &err {
        let status = response.status().as_u16();
        return WsConnectError {
            status: Some(status),
            message: format!("grok imagine websocket connect failed with status {status}"),
        };
    }
    WsConnectError {
        status: None,
        message: err.to_string(),
    }
}

enum ParsedImageSocketMessage {
    Image(GrokImageCandidate),
    Error(String),
}
