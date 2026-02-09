use base64::Engine;
use bytes::Bytes;
use rand::RngCore;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::time::{SystemTime, UNIX_EPOCH};
use tiktoken_rs::{CoreBPE, get_bpe_from_model, o200k_base};

use gproxy_provider_core::credential::CodexCredential;
use gproxy_provider_core::{
    AuthRetryAction, Credential, DispatchRule, DispatchTable, HttpMethod, OAuthCallbackRequest,
    OAuthCallbackResult, OAuthCredential, OAuthStartRequest, Op, OpenAIResponsesPassthroughRequest,
    Proto, ProviderConfig, ProviderError, ProviderResult, Request, UpstreamBody, UpstreamCtx,
    UpstreamHttpRequest, UpstreamHttpResponse, UpstreamProvider, header_set,
};

use gproxy_protocol::openai;
use gproxy_protocol::openai::create_response::types::{
    EasyInputMessage, EasyInputMessageContent, EasyInputMessageRole, EasyInputMessageType,
    InputItem, InputParam, Instructions, Metadata,
};

use crate::auth_extractor;
mod oauth;
mod usage;

const PROVIDER_NAME: &str = "codex";
const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const DEFAULT_ISSUER: &str = "https://auth.openai.com";
const OAUTH_STATE_TTL_SECS: u64 = 600;
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CLIENT_VERSION: &str = "0.99.0";
const GPROXY_COMPACT_METADATA_KEY: &str = "gproxy_internal_compact";
const GPROXY_COMPACT_METADATA_VALUE: &str = "1";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Debug, Default)]
struct IdTokenClaims {
    email: Option<String>,
    plan: Option<String>,
    account_id: Option<String>,
}

#[derive(Debug, Default)]
pub struct CodexProvider;

impl CodexProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for CodexProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn dispatch_table(&self, _config: &ProviderConfig) -> DispatchTable {
        DispatchTable::new([
            // Claude
            DispatchRule::Unsupported,
            DispatchRule::Transform {
                target: Proto::OpenAIResponse,
            },
            DispatchRule::Transform {
                target: Proto::OpenAI,
            },
            DispatchRule::Transform {
                target: Proto::OpenAI,
            },
            DispatchRule::Transform {
                target: Proto::OpenAI,
            },
            // Gemini
            DispatchRule::Unsupported,
            DispatchRule::Transform {
                target: Proto::OpenAIResponse,
            },
            DispatchRule::Transform {
                target: Proto::OpenAI,
            },
            DispatchRule::Transform {
                target: Proto::OpenAI,
            },
            DispatchRule::Transform {
                target: Proto::OpenAI,
            },
            // OpenAI chat completions
            DispatchRule::Unsupported,
            DispatchRule::Transform {
                target: Proto::OpenAIResponse,
            },
            // OpenAI Responses
            DispatchRule::Unsupported,
            DispatchRule::Native,
            // OpenAI basic ops
            DispatchRule::Native,
            DispatchRule::Native,
            DispatchRule::Native,
            // OAuth start/callback + upstream usage are supported (see samples).
            DispatchRule::Native,
            DispatchRule::Native,
            DispatchRule::Native,
        ])
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
        _ctx: &'a UpstreamCtx,
        _config: &'a ProviderConfig,
        credential: &'a Credential,
        _req: &'a Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = ProviderResult<Option<Credential>>> + Send + 'a>,
    > {
        Box::pin(async move { oauth::enrich_credential_profile_if_missing(credential).await })
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

    async fn build_upstream_usage(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
    ) -> ProviderResult<UpstreamHttpRequest> {
        usage::build_upstream_usage(ctx, config, credential)
    }

    async fn build_openai_responses(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &openai::create_response::request::CreateResponseRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = codex_base_url(config)?;
        let (access_token, account_id) = codex_credential(credential)?;
        let mut body = req.body.clone();
        let is_compact = take_compact_marker(&mut body.metadata);
        if !is_compact {
            normalize_codex_input(&mut body);
            // Codex upstream requires explicit non-persistent responses.
            body.store = Some(false);
            // Codex upstream does not support max tokens parameter.
            body.max_output_tokens = None;
            // Codex upstream rejects OpenAI stream_options.
            body.stream_options = None;
        } else {
            // Codex compact endpoint is unary JSON.
            body.stream = Some(false);
            body.stream_options = None;
        }
        ensure_codex_instructions_field(&mut body);
        let is_stream = if is_compact {
            false
        } else {
            body.stream.unwrap_or(false)
        };
        let url = if is_compact {
            format!("{}/responses/compact", base_url.trim_end_matches('/'))
        } else {
            format!("{}/responses", base_url.trim_end_matches('/'))
        };
        let body =
            serde_json::to_vec(&body).map_err(|err| ProviderError::Other(err.to_string()))?;

        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_content_type_json(&mut headers);
        auth_extractor::set_header(&mut headers, "chatgpt-account-id", account_id);

        Ok(UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(Bytes::from(body)),
            is_stream,
        })
    }

