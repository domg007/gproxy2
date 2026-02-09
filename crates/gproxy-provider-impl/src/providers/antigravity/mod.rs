use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use bytes::Bytes;
use rand::RngCore;
use serde::Deserialize;

use gproxy_provider_core::credential::AntigravityCredential;
use gproxy_provider_core::provider::UpstreamFailure;
use gproxy_provider_core::{
    AuthRetryAction, CountTokensRequest, Credential, DispatchRule, DispatchTable, HttpMethod,
    ModelGetRequest, ModelListRequest, OAuthCallbackRequest, OAuthCallbackResult, OAuthCredential,
    OAuthStartRequest, Proto, ProviderConfig, ProviderError, ProviderResult, Request, UpstreamBody,
    UpstreamCtx, UpstreamHttpRequest, UpstreamHttpResponse, UpstreamProvider, header_set,
};

use crate::auth_extractor;
use crate::providers::http_client::{SharedClientKind, client_for_ctx};
mod oauth;
mod usage;

const PROVIDER_NAME: &str = "antigravity";
const DEFAULT_BASE_URL: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
const ANTIGRAVITY_USER_AGENT: &str = "antigravity/1.15.8 (Windows; AMD64)";
const DEFAULT_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const DEFAULT_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "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";
const OAUTH_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs";
const OAUTH_STATE_TTL_SECS: u64 = 600;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

#[derive(Debug, Default)]
pub struct AntigravityProvider;

