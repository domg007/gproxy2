use bytes::Bytes;
use serde::Serialize;
use serde_json::json;
use tiktoken_rs::{get_bpe_from_model, o200k_base};

use gproxy_provider_core::config::{CustomProviderConfig, ModelRecord};
use gproxy_provider_core::header_get;
use gproxy_provider_core::{
    CountTokensMode, Credential, DispatchTable, HttpMethod, ProviderConfig, ProviderError,
    ProviderResult, UpstreamBody, UpstreamCtx, UpstreamHttpRequest, UpstreamHttpResponse,
    UpstreamProvider, credential::ApiKeyCredential, header_set,
};
use gproxy_provider_core::{CountTokensRequest, ModelGetRequest, ModelListRequest, Request};

use crate::auth_extractor;

const PROVIDER_NAME: &str = "custom";
const CLAUDE_CREATED_AT: &str = "2026-01-01T00:00:00Z";

#[derive(Debug, Clone, PartialEq, Eq)]
enum JsonMaskSegment {
    Key(String),
    Index(usize),
    Wildcard,
}

#[derive(Debug, Default)]
pub struct CustomProvider;

impl CustomProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for CustomProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn dispatch_table(&self, config: &ProviderConfig) -> DispatchTable {
        match config {
            ProviderConfig::Custom(cfg) => cfg.dispatch,
            _ => DispatchTable::default(),
        }
    }

    async fn build_claude_messages(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::claude::create_message::request::CreateMessageRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        let url = build_url(&cfg.base_url, "/v1/messages");
        let body =
            serde_json::to_vec(&req.body).map_err(|err| ProviderError::Other(err.to_string()))?;
        let mut headers = Vec::new();
        auth_extractor::set_header(&mut headers, "x-api-key", api_key);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_content_type_json(&mut headers);
        apply_anthropic_headers(&mut headers, &req.headers)?;
        let mut upstream = UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(Bytes::from(body)),
            is_stream: req.body.stream.unwrap_or(false),
        };
        finalize_json_request(cfg, &mut upstream)?;
        Ok(upstream)
    }

    async fn build_claude_count_tokens(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::claude::count_tokens::request::CountTokensRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        match cfg.count_tokens {
            CountTokensMode::Upstream => {
                let url = build_url(&cfg.base_url, "/v1/messages/count_tokens");
                let body = serde_json::to_vec(&req.body)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                let mut headers = Vec::new();
                auth_extractor::set_header(&mut headers, "x-api-key", api_key);
                auth_extractor::set_accept_json(&mut headers);
                auth_extractor::set_content_type_json(&mut headers);
                apply_anthropic_headers(&mut headers, &req.headers)?;
                let mut upstream = UpstreamHttpRequest {
                    method: HttpMethod::Post,
                    url,
                    headers,
                    body: Some(Bytes::from(body)),
                    is_stream: false,
                };
                finalize_json_request(cfg, &mut upstream)?;
                Ok(upstream)
            }
            CountTokensMode::Tokenizers | CountTokensMode::Tiktoken => {
                let model =
                    model_to_string(&req.body.model).unwrap_or_else(|| "gpt-4o-mini".to_string());
                let text = serde_json::to_string(&req.body)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                let count = count_text_tiktoken(&model, &text)?;
                let body = serde_json::to_vec(&json!({ "input_tokens": count }))
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(local_json_request(body))
            }
        }
    }

    async fn build_claude_models_list(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::claude::list_models::request::ListModelsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        let mut url = build_url(&cfg.base_url, "/v1/models");
        let query = build_claude_models_list_query(&req.query);
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
        let mut headers = Vec::new();
        auth_extractor::set_header(&mut headers, "x-api-key", api_key);
        auth_extractor::set_accept_json(&mut headers);
        apply_anthropic_headers(&mut headers, &req.headers)?;
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
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        let url = build_url(&cfg.base_url, &format!("/v1/models/{}", req.path.model_id));
        let mut headers = Vec::new();
        auth_extractor::set_header(&mut headers, "x-api-key", api_key);
        auth_extractor::set_accept_json(&mut headers);
        apply_anthropic_headers(&mut headers, &req.headers)?;
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Get,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    async fn build_gemini_generate(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::generate_content::request::GenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        build_gemini_request(
            custom_config(config)?,
            custom_api_key(credential)?,
            &format!("/v1beta/{}:generateContent", req.path.model),
            &req.body,
            false,
        )
    }

    async fn build_gemini_generate_stream(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::stream_content::request::StreamGenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        build_gemini_request(
            custom_config(config)?,
            custom_api_key(credential)?,
            &format!("/v1beta/{}:streamGenerateContent", req.path.model),
            &req.body,
            true,
        )
    }

    async fn build_gemini_count_tokens(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::count_tokens::request::CountTokensRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        match cfg.count_tokens {
            CountTokensMode::Upstream => build_gemini_request(
                cfg,
                api_key,
                &format!("/v1beta/{}:countTokens", req.path.model),
                &req.body,
                false,
            ),
            CountTokensMode::Tokenizers | CountTokensMode::Tiktoken => {
                let model = normalize_model_id(&req.path.model);
                let text = serde_json::to_string(&req.body)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                let count = count_text_tiktoken(&model, &text)?;
                let body = serde_json::to_vec(&json!({ "totalTokens": count }))
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(local_json_request(body))
            }
        }
    }

    async fn build_gemini_models_list(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::list_models::request::ListModelsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        let mut url = build_url(&cfg.base_url, "/v1beta/models");
        if let Some(q) = build_gemini_list_query(&req.query) {
            url = format!("{url}?{q}");
        }
        let mut headers = Vec::new();
        auth_extractor::set_header(&mut headers, "x-goog-api-key", api_key);
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
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::gemini::get_model::request::GetModelRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        let url = build_url(&cfg.base_url, &format!("/v1beta/{}", req.path.name));
        let mut headers = Vec::new();
        auth_extractor::set_header(&mut headers, "x-goog-api-key", api_key);
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
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::create_chat_completions::request::CreateChatCompletionRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        let url = build_url(&cfg.base_url, "/v1/chat/completions");
        let body =
            serde_json::to_vec(&req.body).map_err(|err| ProviderError::Other(err.to_string()))?;
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, api_key);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_content_type_json(&mut headers);
        let mut upstream = UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(Bytes::from(body)),
            is_stream: req.body.stream.unwrap_or(false),
        };
        finalize_json_request(cfg, &mut upstream)?;
        Ok(upstream)
    }

    async fn build_openai_responses(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::create_response::request::CreateResponseRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        let url = build_url(&cfg.base_url, "/v1/responses");
        let body =
            serde_json::to_vec(&req.body).map_err(|err| ProviderError::Other(err.to_string()))?;
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, api_key);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_content_type_json(&mut headers);
        let mut upstream = UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(Bytes::from(body)),
            is_stream: req.body.stream.unwrap_or(false),
        };
        finalize_json_request(cfg, &mut upstream)?;
        Ok(upstream)
    }

    async fn build_openai_input_tokens(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::count_tokens::request::InputTokenCountRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        match cfg.count_tokens {
            CountTokensMode::Upstream => {
                let url = build_url(&cfg.base_url, "/v1/responses/input_tokens");
                let body = serde_json::to_vec(&req.body)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                let mut headers = Vec::new();
                auth_extractor::set_bearer(&mut headers, api_key);
                auth_extractor::set_accept_json(&mut headers);
                auth_extractor::set_content_type_json(&mut headers);
                let mut upstream = UpstreamHttpRequest {
                    method: HttpMethod::Post,
                    url,
                    headers,
                    body: Some(Bytes::from(body)),
                    is_stream: false,
                };
                finalize_json_request(cfg, &mut upstream)?;
                Ok(upstream)
            }
            CountTokensMode::Tokenizers | CountTokensMode::Tiktoken => {
                let text = serde_json::to_string(&req.body)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                let count = count_text_tiktoken(&req.body.model, &text)?;
                let body = serde_json::to_vec(&json!({
                    "object": "response.input_tokens",
                    "input_tokens": count,
                }))
                .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(local_json_request(body))
            }
        }
    }

    async fn build_openai_models_list(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        _req: &gproxy_protocol::openai::list_models::request::ListModelsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        let url = build_url(&cfg.base_url, "/v1/models");
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, api_key);
        auth_extractor::set_accept_json(&mut headers);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Get,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    async fn build_openai_models_get(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::get_model::request::GetModelRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let cfg = custom_config(config)?;
        let api_key = custom_api_key(credential)?;
        let url = build_url(&cfg.base_url, &format!("/v1/models/{}", req.path.model));
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, api_key);
        auth_extractor::set_accept_json(&mut headers);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Get,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    fn local_response(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        _credential: &Credential,
        req: &Request,
    ) -> ProviderResult<Option<UpstreamHttpResponse>> {
        let cfg = custom_config(config)?;
        let Some(table) = cfg.model_table.as_ref() else {
            return Ok(None);
        };
        if table.models.is_empty() {
            return Ok(None);
        }

        match req {
            Request::ModelList(ModelListRequest::OpenAI(_)) => {
                let body = serde_json::to_vec(&openai_models_list_json(&table.models))
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            Request::ModelGet(ModelGetRequest::OpenAI(r)) => {
                let target = normalize_model_id(&r.path.model);
                let Some(model) = table
                    .models
                    .iter()
                    .find(|m| normalize_model_id(&m.id) == target)
                else {
                    return Ok(Some(local_json_response(
                        404,
                        serde_json::to_vec(&json!({ "error": { "message": "model not found" } }))
                            .map_err(|err| ProviderError::Other(err.to_string()))?,
                    )));
                };
                let body = serde_json::to_vec(&openai_model_json(model))
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            Request::ModelList(ModelListRequest::Claude(_)) => {
                let body = serde_json::to_vec(&claude_models_list_json(&table.models))
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            Request::ModelGet(ModelGetRequest::Claude(r)) => {
                let target = normalize_model_id(&r.path.model_id);
                let Some(model) = table
                    .models
                    .iter()
                    .find(|m| normalize_model_id(&m.id) == target)
                else {
                    return Ok(Some(local_json_response(
                        404,
                        serde_json::to_vec(&json!({ "error": "model_not_found" }))
                            .map_err(|err| ProviderError::Other(err.to_string()))?,
                    )));
                };
                let body = serde_json::to_vec(&claude_model_json(model))
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            Request::ModelList(ModelListRequest::Gemini(_)) => {
                let body = serde_json::to_vec(&gemini_models_list_json(&table.models))
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            Request::ModelGet(ModelGetRequest::Gemini(r)) => {
                let target = normalize_model_id(&r.path.name);
                let Some(model) = table
                    .models
                    .iter()
                    .find(|m| normalize_model_id(&m.id) == target)
                else {
                    return Ok(Some(local_json_response(
                        404,
                        serde_json::to_vec(&json!({ "error": { "message": "model not found" } }))
                            .map_err(|err| ProviderError::Other(err.to_string()))?,
                    )));
                };
                let body = serde_json::to_vec(&gemini_model_json(model))
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            Request::CountTokens(CountTokensRequest::OpenAI(r))
                if matches!(
                    cfg.count_tokens,
                    CountTokensMode::Tokenizers | CountTokensMode::Tiktoken
                ) =>
            {
                let text = serde_json::to_string(&r.body)
                    .map_err(|err| ProviderError::Other(err.to_string()))?;
                let count = count_text_tiktoken(&r.body.model, &text)?;
                let body = serde_json::to_vec(&json!({
                    "object": "response.input_tokens",
                    "input_tokens": count,
                }))
                .map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(local_json_response(200, body)))
            }
            _ => Ok(None),
        }
    }
}

fn custom_config(config: &ProviderConfig) -> ProviderResult<&CustomProviderConfig> {
    match config {
        ProviderConfig::Custom(cfg) => Ok(cfg),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::Custom".to_string(),
        )),
    }
}

fn custom_api_key(credential: &Credential) -> ProviderResult<&str> {
    match credential {
        Credential::Custom(ApiKeyCredential { api_key }) => Ok(api_key.as_str()),
        _ => Err(ProviderError::InvalidConfig(
            "expected Credential::Custom".to_string(),
        )),
    }
}

fn count_text_tiktoken(model: &str, text: &str) -> ProviderResult<i64> {
    let bpe = get_bpe_from_model(model)
        .or_else(|_| o200k_base())
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    Ok(bpe.encode_ordinary(text).len() as i64)
}

fn build_gemini_request<T: serde::Serialize>(
    cfg: &CustomProviderConfig,
    api_key: &str,
    path: &str,
    body: &T,
    is_stream: bool,
) -> ProviderResult<UpstreamHttpRequest> {
    let url = build_url(&cfg.base_url, path);
    let body = serde_json::to_vec(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    let mut headers = Vec::new();
    auth_extractor::set_header(&mut headers, "x-goog-api-key", api_key);
    auth_extractor::set_accept_json(&mut headers);
    auth_extractor::set_content_type_json(&mut headers);
    let mut upstream = UpstreamHttpRequest {
        method: HttpMethod::Post,
        url,
        headers,
        body: Some(Bytes::from(body)),
        is_stream,
    };
    finalize_json_request(cfg, &mut upstream)?;
    Ok(upstream)
}

fn finalize_json_request(
    cfg: &CustomProviderConfig,
    req: &mut UpstreamHttpRequest,
) -> ProviderResult<()> {
    if cfg.json_param_mask.is_empty() || req.body.is_none() {
        return Ok(());
    }
    if !is_json_content_type(req) {
        return Ok(());
    }
    apply_json_param_mask(&cfg.json_param_mask, req)
}

fn is_json_content_type(req: &UpstreamHttpRequest) -> bool {
    header_get(&req.headers, "content-type")
        .map(|v| v.to_ascii_lowercase().contains("application/json"))
        .unwrap_or(false)
}

fn apply_json_param_mask(
    mask_table: &[String],
    req: &mut UpstreamHttpRequest,
) -> ProviderResult<()> {
    let mask_paths = parse_json_mask_paths(mask_table)?;
    if mask_paths.is_empty() {
        return Ok(());
    }

    let Some(body) = req.body.as_ref() else {
        return Ok(());
    };
    let mut value: serde_json::Value =
        serde_json::from_slice(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    for path in &mask_paths {
        mask_json_value_by_path(&mut value, path);
    }
    let bytes = serde_json::to_vec(&value).map_err(|err| ProviderError::Other(err.to_string()))?;
    req.body = Some(Bytes::from(bytes));
    Ok(())
}

fn parse_json_mask_paths(mask_table: &[String]) -> ProviderResult<Vec<Vec<JsonMaskSegment>>> {
    let mut out = Vec::new();
    for raw in mask_table {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let path = parse_json_mask_path(line).map_err(|msg| {
            ProviderError::InvalidConfig(format!(
                "invalid custom json_param_mask entry `{line}`: {msg}"
            ))
        })?;
        out.push(path);
    }
    Ok(out)
}

fn parse_json_mask_path(line: &str) -> Result<Vec<JsonMaskSegment>, &'static str> {
    if line.starts_with('/') {
        return parse_json_pointer_path(line);
    }
    parse_dot_bracket_path(line)
}

fn parse_json_pointer_path(line: &str) -> Result<Vec<JsonMaskSegment>, &'static str> {
    let mut segments = Vec::new();
    for token in line.split('/').skip(1) {
        if token.is_empty() {
            return Err("empty pointer segment");
        }
        let decoded = token.replace("~1", "/").replace("~0", "~");
        segments.push(parse_json_mask_segment(&decoded)?);
    }
    if segments.is_empty() {
        return Err("empty path");
    }
    Ok(segments)
}

fn parse_dot_bracket_path(line: &str) -> Result<Vec<JsonMaskSegment>, &'static str> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    let mut current = String::new();
    let mut segments = Vec::new();
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '.' => {
                if current.is_empty() {
                    return Err("empty segment");
                }
                segments.push(parse_json_mask_segment(&current)?);
                current.clear();
                if i + 1 >= chars.len() {
                    return Err("trailing dot");
                }
                i += 1;
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(parse_json_mask_segment(&current)?);
                    current.clear();
                }
                i += 1;
                let mut inner = String::new();
                while i < chars.len() && chars[i] != ']' {
                    inner.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() || chars[i] != ']' {
                    return Err("missing closing ]");
                }
                let inner = inner.trim();
                if inner.is_empty() {
                    return Err("empty bracket segment");
                }
                let quoted = (inner.starts_with('"') && inner.ends_with('"'))
                    || (inner.starts_with('\'') && inner.ends_with('\''));
                let token = if quoted && inner.len() >= 2 {
                    &inner[1..inner.len() - 1]
                } else {
                    inner
                };
                if token.is_empty() {
                    return Err("empty bracket segment");
                }
                segments.push(parse_json_mask_segment(token)?);
                i += 1;
                if i < chars.len() && chars[i] == '.' {
                    if i + 1 >= chars.len() {
                        return Err("trailing dot");
                    }
                    i += 1;
                }
            }
            ']' => return Err("unexpected ]"),
            _ => {
                current.push(ch);
                i += 1;
            }
        }
    }
    if !current.is_empty() {
        segments.push(parse_json_mask_segment(&current)?);
    }
    if segments.is_empty() {
        return Err("empty path");
    }
    Ok(segments)
}