    async fn build_openai_responses_passthrough(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &OpenAIResponsesPassthroughRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = codex_base_url(config)?;
        let (access_token, account_id) = codex_credential(credential)?;
        let upstream_path = codex_responses_upstream_path(&req.path)?;

        let mut body = req.body.clone();
        let mut is_stream = req.is_stream;
        if matches!(
            req.method,
            HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch
        ) && let Some(raw) = body.as_ref()
            && let Ok(mut value) = serde_json::from_slice::<JsonValue>(raw)
            && let Some(obj) = value.as_object_mut()
        {
            if upstream_path == "/responses/compact" {
                obj.insert("stream".to_string(), JsonValue::Bool(false));
                obj.remove("stream_options");
                is_stream = false;
            } else if upstream_path == "/responses" {
                normalize_codex_input_json(obj);
                obj.insert("store".to_string(), JsonValue::Bool(false));
                obj.remove("max_output_tokens");
                obj.remove("stream_options");
                is_stream = obj
                    .get("stream")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(req.is_stream);
            }
            ensure_codex_instructions_field_json(obj);
            body = Some(Bytes::from(
                serde_json::to_vec(&value).map_err(|err| ProviderError::Other(err.to_string()))?,
            ));
        }
        if upstream_path == "/responses/compact" {
            is_stream = false;
        }

        let mut url = format!("{}{}", base_url.trim_end_matches('/'), upstream_path);
        if let Some(query) = req.query.as_deref().filter(|q| !q.trim().is_empty()) {
            url.push('?');
            url.push_str(query);
        }

        let mut headers = req.headers.clone();
        auth_extractor::set_bearer(&mut headers, access_token);
        auth_extractor::set_header(&mut headers, "chatgpt-account-id", account_id);
        if !has_header(&headers, "accept") {
            auth_extractor::set_accept_json(&mut headers);
        }
        if body.is_some() && !has_header(&headers, "content-type") {
            auth_extractor::set_content_type_json(&mut headers);
        }

        Ok(UpstreamHttpRequest {
            method: req.method,
            url,
            headers,
            body,
            is_stream,
        })
    }

    async fn build_openai_input_tokens(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        credential: &Credential,
        req: &openai::count_tokens::request::InputTokenCountRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let _ = codex_credential(credential)?;
        let tokens = count_input_tokens(&req.body)?;
        let response = openai::count_tokens::response::InputTokenCountResponse {
            object: openai::count_tokens::types::InputTokenObjectType::ResponseInputTokens,
            input_tokens: tokens,
        };
        let body =
            serde_json::to_vec(&response).map_err(|err| ProviderError::Other(err.to_string()))?;
        Ok(local_json_request(body))
    }

