use base64::Engine;
use bytes::Bytes;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use gproxy_provider_core::credential::ClaudeCodeCredential;
use gproxy_provider_core::{
    AuthRetryAction, ClaudeCodePreludeText, Credential, DispatchRule, DispatchTable, HttpMethod,
    OAuthCallbackRequest, OAuthCallbackResult, OAuthCredential, OAuthStartRequest, Proto,
    ProviderConfig, ProviderError, ProviderResult, Request, UpstreamCtx, UpstreamHttpRequest,
    UpstreamHttpResponse, UpstreamProvider, header_get, header_set,
};

use crate::auth_extractor;
mod cookie;
mod oauth;
mod usage;

const PROVIDER_NAME: &str = "claudecode";
const DEFAULT_API_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_CLAUDE_AI_BASE_URL: &str = "https://claude.ai";
// Usage endpoint should prefer platform host to avoid 302 from console -> platform.
const DEFAULT_PLATFORM_BASE_URL: &str = "https://platform.claude.com";
const DEFAULT_OAUTH_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
const CLAUDE_CODE_UA: &str = "claude-code/2.1.27";
const CLAUDE_CODE_SYSTEM_PRELUDE: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";
const CLAUDE_AGENT_SDK_PRELUDE: &str =
    "You are a Claude agent, built on Anthropic's Claude Agent SDK.";
const TOKEN_UA: &str = "claude-cli/2.1.27 (external, cli)";
const COOKIE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const HEADER_BETA: &str = "anthropic-beta";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const CONTEXT_1M_BETA: &str = "context-1m-2025-08-07";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const OAUTH_SCOPE: &str = "user:profile user:inference user:sessions:claude_code";
const OAUTH_STATE_TTL_SECS: u64 = 600;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default, alias = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(default, alias = "rateLimitTier")]
    rate_limit_tier: Option<String>,
}

#[derive(Debug)]
struct PkceCodes {
    code_verifier: String,
    code_challenge: String,
}

#[derive(Debug, Default)]
pub struct ClaudeCodeProvider;