fn parse_json_mask_segment(token: &str) -> Result<JsonMaskSegment, &'static str> {
    let token = token.trim();
    if token.is_empty() {
        return Err("empty segment");
    }
    if token == "*" {
        return Ok(JsonMaskSegment::Wildcard);
    }
    if let Ok(index) = token.parse::<usize>() {
        return Ok(JsonMaskSegment::Index(index));
    }
    Ok(JsonMaskSegment::Key(token.to_string()))
}

fn mask_json_value_by_path(value: &mut serde_json::Value, path: &[JsonMaskSegment]) {
    if path.is_empty() {
        *value = serde_json::Value::Null;
        return;
    }
    match &path[0] {
        JsonMaskSegment::Wildcard => match value {
            serde_json::Value::Object(map) => {
                for child in map.values_mut() {
                    mask_json_value_by_path(child, &path[1..]);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    mask_json_value_by_path(child, &path[1..]);
                }
            }
            _ => {}
        },
        JsonMaskSegment::Key(key) => {
            if let serde_json::Value::Object(map) = value
                && let Some(child) = map.get_mut(key)
            {
                mask_json_value_by_path(child, &path[1..]);
            }
        }
        JsonMaskSegment::Index(index) => {
            if let serde_json::Value::Array(items) = value
                && let Some(child) = items.get_mut(*index)
            {
                mask_json_value_by_path(child, &path[1..]);
            }
        }
    }
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

fn build_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{base}/{}", path.trim_start_matches('/'))
}

