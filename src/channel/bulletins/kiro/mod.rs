//! Kiro channel (Amazon Q / Kiro IDE) — DUAL OAuth + AWS Smithy event-stream.
//!
//! Kiro exposes no OpenAI/Claude/Gemini-compatible surface: chat goes through
//! the Smithy REST-JSON `POST /generateAssistantResponse`, whose RESPONSE is an
//! AWS binary event-stream. The upstream speaks the OpenAI Responses format, so the M2 layer
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
#[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
mod fingerprint;
mod request;
mod request_tools;
mod response;
mod smithy;
mod sse;
mod tool_calls;
mod usage;
use std::sync::Arc;

use bytes::Bytes;
use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT};
use serde_json::Value;

use crate::channel::http_util::{allow_headers, build_request, join_url};
use crate::channel::{
    AuthCodeStart, Channel, ChannelError, ChannelLogin, ChannelStreamDecoder, PrepareCtx,
    PreparedRequest,
};
use crate::http::client::UpstreamClient;
use crate::protocol::Provider;

use response::KiroStreamDecoder;

/// Amazon Q runtime host; chat lives at `/generateAssistantResponse`.
const DEFAULT_BASE_URL: &str = "https://q.us-east-1.amazonaws.com";
/// Kiro chat endpoint (Smithy REST-JSON, AWS event-stream response).
const GENERATE_PATH: &str = "/generateAssistantResponse";
/// User-Agent the Kiro IDE sends.
const USER_AGENT_VALUE: &str = "aws-sdk-rust/1.3.15 ua/2.1 api/codewhispererstreaming/0.1.16551 os/linux lang/rust/1.92.0 md/appVersion-2.6.1 app/AmazonQ-For-CLI";
/// Kiro agent mode header value.
const AGENT_MODE: &str = "vibe";

