use std::sync::OnceLock;
use std::time::Duration;

use bytes::Bytes;
use rand::RngCore;
use serde::Deserialize;
use serde_json::Value as JsonValue;

use gproxy_provider_core::{
    AuthRetryAction, Credential, DispatchRule, DispatchTable, HttpMethod, ModelGetRequest,
    ModelListRequest, OAuthCallbackRequest, OAuthCallbackResult, OAuthCredential,
    OAuthStartRequest, Proto, ProviderConfig, ProviderError, ProviderResult, Request, UpstreamBody,
    UpstreamCtx, UpstreamHttpRequest, UpstreamHttpResponse, UpstreamProvider, header_set,
};

use gproxy_protocol::gemini;

use crate::auth_extractor;
use crate::providers::http_client::{SharedClientKind, client_for_ctx};
mod oauth;
mod usage;

const PROVIDER_NAME: &str = "geminicli";
const DEFAULT_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";
const GEMINICLI_USER_AGENT: &str = "GeminiCLI/0.1.5 (Windows; AMD64)";

const DEFAULT_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const DEFAULT_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CLIENT_ID: &str = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "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";
const OAUTH_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";
const OAUTH_STATE_TTL_SECS: u64 = 600;
const MODELS_JSON: &str = include_str!("models.json");

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

// Mirrors `samples/crates/gproxy-provider-impl/src/provider/geminicli/mod.rs` dispatch semantics.
const DISPATCH_TABLE: DispatchTable = DispatchTable::new([
    // Claude
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    // Gemini
    DispatchRule::Native,
    DispatchRule::Native,
    DispatchRule::Native,
    DispatchRule::Native,
    DispatchRule::Native,
    // OpenAI chat completions (transform to Gemini)
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    // OpenAI Responses
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    // OpenAI basic ops
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    DispatchRule::Transform {
        target: Proto::Gemini,
    },
    // OAuth start/callback are supported by GeminiCli.
    DispatchRule::Native,
    DispatchRule::Native,
    // Upstream usage (retrieveUserQuota)
    DispatchRule::Native,
]);

#[derive(Debug, Default)]
pub struct GeminiCliProvider;