impl AntigravityProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for AntigravityProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn dispatch_table(&self, _config: &ProviderConfig) -> DispatchTable {
        DispatchTable::new([
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
            // OpenAI chat completions
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
            // OAuth start/callback + upstream usage are supported (see samples).
            DispatchRule::Native,
            DispatchRule::Native,
            DispatchRule::Native,
        ])
    }

    async fn build_gemini_generate(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::generate_content::request::GenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let project_id = antigravity_project_id(credential)?;
        let model = normalize_model_name(&req.path.model);
        let wrapped = wrap_internal_request(&model, project_id, &req.body);
        build_gemini_request(
            config,
            credential,
            "/v1internal:generateContent",
            &wrapped,
            false,
            Some(&model),
        )
    }

    async fn build_gemini_generate_stream(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::stream_content::request::StreamGenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let project_id = antigravity_project_id(credential)?;
        let model = normalize_model_name(&req.path.model);
        let wrapped = wrap_internal_request(&model, project_id, &req.body);
        build_gemini_request(
            config,
            credential,
            "/v1internal:streamGenerateContent?alt=sse",
            &wrapped,
            true,
            Some(&model),
        )
    }

    async fn build_gemini_count_tokens(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::count_tokens::request::CountTokensRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let model = normalize_model_name(&req.path.model);
        let mut request_obj = serde_json::Map::new();
        request_obj.insert(
            "model".to_string(),
            serde_json::Value::String(format!("models/{model}")),
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
            "request": serde_json::Value::Object(request_obj),
        });
        build_gemini_request(
            config,
            credential,
            "/v1internal:countTokens",
            &wrapped,
            false,
            Some(&model),
        )
    }

    async fn build_gemini_models_list(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::list_models::request::ListModelsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = antigravity_base_url(config)?;
        let access_token = antigravity_access_token(credential)?;
        let mut url = build_url(
            Some(base_url),
            DEFAULT_BASE_URL,
            "/v1internal:fetchAvailableModels",
        );
        if let Some(q) = build_gemini_query(&req.query) {
            url = format!("{url}?{q}");
        }
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_content_type_json(&mut headers);
        auth_extractor::set_user_agent(&mut headers, ANTIGRAVITY_USER_AGENT);
        auth_extractor::set_header(&mut headers, "Accept-Encoding", "gzip");
        auth_extractor::set_header(&mut headers, "requestid", &make_request_id());
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(Bytes::from_static(b"{}")),
            is_stream: false,
        })
    }

    async fn build_gemini_models_get(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::get_model::request::GetModelRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = antigravity_base_url(config)?;
        let access_token = antigravity_access_token(credential)?;
        let mut url = build_url(
            Some(base_url),
            DEFAULT_BASE_URL,
            "/v1internal:fetchAvailableModels",
        );
        if !req.path.name.is_empty() {
            url = format!("{url}?name={}", urlencoding::encode(&req.path.name));
        }
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_content_type_json(&mut headers);
        auth_extractor::set_user_agent(&mut headers, ANTIGRAVITY_USER_AGENT);
        auth_extractor::set_header(&mut headers, "Accept-Encoding", "gzip");
        auth_extractor::set_header(&mut headers, "requestid", &make_request_id());
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(Bytes::from_static(b"{}")),
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

    fn upgrade_credential<'a>(
        &'a self,
        ctx: &'a UpstreamCtx,
        config: &'a ProviderConfig,
        credential: &'a Credential,
        _req: &'a Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ProviderResult<Option<Credential>>> + Send + 'a>,
    > {
        Box::pin(
            async move { oauth::enrich_credential_profile_if_missing(ctx, config, credential).await },
        )
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
        Box::pin(async move {
            let action = oauth::on_auth_failure(ctx, config, credential, req, failure).await?;
            match action {
                AuthRetryAction::UpdateCredential(mut new_cred) => {
                    if let Credential::Antigravity(cred) = &mut *new_cred {
                        let base_url = antigravity_base_url(config)?;
                        if let Ok(Some(project_id)) =
                            detect_project_id(ctx, &cred.access_token, base_url)
                            && !project_id.trim().is_empty()
                            && project_id != cred.project_id
                        {
                            cred.project_id = project_id;
                        }
                    }
                    Ok(AuthRetryAction::UpdateCredential(new_cred))
                }
                other => Ok(other),
            }
        })
    }

    fn on_upstream_failure<'a>(
        &'a self,
        ctx: &'a UpstreamCtx,
        config: &'a ProviderConfig,
        credential: &'a Credential,
        _req: &'a Request,
        failure: &'a UpstreamFailure,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ProviderResult<AuthRetryAction>> + Send + 'a>,
    > {
        Box::pin(async move {
            let UpstreamFailure::Http { status, .. } = failure else {
                return Ok(AuthRetryAction::None);
            };
            if *status != 404 {
                return Ok(AuthRetryAction::None);
            }
            let Credential::Antigravity(cred) = credential else {
                return Ok(AuthRetryAction::None);
            };
            let base_url = antigravity_base_url(config)?;
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
            if let Credential::Antigravity(cred_mut) = &mut updated {
                cred_mut.project_id = project_id;
            }
            Ok(AuthRetryAction::UpdateCredential(Box::new(updated)))
        })
    }

    fn local_response(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &Request,
    ) -> ProviderResult<Option<UpstreamHttpResponse>> {
        match req {
            Request::CountTokens(CountTokensRequest::Gemini(req)) => {
                let body = gproxy_protocol::gemini::count_tokens::response::CountTokensResponse {
                    total_tokens: estimate_tokens(&req.body),
                    cached_content_token_count: None,
                    prompt_tokens_details: None,
                    cache_tokens_details: None,
                };
                let body = serde_json::to_vec(&body)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            Request::ModelList(ModelListRequest::Gemini(_)) => {
                let payload = fetch_available_models_from_upstream(ctx, config, credential)?;
                let models = extract_available_models(&payload);
                let body = serde_json::json!({
                    "models": models,
                });
                let body = serde_json::to_vec(&body)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            Request::ModelGet(ModelGetRequest::Gemini(req)) => {
                let name = normalize_model_name(&req.path.name);
                let payload = fetch_available_models_from_upstream(ctx, config, credential)?;
                let Some(model) = find_available_model(&payload, &name) else {
                    let body = serde_json::to_vec(&serde_json::json!({
                        "error": { "message": "model not found" }
                    }))
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                    return Ok(Some(local_json_response(404, body)));
                };
                let body = serde_json::to_vec(&model)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            _ => Ok(None),
        }
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

fn antigravity_base_url(config: &ProviderConfig) -> ProviderResult<&str> {
    match config {
        ProviderConfig::Antigravity(cfg) => Ok(cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::Antigravity".to_string(),
        )),
    }
}

fn antigravity_access_token(credential: &Credential) -> ProviderResult<&str> {
    match credential {
        Credential::Antigravity(cred) => Ok(cred.access_token.as_str()),
        _ => Err(ProviderError::InvalidConfig(
            "expected Credential::Antigravity".to_string(),
        )),
    }
}

fn build_gemini_request<T: serde::Serialize>(
    config: &ProviderConfig,
    credential: &Credential,
    path: &str,
    body: &T,
    is_stream: bool,
    model_name: Option<&str>,
) -> ProviderResult<UpstreamHttpRequest> {
    let base_url = antigravity_base_url(config)?;
    let access_token = antigravity_access_token(credential)?;
    let url = build_url(Some(base_url), DEFAULT_BASE_URL, path);
    let body = serde_json::to_vec(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let mut headers = Vec::new();
    auth_extractor::set_bearer(&mut headers, access_token);
    auth_extractor::set_accept_json(&mut headers);
    auth_extractor::set_content_type_json(&mut headers);
    auth_extractor::set_user_agent(&mut headers, ANTIGRAVITY_USER_AGENT);
    auth_extractor::set_header(&mut headers, "Accept-Encoding", "gzip");
    auth_extractor::set_header(&mut headers, "requestid", &make_request_id());
    if let Some(model_name) = model_name {
        auth_extractor::set_header(
            &mut headers,
            "requesttype",
            request_type_for_model(model_name),
        );
    }
    Ok(UpstreamHttpRequest {
        method: HttpMethod::Post,
        url,
        headers,
        body: Some(Bytes::from(body)),
        is_stream,
    })
}

fn build_gemini_query(
    query: &gproxy_protocol::gemini::list_models::request::ListModelsQuery,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(size) = query.page_size {
        parts.push(format!("pageSize={size}"));
    }
    if let Some(token) = query.page_token.as_ref()
        && !token.is_empty()
    {
        parts.push(format!("pageToken={}", urlencoding::encode(token)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("&"))
    }
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
    let mut name = model.strip_prefix("models/").unwrap_or(model).trim();
    for prefix in [FAKE_PREFIX, ANTI_TRUNC_PREFIX] {
        if let Some(stripped) = name.strip_prefix(prefix) {
            name = stripped;
        }
    }
    if let Some(stripped) = name.strip_suffix(FAKE_SUFFIX) {
        name = stripped.trim_end_matches('-');
    }
    if let Some(stripped) = name.strip_suffix(ANTI_TRUNC_SUFFIX) {
        name = stripped.trim_end_matches('-');
    }
    name.to_string()
}

fn antigravity_project_id(credential: &Credential) -> ProviderResult<&str> {
    match credential {
        Credential::Antigravity(cred) => {
            if cred.project_id.trim().is_empty() {
                Err(ProviderError::InvalidConfig(
                    "missing project_id".to_string(),
                ))
            } else {
                Ok(cred.project_id.as_str())
            }
        }
        _ => Err(ProviderError::InvalidConfig(
            "expected Credential::Antigravity".to_string(),
        )),
    }
}

fn wrap_internal_request(
    model: &str,
    project_id: &str,
    request: &gproxy_protocol::gemini::generate_content::request::GenerateContentRequestBody,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "project": project_id,
        "request": request,
    })
}

fn request_type_for_model(model: &str) -> &'static str {
    if model.to_ascii_lowercase().contains("image") {
        "image_gen"
    } else {
        "agent"
    }
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

fn local_json_response(status: u16, body: Vec<u8>) -> UpstreamHttpResponse {
    let mut headers = Vec::new();
    header_set(&mut headers, "content-type", "application/json");
    UpstreamHttpResponse {
        status,
        headers,
        body: UpstreamBody::Bytes(Bytes::from(body)),
    }
}

fn estimate_tokens(
    body: &gproxy_protocol::gemini::count_tokens::request::CountTokensRequestBody,
) -> u32 {
    if let Some(contents) = body.contents.as_ref() {
        return estimate_tokens_from_contents(contents);
    }
    if let Some(req) = body.generate_content_request.as_ref() {
        if let Some(contents) = req.get("contents").and_then(|v| v.as_array()) {
            let mut text = String::new();
            for item in contents {
                if let Some(parts) = item.get("parts").and_then(|v| v.as_array()) {
                    for part in parts {
                        if let Some(value) = part.get("text").and_then(|v| v.as_str()) {
                            text.push_str(value);
                        }
                    }
                }
            }
            return estimate_tokens_from_text(&text);
        }
        let raw = serde_json::to_string(req).unwrap_or_default();
        return estimate_tokens_from_text(&raw);
    }
    0
}

fn estimate_tokens_from_contents(
    contents: &[gproxy_protocol::gemini::count_tokens::types::Content],
) -> u32 {
    let mut text = String::new();
    for content in contents {
        for part in &content.parts {
            if let Some(value) = part.text.as_ref() {
                text.push_str(value);
            }
        }
    }
    estimate_tokens_from_text(&text)
}

fn estimate_tokens_from_text(text: &str) -> u32 {
    let chars = text.chars().count() as u32;
    chars.div_ceil(4)
}

fn fetch_available_models_from_upstream(
    ctx: &UpstreamCtx,
    config: &ProviderConfig,
    credential: &Credential,
) -> ProviderResult<serde_json::Value> {
    let base_url = antigravity_base_url(config)?
        .trim_end_matches('/')
        .to_string();
    let access_token = antigravity_access_token(credential)?.to_string();
    crate::providers::oauth_common::block_on(async move {
        let client = client_for_ctx(ctx, SharedClientKind::Global)?;
        let response = client
            .post(format!("{base_url}/v1internal:fetchAvailableModels"))
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", ANTIGRAVITY_USER_AGENT)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("Accept-Encoding", "gzip")
            .header("requestid", make_request_id())
            .body(Bytes::from_static(b"{}"))
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
                "fetchAvailableModels failed: {status}"
            )));
        }
        serde_json::from_slice(&body).map_err(|err| ProviderError::Other(err.to_string()))
    })
}