    async fn build_openai_models_list(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        _req: &gproxy_protocol::openai::list_models::request::ListModelsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = codex_base_url(config)?;
        let (access_token, account_id) = codex_credential(credential)?;
        let url = codex_models_url(base_url);

        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_header(&mut headers, "chatgpt-account-id", account_id);

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
        let base_url = codex_base_url(config)?;
        let (access_token, account_id) = codex_credential(credential)?;
        let _model = normalize_model_id(&req.path.model);
        let url = codex_models_url(base_url);

        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, access_token);
        auth_extractor::set_accept_json(&mut headers);
        auth_extractor::set_header(&mut headers, "chatgpt-account-id", account_id);

        Ok(UpstreamHttpRequest {
            method: HttpMethod::Get,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    fn normalize_nonstream_response(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        proto: Proto,
        op: Op,
        req: &Request,
        body: Bytes,
    ) -> ProviderResult<Bytes> {
        if proto == Proto::OpenAIResponse
            && op == Op::GenerateContent
            && request_has_compact_marker(req)
        {
            return normalize_codex_compact_response_for_decode(&body, req);
        }

        if proto != Proto::OpenAI {
            return Ok(body);
        }

        let Ok(value) = serde_json::from_slice::<JsonValue>(&body) else {
            return Ok(body);
        };

        let normalized = match op {
            Op::ModelList => {
                if is_openai_model_list(&value) {
                    value
                } else if let Some(list) = normalize_codex_model_list(&value) {
                    list
                } else {
                    return Ok(body);
                }
            }
            Op::ModelGet => {
                let Some(target_model) = model_get_target_model(req) else {
                    return Ok(body);
                };
                if is_openai_model_value(&value) {
                    value
                } else if is_openai_model_list(&value) {
                    match find_model_value(&value, &target_model) {
                        Some(model) => model,
                        None => return Ok(body),
                    }
                } else if let Some(list) = normalize_codex_model_list(&value) {
                    match find_model_value(&list, &target_model) {
                        Some(model) => model,
                        None => return Ok(body),
                    }
                } else if let Some(model) = normalize_codex_model_get(&value) {
                    model
                } else {
                    return Ok(body);
                }
            }
            _ => return Ok(body),
        };

        serde_json::to_vec(&normalized)
            .map(Bytes::from)
            .map_err(|err| ProviderError::Other(err.to_string()))
    }
}

fn codex_base_url(config: &ProviderConfig) -> ProviderResult<&str> {
    match config {
        ProviderConfig::Codex(cfg) => Ok(cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::Codex".to_string(),
        )),
    }
}

fn codex_credential(credential: &Credential) -> ProviderResult<(&str, &str)> {
    match credential {
        Credential::Codex(cred) => Ok((cred.access_token.as_str(), cred.account_id.as_str())),
        _ => Err(ProviderError::InvalidConfig(
            "expected Credential::Codex".to_string(),
        )),
    }
}

fn codex_models_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{base}/models?client_version={CLIENT_VERSION}")
}

fn codex_responses_upstream_path(path: &str) -> ProviderResult<String> {
    let normalized = path.trim_start_matches('/');
    if let Some(rest) = normalized.strip_prefix("v1/responses") {
        return Ok(format!("/responses{rest}"));
    }
    if normalized == "responses" || normalized.starts_with("responses/") {
        return Ok(format!("/{normalized}"));
    }
    Err(ProviderError::Other(format!(
        "invalid openai responses path: {path}"
    )))
}

fn normalize_codex_input_json(obj: &mut serde_json::Map<String, JsonValue>) {
    let Some(input) = obj.remove("input") else {
        return;
    };
    if let Some(text) = input.as_str() {
        obj.insert(
            "input".to_string(),
            serde_json::json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": text
                }
            ]),
        );
        return;
    }
    obj.insert("input".to_string(), input);
}

