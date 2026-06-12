//! Codex channel — OpenAI ChatGPT-backend Responses API over OAuth2
//! (`refresh_token` grant) plus the `codex_exec` impersonation header set.
//!
//! `target_kind` is `OpenAiResponses`: the upstream already speaks Responses
//! SSE, so there is NO envelope, NO stream decoder, NO normalize. This channel
//! does, however, SHAPE the request body in [`prepare`](CodexChannel::prepare)
//! (documented body mutation) — forcing `stream`/`store`, stripping sampling
//! fields, and lifting system messages into `instructions` — via
//! [`auth::normalize_responses_body`]. [`auth`] owns the OAuth refresh + the
//! fingerprint headers. The inbound `/v1/responses` path is rewritten to the
//! backend `/responses`.

mod auth;
#[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
mod fingerprint;

use std::sync::Arc;

use serde_json::Value;

use crate::channel::http_util::{allow_headers, build_request, join_url};
use crate::channel::{
    AuthCodeStart, Channel, ChannelError, ChannelLogin, PrepareCtx, PreparedRequest,
};
use crate::http::client::UpstreamClient;
use crate::protocol::ContentGenerationKind;

pub struct CodexChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for CodexChannel {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::OpenAiResponses
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
    fn default_emulation(&self) -> Option<wreq::Emulation> {
        Some(fingerprint::default_emulation())
    }

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        let access_token = auth::access_token(ctx.secret)?.to_string();
        let account_id = auth::account_id(ctx.secret).map(str::to_owned);
        let base = ctx
            .provider_settings
            .get("base_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(auth::DEFAULT_BASE_URL);

        // The inbound OpenAiResponses path is provider-relative `/v1/responses`
        // (`/v1/responses/compact` for the compact op); the codex backend drops
        // the `/v1` segment — base already ends in `/backend-api/codex`.
        let path = ctx.path.strip_prefix("/v1").unwrap_or(ctx.path);
        let uri = join_url(base, path, ctx.query)?;

        // Shape the Responses body for the ChatGPT backend (force stream/store,
        // strip sampling fields, lift system messages → instructions).
        let body = auth::normalize_responses_body(&ctx.body);

        // Impersonation channel: it injects its own auth + fingerprint headers
        // and forwards the codex protocol headers a client may set (base
        // allow-list adds content-type / accept).
        let headers = allow_headers(
            ctx.headers,
            &[
                "x-codex-beta-features",
                "x-codex-turn-metadata",
                "x-codex-window-id",
                "thread-id",
                "session-id",
                "x-client-request-id",
            ],
        );
        let mut req = build_request(ctx.method, uri, headers, body)?;
        auth::apply(&mut req, &access_token, account_id.as_deref())?;
        Ok(PreparedRequest::new(req))
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
impl ChannelLogin for CodexChannel {
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
    use bytes::Bytes;
    use http::{HeaderMap, Method};
    use serde_json::json;

    fn prepared_body(body: &'static [u8]) -> Value {
        let secret = json!({ "access_token": "tok-abc" });
        let settings = json!({});
        let headers = HeaderMap::new();
        let ctx = PrepareCtx {
            secret: &secret,
            provider_settings: &settings,
            upstream_model_id: "gpt-5.4",
            method: Method::POST,
            path: "/v1/responses",
            query: None,
            headers: &headers,
            body: Bytes::from_static(body),
        };
        let req = CodexChannel.prepare(ctx).unwrap().request;
        serde_json::from_slice(req.body()).unwrap()
    }

    #[test]
    fn normalizes_responses_body() {
        // String input → forced stream/store, sampling fields dropped, input
        // wrapped as a single user message.
        let v = prepared_body(
            br#"{"model":"gpt-5.4","input":"hi","temperature":0.7,"max_output_tokens":100,"stream":false}"#,
        );
        assert_eq!(v["stream"], json!(true));
        assert_eq!(v["store"], json!(false));
        assert!(v.get("temperature").is_none());
        assert!(v.get("max_output_tokens").is_none());
        assert_eq!(
            v["input"],
            json!([{ "type": "message", "role": "user", "content": "hi" }])
        );

        // System message lifted into instructions; only the user message kept.
        let v = prepared_body(
            br#"{"model":"gpt-5.4","input":[{"role":"system","content":"S"},{"role":"user","content":"U"}]}"#,
        );
        assert_eq!(v["instructions"], json!("S"));
        let roles: Vec<&str> = v["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, vec!["user"]);
    }

    #[test]
    fn prepare_url_and_headers() {
        let secret = json!({ "access_token": "tok-abc", "account_id": "acct-9" });
        let settings = json!({});
        let headers = HeaderMap::new();
        let ctx = PrepareCtx {
            secret: &secret,
            provider_settings: &settings,
            upstream_model_id: "gpt-5.4",
            method: Method::POST,
            path: "/v1/responses",
            query: None,
            headers: &headers,
            body: Bytes::from_static(br#"{"model":"gpt-5.4","input":"hi"}"#),
        };
        let req = CodexChannel.prepare(ctx).unwrap().request;

        assert_eq!(
            req.uri().to_string(),
            "https://chatgpt.com/backend-api/codex/responses"
        );
        assert_eq!(
            req.headers().get("authorization").unwrap(),
            "Bearer tok-abc"
        );
        assert_eq!(req.headers().get("originator").unwrap(), "codex_exec");
        assert_eq!(req.headers().get("chatgpt-account-id").unwrap(), "acct-9");
        // session-id and x-client-request-id share the same generated value.
        assert_eq!(
            req.headers().get("session-id").unwrap(),
            req.headers().get("x-client-request-id").unwrap()
        );
    }

    #[test]
    fn forwards_codex_client_headers() {
        let secret = json!({ "access_token": "tok-abc" });
        let settings = json!({});
        let id = "019ebb45-a25d-7520-a8e3-fda4ebc99692";
        let mut headers = HeaderMap::new();
        headers.insert("session-id", id.parse().unwrap());
        headers.insert("thread-id", id.parse().unwrap());
        headers.insert("x-client-request-id", id.parse().unwrap());
        headers.insert("x-codex-window-id", format!("{id}:0").parse().unwrap());
        headers.insert(
            "x-codex-beta-features",
            "terminal_resize_reflow,memories".parse().unwrap(),
        );
        let ctx = PrepareCtx {
            secret: &secret,
            provider_settings: &settings,
            upstream_model_id: "gpt-5.4",
            method: Method::POST,
            path: "/v1/responses",
            query: None,
            headers: &headers,
            body: Bytes::from_static(br#"{"input":"hi"}"#),
        };
        let req = CodexChannel.prepare(ctx).unwrap().request;
        // A codex-aware client's protocol headers pass through verbatim — gproxy
        // does NOT regenerate them (so they stay consistent with turn-metadata).
        assert_eq!(req.headers().get("session-id").unwrap(), id);
        assert_eq!(req.headers().get("thread-id").unwrap(), id);
        assert_eq!(req.headers().get("x-client-request-id").unwrap(), id);
        assert_eq!(
            req.headers().get("x-codex-window-id").unwrap(),
            &format!("{id}:0")
        );
        assert_eq!(
            req.headers().get("x-codex-beta-features").unwrap(),
            "terminal_resize_reflow,memories"
        );
        // gproxy still owns auth/originator/UA.
        assert_eq!(req.headers().get("originator").unwrap(), "codex_exec");
    }

    #[test]
    fn codex_authcode_start_url() {
        // Empty redirect_uri → codex default; URL carries the PKCE + state set.
        let start = CodexChannel
            .authcode_start("", "STATE", "CHAL")
            .expect("codex supports authcode");
        let url = &start.authorize_url;
        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
        assert!(
            url.contains("client_id=app_EMoamEEZ73f0CkXaXp7hrann"),
            "{url}"
        );
        assert!(url.contains("code_challenge=CHAL"), "{url}");
        assert!(url.contains("state=STATE"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        assert!(url.contains("redirect_uri="), "{url}");
        assert_eq!(start.redirect_uri, "http://localhost:1455/auth/callback");
    }
}