pub struct KiroChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for KiroChannel {
    fn id(&self) -> &'static str {
        "kiro"
    }

    fn provider_family(&self) -> Provider {
        Provider::OpenAi
    }

    fn routing_table(&self) -> crate::channel::routes::RouteList {
        use crate::channel::routes::{cg, local, pass, pv, xform};
        use crate::protocol::{ContentGenerationKind::*, Operation::*, Provider as P};
        vec![
            pass(ListModels, pv(P::OpenAi)),
            xform(ListModels, pv(P::Claude), ListModels, pv(P::OpenAi)),
            xform(ListModels, pv(P::Gemini), ListModels, pv(P::OpenAi)),
            local(CountTokens, pv(P::OpenAi)),
            local(CountTokens, pv(P::Claude)),
            local(CountTokens, pv(P::Gemini)),
            pass(GenerateContent, cg(OpenAiResponses)),
            xform(
                GenerateContent,
                cg(OpenAiChatCompletions),
                GenerateContent,
                cg(OpenAiResponses),
            ),
            xform(
                GenerateContent,
                cg(ClaudeMessages),
                GenerateContent,
                cg(OpenAiResponses),
            ),
            xform(
                GenerateContent,
                cg(GeminiGenerateContent),
                GenerateContent,
                cg(OpenAiResponses),
            ),
            pass(StreamGenerateContent, cg(OpenAiResponses)),
            xform(
                StreamGenerateContent,
                cg(OpenAiChatCompletions),
                StreamGenerateContent,
                cg(OpenAiResponses),
            ),
            xform(
                StreamGenerateContent,
                cg(ClaudeMessages),
                StreamGenerateContent,
                cg(OpenAiResponses),
            ),
            xform(
                StreamGenerateContent,
                cg(GeminiGenerateContent),
                StreamGenerateContent,
                cg(OpenAiResponses),
            ),
        ]
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
    fn default_emulation(&self) -> Option<wreq::Emulation> {
        Some(fingerprint::default_emulation())
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

    fn prepare_usage_request(
        &self,
        secret: &Value,
        settings: &Value,
    ) -> Result<Option<http::Request<Bytes>>, ChannelError> {
        usage::request(secret, settings)
    }

    fn parse_usage(
        &self,
        status: http::StatusCode,
        _headers: &http::HeaderMap,
        body: &Bytes,
    ) -> Option<crate::channel::UsageSnapshot> {
        usage::parse(status, body)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl ChannelLogin for KiroChannel {
    async fn authcode_start(
        &self,
        client: &Arc<dyn UpstreamClient>,
        params: &Value,
        redirect_uri: &str,
        state: &str,
        pkce_challenge: &str,
    ) -> Result<Option<AuthCodeStart>, ChannelError> {
        // IdC (AWS SSO-OIDC) needs an async RegisterClient before the authorize
        // URL; the registered creds ride `extra` to the exchange. Else: social.
        if auth::idc_requested(params) {
            let (authorize_url, redirect_uri, extra) =
                auth::idc_authcode_start(client, params, redirect_uri, state, pkce_challenge)
                    .await?;
            return Ok(Some(AuthCodeStart {
                authorize_url,
                redirect_uri,
                extra: Some(extra),
            }));
        }
        let (authorize_url, redirect_uri) =
            auth::authcode_start(redirect_uri, state, pkce_challenge);
        Ok(Some(AuthCodeStart {
            authorize_url,
            redirect_uri,
            extra: None,
        }))
    }

    async fn authcode_exchange(
        &self,
        client: &Arc<dyn UpstreamClient>,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
        extra: Option<&Value>,
    ) -> Result<Value, ChannelError> {
        // IdC when start stashed registered client creds; else social.
        if let Some(extra) = extra.filter(|e| e.get("client_id").is_some()) {
            return auth::idc_authcode_exchange(client, code, verifier, redirect_uri, extra).await;
        }
        auth::authcode_exchange(client, code, verifier, redirect_uri).await
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

/// Fresh v4-shaped UUID string from `crate::util::rand` (opaque to Kiro).
fn gen_uuid() -> String {
    crate::util::rand::uuid_v4()
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

    #[tokio::test]
    async fn kiro_social_authcode_start() {
        // Social path: client/params unused; builds the static portal URL.
        let client: Arc<dyn UpstreamClient> = mock(json!({}));
        let start = KiroChannel
            .authcode_start(&client, &json!({}), "", "ST", "CH")
            .await
            .expect("authcode_start ok")
            .expect("kiro supports social authcode");
        let url = &start.authorize_url;
        assert!(url.starts_with("https://app.kiro.dev/signin?"), "{url}");
        assert!(url.contains("state=ST"), "{url}");
        assert!(url.contains("code_challenge=CH"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        assert!(url.contains("redirect_uri="), "{url}");
        assert!(url.contains("redirect_from=KiroIDE"), "{url}");
    }

    #[tokio::test]
    async fn kiro_idc_authcode_start_registers_and_builds_url() {
        // IdC: the mock answers the RegisterClient call; the registered client_id
        // must flow into the authorize URL and the stashed `extra`.
        let client: Arc<dyn UpstreamClient> = mock(json!({
            "clientId": "cid-123",
            "clientSecret": "csec-456",
        }));
        let params = json!({
            "auth_method": "idc",
            "start_url": "https://my.awsapps.com/start",
            "region": "us-west-2",
        });
        let start = KiroChannel
            .authcode_start(&client, &params, "", "ST", "CH")
            .await
            .expect("authcode_start ok")
            .expect("kiro idc authcode");
        let url = &start.authorize_url;
        assert!(
            url.starts_with("https://oidc.us-west-2.amazonaws.com/authorize?"),
            "{url}"
        );
        assert!(url.contains("response_type=code"), "{url}");
        assert!(url.contains("client_id=cid-123"), "{url}");
        assert!(url.contains("code_challenge=CH"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        let extra = start.extra.expect("idc extra");
        assert_eq!(extra["client_id"], "cid-123");
        assert_eq!(extra["client_secret"], "csec-456");
        assert_eq!(extra["region"], "us-west-2");
        assert_eq!(extra["provider"], "Enterprise");
    }

    #[tokio::test]
    async fn kiro_idc_authcode_exchange_shapes_secret() {
        // IdC exchange uses the stashed creds and mints an `auth_method:"IdC"`
        // secret carrying client_id/client_secret/region for later refresh.
        let client: Arc<dyn UpstreamClient> = mock(json!({
            "accessToken": "at-idc",
            "refreshToken": "rt-idc",
            "expiresIn": 3600,
        }));
        let extra = json!({
            "client_id": "cid",
            "client_secret": "csec",
            "region": "us-west-2",
            "provider": "Enterprise",
        });
        let secret = KiroChannel
            .authcode_exchange(
                &client,
                "code-1",
                "verifier-1",
                "http://127.0.0.1/oauth/callback",
                Some(&extra),
            )
            .await
            .expect("idc exchange");
        assert_eq!(secret["access_token"], "at-idc");
        assert_eq!(secret["refresh_token"], "rt-idc");
        assert_eq!(secret["auth_method"], "IdC");
        assert_eq!(secret["client_id"], "cid");
        assert_eq!(secret["client_secret"], "csec");
        assert_eq!(secret["region"], "us-west-2");
    }
}
