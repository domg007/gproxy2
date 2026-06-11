//! Kiro channel (Amazon Q / Kiro IDE) — DUAL OAuth + AWS Smithy event-stream.
//!
//! Kiro exposes no OpenAI/Claude/Gemini-compatible surface: chat goes through
//! the Smithy REST-JSON `POST /generateAssistantResponse`, whose RESPONSE is an
//! AWS binary event-stream. `target_kind` is `OpenAiResponses`, so the M2 layer
//! sees Responses on both sides — but the channel must SHAPE both directions:
//!
//!   * **request** ([`prepare`](KiroChannel::prepare)) — convert the inbound
//!     OpenAI Responses body into Kiro's `conversationState` JSON
//!     ([`request::build_request_body`]), lift `profileArn` to the top level, and
//!     inject the Kiro auth + IDE fingerprint headers.
//!   * **response** ([`stream_decoder`](KiroChannel::stream_decoder)) — decode the
//!     Smithy event-stream into Responses SSE ([`response::KiroStreamDecoder`]).
//!
//! Auth is a dual `refresh_token` grant (social vs AWS IdC) — see [`auth`]. This
//! is the heaviest channel; the binary frame parser lives in [`smithy`] and is
//! the most-tested piece. All decode is synchronous, so the channel compiles on
//! the wasm edge target (refresh is async via [`UpstreamClient`], fine on all).

mod auth;
mod request;
mod response;
mod smithy;
mod sse;

use std::sync::Arc;

use bytes::Bytes;
use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT};
use serde_json::Value;

use crate::channel::http_util::{allow_headers, build_request, join_url};
use crate::channel::{Channel, ChannelError, ChannelStreamDecoder, PrepareCtx, PreparedRequest};
use crate::http::client::UpstreamClient;
use crate::protocol::ContentGenerationKind;

use response::KiroStreamDecoder;

/// Amazon Q runtime host; chat lives at `/generateAssistantResponse`.
const DEFAULT_BASE_URL: &str = "https://q.us-east-1.amazonaws.com";
/// Kiro chat endpoint (Smithy REST-JSON, AWS event-stream response).
const GENERATE_PATH: &str = "/generateAssistantResponse";
/// User-Agent the Kiro IDE sends.
const USER_AGENT_VALUE: &str = "KiroIDE-0.12.224-gproxy";
/// Kiro agent mode header value.
const AGENT_MODE: &str = "vibe";

pub struct KiroChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for KiroChannel {
    fn id(&self) -> &'static str {
        "kiro"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::OpenAiResponses
    }

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        let access_token = auth::access_token(ctx.secret)?.to_string();
        let profile_arn = auth::profile_arn(ctx.secret, ctx.provider_settings).map(str::to_owned);
        let base = ctx
            .provider_settings
            .get("base_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_BASE_URL);

        // Shape the inbound Responses body into Kiro's conversationState graph,
        // then lift profileArn to the top level (where Kiro expects it). The
        // upstream-mapped model id selects the Kiro model.
        let body = request::build_request_body(&ctx.body, ctx.upstream_model_id, &gen_uuid())?;
        let body = with_profile_arn(body, profile_arn.as_deref())?;

        let uri = join_url(base, GENERATE_PATH, None)?;
        // Smithy/binary channel: it injects its own auth + IDE fingerprint and
        // forwards no inbound headers beyond the base content-type/accept set.
        let headers = allow_headers(ctx.headers, &[]);
        let mut req = build_request(ctx.method, uri, headers, Bytes::from(body))?;
        apply_headers(&mut req, &access_token)?;
        Ok(PreparedRequest::new(req))
    }

    fn stream_decoder(&self) -> Option<Box<dyn ChannelStreamDecoder>> {
        Some(Box::new(KiroStreamDecoder::new()))
    }

    fn needs_refresh(&self, secret: &Value) -> bool {
        auth::needs_refresh(secret)
    }

    async fn refresh(
        &self,
        client: &Arc<dyn UpstreamClient>,
        secret: &Value,
    ) -> Result<Value, ChannelError> {
        // `provider_settings` are not threaded into refresh; the social auth base
        // defaults inside `auth::refresh` when absent (the common case).
        auth::refresh(client, &Value::Null, secret).await
    }
}

/// Inject `profileArn` at the top level of the serialized request body when the
/// credential carries one and the body does not already set it.
fn with_profile_arn(body: Vec<u8>, profile_arn: Option<&str>) -> Result<Vec<u8>, ChannelError> {
    let Some(arn) = profile_arn else {
        return Ok(body);
    };
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|e| ChannelError::Build(format!("kiro request body re-parse: {e}")))?;
    if value.get("profileArn").is_none()
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("profileArn".into(), Value::String(arn.to_string()));
    }
    serde_json::to_vec(&value)
        .map_err(|e| ChannelError::Build(format!("kiro request body re-serialize: {e}")))
}

