use bytes::Bytes;
use serde_json::Value as JsonValue;

use gproxy_provider_core::{
    AuthRetryAction, Credential, DispatchRule, DispatchTable, HttpMethod, Op, Proto,
    ProviderConfig, ProviderError, ProviderResult, Request, UpstreamCtx, UpstreamHttpRequest,
    UpstreamProvider,
};

use crate::auth_extractor;
mod oauth;

const PROVIDER_NAME: &str = "vertex";
const DEFAULT_BASE_URL: &str = "https://aiplatform.googleapis.com";
const DEFAULT_LOCATION: &str = "us-central1";
const DEFAULT_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

// Mirrors `samples/crates/gproxy-provider-impl/src/provider/vertex/mod.rs` dispatch semantics.
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
    // OpenAI chat completions (Vertex supports OpenAI-compat for chat)
    DispatchRule::Native,
    DispatchRule::Native,
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
    // OAuth / usage (not implemented)
    DispatchRule::Unsupported,
    DispatchRule::Unsupported,
    DispatchRule::Unsupported,
]);

#[derive(Debug, Default)]
pub struct VertexProvider;

impl VertexProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for VertexProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn dispatch_table(&self, _config: &ProviderConfig) -> DispatchTable {
        DISPATCH_TABLE
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

    fn normalize_nonstream_response(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        proto: Proto,
        op: Op,
        _req: &Request,
        body: Bytes,
    ) -> ProviderResult<Bytes> {
        if proto != Proto::Gemini {
            return Ok(body);
        }
        let value: JsonValue =
            serde_json::from_slice(&body).map_err(|err| ProviderError::Other(err.to_string()))?;
        let normalized = match op {
            Op::ModelList => vertex_model_list_payload(value),
            Op::ModelGet => vertex_model_get_payload(value),
            _ => value,
        };
        serde_json::to_vec(&normalized)
            .map(Bytes::from)
            .map_err(|err| ProviderError::Other(err.to_string()))
    }

    async fn build_gemini_generate(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::generate_content::request::GenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let (project_id, location, token_uri) = vertex_context(config, credential)?;
        let model_id = normalize_model_name(&req.path.model);
        let body = vertex_generate_payload(&model_id, &req.body)?;
        let path = format!(
            "/v1beta1/projects/{project_id}/locations/{location}/publishers/google/models/{model_id}:generateContent"
        );
        build_vertex_request(ctx, config, credential, &path, &body, false, &token_uri)
    }

    async fn build_gemini_generate_stream(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::stream_content::request::StreamGenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let (project_id, location, token_uri) = vertex_context(config, credential)?;
        let model_id = normalize_model_name(&req.path.model);
        let body = vertex_generate_payload(&model_id, &req.body)?;
        let path = append_query(
            &format!(
                "/v1beta1/projects/{project_id}/locations/{location}/publishers/google/models/{model_id}:streamGenerateContent"
            ),
            req.query.as_deref(),
        );
        build_vertex_request(ctx, config, credential, &path, &body, true, &token_uri)
    }

    async fn build_gemini_count_tokens(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::count_tokens::request::CountTokensRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let (project_id, location, token_uri) = vertex_context(config, credential)?;
        let model_id = normalize_model_name(&req.path.model);
        let body = vertex_count_tokens_payload(&model_id, &req.body);
        let path = format!(
            "/v1beta1/projects/{project_id}/locations/{location}/publishers/google/models/{model_id}:countTokens"
        );
        build_vertex_request(ctx, config, credential, &path, &body, false, &token_uri)
    }

    async fn build_gemini_models_list(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::list_models::request::ListModelsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let (_, _, token_uri) = vertex_context(config, credential)?;
        let path = "/v1beta1/publishers/google/models".to_string();
        let mut url = build_url(Some(vertex_base_url(config)?), DEFAULT_BASE_URL, &path);
        if let Some(query) = build_gemini_query(&req.query) {
            url = format!("{url}?{query}");
        }
        let (access_token, _) = oauth::fetch_access_token(ctx, credential, &token_uri, false)?;
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, &access_token);
        auth_extractor::set_accept_json(&mut headers);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Get,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    async fn build_gemini_models_get(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::get_model::request::GetModelRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let (_, _, token_uri) = vertex_context(config, credential)?;
        let model_id = normalize_model_name(&req.path.name);
        let path = format!("/v1beta1/publishers/google/models/{model_id}");
        let url = build_url(Some(vertex_base_url(config)?), DEFAULT_BASE_URL, &path);
        let (access_token, _) = oauth::fetch_access_token(ctx, credential, &token_uri, false)?;
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, &access_token);
        auth_extractor::set_accept_json(&mut headers);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Get,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    async fn build_openai_chat(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::create_chat_completions::request::CreateChatCompletionRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let (project_id, location, token_uri) = vertex_context(config, credential)?;
        let mut body = req.body.clone();
        body.model = normalize_vertex_openai_model(&body.model);
        let endpoint_path = format!("projects/{project_id}/locations/{location}/endpoints/openapi");
        let path = format!("/v1beta1/{endpoint_path}/chat/completions");
        let url = build_url(Some(vertex_base_url(config)?), DEFAULT_BASE_URL, &path);
        let (access_token, _) = oauth::fetch_access_token(ctx, credential, &token_uri, false)?;
        let body_bytes =
            serde_json::to_vec(&body).map_err(|err| ProviderError::Other(err.to_string()))?;
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, &access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_content_type_json(&mut headers);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(Bytes::from(body_bytes)),
            is_stream: body.stream.unwrap_or(false),
        })
    }
}