impl ClaudeCodeProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for ClaudeCodeProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn dispatch_table(&self, _config: &ProviderConfig) -> DispatchTable {
        DispatchTable::new([
            // Claude
            DispatchRule::Native,
            DispatchRule::Native,
            DispatchRule::Native,
            DispatchRule::Native,
            DispatchRule::Native,
            // Gemini
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            // OpenAI chat completions
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            // OpenAI Responses
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            // OpenAI basic ops
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            DispatchRule::Transform {
                target: Proto::Claude,
            },
            // OAuth start/callback + upstream usage are supported.
            DispatchRule::Native,
            DispatchRule::Native,
            DispatchRule::Native,
        ])
    }

    async fn build_claude_messages(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::claude::create_message::request::CreateMessageRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = claudecode_api_base_url(config)?;
        let access_token = claudecode_access_token(config, credential)?;
        let system_prelude = claudecode_system_prelude(config)?;
        let url = build_url(Some(base_url), DEFAULT_API_BASE_URL, "/v1/messages");
        let mut body_obj = req.body.clone();
        apply_claude_code_system(
            &mut body_obj.system,
            ctx.user_agent.as_deref(),
            system_prelude,
        );
        let model = model_to_string(&body_obj.model);
        normalize_claude_code_sampling(model.as_deref(), body_obj.temperature, &mut body_obj.top_p);
        let is_stream = body_obj.stream.unwrap_or(false);
        let body =
            serde_json::to_vec(&body_obj).map_err(|err| ProviderError::Other(err.to_string()))?;
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, &access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_content_type_json(&mut headers);
        auth_extractor::set_user_agent(&mut headers, CLAUDE_CODE_UA);
        apply_anthropic_headers(&mut headers, &req.headers)?;
        let use_context_1m = should_use_context_1m(credential, model.as_deref());
        ensure_oauth_beta(&mut headers, use_context_1m);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(Bytes::from(body)),
            is_stream,
        })
    }

    async fn build_claude_count_tokens(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::claude::count_tokens::request::CountTokensRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = claudecode_api_base_url(config)?;
        let access_token = claudecode_access_token(config, credential)?;
        let system_prelude = claudecode_system_prelude(config)?;
        let url = build_url(
            Some(base_url),
            DEFAULT_API_BASE_URL,
            "/v1/messages/count_tokens",
        );
        let mut body_obj = req.body.clone();
        apply_claude_code_system(
            &mut body_obj.system,
            ctx.user_agent.as_deref(),
            system_prelude,
        );
        let model = model_to_string(&body_obj.model);
        let body =
            serde_json::to_vec(&body_obj).map_err(|err| ProviderError::Other(err.to_string()))?;
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, &access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_content_type_json(&mut headers);
        auth_extractor::set_user_agent(&mut headers, CLAUDE_CODE_UA);
        apply_anthropic_headers(&mut headers, &req.headers)?;
        let use_context_1m = should_use_context_1m(credential, model.as_deref());
        ensure_oauth_beta(&mut headers, use_context_1m);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(Bytes::from(body)),
            is_stream: false,
        })
    }

    async fn build_claude_models_list(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::claude::list_models::request::ListModelsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = claudecode_api_base_url(config)?;
        let access_token = claudecode_access_token(config, credential)?;
        let mut url = build_url(Some(base_url), DEFAULT_API_BASE_URL, "/v1/models");
        let query = build_claude_models_list_query(&req.query);
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, &access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_user_agent(&mut headers, CLAUDE_CODE_UA);
        apply_anthropic_headers(&mut headers, &req.headers)?;
        ensure_oauth_beta(&mut headers, false);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Get,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    async fn build_claude_models_get(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::claude::get_model::request::GetModelRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = claudecode_api_base_url(config)?;
        let access_token = claudecode_access_token(config, credential)?;
        let url = build_url(
            Some(base_url),
            DEFAULT_API_BASE_URL,
            &format!("/v1/models/{}", req.path.model_id),
        );
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, &access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_user_agent(&mut headers, CLAUDE_CODE_UA);
        apply_anthropic_headers(&mut headers, &req.headers)?;
        ensure_oauth_beta(&mut headers, false);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Get,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    fn oauth_start(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        req: &OAuthStartRequest,
    ) -> ProviderResult<UpstreamHttpResponse> {
        oauth::oauth_start(ctx, config, req)
    }

    fn oauth_callback(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        req: &OAuthCallbackRequest,
    ) -> ProviderResult<OAuthCallbackResult> {
        oauth::oauth_callback(ctx, config, req)
    }

    fn on_auth_failure<'a>(
        &'a self,
        ctx: &'a UpstreamCtx,
        config: &'a ProviderConfig,
        credential: &'a Credential,
        req: &'a Request,
        failure: &'a gproxy_provider_core::provider::UpstreamFailure,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ProviderResult<AuthRetryAction>> + Send + 'a>,
    > {
        oauth::on_auth_failure(ctx, config, credential, req, failure)
    }

    fn upgrade_credential<'a>(
        &'a self,
        _ctx: &'a UpstreamCtx,
        config: &'a ProviderConfig,
        credential: &'a Credential,
        _req: &'a Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ProviderResult<Option<Credential>>> + Send + 'a>,
    > {
        Box::pin(async move {
            match credential {
                Credential::ClaudeCode(secret) => {
                    let mut candidate: Option<Credential> = None;
                    if let Some(session_key) = secret.session_key.as_deref() {
                        let tokens = cookie::ensure_session_tokens_full(config, session_key)?;
                        let mut updated = secret.clone();
                        updated.access_token = tokens.access_token;
                        updated.refresh_token = tokens.refresh_token;
                        updated.expires_at = tokens.expires_at.unwrap_or(updated.expires_at);
                        if let Some(subscription_type) = tokens.subscription_type {
                            updated.subscription_type = subscription_type;
                        }
                        if let Some(rate_limit_tier) = tokens.rate_limit_tier {
                            updated.rate_limit_tier = rate_limit_tier;
                        }
                        if updated.session_key.is_none() {
                            updated.session_key = Some(session_key.to_string());
                        }
                        candidate = Some(Credential::ClaudeCode(updated));
                    }

                    let enrich_base = candidate.as_ref().unwrap_or(credential);
                    if let Some(enriched) =
                        oauth::enrich_credential_profile_if_missing(config, enrich_base).await?
                    {
                        return Ok(Some(enriched));
                    }

                    Ok(candidate)
                }
                _ => Ok(None),
            }
        })
    }

    fn on_upstream_failure<'a>(
        &'a self,
        _ctx: &'a UpstreamCtx,
        _config: &'a ProviderConfig,
        credential: &'a Credential,
        req: &'a Request,
        failure: &'a gproxy_provider_core::provider::UpstreamFailure,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ProviderResult<AuthRetryAction>> + Send + 'a>,
    > {
        Box::pin(async move {
            let model = request_model_for_1m(req);
            if !should_use_context_1m(credential, model.as_deref()) {
                return Ok(AuthRetryAction::None);
            }
            if !is_1m_forbidden_response(failure) {
                return Ok(AuthRetryAction::None);
            }
            let Some(family) = one_m_family_for_model(model.as_deref()) else {
                return Ok(AuthRetryAction::None);
            };
            let updated = claude_code_meta_set_supports_1m(credential, family, false);
            Ok(AuthRetryAction::UpdateCredential(Box::new(updated)))
        })
    }

    fn on_upstream_success<'a>(
        &'a self,
        _ctx: &'a UpstreamCtx,
        _config: &'a ProviderConfig,
        credential: &'a Credential,
        req: &'a Request,
        _response: &'a UpstreamHttpResponse,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ProviderResult<Option<Credential>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let model = request_model_for_1m(req);
            if !should_use_context_1m(credential, model.as_deref()) {
                return Ok(None);
            }
            let Some(family) = one_m_family_for_model(model.as_deref()) else {
                return Ok(None);
            };
            if claude_code_meta_get_supports_1m(credential, family).is_some() {
                return Ok(None);
            }
            let updated = claude_code_meta_set_supports_1m(credential, family, true);
            Ok(Some(updated))
        })
    }

    async fn build_upstream_usage(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
    ) -> ProviderResult<UpstreamHttpRequest> {
        usage::build_upstream_usage(ctx, config, credential)
    }
}