/// Inject the Kiro Bearer + IDE fingerprint headers (agent-mode, optout, the AWS
/// SDK retry/invocation ids, UA). `accept: */*` matches the Kiro IDE request —
/// the event-stream body is selected by the endpoint, not Accept.
fn apply_headers(req: &mut http::Request<Bytes>, access_token: &str) -> Result<(), ChannelError> {
    let bearer = HeaderValue::from_str(&format!("Bearer {access_token}"))
        .map_err(|e| ChannelError::InvalidCredential(format!("bad access_token: {e}")))?;
    let invocation_id = HeaderValue::from_str(&gen_uuid())
        .map_err(|e| ChannelError::Build(format!("bad invocation id: {e}")))?;

    let h = req.headers_mut();
    h.insert(AUTHORIZATION, bearer);
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h.insert(ACCEPT, HeaderValue::from_static("*/*"));
    h.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    h.insert(
        HeaderName::from_static("x-amzn-kiro-agent-mode"),
        HeaderValue::from_static(AGENT_MODE),
    );
    h.insert(
        HeaderName::from_static("x-amzn-codewhisperer-optout"),
        HeaderValue::from_static("true"),
    );
    h.insert(
        HeaderName::from_static("amz-sdk-request"),
        HeaderValue::from_static("attempt=1; max=3"),
    );
    h.insert(
        HeaderName::from_static("amz-sdk-invocation-id"),
        invocation_id,
    );
    Ok(())
}

/// Fresh v4-shaped UUID string (`8-4-4-4-12` hex) from `crate::util::rand`
/// (one cross-target RNG source; avoids uuid's native-only gate — the value is
/// opaque to Kiro, only its shape matters).
fn gen_uuid() -> String {
    let mut b = crate::util::rand::bytes::<16>();
    // RFC-4122 version/variant bits (cosmetic — Kiro treats it as opaque).
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::http::client::ClientError;
    use http::{HeaderMap, Method, Response};
    use serde_json::json;

    /// Mock client returning a fixed JSON body and recording each request's URI.
    struct MockClient {
        body: Vec<u8>,
        seen: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl UpstreamClient for MockClient {
        async fn send(&self, req: http::Request<Bytes>) -> Result<Response<Bytes>, ClientError> {
            self.seen.lock().unwrap().push(req.uri().to_string());
            Ok(Response::new(Bytes::from(self.body.clone())))
        }
    }

    fn mock(body: Value) -> Arc<MockClient> {
        Arc::new(MockClient {
            body: serde_json::to_vec(&body).unwrap(),
            seen: std::sync::Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn dual_refresh() {
        // SOCIAL: refreshToken POST → rotates tokens + stores profileArn.
        let client = mock(json!({
            "accessToken": "new-access",
            "refreshToken": "new-refresh",
            "profileArn": "arn:aws:kiro:profile/p1",
            "expiresIn": 3600,
        }));
        let dyn_client: Arc<dyn UpstreamClient> = client.clone();
        let secret = json!({ "access_token": "old", "refresh_token": "old-rt" });
        let out = auth::refresh(&dyn_client, &Value::Null, &secret)
            .await
            .unwrap();
        assert_eq!(out["access_token"], "new-access");
        assert_eq!(out["refresh_token"], "new-refresh");
        assert_eq!(out["profile_arn"], "arn:aws:kiro:profile/p1");
        assert!(out["expires_at_ms"].as_i64().unwrap() > crate::util::time::unix_now() * 1000);
        assert!(client.seen.lock().unwrap()[0].contains("/refreshToken"));

        // IdC: region-templated oidc token POST → rotates access token; the
        // response omits refreshToken so the old one is preserved.
        let client = mock(json!({ "accessToken": "idc-access", "expiresIn": 1800 }));
        let dyn_client: Arc<dyn UpstreamClient> = client.clone();
        let secret = json!({
            "access_token": "old",
            "refresh_token": "old-rt",
            "client_id": "cid",
            "client_secret": "csecret",
            "region": "eu-west-1",
        });
        let out = auth::refresh(&dyn_client, &Value::Null, &secret)
            .await
            .unwrap();
        assert_eq!(out["access_token"], "idc-access");
        assert_eq!(out["refresh_token"], "old-rt");
        assert!(
            client.seen.lock().unwrap()[0].contains("oidc.eu-west-1.amazonaws.com/token"),
            "IdC refresh must hit the region-templated oidc host"
        );
    }

    #[test]
    fn request_build() {
        // Minimal Responses body → Smithy conversationState with profileArn +
        // currentMessage.userInputMessage.content.
        let secret = json!({
            "access_token": "tok",
            "profile_arn": "arn:aws:kiro:profile/abc",
        });
        let settings = json!({});
        let headers = HeaderMap::new();
        let ctx = PrepareCtx {
            secret: &secret,
            provider_settings: &settings,
            upstream_model_id: "claude-sonnet-4-5",
            method: Method::POST,
            path: "/v1/responses",
            query: None,
            headers: &headers,
            body: Bytes::from_static(br#"{"input":"hello kiro"}"#),
        };
        let req = KiroChannel.prepare(ctx).unwrap().request;

        assert_eq!(
            req.uri().to_string(),
            "https://q.us-east-1.amazonaws.com/generateAssistantResponse"
        );
        assert_eq!(req.headers().get("authorization").unwrap(), "Bearer tok");
        assert_eq!(req.headers().get("x-amzn-kiro-agent-mode").unwrap(), "vibe");
        assert!(req.headers().get("amz-sdk-invocation-id").is_some());

        let v: Value = serde_json::from_slice(req.body()).unwrap();
        assert_eq!(v["profileArn"], "arn:aws:kiro:profile/abc");
        let user = &v["conversationState"]["currentMessage"]["userInputMessage"];
        assert_eq!(user["content"], "hello kiro");
        // model is dot-versioned via map_model.
        assert_eq!(user["modelId"], "claude-sonnet-4.5");
    }
}
