//! Gemini CLI channel (Google Code Assist, OAuth2 `refresh_token` grant; the
//! Code Assist envelope). FIRST envelope channel — `antigravity` reuses this
//! shape.
//!
//! the request is a Gemini
//! `generateContent` body, but Code Assist nests it under `request` alongside
//! routing metadata (`{model, project, user_prompt_id, request:<body>}`) and
//! nests the response under `.response`. [`prepare`](GeminiCliChannel::prepare)
//! wraps via [`envelope::wrap_code_assist`]; [`shape_response`] and
//! [`stream_decoder`] unwrap the non-stream body and each SSE frame.
//! [`auth`] owns the OAuth bearer + refresh + the CLI fingerprint headers.
//!
//! [`shape_response`]: GeminiCliChannel::shape_response
//! [`stream_decoder`]: GeminiCliChannel::stream_decoder

mod auth;
#[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
mod fingerprint;

use std::sync::Arc;

use bytes::Bytes;
use serde_json::Value;

use crate::channel::envelope::{self, CodeAssistStreamDecoder};
use crate::channel::http_util::{allow_headers, build_request, join_url};
use crate::channel::{
    AuthCodeStart, Channel, ChannelError, ChannelLogin, ChannelStreamDecoder, PrepareCtx,
    PreparedRequest,
};
use crate::http::client::UpstreamClient;
use crate::protocol::Provider;