fn claudecode_api_base_url(config: &ProviderConfig) -> ProviderResult<&str> {
    match config {
        ProviderConfig::ClaudeCode(cfg) => {
            Ok(cfg.base_url.as_deref().unwrap_or(DEFAULT_API_BASE_URL))
        }
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::ClaudeCode".to_string(),
        )),
    }
}

fn claudecode_ai_base_url(config: &ProviderConfig) -> ProviderResult<&str> {
    match config {
        ProviderConfig::ClaudeCode(cfg) => Ok(cfg
            .claude_ai_base_url
            .as_deref()
            .unwrap_or(DEFAULT_CLAUDE_AI_BASE_URL)),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::ClaudeCode".to_string(),
        )),
    }
}

fn claudecode_platform_base_url(config: &ProviderConfig) -> ProviderResult<&str> {
    match config {
        ProviderConfig::ClaudeCode(cfg) => Ok(cfg
            .platform_base_url
            .as_deref()
            .unwrap_or(DEFAULT_PLATFORM_BASE_URL)),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::ClaudeCode".to_string(),
        )),
    }
}

fn claudecode_system_prelude(config: &ProviderConfig) -> ProviderResult<&'static str> {
    match config {
        ProviderConfig::ClaudeCode(cfg) => Ok(match cfg.prelude_text.unwrap_or_default() {
            ClaudeCodePreludeText::ClaudeCodeSystem => CLAUDE_CODE_SYSTEM_PRELUDE,
            ClaudeCodePreludeText::ClaudeAgentSdk => CLAUDE_AGENT_SDK_PRELUDE,
        }),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::ClaudeCode".to_string(),
        )),
    }
}

pub(super) fn claudecode_oauth_redirect_uri(config: &ProviderConfig) -> ProviderResult<String> {
    match config {
        ProviderConfig::ClaudeCode(_) => Ok(DEFAULT_OAUTH_REDIRECT_URI.to_string()),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::ClaudeCode".to_string(),
        )),
    }
}

fn claudecode_access_token(
    _config: &ProviderConfig,
    credential: &Credential,
) -> ProviderResult<String> {
    match credential {
        Credential::ClaudeCode(secret) => {
            if secret.access_token.is_empty() {
                Err(ProviderError::MissingCredentialField("access_token"))
            } else {
                Ok(secret.access_token.clone())
            }
        }
        _ => Err(ProviderError::InvalidConfig(
            "expected Credential::ClaudeCode".to_string(),
        )),
    }
}