impl GeminiCliProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for GeminiCliProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn dispatch_table(&self, _config: &ProviderConfig) -> DispatchTable {
        DISPATCH_TABLE
    }

    async fn build_gemini_generate(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gemini::generate_content::request::GenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let project_id = geminicli_project_id(credential)?;
        let model = normalize_model_name(&req.path.model);
        let user_prompt_id = generate_user_prompt_id();
        let wrapped = wrap_internal_request(&model, project_id, &user_prompt_id, &req.body);
        build_gemini_request(
            config,
            credential,
            "/v1internal:generateContent",
            &wrapped,
            false,
        )
    }

    async fn build_gemini_generate_stream(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gemini::stream_content::request::StreamGenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let project_id = geminicli_project_id(credential)?;
        let model = normalize_model_name(&req.path.model);
        let user_prompt_id = generate_user_prompt_id();
        let wrapped = wrap_internal_request(&model, project_id, &user_prompt_id, &req.body);
        build_gemini_request(
            config,
            credential,
            "/v1internal:streamGenerateContent?alt=sse",
            &wrapped,
            true,
        )
    }

    async fn build_gemini_count_tokens(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gemini::count_tokens::request::CountTokensRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let model = normalize_model_name(&req.path.model);
        let mut request_obj = serde_json::Map::new();
        request_obj.insert(
            "model".to_string(),
            JsonValue::String(format!("models/{model}")),
        );
        if let Some(contents) = &req.body.contents {
            let contents_value = serde_json::to_value(contents)
                .map_err(|err| ProviderError::Other(err.to_string()))?;
            request_obj.insert("contents".to_string(), contents_value);
        } else if let Some(contents_value) = req
            .body
            .generate_content_request
            .as_ref()
            .and_then(|value| value.get("contents"))
            .cloned()
        {
            request_obj.insert("contents".to_string(), contents_value);
        }
        let wrapped = serde_json::json!({
            "request": JsonValue::Object(request_obj),
        });
        build_gemini_request(
            config,
            credential,
            "/v1internal:countTokens",
            &wrapped,
            false,
        )
    }

    fn local_response(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        req: &Request,
    ) -> ProviderResult<Option<UpstreamHttpResponse>> {
        match req {
            Request::ModelList(ModelListRequest::Gemini(_)) => {
                let list = load_models_value()?;
                let body = serde_json::to_vec(list)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            Request::ModelGet(ModelGetRequest::Gemini(req)) => {
                let list = load_models_value()?;
                let name = normalize_model_name(&req.path.name);
                let (status, body_json) = match find_model_value(list, &name) {
                    Some(model) => (200, model),
                    None => (
                        404,
                        serde_json::json!({ "error": { "message": "model not found" } }),
                    ),
                };
                let body = serde_json::to_vec(&body_json)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(status, body)))
            }
            _ => Ok(None),
        }
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

    fn upgrade_credential<'a>(
        &'a self,
        ctx: &'a UpstreamCtx,
        config: &'a ProviderConfig,
        credential: &'a Credential,
        _req: &'a Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ProviderResult<Option<Credential>>> + Send + 'a>,
    > {
        Box::pin(async move {
            oauth::enrich_credential_profile_if_missing(ctx, config, credential).await
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

    fn on_upstream_failure<'a>(
        &'a self,
        ctx: &'a UpstreamCtx,
        config: &'a ProviderConfig,
        credential: &'a Credential,
        _req: &'a Request,
        failure: &'a gproxy_provider_core::provider::UpstreamFailure,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ProviderResult<AuthRetryAction>> + Send + 'a>,
    > {
        Box::pin(async move {
            let gproxy_provider_core::provider::UpstreamFailure::Http { status, .. } = failure
            else {
                return Ok(AuthRetryAction::None);
            };
            if *status != 404 {
                return Ok(AuthRetryAction::None);
            }
            let Credential::GeminiCli(cred) = credential else {
                return Ok(AuthRetryAction::None);
            };
            let base_url = geminicli_base_url(config)?;
            let detected = match detect_project_id(ctx, &cred.access_token, base_url) {
                Ok(Some(project_id)) if !project_id.trim().is_empty() => Some(project_id),
                _ => None,
            };
            let Some(project_id) = detected else {
                return Ok(AuthRetryAction::None);
            };
            if project_id == cred.project_id {
                return Ok(AuthRetryAction::None);
            }
            let mut updated = credential.clone();
            if let Credential::GeminiCli(cred_mut) = &mut updated {
                cred_mut.project_id = project_id;
            }
            Ok(AuthRetryAction::UpdateCredential(Box::new(updated)))
        })
    }
}

fn geminicli_base_url(config: &ProviderConfig) -> ProviderResult<&str> {
    match config {
        ProviderConfig::GeminiCli(cfg) => Ok(cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::GeminiCli".to_string(),
        )),
    }
}

fn geminicli_access_token(credential: &Credential) -> ProviderResult<&str> {
    match credential {
        Credential::GeminiCli(cred) => Ok(cred.access_token.as_str()),
        _ => Err(ProviderError::InvalidConfig(
            "expected Credential::GeminiCli".to_string(),
        )),
    }
}

fn build_gemini_request<T: serde::Serialize>(
    config: &ProviderConfig,
    credential: &Credential,
    path: &str,
    body: &T,
    is_stream: bool,
) -> ProviderResult<UpstreamHttpRequest> {
    let base_url = geminicli_base_url(config)?;
    let access_token = geminicli_access_token(credential)?;
    let url = build_url(Some(base_url), DEFAULT_BASE_URL, path);
    let body = serde_json::to_vec(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let mut headers = Vec::new();
    auth_extractor::set_bearer(&mut headers, access_token);
    auth_extractor::set_accept_json(&mut headers);
    auth_extractor::set_content_type_json(&mut headers);
    auth_extractor::set_user_agent(&mut headers, GEMINICLI_USER_AGENT);
    auth_extractor::set_header(&mut headers, "Accept-Encoding", "gzip");
    Ok(UpstreamHttpRequest {
        method: HttpMethod::Post,
        url,
        headers,
        body: Some(Bytes::from(body)),
        is_stream,
    })
}

fn build_url(base_url: Option<&str>, default_base: &str, path: &str) -> String {
    let base = base_url.unwrap_or(default_base).trim_end_matches('/');
    let mut path = path.trim_start_matches('/');
    if base.ends_with("/v1") && (path == "v1" || path.starts_with("v1/")) {
        path = path.trim_start_matches("v1/").trim_start_matches("v1");
    }
    if base.ends_with("/v1beta1") && (path == "v1beta1" || path.starts_with("v1beta1/")) {
        path = path
            .trim_start_matches("v1beta1/")
            .trim_start_matches("v1beta1");
    }
    format!("{base}/{path}")
}

fn normalize_model_name(model: &str) -> String {
    model.strip_prefix("models/").unwrap_or(model).to_string()
}

static MODELS_CACHE: OnceLock<JsonValue> = OnceLock::new();

fn load_models_value() -> ProviderResult<&'static JsonValue> {
    if let Some(value) = MODELS_CACHE.get() {
        return Ok(value);
    }
    let parsed: JsonValue =
        serde_json::from_str(MODELS_JSON).map_err(|err| ProviderError::Other(err.to_string()))?;
    if parsed.get("models").and_then(|v| v.as_array()).is_none() {
        return Err(ProviderError::Other(
            "models.json missing models array".to_string(),
        ));
    }
    let _ = MODELS_CACHE.set(parsed);
    Ok(MODELS_CACHE.get().expect("models cache"))
}

fn find_model_value(list: &JsonValue, target: &str) -> Option<JsonValue> {
    let models = list.get("models")?.as_array()?;
    models
        .iter()
        .find(|item| {
            item.get("name")
                .and_then(|value| value.as_str())
                .map(|name| normalize_model_name(name) == target)
                .unwrap_or(false)
        })
        .cloned()
}

fn local_json_response(status: u16, body: Vec<u8>) -> UpstreamHttpResponse {
    let mut headers = Vec::new();
    header_set(&mut headers, "content-type", "application/json");
    UpstreamHttpResponse {
        status,
        headers,
        body: UpstreamBody::Bytes(Bytes::from(body)),
    }
}

fn geminicli_project_id(credential: &Credential) -> ProviderResult<&str> {
    match credential {
        Credential::GeminiCli(cred) => {
            if cred.project_id.trim().is_empty() {
                Err(ProviderError::InvalidConfig(
                    "missing project_id".to_string(),
                ))
            } else {
                Ok(cred.project_id.as_str())
            }
        }
        _ => Err(ProviderError::InvalidConfig(
            "expected Credential::GeminiCli".to_string(),
        )),
    }
}

fn generate_user_prompt_id() -> String {
    let mut bytes = [0u8; 16];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn wrap_internal_request(
    model: &str,
    project_id: &str,
    user_prompt_id: &str,
    request: &gemini::generate_content::request::GenerateContentRequestBody,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "project": project_id,
        "user_prompt_id": user_prompt_id,
        "request": request,
    })
}