fn ensure_codex_instructions_field_json(obj: &mut serde_json::Map<String, JsonValue>) {
    if !obj.contains_key("instructions") {
        obj.insert("instructions".to_string(), JsonValue::String(String::new()));
    }
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

fn local_json_request(body: Vec<u8>) -> UpstreamHttpRequest {
    let mut headers = Vec::new();
    auth_extractor::set_accept_json(&mut headers);
    auth_extractor::set_content_type_json(&mut headers);
    UpstreamHttpRequest {
        method: HttpMethod::Post,
        url: "local://codex".to_string(),
        headers,
        body: Some(Bytes::from(body)),
        is_stream: false,
    }
}

fn count_input_tokens(
    body: &openai::count_tokens::request::InputTokenCountRequestBody,
) -> ProviderResult<i64> {
    let bpe = bpe_for_model(&body.model)?;
    let mut total = 0i64;
    if let Some(input) = &body.input {
        total += count_input_param(input, &bpe);
    }
    if let Some(instructions) = &body.instructions {
        total += count_text(instructions, &bpe);
    }
    Ok(total)
}

fn bpe_for_model(model: &str) -> ProviderResult<CoreBPE> {
    if let Ok(bpe) = get_bpe_from_model(model) {
        return Ok(bpe);
    }
    o200k_base().map_err(|err| ProviderError::Other(err.to_string()))
}

fn count_input_param(input: &openai::create_response::types::InputParam, bpe: &CoreBPE) -> i64 {
    match input {
        openai::create_response::types::InputParam::Text(text) => count_text(text, bpe),
        openai::create_response::types::InputParam::Items(items) => {
            items.iter().map(|item| count_input_item(item, bpe)).sum()
        }
    }
}

fn count_input_item(item: &openai::create_response::types::InputItem, bpe: &CoreBPE) -> i64 {
    use openai::create_response::types::InputItem;
    match item {
        InputItem::EasyMessage(message) => count_easy_message(&message.content, bpe),
        InputItem::Reference(_) => 0,
        InputItem::Item(item) => count_item(item, bpe),
    }
}

fn count_easy_message(
    content: &openai::create_response::types::EasyInputMessageContent,
    bpe: &CoreBPE,
) -> i64 {
    match content {
        openai::create_response::types::EasyInputMessageContent::Text(text) => {
            count_text(text, bpe)
        }
        openai::create_response::types::EasyInputMessageContent::Parts(parts) => parts
            .iter()
            .map(|part| count_input_content(part, bpe))
            .sum(),
    }
}

fn count_item(item: &openai::create_response::types::Item, bpe: &CoreBPE) -> i64 {
    use openai::create_response::types::Item;
    match item {
        Item::InputMessage(message) => count_input_message(message, bpe),
        Item::OutputMessage(message) => count_output_message(message, bpe),
        Item::FunctionOutput(output) => count_tool_call_output(&output.output, bpe),
        Item::CustomToolCallOutput(output) => count_tool_call_output(&output.output, bpe),
        _ => 0,
    }
}

fn count_input_message(
    message: &openai::create_response::types::InputMessage,
    bpe: &CoreBPE,
) -> i64 {
    message
        .content
        .iter()
        .map(|part| count_input_content(part, bpe))
        .sum()
}

fn count_output_message(
    message: &openai::create_response::types::OutputMessage,
    bpe: &CoreBPE,
) -> i64 {
    use openai::create_response::types::OutputMessageContent;
    message
        .content
        .iter()
        .map(|part| match part {
            OutputMessageContent::OutputText(text) => count_text(&text.text, bpe),
            OutputMessageContent::Refusal(refusal) => count_text(&refusal.refusal, bpe),
        })
        .sum()
}

fn count_tool_call_output(
    output: &openai::create_response::types::ToolCallOutput,
    bpe: &CoreBPE,
) -> i64 {
    match output {
        openai::create_response::types::ToolCallOutput::Text(text) => count_text(text, bpe),
        openai::create_response::types::ToolCallOutput::Content(items) => items
            .iter()
            .map(|item| match item {
                openai::create_response::types::FunctionAndCustomToolCallOutput::InputText(
                    content,
                ) => count_text(&content.text, bpe),
                openai::create_response::types::FunctionAndCustomToolCallOutput::InputImage(_) => 0,
                openai::create_response::types::FunctionAndCustomToolCallOutput::InputFile(_) => 0,
            })
            .sum(),
    }
}

fn count_input_content(
    content: &openai::create_response::types::InputContent,
    bpe: &CoreBPE,
) -> i64 {
    match content {
        openai::create_response::types::InputContent::InputText(text) => {
            count_text(&text.text, bpe)
        }
        openai::create_response::types::InputContent::InputImage(_) => 0,
        openai::create_response::types::InputContent::InputFile(_) => 0,
    }
}

fn count_text(text: &str, bpe: &CoreBPE) -> i64 {
    bpe.encode_ordinary(text).len() as i64
}

fn is_openai_model_list(value: &JsonValue) -> bool {
    value
        .get("object")
        .and_then(|v| v.as_str())
        .map(|v| v == "list")
        .unwrap_or(false)
        && value.get("data").and_then(|v| v.as_array()).is_some()
}

fn is_openai_model_value(value: &JsonValue) -> bool {
    value
        .get("object")
        .and_then(|v| v.as_str())
        .map(|v| v == "model")
        .unwrap_or(false)
        && value.get("id").and_then(|v| v.as_str()).is_some()
        && value.get("owned_by").and_then(|v| v.as_str()).is_some()
}

fn normalize_codex_model_list(value: &JsonValue) -> Option<JsonValue> {
    let models = value.get("models")?.as_array()?;
    let data = models
        .iter()
        .filter_map(normalize_codex_model_value)
        .collect::<Vec<_>>();

    Some(serde_json::json!({
        "object": "list",
        "data": data,
    }))
}

fn normalize_codex_model_value(value: &JsonValue) -> Option<JsonValue> {
    let object = value.as_object()?;
    let id = object
        .get("id")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            object
                .get("slug")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })?;

    let created = object.get("created").and_then(|v| v.as_i64());
    let owned_by = object
        .get("owned_by")
        .and_then(|v| v.as_str())
        .unwrap_or("openai");
    let display_name = object
        .get("display_name")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    let mut model = serde_json::Map::new();
    model.insert("id".to_string(), JsonValue::String(id));
    model.insert("object".to_string(), JsonValue::String("model".to_string()));
    model.insert(
        "owned_by".to_string(),
        JsonValue::String(owned_by.to_string()),
    );
    if let Some(created) = created {
        model.insert("created".to_string(), JsonValue::Number(created.into()));
    }
    if let Some(display_name) = display_name {
        model.insert("display_name".to_string(), JsonValue::String(display_name));
    }

    Some(JsonValue::Object(model))
}