fn build_url(base_url: Option<&str>, default_base: &str, path: &str) -> String {
    let base = base_url.unwrap_or(default_base).trim_end_matches('/');
    let mut path = path.trim_start_matches('/');
    if base.ends_with("/v1") && (path == "v1" || path.starts_with("v1/")) {
        path = path.trim_start_matches("v1/").trim_start_matches("v1");
    }
    format!("{base}/{path}")
}

fn build_claude_models_list_query(query: &gproxy_protocol::claude::ListModelsQuery) -> String {
    let mut parts = Vec::new();
    if let Some(limit) = query.limit {
        parts.push(format!("limit={limit}"));
    }
    if let Some(before_id) = query.before_id.as_ref() {
        parts.push(format!("before_id={}", urlencoding::encode(before_id)));
    }
    if let Some(after_id) = query.after_id.as_ref() {
        parts.push(format!("after_id={}", urlencoding::encode(after_id)));
    }
    parts.join("&")
}

fn apply_anthropic_headers(
    headers: &mut gproxy_provider_core::Headers,
    anthropic_headers: &impl Serialize,
) -> ProviderResult<()> {
    let value = serde_json::to_value(anthropic_headers)
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let map = value
        .as_object()
        .ok_or_else(|| ProviderError::Other("unexpected anthropic headers shape".to_string()))?;

    if let Some(version) = map
        .get("anthropic-version")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        auth_extractor::set_header(headers, "anthropic-version", version);
    }
    if let Some(beta) = map.get("anthropic-beta") {
        let s = match beta {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(items) => {
                let mut out = Vec::new();
                for item in items {
                    if let Some(s) = item.as_str() {
                        out.push(s.to_string());
                    }
                }
                if out.is_empty() {
                    None
                } else {
                    Some(out.join(","))
                }
            }
            _ => None,
        };
        if let Some(s) = s {
            auth_extractor::set_header(headers, "anthropic-beta", &s);
        }
    }
    Ok(())
}