fn json_response(body: serde_json::Value) -> UpstreamHttpResponse {
    let mut headers = Vec::new();
    header_set(&mut headers, "content-type", "application/json");
    let bytes = Bytes::from(serde_json::to_vec(&body).unwrap_or_default());
    UpstreamHttpResponse {
        status: 200,
        headers,
        body: UpstreamBody::Bytes(bytes),
    }
}

fn json_error(status: u16, message: &str) -> UpstreamHttpResponse {
    let mut headers = Vec::new();
    header_set(&mut headers, "content-type", "application/json");
    let bytes = Bytes::from(
        serde_json::to_vec(&serde_json::json!({ "error": message })).unwrap_or_default(),
    );
    UpstreamHttpResponse {
        status,
        headers,
        body: UpstreamBody::Bytes(bytes),
    }
}

fn detect_project_id(
    ctx: &UpstreamCtx,
    access_token: &str,
    base_url: &str,
) -> ProviderResult<Option<String>> {
    crate::providers::oauth_common::block_on(async move {
        if let Ok(Some(project_id)) =
            try_load_code_assist(ctx, access_token, base_url, GEMINICLI_USER_AGENT).await
        {
            return Ok(Some(project_id));
        }
        try_onboard_user(ctx, access_token, base_url, GEMINICLI_USER_AGENT).await
    })
}

