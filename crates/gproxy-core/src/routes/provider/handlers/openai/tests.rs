use super::{
    build_openai_payload, join_base_url_and_path_local, openai_ws_headers_from_upgrade_headers,
    prepare_upstream_websocket_request,
};
use axum::http::{HeaderMap, HeaderValue};
use gproxy_middleware::TransformRequest;
use gproxy_protocol::gemini::live::request::GeminiLiveConnectRequest;
use gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest;
use gproxy_provider::{
    BuiltinChannel, BuiltinChannelCredential, BuiltinChannelSettings, ChannelCredential, ChannelId,
    ChannelSettings, CredentialPickMode, CredentialRef, DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS,
    ProviderCredentialState, ProviderDefinition, ProviderDispatchTable,
};
use serde_json::json;

fn build_aistudio_provider(base_url: &str, api_key: &str) -> ProviderDefinition {
    let channel = ChannelId::Builtin(BuiltinChannel::AiStudio);
    let mut settings = ChannelSettings::Builtin(BuiltinChannelSettings::default_for(
        BuiltinChannel::AiStudio,
    ));
    if let ChannelSettings::Builtin(BuiltinChannelSettings::AiStudio(value)) = &mut settings {
        value.base_url = base_url.to_string();
    }

    let mut credential = BuiltinChannelCredential::blank_for(BuiltinChannel::AiStudio);
    if let BuiltinChannelCredential::AiStudio(value) = &mut credential {
        value.api_key = api_key.to_string();
    }

    ProviderDefinition {
        channel,
        dispatch: ProviderDispatchTable::default(),
        settings,
        credential_pick_mode: CredentialPickMode::RoundRobinWithCache,
        cache_affinity_max_keys: DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS,
        credentials: ProviderCredentialState {
            credentials: vec![CredentialRef {
                id: 1,
                label: None,
                credential: ChannelCredential::Builtin(credential),
            }],
            channel_states: Vec::new(),
        },
    }
}

#[test]
fn websocket_join_strips_version_suffix_for_live_paths() {
    assert_eq!(
        join_base_url_and_path_local("wss://generativelanguage.googleapis.com/v1beta", "/ws/rpc"),
        "wss://generativelanguage.googleapis.com/ws/rpc"
    );
    assert_eq!(
        join_base_url_and_path_local("wss://generativelanguage.googleapis.com/v1beta1", "/ws/rpc"),
        "wss://generativelanguage.googleapis.com/ws/rpc"
    );
}

#[test]
fn prepare_aistudio_live_ws_request_injects_key_query() {
    let channel = ChannelId::Builtin(BuiltinChannel::AiStudio);
    let provider = build_aistudio_provider(
        "https://generativelanguage.googleapis.com/v1beta",
        "test-key",
    );
    let request = TransformRequest::GeminiLive(GeminiLiveConnectRequest::default());
    let credential = provider
        .credentials
        .credentials
        .first()
        .expect("provider credential");

    let (url, headers) =
        prepare_upstream_websocket_request(&channel, &provider, &request, credential)
            .expect("prepare websocket request");

    assert!(
        url.starts_with(
            "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?"
        )
    );
    assert!(url.contains("key=test-key"));
    assert!(headers.iter().any(
        |(name, value)| name.eq_ignore_ascii_case("x-goog-api-key") && value == "test-key"
    ));
}

#[test]
fn openai_ws_upgrade_headers_keep_business_headers_and_drop_transport_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer client-secret"),
    );
    headers.insert("user-agent", HeaderValue::from_static("codex_vscode/0.1"));
    headers.insert("connection", HeaderValue::from_static("Upgrade"));
    headers.insert("upgrade", HeaderValue::from_static("websocket"));
    headers.insert("sec-websocket-key", HeaderValue::from_static("abc123"));
    headers.insert(
        "openai-beta",
        HeaderValue::from_static("responses_websockets=2026-02-04"),
    );
    headers.insert(
        "x-codex-turn-metadata",
        HeaderValue::from_static("{\"turn_id\":\"1\"}"),
    );
    headers.insert("session_id", HeaderValue::from_static("sess-123"));
    headers.insert("x-app", HeaderValue::from_static("cli"));

    let parsed = openai_ws_headers_from_upgrade_headers(&headers);

    assert_eq!(
        parsed.openai_beta.as_deref(),
        Some("responses_websockets=2026-02-04")
    );
    assert_eq!(
        parsed.x_codex_turn_metadata.as_deref(),
        Some("{\"turn_id\":\"1\"}")
    );
    assert_eq!(parsed.session_id.as_deref(), Some("sess-123"));
    assert_eq!(parsed.extra.get("x-app").map(String::as_str), Some("cli"));
    assert!(!parsed.extra.contains_key("authorization"));
    assert!(!parsed.extra.contains_key("user-agent"));
    assert!(!parsed.extra.contains_key("connection"));
    assert!(!parsed.extra.contains_key("upgrade"));
    assert!(!parsed.extra.contains_key("sec-websocket-key"));
}

#[test]
fn build_openai_payload_flattens_passthrough_headers_for_typed_decode() {
    let mut headers = HeaderMap::new();
    headers.insert("x-test", HeaderValue::from_static("value"));

    let payload = build_openai_payload(
        json!({
            "model": "claude-sonnet-4-6",
            "input": "hello"
        }),
        &headers,
        "invalid openai responses request body",
    )
    .expect("payload");

    let decoded: OpenAiCreateResponseRequest =
        serde_json::from_slice(payload.as_ref()).expect("request should decode");

    assert_eq!(
        decoded.headers.extra.get("x-test").map(String::as_str),
        Some("value")
    );
}