fn ensure_oauth_beta(headers: &mut gproxy_provider_core::Headers, use_context_1m: bool) {
    let mut values: Vec<String> = header_get(headers, HEADER_BETA)
        .map(|value| {
            value
                .split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !values.iter().any(|v| v.eq_ignore_ascii_case(OAUTH_BETA)) {
        values.push(OAUTH_BETA.to_string());
    }
    if use_context_1m
        && !values
            .iter()
            .any(|v| v.eq_ignore_ascii_case(CONTEXT_1M_BETA))
    {
        values.push(CONTEXT_1M_BETA.to_string());
    }
    header_set(headers, HEADER_BETA, values.join(","));
}

fn json_response(body: serde_json::Value) -> UpstreamHttpResponse {
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    UpstreamHttpResponse {
        status: 200,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: gproxy_provider_core::provider::UpstreamBody::Bytes(Bytes::from(bytes)),
    }
}

fn json_error(status: u16, message: &str) -> UpstreamHttpResponse {
    let body = serde_json::json!({ "error": message });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    UpstreamHttpResponse {
        status,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: gproxy_provider_core::provider::UpstreamBody::Bytes(Bytes::from(bytes)),
    }
}

fn generate_state_and_pkce() -> (String, PkceCodes) {
    let mut state_bytes = [0u8; 24];
    rand::rng().fill_bytes(&mut state_bytes);
    let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state_bytes);

    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (
        state,
        PkceCodes {
            code_verifier,
            code_challenge,
        },
    )
}

fn chrono_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gproxy_provider_core::provider::UpstreamFailure;

    fn oauth_cred(secret: ClaudeCodeCredential) -> Credential {
        Credential::ClaudeCode(secret)
    }

    fn default_secret() -> ClaudeCodeCredential {
        ClaudeCodeCredential {
            access_token: "tok".to_string(),
            refresh_token: "rtok".to_string(),
            expires_at: 0,
            enable_claude_1m_sonnet: None,
            enable_claude_1m_opus: None,
            supports_claude_1m_sonnet: None,
            supports_claude_1m_opus: None,
            subscription_type: String::new(),
            rate_limit_tier: String::new(),
            user_email: None,
            session_key: None,
        }
    }

    #[test]
    fn enable_1m_defaults_true() {
        let cred = oauth_cred(default_secret());
        assert!(claude_code_meta_get_enable_1m(&cred, OneMFamily::Sonnet));
        assert!(claude_code_meta_get_enable_1m(&cred, OneMFamily::Opus));
    }

    #[test]
    fn enable_false_disables_context_1m_even_if_supported() {
        let mut secret = default_secret();
        secret.enable_claude_1m_sonnet = Some(false);
        secret.supports_claude_1m_sonnet = Some(true);
        let cred = oauth_cred(secret);
        assert!(!should_use_context_1m(&cred, Some("claude-sonnet-4-5")));
    }

    #[test]
    fn supports_true_and_enable_true_uses_context_1m() {
        let mut secret = default_secret();
        secret.enable_claude_1m_opus = Some(true);
        secret.supports_claude_1m_opus = Some(true);
        let cred = oauth_cred(secret);
        assert!(should_use_context_1m(&cred, Some("claude-opus-4-6")));
    }

    #[test]
    fn forbidden_response_detected() {
        let failure = UpstreamFailure::Http {
            status: 403,
            headers: vec![],
            body: Bytes::from_static(
                b"feature context-1m-2025-08-07 is not available for this account",
            ),
        };
        assert!(is_1m_forbidden_response(&failure));
    }

    #[test]
    fn forbidden_response_detected_not_yet_available_long_context_beta() {
        let failure = UpstreamFailure::Http {
            status: 400,
            headers: vec![],
            body: Bytes::from_static(
                b"The long context beta is not yet available for this subscription.",
            ),
        };
        assert!(is_1m_forbidden_response(&failure));
    }

    #[test]
    fn forbidden_response_detected_incompatible_long_context_beta_header() {
        let failure = UpstreamFailure::Http {
            status: 400,
            headers: vec![],
            body: Bytes::from_static(
                b"This authentication style is incompatible with the long context beta header.",
            ),
        };
        assert!(is_1m_forbidden_response(&failure));
    }

    #[test]
    fn token_profile_written_to_fields() {
        let mut secret = default_secret();
        secret.enable_claude_1m_sonnet = Some(true);
        apply_token_profile_to_credential(
            &mut secret,
            Some(&"max".to_string()),
            Some(&"default_claude_max_5x".to_string()),
        );
        assert_eq!(secret.subscription_type, "max");
        assert_eq!(secret.rate_limit_tier, "default_claude_max_5x");
        assert_eq!(secret.enable_claude_1m_sonnet, Some(true));
    }

    #[test]
    fn apply_claude_code_system_injects_default_prelude_for_non_cc_ua() {
        let mut system = None;
        apply_claude_code_system(&mut system, Some("curl/8.6.0"), CLAUDE_CODE_SYSTEM_PRELUDE);
        let Some(gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Blocks(blocks)) =
            system
        else {
            panic!("expected blocks system");
        };
        assert_eq!(
            blocks.first().map(|b| b.text.as_str()),
            Some(CLAUDE_CODE_SYSTEM_PRELUDE)
        );
    }

    #[test]
    fn apply_claude_code_system_skips_for_claude_code_ua() {
        let mut system = None;
        apply_claude_code_system(
            &mut system,
            Some("claude-code/2.1.27"),
            CLAUDE_CODE_SYSTEM_PRELUDE,
        );
        assert!(system.is_none());
    }

    #[test]
    fn apply_claude_code_system_does_not_duplicate_existing_known_prelude() {
        let mut system = Some(
            gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Text(
                CLAUDE_AGENT_SDK_PRELUDE.to_string(),
            ),
        );
        apply_claude_code_system(&mut system, Some("curl/8.6.0"), CLAUDE_CODE_SYSTEM_PRELUDE);
        let Some(gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Text(text)) =
            system
        else {
            panic!("expected text system");
        };
        assert_eq!(text, CLAUDE_AGENT_SDK_PRELUDE);
    }

    #[test]
    fn apply_claude_code_system_injects_agent_sdk_prelude_when_selected() {
        let mut system = None;
        apply_claude_code_system(&mut system, Some("curl/8.6.0"), CLAUDE_AGENT_SDK_PRELUDE);
        let Some(gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Blocks(blocks)) =
            system
        else {
            panic!("expected blocks system");
        };
        assert_eq!(
            blocks.first().map(|b| b.text.as_str()),
            Some(CLAUDE_AGENT_SDK_PRELUDE)
        );
    }

    #[test]
    fn claudecode_system_prelude_uses_config_option() {
        let cfg = ProviderConfig::ClaudeCode(gproxy_provider_core::config::ClaudeCodeConfig {
            prelude_text: Some(ClaudeCodePreludeText::ClaudeAgentSdk),
            ..Default::default()
        });
        assert_eq!(
            claudecode_system_prelude(&cfg).unwrap(),
            CLAUDE_AGENT_SDK_PRELUDE
        );
    }

    #[test]
    fn normalize_claude_code_sampling_clears_top_p_for_supported_models() {
        let mut top_p = Some(0.95);
        normalize_claude_code_sampling(Some("claude-opus-4-6"), Some(0.7), &mut top_p);
        assert_eq!(top_p, None);
    }

    #[test]
    fn normalize_claude_code_sampling_keeps_top_p_when_temperature_missing() {
        let mut top_p = Some(0.95);
        normalize_claude_code_sampling(Some("claude-opus-4-6"), None, &mut top_p);
        assert_eq!(top_p, Some(0.95));
    }

    #[test]
    fn normalize_claude_code_sampling_keeps_top_p_for_other_models() {
        let mut top_p = Some(0.95);
        normalize_claude_code_sampling(Some("claude-haiku-4-5"), Some(0.7), &mut top_p);
        assert_eq!(top_p, Some(0.95));
    }
}