fn extract_available_models(payload: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    if let Some(models_obj) = payload.get("models").and_then(|v| v.as_object()) {
        for (model_id, model_meta) in models_obj {
            out.push(build_available_model(model_id, model_meta));
        }
    } else if let Some(models_arr) = payload.get("models").and_then(|v| v.as_array()) {
        for item in models_arr {
            if let Some(id) = item
                .get("id")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("name").and_then(|v| v.as_str()))
            {
                out.push(build_available_model(&normalize_model_name(id), item));
            } else if let Some(s) = item.as_str() {
                out.push(build_available_model(
                    &normalize_model_name(s),
                    &serde_json::Value::Null,
                ));
            }
        }
    }
    out.sort_by(|a, b| {
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        a_name.cmp(b_name)
    });
    out.dedup_by(|a, b| {
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        a_name == b_name
    });
    out
}

fn find_available_model(payload: &serde_json::Value, model_id: &str) -> Option<serde_json::Value> {
    if let Some(models_obj) = payload.get("models").and_then(|v| v.as_object()) {
        if let Some(meta) = models_obj.get(model_id) {
            return Some(build_available_model(model_id, meta));
        }
        return models_obj
            .iter()
            .find(|(id, _)| normalize_model_name(id) == model_id)
            .map(|(id, meta)| build_available_model(id, meta));
    }
    if let Some(models_arr) = payload.get("models").and_then(|v| v.as_array()) {
        for item in models_arr {
            let raw_id = item
                .get("id")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("name").and_then(|v| v.as_str()))
                .or_else(|| item.as_str());
            if let Some(raw_id) = raw_id
                && normalize_model_name(raw_id) == model_id
            {
                return Some(build_available_model(&normalize_model_name(raw_id), item));
            }
        }
    }
    None
}