async fn try_load_code_assist(
    ctx: &UpstreamCtx,
    access_token: &str,
    base_url: &str,
    user_agent: &str,
) -> ProviderResult<Option<String>> {
    let client = client_for_ctx(ctx, SharedClientKind::Global)?;
    let url = format!(
        "{}/v1internal:loadCodeAssist",
        base_url.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    let body = serde_json::to_vec(&body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", user_agent)
        .header("Accept-Encoding", "gzip")
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    if !status.is_success() {
        return Err(ProviderError::Other(format!(
            "loadCodeAssist failed: {status}"
        )));
    }
    let payload: serde_json::Value =
        serde_json::from_slice(&body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let current_tier = payload.get("currentTier");
    if current_tier.is_none() || current_tier.map(|value| value.is_null()).unwrap_or(true) {
        return Ok(None);
    }
    let project_id = payload
        .get("cloudaicompanionProject")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    Ok(project_id)
}

async fn try_onboard_user(
    ctx: &UpstreamCtx,
    access_token: &str,
    base_url: &str,
    user_agent: &str,
) -> ProviderResult<Option<String>> {
    let tier_id = get_onboard_tier(ctx, access_token, base_url, user_agent).await?;
    let client = client_for_ctx(ctx, SharedClientKind::Global)?;
    let url = format!("{}/v1internal:onboardUser", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "tierId": tier_id,
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    let body = serde_json::to_vec(&body).map_err(|err| ProviderError::Other(err.to_string()))?;
    for _ in 0..5 {
        let response = client
            .post(url.clone())
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", user_agent)
            .header("Accept-Encoding", "gzip")
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
            .await
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "onboardUser failed: {status}"
            )));
        }
        let payload: serde_json::Value =
            serde_json::from_slice(&body).map_err(|err| ProviderError::Other(err.to_string()))?;
        if payload.get("done").and_then(|value| value.as_bool()) == Some(true) {
            let project_value = payload
                .get("response")
                .and_then(|value| value.get("cloudaicompanionProject"));
            let project_id = project_value
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
                .or_else(|| {
                    project_value
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string())
                });
            return Ok(project_id);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(None)
}

async fn get_onboard_tier(
    ctx: &UpstreamCtx,
    access_token: &str,
    base_url: &str,
    user_agent: &str,
) -> ProviderResult<String> {
    let client = client_for_ctx(ctx, SharedClientKind::Global)?;
    let url = format!(
        "{}/v1internal:loadCodeAssist",
        base_url.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    let body = serde_json::to_vec(&body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", user_agent)
        .header("Accept-Encoding", "gzip")
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    if !status.is_success() {
        return Ok("LEGACY".to_string());
    }
    let payload: serde_json::Value =
        serde_json::from_slice(&body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let tiers = payload
        .get("allowedTiers")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    for tier in tiers {
        let is_default = tier.get("isDefault").and_then(|value| value.as_bool());
        let id = tier.get("id").and_then(|value| value.as_str());
        if is_default == Some(true)
            && let Some(id) = id
        {
            return Ok(id.to_string());
        }
    }
    Ok("LEGACY".to_string())
}

fn chrono_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