fn build_claude_models_list_query(
    query: &gproxy_protocol::claude::list_models::request::ListModelsQuery,
) -> String {
    let mut parts = Vec::new();
    if let Some(after_id) = &query.after_id {
        parts.push(format!("after_id={}", urlencoding::encode(after_id)));
    }
    if let Some(before_id) = &query.before_id {
        parts.push(format!("before_id={}", urlencoding::encode(before_id)));
    }
    if let Some(limit) = query.limit {
        parts.push(format!("limit={limit}"));
    }
    parts.join("&")
}

fn build_gemini_list_query(
    query: &gproxy_protocol::gemini::list_models::request::ListModelsQuery,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(size) = query.page_size {
        parts.push(format!("pageSize={size}"));
    }
    if let Some(token) = &query.page_token
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

fn local_json_request(body: Vec<u8>) -> UpstreamHttpRequest {
    let mut headers = Vec::new();
    auth_extractor::set_accept_json(&mut headers);
    auth_extractor::set_content_type_json(&mut headers);
    UpstreamHttpRequest {
        method: HttpMethod::Post,
        url: "local://custom".to_string(),
        headers,
        body: Some(Bytes::from(body)),
        is_stream: false,
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

fn normalize_model_id(value: &str) -> String {
    value
        .trim_start_matches('/')
        .trim_start_matches("models/")
        .to_string()
}

fn model_to_string(model: &gproxy_protocol::claude::count_tokens::types::Model) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn openai_models_list_json(models: &[ModelRecord]) -> serde_json::Value {
    json!({
        "object": "list",
        "data": models.iter().map(openai_model_json).collect::<Vec<_>>(),
    })
}

fn openai_model_json(model: &ModelRecord) -> serde_json::Value {
    json!({
        "id": normalize_model_id(&model.id),
        "object": "model",
        "owned_by": "custom",
    })
}

fn claude_models_list_json(models: &[ModelRecord]) -> serde_json::Value {
    json!({
        "data": models.iter().map(claude_model_json).collect::<Vec<_>>(),
        "has_more": false,
    })
}

fn claude_model_json(model: &ModelRecord) -> serde_json::Value {
    json!({
        "id": normalize_model_id(&model.id),
        "created_at": CLAUDE_CREATED_AT,
        "display_name": model.display_name.clone().unwrap_or_else(|| normalize_model_id(&model.id)),
        "type": "model",
    })
}

fn gemini_models_list_json(models: &[ModelRecord]) -> serde_json::Value {
    json!({
        "models": models.iter().map(gemini_model_json).collect::<Vec<_>>(),
    })
}

fn gemini_model_json(model: &ModelRecord) -> serde_json::Value {
    let normalized = normalize_model_id(&model.id);
    json!({
        "name": format!("models/{normalized}"),
        "version": "custom",
        "displayName": model.display_name.clone().unwrap_or(normalized),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_json_value_by_path_keeps_top_level_compatibility() {
        let mut value = json!({
            "temperature": 0.7,
            "top_p": 0.9,
            "nested": {
                "temperature": 0.2
            }
        });
        mask_json_value_by_path(
            &mut value,
            &[JsonMaskSegment::Key("temperature".to_string())],
        );
        mask_json_value_by_path(&mut value, &[JsonMaskSegment::Key("top_p".to_string())]);
        assert_eq!(value["temperature"], serde_json::Value::Null);
        assert_eq!(value["top_p"], serde_json::Value::Null);
        assert_eq!(value["nested"]["temperature"], json!(0.2));
    }

    #[test]
    fn mask_json_value_by_path_supports_nested_array_path() {
        let mut value = json!({
            "messages": [
                { "content": "a", "role": "user" },
                { "content": "b", "role": "assistant" }
            ]
        });
        let path = parse_json_mask_path("messages[1].content").unwrap();
        mask_json_value_by_path(&mut value, &path);
        assert_eq!(value["messages"][0]["content"], json!("a"));
        assert_eq!(value["messages"][1]["content"], serde_json::Value::Null);
    }

    #[test]
    fn mask_json_value_by_path_supports_wildcard() {
        let mut value = json!({
            "messages": [
                { "content": "a" },
                { "content": "b" }
            ]
        });
        let path = parse_json_mask_path("messages[*].content").unwrap();
        mask_json_value_by_path(&mut value, &path);
        assert_eq!(value["messages"][0]["content"], serde_json::Value::Null);
        assert_eq!(value["messages"][1]["content"], serde_json::Value::Null);
    }

    #[test]
    fn apply_json_param_mask_rewrites_request_body() {
        let mut req = UpstreamHttpRequest {
            method: HttpMethod::Post,
            url: "https://example.com/v1/chat/completions".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: Some(Bytes::from(
                serde_json::to_vec(&json!({
                    "model": "gpt-4o-mini",
                    "messages": [
                        { "content": "a" },
                        { "content": "b" }
                    ],
                    "temperature": 0.5
                }))
                .unwrap(),
            )),
            is_stream: false,
        };
        apply_json_param_mask(
            &["temperature".to_string(), "messages[*].content".to_string()],
            &mut req,
        )
        .unwrap();
        let body = serde_json::from_slice::<serde_json::Value>(req.body.unwrap().as_ref()).unwrap();
        assert_eq!(body["temperature"], serde_json::Value::Null);
        assert_eq!(body["messages"][0]["content"], serde_json::Value::Null);
        assert_eq!(body["messages"][1]["content"], serde_json::Value::Null);
        assert_eq!(body["model"], json!("gpt-4o-mini"));
    }

    #[test]
    fn parse_json_mask_path_supports_json_pointer() {
        let path = parse_json_mask_path("/messages/0/content").unwrap();
        assert_eq!(
            path,
            vec![
                JsonMaskSegment::Key("messages".to_string()),
                JsonMaskSegment::Index(0),
                JsonMaskSegment::Key("content".to_string())
            ]
        );
    }
}
