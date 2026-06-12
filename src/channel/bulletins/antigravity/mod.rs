//! Antigravity channel (Google Code Assist via gcli2api — the same Code Assist
//! envelope as `geminicli`, with a distinct OAuth client and User-Agent).
//!
//! `target_kind` is `GeminiGenerateContent`: the request is a Gemini
//! `generateContent` body, nested by Code Assist under `request` alongside
//! routing metadata (`{model, project, user_prompt_id, request:<body>}`) with
//! the response under `.response`. [`prepare`](AntigravityChannel::prepare)
//! wraps via [`envelope::wrap_code_assist`]; [`normalize`] and
//! [`stream_decoder`] unwrap the non-stream body and each SSE frame — all shared
//! with `geminicli`. [`auth`] owns the OAuth bearer + refresh + Antigravity's
//! distinct fingerprint (UA + `requestId`/`requestType`).
//!
//! [`normalize`]: AntigravityChannel::normalize
//! [`stream_decoder`]: AntigravityChannel::stream_decoder

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

pub struct AntigravityChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for AntigravityChannel {
    fn id(&self) -> &'static str {
        "antigravity"
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
        let request_type = auth::request_type(ctx.upstream_model_id);

        // Wrap the gemini body in the Code Assist envelope. `ctx.body` is moved
        // out here (the request body becomes the wrapped bytes).
        let wrapped = envelope::wrap_code_assist(
            &ctx.body,
            ctx.upstream_model_id,
            project_id,
            &envelope::random_user_prompt_id(),
        )?;

        // Verb encoded in the inbound path (shared with geminicli/vertex): the
        // Code Assist endpoints live under `/v1internal:` and stream only when
        // `alt=sse` is set explicitly.
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
        auth::apply(&mut req, &access_token, request_type)?;
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
impl ChannelLogin for AntigravityChannel {
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

    #[test]
    fn prepare_wraps_envelope_antigravity_ua() {
        let secret = json!({ "access_token": "tok-abc", "project_id": "proj" });
        let settings = json!({});
        let headers = HeaderMap::new();

        let ctx = PrepareCtx {
            secret: &secret,
            provider_settings: &settings,
            upstream_model_id: "gemini-2.5-pro",
            method: Method::POST,
            path: "/v1beta/models/gemini-2.5-pro:generateContent",
            query: None,
            headers: &headers,
            body: Bytes::from_static(br#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#),
        };
        let req = AntigravityChannel.prepare(ctx).unwrap().request;

        // Distinct from geminicli: code-assist path + Antigravity UA/client wiring.
        assert_eq!(
            req.uri().to_string(),
            "https://cloudcode-pa.googleapis.com/v1internal:generateContent"
        );
        assert_eq!(
            req.headers().get("authorization").unwrap(),
            "Bearer tok-abc"
        );
        assert_eq!(
            req.headers().get("user-agent").unwrap(),
            "antigravity/cli/1.0.6 linux/amd64"
        );
        assert_eq!(req.headers().get("requesttype").unwrap(), "agent");
        assert!(req.headers().get("requestid").is_some());

        // Body is the Code Assist envelope carrying the original request.
        let v: Value = serde_json::from_slice(req.body()).unwrap();
        assert_eq!(v["model"], "gemini-2.5-pro");
        assert_eq!(v["project"], "proj");
        assert!(v["user_prompt_id"].as_str().is_some_and(|s| s.len() == 32));
        assert_eq!(v["request"]["contents"][0]["parts"][0]["text"], "hi");
    }
}