fn vertex_base_url(config: &ProviderConfig) -> ProviderResult<&str> {
    match config {
        ProviderConfig::Vertex(cfg) => Ok(cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::Vertex".to_string(),
        )),
    }
}

fn vertex_context(
    config: &ProviderConfig,
    credential: &Credential,
) -> ProviderResult<(String, String, String)> {
    let cfg = match config {
        ProviderConfig::Vertex(cfg) => cfg,
        _ => {
            return Err(ProviderError::InvalidConfig(
                "expected ProviderConfig::Vertex".to_string(),
            ));
        }
    };
    let sa = match credential {
        Credential::Vertex(sa) => sa,
        _ => {
            return Err(ProviderError::InvalidConfig(
                "expected Credential::Vertex".to_string(),
            ));
        }
    };
    let project_id = sa.project_id.clone();
    let location = cfg
        .location
        .as_deref()
        .unwrap_or(DEFAULT_LOCATION)
        .to_string();
    let token_uri = cfg
        .oauth_token_url
        .as_deref()
        .or(cfg.token_uri.as_deref())
        .or(sa.token_uri.as_deref())
        .unwrap_or(DEFAULT_TOKEN_URI)
        .to_string();
    Ok((project_id, location, token_uri))
}

fn build_vertex_request<T: serde::Serialize>(
    ctx: &UpstreamCtx,
    config: &ProviderConfig,
    credential: &Credential,
    path: &str,
    body: &T,
    is_stream: bool,
    token_uri: &str,
) -> ProviderResult<UpstreamHttpRequest> {
    let url = build_url(Some(vertex_base_url(config)?), DEFAULT_BASE_URL, path);
    let (access_token, _) = oauth::fetch_access_token(ctx, credential, token_uri, false)?;
    let body = serde_json::to_vec(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let mut headers = Vec::new();
    auth_extractor::set_bearer(&mut headers, &access_token);
    auth_extractor::set_accept_json(&mut headers);
    auth_extractor::set_content_type_json(&mut headers);
    Ok(UpstreamHttpRequest {
        method: HttpMethod::Post,
        url,
        headers,
        body: Some(Bytes::from(body)),
        is_stream,
    })
}

fn normalize_model_name(name: &str) -> String {
    let name = name.strip_prefix("models/").unwrap_or(name);
    let name = name
        .strip_prefix("publishers/google/models/")
        .unwrap_or(name);
    name.to_string()
}

fn normalize_vertex_openai_model(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    if let Some(stripped) = trimmed.strip_prefix("publishers/")
        && let Some((publisher, model_name)) = stripped.split_once("/models/")
    {
        return format!("{publisher}/{model_name}");
    }
    if let Some(idx) = trimmed.find("/publishers/") {
        let tail = &trimmed[(idx + "/publishers/".len())..];
        if let Some((publisher, model_name)) = tail.split_once("/models/") {
            return format!("{publisher}/{model_name}");
        }
    }
    if let Some(stripped) = trimmed.strip_prefix("models/") {
        return format!("google/{stripped}");
    }
    if trimmed.contains('/') {
        return trimmed.to_string();
    }
    format!("google/{trimmed}")
}

fn vertex_generate_payload(
    path_model: &str,
    body: &gproxy_protocol::gemini::generate_content::request::GenerateContentRequestBody,
) -> ProviderResult<JsonValue> {
    let mut value =
        serde_json::to_value(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    if let JsonValue::Object(map) = &mut value
        && let Some(model) = map.get("model").and_then(|m| m.as_str())
    {
        map.insert(
            "model".to_string(),
            JsonValue::String(normalize_vertex_model_ref(model, path_model)),
        );
    }
    Ok(value)
}

fn vertex_count_tokens_payload(
    path_model: &str,
    body: &gproxy_protocol::gemini::count_tokens::request::CountTokensRequestBody,
) -> JsonValue {
    let mut out = serde_json::Map::new();

    out.insert(
        "model".to_string(),
        JsonValue::String(format!("publishers/google/models/{path_model}")),
    );

    if let Some(contents) = body.contents.as_ref()
        && let Ok(value) = serde_json::to_value(contents)
    {
        out.insert("contents".to_string(), value);
    }

    if let Some(generate) = body.generate_content_request.as_ref() {
        if !out.contains_key("contents")
            && let Some(v) = generate.get("contents")
        {
            out.insert("contents".to_string(), v.clone());
        }
        if let Some(v) = generate.get("instances") {
            out.insert("instances".to_string(), v.clone());
        }
        if let Some(v) = generate.get("tools") {
            out.insert("tools".to_string(), v.clone());
        }
        if let Some(v) = generate
            .get("systemInstruction")
            .or_else(|| generate.get("system_instruction"))
        {
            out.insert("systemInstruction".to_string(), v.clone());
        }
        if let Some(v) = generate
            .get("generationConfig")
            .or_else(|| generate.get("generation_config"))
        {
            out.insert("generationConfig".to_string(), v.clone());
        }
        if let Some(v) = generate.get("model").and_then(|m| m.as_str()) {
            out.insert(
                "model".to_string(),
                JsonValue::String(normalize_vertex_model_ref(v, path_model)),
            );
        }
    }

    JsonValue::Object(out)
}

fn normalize_vertex_model_ref(model: &str, fallback_model: &str) -> String {
    let m = model.trim().trim_start_matches('/');
    if m.is_empty() {
        return format!("publishers/google/models/{fallback_model}");
    }
    if m.starts_with("publishers/") && m.contains("/models/") {
        return m.to_string();
    }
    if let Some(id) = m.strip_prefix("models/") {
        return format!("publishers/google/models/{id}");
    }
    if let Some((publisher, id)) = m.split_once('/')
        && !publisher.is_empty()
        && !id.is_empty()
    {
        return format!("publishers/{publisher}/models/{id}");
    }
    format!("publishers/google/models/{m}")
}

fn append_query(path: &str, query: Option<&str>) -> String {
    let Some(query) = query.map(str::trim).filter(|q| !q.is_empty()) else {
        return path.to_string();
    };
    if path.contains('?') {
        format!("{path}&{query}")
    } else {
        format!("{path}?{query}")
    }
}

fn vertex_model_list_payload(value: JsonValue) -> JsonValue {
    let JsonValue::Object(mut map) = value else {
        return value;
    };
    if map.contains_key("models") {
        return JsonValue::Object(map);
    }

    let models = match map.remove("publisherModels") {
        Some(JsonValue::Array(items)) => items
            .into_iter()
            .map(vertex_publisher_model_to_gemini)
            .collect::<Vec<_>>(),
        Some(item) => vec![vertex_publisher_model_to_gemini(item)],
        None => Vec::new(),
    };

    let mut out = serde_json::Map::new();
    out.insert("models".to_string(), JsonValue::Array(models));
    if let Some(token) = map.remove("nextPageToken").filter(|v| !v.is_null()) {
        out.insert("nextPageToken".to_string(), token);
    }
    JsonValue::Object(out)
}

fn vertex_model_get_payload(value: JsonValue) -> JsonValue {
    let JsonValue::Object(mut map) = value else {
        return value;
    };
    if map
        .get("name")
        .and_then(|v| v.as_str())
        .map(|name| name.starts_with("models/"))
        .unwrap_or(false)
        && map.get("version").is_some()
    {
        return JsonValue::Object(map);
    }
    if let Some(inner) = map.remove("publisherModel") {
        return vertex_publisher_model_to_gemini(inner);
    }
    vertex_publisher_model_to_gemini(JsonValue::Object(map))
}

fn vertex_publisher_model_to_gemini(value: JsonValue) -> JsonValue {
    let JsonValue::Object(map) = value else {
        return value;
    };

    let raw_name = map
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    let model_id = if let Some((_, tail)) = raw_name.rsplit_once("/models/") {
        tail
    } else {
        raw_name.strip_prefix("models/").unwrap_or(raw_name)
    };

    let mut out = serde_json::Map::new();
    out.insert(
        "name".to_string(),
        JsonValue::String(format!("models/{model_id}")),
    );

    let version = map
        .get("version")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            map.get("versionId")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
        })
        .or_else(|| {
            model_id
                .rsplit_once('@')
                .map(|(_, v)| v)
                .filter(|v| !v.is_empty())
        })
        .unwrap_or("unknown");
    out.insert(
        "version".to_string(),
        JsonValue::String(version.to_string()),
    );

    if let Some(v) = map
        .get("displayName")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        out.insert("displayName".to_string(), JsonValue::String(v.to_string()));
    }
    if let Some(v) = map
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        out.insert("description".to_string(), JsonValue::String(v.to_string()));
    }
    if let Some(v) = map
        .get("inputTokenLimit")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
    {
        out.insert("inputTokenLimit".to_string(), JsonValue::from(v));
    }
    if let Some(v) = map
        .get("outputTokenLimit")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
    {
        out.insert("outputTokenLimit".to_string(), JsonValue::from(v));
    }
    if let Some(methods) = map
        .get("supportedGenerationMethods")
        .and_then(|v| v.as_array())
    {
        let arr = methods
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| JsonValue::String(s.to_string()))
            .collect::<Vec<_>>();
        if !arr.is_empty() {
            out.insert(
                "supportedGenerationMethods".to_string(),
                JsonValue::Array(arr),
            );
        }
    }

    JsonValue::Object(out)
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
