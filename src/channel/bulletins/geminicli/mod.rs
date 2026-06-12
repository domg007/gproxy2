//! Gemini CLI channel (Google Code Assist, OAuth2 `refresh_token` grant; the
//! Code Assist envelope). FIRST envelope channel — `antigravity` reuses this
//! shape.
//!
//! `target_kind` is `GeminiGenerateContent`: the request is a Gemini
//! `generateContent` body, but Code Assist nests it under `request` alongside
//! routing metadata (`{model, project, user_prompt_id, request:<body>}`) and
//! nests the response under `.response`. [`prepare`](GeminiCliChannel::prepare)
//! wraps via [`envelope::wrap_code_assist`]; [`normalize`] and
//! [`stream_decoder`] unwrap the non-stream body and each SSE frame.
//! [`auth`] owns the OAuth bearer + refresh + the CLI fingerprint headers.
//!
//! [`normalize`]: GeminiCliChannel::normalize
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
use crate::protocol::ContentGenerationKind;

pub struct GeminiCliChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for GeminiCliChannel {
    fn id(&self) -> &'static str {
        "geminicli"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::GeminiGenerateContent
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

    fn normalize(&self, body: Bytes) -> Bytes {
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
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl ChannelLogin for GeminiCliChannel {
    fn authcode_start(
        &self,
        redirect_uri: &str,
        state: &str,
        pkce_challenge: &str,
    ) -> Option<AuthCodeStart> {
        let (authorize_url, redirect_uri) =
            auth::authcode_start(redirect_uri, state, pkce_challenge);
        Some(AuthCodeStart {
            authorize_url,
            redirect_uri,
        })
    }

    async fn authcode_exchange(
        &self,
        client: &Arc<dyn UpstreamClient>,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
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
        // normalize extracts `.response`.
        let out =
            GeminiCliChannel.normalize(Bytes::from_static(br#"{"response":{"candidates":[]}}"#));
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