fn normalize_codex_model_get(value: &JsonValue) -> Option<JsonValue> {
    if let Some(model) = normalize_codex_model_value(value) {
        return Some(model);
    }

    if let Some(model) = value.get("model").and_then(normalize_codex_model_value) {
        return Some(model);
    }

    if let Some(data) = value.get("data").and_then(|v| v.as_array())
        && data.len() == 1
    {
        if is_openai_model_value(&data[0]) {
            return Some(data[0].clone());
        }
        if let Some(model) = normalize_codex_model_value(&data[0]) {
            return Some(model);
        }
    }

    if let Some(models) = value.get("models").and_then(|v| v.as_array())
        && models.len() == 1
    {
        return normalize_codex_model_value(&models[0]);
    }

    None
}

fn find_model_value(list: &JsonValue, target: &str) -> Option<JsonValue> {
    let data = list.get("data")?.as_array()?;
    data.iter()
        .find(|item| {
            item.get("id")
                .and_then(|value| value.as_str())
                .map(|id| normalize_model_id(id) == target)
                .unwrap_or(false)
        })
        .cloned()
}

fn model_get_target_model(req: &Request) -> Option<String> {
    match req {
        Request::ModelGet(gproxy_provider_core::ModelGetRequest::OpenAI(inner)) => {
            Some(normalize_model_id(&inner.path.model))
        }
        _ => None,
    }
}