pub struct GeminiCliChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for GeminiCliChannel {
    fn id(&self) -> &'static str {
        "geminicli"
    }

    fn provider_family(&self) -> Provider {
        Provider::Gemini
    }

    fn routing_table(&self) -> crate::channel::routes::RouteList {
        use crate::channel::routes::{cg, pass, pv, xform};
        use crate::protocol::{ContentGenerationKind::*, Operation::*, Provider as P};
        vec![
            pass(ListModels, pv(P::Gemini)),
            xform(ListModels, pv(P::Claude), ListModels, pv(P::Gemini)),
            xform(ListModels, pv(P::OpenAi), ListModels, pv(P::Gemini)),
            pass(GetModel, pv(P::Gemini)),
            xform(GetModel, pv(P::Claude), GetModel, pv(P::Gemini)),
            xform(GetModel, pv(P::OpenAi), GetModel, pv(P::Gemini)),
            pass(CountTokens, pv(P::Gemini)),
            xform(CountTokens, pv(P::Claude), CountTokens, pv(P::Gemini)),
            xform(CountTokens, pv(P::OpenAi), CountTokens, pv(P::Gemini)),
            pass(GenerateContent, cg(GeminiGenerateContent)),
            xform(
                GenerateContent,
                cg(ClaudeMessages),
                GenerateContent,
                cg(GeminiGenerateContent),
            ),
            xform(
                GenerateContent,
                cg(OpenAiChatCompletions),
                GenerateContent,
                cg(GeminiGenerateContent),
            ),
            xform(
                GenerateContent,
                cg(OpenAiResponses),
                GenerateContent,
                cg(GeminiGenerateContent),
            ),
            pass(StreamGenerateContent, cg(GeminiGenerateContent)),
            xform(
                StreamGenerateContent,
                cg(ClaudeMessages),
                StreamGenerateContent,
                cg(GeminiGenerateContent),
            ),
            xform(
                StreamGenerateContent,
                cg(OpenAiChatCompletions),
                StreamGenerateContent,
                cg(GeminiGenerateContent),
            ),
            xform(
                StreamGenerateContent,
                cg(OpenAiResponses),
                StreamGenerateContent,
                cg(GeminiGenerateContent),
            ),
            xform(
                CreateImage,
                pv(P::OpenAi),
                GenerateContent,
                cg(GeminiGenerateContent),
            ),
            xform(
                EditImage,
                pv(P::OpenAi),
                GenerateContent,
                cg(GeminiGenerateContent),
            ),
            pass(CreateEmbedding, pv(P::Gemini)),
            xform(
                CreateEmbedding,
                pv(P::OpenAi),
                CreateEmbedding,
                pv(P::Gemini),
            ),
            xform(
                CompactContent,
                pv(P::OpenAi),
                GenerateContent,
                cg(GeminiGenerateContent),
            ),
        ]
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
    fn default_emulation(&self) -> Option<wreq::Emulation> {
        Some(fingerprint::default_emulation())
    }

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        let access_token = auth::access_token(ctx.secret)?.to_string();
        let project_id = auth::project_id(ctx.secret)?;

        // Wrap the gemini body in the Code Assist envelope. `ctx.body` is moved
        // out here (the request body becomes the wrapped bytes).
        let wrapped = envelope::wrap_code_assist(
            &ctx.body,
            ctx.upstream_model_id,
            project_id,
            &envelope::random_user_prompt_id(),
        )?;

        // M2 encodes the verb in the path for gemini targets; reuse only the
        // stream flag (`:streamGenerateContent` → SSE, else `:generateContent`),
        // same as vertex. The Code Assist endpoints live under `/v1internal:`
        // and stream only when `alt=sse` is set explicitly.
        let (verb, query) = if ctx.path.contains(":streamGenerateContent") {
            (":streamGenerateContent", Some("alt=sse"))
        } else {
            (":generateContent", None)
        };
        let path = format!("/v1internal{verb}");

        let uri = join_url(auth::BASE_URL, &path, query)?;
        // Envelope channel: it injects its own auth + fingerprint; forward no
        // inbound headers beyond the base content-type/accept allow-list.
        let headers = allow_headers(ctx.headers, &[]);
        let mut req = build_request(ctx.method, uri, headers, Bytes::from(wrapped))?;
        auth::apply(&mut req, &access_token, ctx.upstream_model_id)?;
        Ok(PreparedRequest::new(req))
    }

    fn shape_response(&self, body: Bytes, _ctx: &crate::channel::ShapeCtx) -> Bytes {
        envelope::unwrap_code_assist(body)
    }

    fn stream_decoder(&self) -> Option<Box<dyn ChannelStreamDecoder>> {
        Some(Box::new(CodeAssistStreamDecoder::new()))
    }

    fn needs_refresh(&self, secret: &Value) -> bool {
        auth::needs_refresh(secret)
    }

    async fn refresh(
        &self,
        client: &Arc<dyn UpstreamClient>,
        secret: &Value,
    ) -> Result<Value, ChannelError> {
        auth::refresh(client, secret).await
    }

    fn prepare_usage_request(
        &self,
        secret: &Value,
        settings: &Value,
    ) -> Result<Option<http::Request<Bytes>>, ChannelError> {
        let access_token = auth::access_token(secret)?;
        let project_id = auth::project_id(secret)?;
        let base = settings
            .get("base_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(auth::BASE_URL);
        // The usage call carries no model; the UA embeds a representative one.
        let ua = auth::user_agent("gemini-2.5-pro");
        envelope::user_quota_request(base, access_token, project_id, &ua)
    }

    fn parse_usage(
        &self,
        status: http::StatusCode,
        _headers: &http::HeaderMap,
        body: &Bytes,
    ) -> Option<crate::channel::UsageSnapshot> {
        envelope::parse_user_quota(status, body)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl ChannelLogin for GeminiCliChannel {
    async fn authcode_start(
        &self,
        _client: &Arc<dyn UpstreamClient>,
        _params: &Value,
        redirect_uri: &str,
        state: &str,
        pkce_challenge: &str,
    ) -> Result<Option<AuthCodeStart>, ChannelError> {
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
        _extra: Option<&Value>,
    ) -> Result<Value, ChannelError> {
        auth::authcode_exchange(client, code, verifier, redirect_uri).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use http::{HeaderMap, Method};
    use serde_json::json;

    fn ctx_for<'a>(
        secret: &'a Value,
        settings: &'a Value,
        headers: &'a HeaderMap,
        path: &'a str,
        body: &'static [u8],
    ) -> PrepareCtx<'a> {
        PrepareCtx {
            secret,
            provider_settings: settings,
            upstream_model_id: "gemini-2.5-pro",
            method: Method::POST,
            path,
            query: None,
            headers,
            body: Bytes::from_static(body),
        }
    }

    #[test]
    fn prepare_wraps_envelope_and_builds_v1internal() {
        let secret = json!({ "access_token": "tok-abc", "project_id": "proj" });
        let settings = json!({});
        let headers = HeaderMap::new();

        // Non-stream: :generateContent, no query, body is the envelope.
        let ctx = ctx_for(
            &secret,
            &settings,
            &headers,
            "/v1beta/models/gemini-2.5-pro:generateContent",
            br#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#,
        );
        let req = GeminiCliChannel.prepare(ctx).unwrap().request;
        assert_eq!(
            req.uri().to_string(),
            "https://cloudcode-pa.googleapis.com/v1internal:generateContent"
        );
        assert_eq!(
            req.headers().get("authorization").unwrap(),
            "Bearer tok-abc"
        );
        // User-agent embeds the requested model id (dynamic per request).
        assert_eq!(
            req.headers().get("user-agent").unwrap(),
            "GeminiCLI-tui/0.46.0/gemini-2.5-pro (linux; x64; terminal) google-api-nodejs-client/9.15.1"
        );

        // Body is the Code Assist envelope carrying the original request.
        let v: Value = serde_json::from_slice(req.body()).unwrap();
        assert_eq!(v["model"], "gemini-2.5-pro");
        assert_eq!(v["project"], "proj");
        assert!(v["user_prompt_id"].as_str().is_some_and(|s| s.len() == 32));
        assert_eq!(v["request"]["contents"][0]["parts"][0]["text"], "hi");

        // Stream: :streamGenerateContent?alt=sse.
        let ctx = ctx_for(
            &secret,
            &settings,
            &headers,
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent",
            br#"{"contents":[]}"#,
        );
        let req = GeminiCliChannel.prepare(ctx).unwrap().request;
        assert_eq!(
            req.uri().to_string(),
            "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn normalize_unwraps_and_needs_refresh_expiry() {
        // shape_response extracts `.response`.
        let shape = crate::channel::ShapeCtx {
            op: crate::protocol::OperationKey::content_generation(
                crate::protocol::Operation::GenerateContent,
                crate::protocol::ContentGenerationKind::GeminiGenerateContent,
            ),
            stream: false,
            status: http::StatusCode::OK,
        };
        let out = GeminiCliChannel.shape_response(
            Bytes::from_static(br#"{"response":{"candidates":[]}}"#),
            &shape,
        );
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v, json!({"candidates": []}));

        // needs_refresh: missing project_id errors cleanly in prepare.
        let settings = json!({});
        let headers = HeaderMap::new();
        let no_proj = json!({ "access_token": "t" });
        let err = GeminiCliChannel
            .prepare(ctx_for(
                &no_proj,
                &settings,
                &headers,
                "/x:generateContent",
                b"{}",
            ))
            .unwrap_err();
        assert!(matches!(err, ChannelError::InvalidCredential(m) if m.contains("project_id")));

        // needs_refresh: no token → refresh; near-expiry → refresh; fresh → no.
        let now_ms = crate::util::time::unix_now().saturating_mul(1000);
        assert!(GeminiCliChannel.needs_refresh(&json!({})));
        assert!(GeminiCliChannel.needs_refresh(&json!({
            "access_token": "t", "expires_at_ms": now_ms + 10_000,
        })));
        assert!(!GeminiCliChannel.needs_refresh(&json!({
            "access_token": "t", "expires_at_ms": now_ms + 600_000,
        })));
    }
}