fn request_model_for_1m(req: &Request) -> Option<String> {
    match req {
        Request::GenerateContent(gproxy_provider_core::GenerateContentRequest::Claude(r)) => {
            model_to_string(&r.body.model)
        }
        Request::CountTokens(gproxy_provider_core::CountTokensRequest::Claude(r)) => {
            model_to_string(&r.body.model)
        }
        _ => None,
    }
}

fn model_to_string(model: &gproxy_protocol::claude::count_tokens::types::Model) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn requires_claude_code_sampling_guard(model: Option<&str>) -> bool {
    let model = model.unwrap_or_default().to_ascii_lowercase();
    model.contains("opus-4-1")
        || model.contains("opus-4-5")
        || model.contains("opus-4-6")
        || model.contains("sonnet-4-5")
}

fn normalize_claude_code_sampling(
    model: Option<&str>,
    temperature: Option<f64>,
    top_p: &mut Option<f64>,
) {
    if temperature.is_some() && top_p.is_some() && requires_claude_code_sampling_guard(model) {
        *top_p = None;
    }
}

fn is_claude_code_user_agent(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.contains("claude-code") || lowered.contains("claude-cli")
}

fn is_known_claude_code_system_text(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains(&CLAUDE_CODE_SYSTEM_PRELUDE.to_ascii_lowercase())
        || lowered.contains(&CLAUDE_AGENT_SDK_PRELUDE.to_ascii_lowercase())
}

fn has_known_claude_code_prelude(
    system: &Option<gproxy_protocol::claude::count_tokens::types::BetaSystemParam>,
) -> bool {
    match system {
        Some(gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Text(text)) => {
            is_known_claude_code_system_text(text)
        }
        Some(gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Blocks(blocks)) => {
            blocks
                .iter()
                .any(|block| is_known_claude_code_system_text(&block.text))
        }
        None => false,
    }
}