fn request_has_compact_marker(req: &Request) -> bool {
    match req {
        Request::GenerateContent(gproxy_provider_core::GenerateContentRequest::OpenAIResponse(
            inner,
        )) => has_compact_marker(inner.body.metadata.as_ref()),
        _ => false,
    }
}

fn request_model(req: &Request) -> Option<String> {
    match req {
        Request::GenerateContent(gproxy_provider_core::GenerateContentRequest::OpenAIResponse(
            inner,
        )) => Some(inner.body.model.clone()),
        _ => None,
    }
}

fn has_compact_marker(metadata: Option<&Metadata>) -> bool {
    metadata
        .and_then(|meta| meta.get(GPROXY_COMPACT_METADATA_KEY))
        .map(|value| value == GPROXY_COMPACT_METADATA_VALUE)
        .unwrap_or(false)
}

fn take_compact_marker(metadata: &mut Option<Metadata>) -> bool {
    let Some(meta) = metadata.as_mut() else {
        return false;
    };
    let is_compact = meta
        .remove(GPROXY_COMPACT_METADATA_KEY)
        .map(|value| value == GPROXY_COMPACT_METADATA_VALUE)
        .unwrap_or(false);
    if meta.is_empty() {
        *metadata = None;
    }
    is_compact
}

fn normalize_codex_compact_response_for_decode(
    body: &Bytes,
    req: &Request,
) -> ProviderResult<Bytes> {
    let Ok(value) = serde_json::from_slice::<JsonValue>(body) else {
        return Ok(body.clone());
    };

    if is_openai_response_value(&value) {
        return Ok(body.clone());
    }

    let Some(output) = value.get("output") else {
        return Ok(body.clone());
    };

    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("resp_compact");
    let created_at = value
        .get("created_at")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(current_unix_ts);
    let model = request_model(req).unwrap_or_else(|| "unknown".to_string());

    let mut wrapped = serde_json::Map::new();
    wrapped.insert("id".to_string(), JsonValue::String(id.to_string()));
    wrapped.insert(
        "object".to_string(),
        JsonValue::String("response".to_string()),
    );
    wrapped.insert(
        "created_at".to_string(),
        JsonValue::Number(serde_json::Number::from(created_at)),
    );
    wrapped.insert(
        "status".to_string(),
        JsonValue::String("completed".to_string()),
    );
    wrapped.insert("model".to_string(), JsonValue::String(model));
    wrapped.insert("output".to_string(), output.clone());

    if let Some(usage) = value.get("usage") {
        wrapped.insert("usage".to_string(), usage.clone());
    }

    serde_json::to_vec(&JsonValue::Object(wrapped))
        .map(Bytes::from)
        .map_err(|err| ProviderError::Other(err.to_string()))
}

fn is_openai_response_value(value: &JsonValue) -> bool {
    value
        .get("object")
        .and_then(|v| v.as_str())
        .map(|v| v == "response")
        .unwrap_or(false)
        && value.get("id").and_then(|v| v.as_str()).is_some()
        && value.get("model").and_then(|v| v.as_str()).is_some()
        && value.get("created_at").and_then(|v| v.as_i64()).is_some()
}

fn current_unix_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn normalize_model_id(model: &str) -> String {
    let model = model.trim_start_matches('/');
    model.strip_prefix("models/").unwrap_or(model).to_string()
}

fn normalize_codex_input(body: &mut openai::create_response::request::CreateResponseRequestBody) {
    let Some(input) = body.input.take() else {
        return;
    };

    body.input = Some(match input {
        InputParam::Text(text) => {
            InputParam::Items(vec![InputItem::EasyMessage(EasyInputMessage {
                r#type: EasyInputMessageType::Message,
                role: EasyInputMessageRole::User,
                content: EasyInputMessageContent::Text(text),
            })])
        }
        InputParam::Items(items) => InputParam::Items(items),
    });
}