fn build_available_model(model_id: &str, meta: &serde_json::Value) -> serde_json::Value {
    let display_name = meta
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or(model_id);
    let mut obj = serde_json::json!({
        "name": format!("models/{model_id}"),
        "baseModelId": model_id,
        "version": "1",
        "displayName": display_name,
        "supportedGenerationMethods": [
            "generateContent",
            "countTokens",
            "streamGenerateContent"
        ],
    });
    if let Some(limit) = meta.get("maxTokens").and_then(|v| v.as_u64()) {
        obj["inputTokenLimit"] = serde_json::json!(limit);
    }
    if let Some(limit) = meta.get("maxOutputTokens").and_then(|v| v.as_u64()) {
        obj["outputTokenLimit"] = serde_json::json!(limit);
    }
    obj
}

const FAKE_PREFIX: &str = "\u{5047}\u{6d41}\u{5f0f}/";
const ANTI_TRUNC_PREFIX: &str = "\u{6d41}\u{5f0f}\u{6297}\u{622a}\u{65ad}/";
const FAKE_SUFFIX: &str = "\u{5047}\u{6d41}\u{5f0f}";
const ANTI_TRUNC_SUFFIX: &str = "\u{6d41}\u{5f0f}\u{6297}\u{622a}\u{65ad}";

fn detect_project_id(
    ctx: &UpstreamCtx,
    access_token: &str,
    base_url: &str,
) -> ProviderResult<Option<String>> {
    crate::providers::oauth_common::block_on(async move {
        if let Ok(Some(project_id)) =
            try_load_code_assist(ctx, access_token, base_url, ANTIGRAVITY_USER_AGENT).await
        {
            return Ok(Some(project_id));
        }
        try_onboard_user(ctx, access_token, base_url, ANTIGRAVITY_USER_AGENT).await
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

fn random_project_id() -> String {
    let mut bytes = [0u8; 6];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut bytes);
    format!(
        "gproxy-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

fn make_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("gproxy-{nanos}")
}