fn apply_claude_code_system(
    system: &mut Option<gproxy_protocol::claude::count_tokens::types::BetaSystemParam>,
    user_agent: Option<&str>,
    prelude_text: &str,
) {
    if user_agent.map(is_claude_code_user_agent).unwrap_or(false) {
        return;
    }
    if has_known_claude_code_prelude(system) {
        return;
    }

    let prelude = gproxy_protocol::claude::count_tokens::types::BetaTextBlockParam {
        text: prelude_text.to_string(),
        r#type: gproxy_protocol::claude::count_tokens::types::BetaTextBlockType::Text,
        cache_control: None,
        citations: None,
    };

    *system = Some(match system.take() {
        Some(gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Text(text)) => {
            gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Blocks(vec![
                prelude,
                gproxy_protocol::claude::count_tokens::types::BetaTextBlockParam {
                    text,
                    r#type: gproxy_protocol::claude::count_tokens::types::BetaTextBlockType::Text,
                    cache_control: None,
                    citations: None,
                },
            ])
        }
        Some(gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Blocks(mut blocks)) => {
            blocks.insert(0, prelude);
            gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Blocks(blocks)
        }
        None => {
            gproxy_protocol::claude::count_tokens::types::BetaSystemParam::Blocks(vec![prelude])
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OneMFamily {
    Sonnet,
    Opus,
}

fn one_m_family_for_model(model: Option<&str>) -> Option<OneMFamily> {
    let model = model?.to_ascii_lowercase();
    if model.starts_with("claude-sonnet-4") {
        Some(OneMFamily::Sonnet)
    } else if model.starts_with("claude-opus-4-6") {
        Some(OneMFamily::Opus)
    } else {
        None
    }
}

fn should_use_context_1m(credential: &Credential, model: Option<&str>) -> bool {
    let Some(family) = one_m_family_for_model(model) else {
        return false;
    };
    if !claude_code_meta_get_enable_1m(credential, family) {
        return false;
    }
    !matches!(
        claude_code_meta_get_supports_1m(credential, family),
        Some(false)
    )
}

fn is_1m_forbidden_response(failure: &gproxy_provider_core::provider::UpstreamFailure) -> bool {
    let gproxy_provider_core::provider::UpstreamFailure::Http { status, body, .. } = failure else {
        return false;
    };
    if *status != 400 && *status != 403 {
        return false;
    }
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    let needles = [
        "context-1m",
        "context 1m",
        "1m context",
        "long context beta",
        "not enabled",
        "not available",
        "not yet available",
        "incompatible",
        "forbidden",
    ];
    needles.iter().any(|needle| text.contains(needle))
}

fn claudecode_secret(credential: &Credential) -> Option<&ClaudeCodeCredential> {
    match credential {
        Credential::ClaudeCode(secret) => Some(secret),
        _ => None,
    }
}

fn claude_code_meta_get_enable_1m(credential: &Credential, family: OneMFamily) -> bool {
    let Some(secret) = claudecode_secret(credential) else {
        return true;
    };
    match family {
        OneMFamily::Sonnet => secret.enable_claude_1m_sonnet.unwrap_or(true),
        OneMFamily::Opus => secret.enable_claude_1m_opus.unwrap_or(true),
    }
}

fn claude_code_meta_get_supports_1m(credential: &Credential, family: OneMFamily) -> Option<bool> {
    let secret = claudecode_secret(credential)?;
    match family {
        OneMFamily::Sonnet => secret.supports_claude_1m_sonnet,
        OneMFamily::Opus => secret.supports_claude_1m_opus,
    }
}

fn claude_code_meta_set_supports_1m(
    credential: &Credential,
    family: OneMFamily,
    value: bool,
) -> Credential {
    match credential {
        Credential::ClaudeCode(secret) => {
            let mut updated = secret.clone();
            match family {
                OneMFamily::Sonnet => updated.supports_claude_1m_sonnet = Some(value),
                OneMFamily::Opus => updated.supports_claude_1m_opus = Some(value),
            }
            Credential::ClaudeCode(updated)
        }
        _ => credential.clone(),
    }
}

fn apply_token_profile_from_token_response(
    secret: &mut ClaudeCodeCredential,
    tokens: &TokenResponse,
) {
    apply_token_profile_to_credential(
        secret,
        tokens.subscription_type.as_ref(),
        tokens.rate_limit_tier.as_ref(),
    );
}

fn apply_token_profile_from_cached_tokens(
    secret: &mut ClaudeCodeCredential,
    tokens: &cookie::CachedTokens,
) {
    apply_token_profile_to_credential(
        secret,
        tokens.subscription_type.as_ref(),
        tokens.rate_limit_tier.as_ref(),
    );
}

fn apply_token_profile_to_credential(
    secret: &mut ClaudeCodeCredential,
    subscription_type: Option<&String>,
    rate_limit_tier: Option<&String>,
) {
    if let Some(v) = subscription_type {
        secret.subscription_type = v.clone();
    }
    if let Some(v) = rate_limit_tier {
        secret.rate_limit_tier = v.clone();
    }
}