fn ensure_codex_instructions_field(
    body: &mut openai::create_response::request::CreateResponseRequestBody,
) {
    if body.instructions.is_none() {
        body.instructions = Some(Instructions::Text(String::new()));
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

fn generate_oauth_state() -> String {
    let mut state_bytes = [0u8; 32];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut state_bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state_bytes)
}

fn parse_id_token_claims(id_token: &str) -> IdTokenClaims {
    let mut claims = IdTokenClaims::default();
    let mut parts = id_token.split('.');
    let (_h, payload_b64, _s) = match (parts.next(), parts.next(), parts.next()) {
        (Some(h), Some(p), Some(s)) if !h.is_empty() && !p.is_empty() && !s.is_empty() => (h, p, s),
        _ => return claims,
    };
    let payload_bytes = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload_b64) {
        Ok(bytes) => bytes,
        Err(_) => return claims,
    };
    let payload = match serde_json::from_slice::<JsonValue>(&payload_bytes) {
        Ok(value) => value,
        Err(_) => return claims,
    };

    let email = payload
        .get("email")
        .and_then(|value| value.as_str())
        .or_else(|| {
            payload
                .get("https://api.openai.com/profile")
                .and_then(|profile| profile.get("email"))
                .and_then(|value| value.as_str())
        })
        .map(|value| value.to_string());

    let (plan, account_id) = payload
        .get("https://api.openai.com/auth")
        .map(|auth| {
            let plan = auth
                .get("chatgpt_plan_type")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string());
            let account_id = auth
                .get("chatgpt_account_id")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string());
            (plan, account_id)
        })
        .unwrap_or((None, None));

    claims.email = email;
    claims.plan = plan;
    claims.account_id = account_id;
    claims
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_codex_models_payload_into_openai_list() {
        let input = serde_json::json!({
            "models": [
                {
                    "slug": "gpt-5.3-codex",
                    "display_name": "GPT 5.3 Codex",
                    "created": 1770249600
                },
                {
                    "id": "gpt-5.2-codex"
                }
            ]
        });

        let normalized = normalize_codex_model_list(&input).expect("should normalize");
        let data = normalized
            .get("data")
            .and_then(|v| v.as_array())
            .expect("data array");

        assert_eq!(
            normalized.get("object").and_then(|v| v.as_str()),
            Some("list")
        );
        assert_eq!(data.len(), 2);
        assert_eq!(
            data[0].get("id").and_then(|v| v.as_str()),
            Some("gpt-5.3-codex")
        );
        assert_eq!(
            data[0].get("object").and_then(|v| v.as_str()),
            Some("model")
        );
        assert_eq!(
            data[0].get("owned_by").and_then(|v| v.as_str()),
            Some("openai")
        );
        assert_eq!(
            data[0].get("created").and_then(|v| v.as_i64()),
            Some(1770249600)
        );
        assert_eq!(
            data[0].get("display_name").and_then(|v| v.as_str()),
            Some("GPT 5.3 Codex")
        );
        assert_eq!(
            data[1].get("id").and_then(|v| v.as_str()),
            Some("gpt-5.2-codex")
        );
    }

    #[test]
    fn accepts_openai_model_list_shape() {
        let value = serde_json::json!({
            "object": "list",
            "data": [
                { "id": "gpt-5", "object": "model", "owned_by": "openai" }
            ]
        });
        assert!(is_openai_model_list(&value));
    }

    #[test]
    fn normalizes_codex_model_get_payload() {
        let input = serde_json::json!({
            "slug": "gpt-5.3-codex",
            "display_name": "GPT 5.3 Codex"
        });
        let normalized = normalize_codex_model_get(&input).expect("should normalize");
        assert_eq!(
            normalized.get("id").and_then(|v| v.as_str()),
            Some("gpt-5.3-codex")
        );
        assert_eq!(
            normalized.get("object").and_then(|v| v.as_str()),
            Some("model")
        );
        assert_eq!(
            normalized.get("owned_by").and_then(|v| v.as_str()),
            Some("openai")
        );
    }

    #[test]
    fn normalizes_model_id_path_prefix() {
        assert_eq!(normalize_model_id("models/gpt-5"), "gpt-5");
        assert_eq!(normalize_model_id("/models/gpt-5"), "gpt-5");
        assert_eq!(normalize_model_id("gpt-5"), "gpt-5");
    }

    #[test]
    fn codex_models_url_appends_client_version() {
        let url = codex_models_url("https://chatgpt.com/backend-api/codex/");
        assert!(url.starts_with("https://chatgpt.com/backend-api/codex/models?client_version="));
        assert!(url.ends_with(CLIENT_VERSION));
    }

    #[test]
    fn takes_compact_marker_and_cleans_metadata() {
        let mut metadata = Metadata::new();
        metadata.insert(
            GPROXY_COMPACT_METADATA_KEY.to_string(),
            GPROXY_COMPACT_METADATA_VALUE.to_string(),
        );
        let mut metadata = Some(metadata);

        let taken = take_compact_marker(&mut metadata);
        assert!(taken);
        assert!(metadata.is_none());
    }

    #[test]
    fn ensure_codex_instructions_field_fills_empty_text_when_missing() {
        let mut body = openai::create_response::request::CreateResponseRequestBody {
            model: "gpt-5".to_string(),
            input: None,
            include: None,
            parallel_tool_calls: None,
            store: None,
            instructions: None,
            stream: Some(false),
            stream_options: None,
            conversation: None,
            previous_response_id: None,
            reasoning: None,
            background: None,
            max_output_tokens: None,
            max_tool_calls: None,
            text: None,
            tools: None,
            tool_choice: None,
            prompt: None,
            truncation: None,
            top_logprobs: None,
            metadata: None,
            temperature: None,
            top_p: None,
            user: None,
            safety_identifier: None,
            prompt_cache_key: None,
            service_tier: None,
            prompt_cache_retention: None,
        };

        ensure_codex_instructions_field(&mut body);

        match body.instructions {
            Some(Instructions::Text(text)) => assert_eq!(text, ""),
            _ => panic!("instructions should be empty text"),
        }
    }

    #[test]
    fn wraps_compact_response_for_openai_decode() {
        let req = Request::GenerateContent(
            gproxy_provider_core::GenerateContentRequest::OpenAIResponse(
                openai::create_response::request::CreateResponseRequest {
                    body: openai::create_response::request::CreateResponseRequestBody {
                        model: "gpt-5".to_string(),
                        input: None,
                        include: None,
                        parallel_tool_calls: None,
                        store: None,
                        instructions: None,
                        stream: Some(false),
                        stream_options: None,
                        conversation: None,
                        previous_response_id: None,
                        reasoning: None,
                        background: None,
                        max_output_tokens: None,
                        max_tool_calls: None,
                        text: None,
                        tools: None,
                        tool_choice: None,
                        prompt: None,
                        truncation: None,
                        top_logprobs: None,
                        metadata: Some({
                            let mut meta = Metadata::new();
                            meta.insert(
                                GPROXY_COMPACT_METADATA_KEY.to_string(),
                                GPROXY_COMPACT_METADATA_VALUE.to_string(),
                            );
                            meta
                        }),
                        temperature: None,
                        top_p: None,
                        user: None,
                        safety_identifier: None,
                        prompt_cache_key: None,
                        service_tier: None,
                        prompt_cache_retention: None,
                    },
                },
            ),
        );
        let compact = serde_json::json!({
            "output": [
                {
                    "type": "message",
                    "id": "m1",
                    "role": "assistant",
                    "content": []
                }
            ]
        });
        let normalized = normalize_codex_compact_response_for_decode(
            &Bytes::from(serde_json::to_vec(&compact).unwrap()),
            &req,
        )
        .expect("normalize compact");
        let value: JsonValue = serde_json::from_slice(&normalized).expect("decode json");
        assert_eq!(
            value.get("object").and_then(|v| v.as_str()),
            Some("response")
        );
        assert_eq!(value.get("model").and_then(|v| v.as_str()), Some("gpt-5"));
        assert!(value.get("output").is_some());
    }
}
