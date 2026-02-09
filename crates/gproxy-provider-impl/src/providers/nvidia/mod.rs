use bytes::Bytes;

use gproxy_provider_core::{
    Credential, DispatchRule, DispatchTable, HttpMethod, Proto, ProviderConfig, ProviderError,
    ProviderResult, UpstreamCtx, UpstreamHttpRequest, UpstreamProvider,
    credential::ApiKeyCredential,
};

use crate::auth_extractor;

mod tokenizer;

const PROVIDER_NAME: &str = "nvidia";
const DEFAULT_BASE_URL: &str = "https://integrate.api.nvidia.com";

// Mirrors `samples/crates/gproxy-provider-impl/src/provider/nvidia/mod.rs` dispatch semantics.
const DISPATCH_TABLE: DispatchTable = DispatchTable::new([
    // Claude
    DispatchRule::Transform {
        target: Proto::OpenAIChat,
    },
    DispatchRule::Transform {
        target: Proto::OpenAIChat,
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
    DispatchRule::Transform {
        target: Proto::OpenAIChat,
    },
    DispatchRule::Transform {
        target: Proto::OpenAIChat,
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
    DispatchRule::Native,
    DispatchRule::Native,
    // OpenAI Responses (map to chat completions)
    DispatchRule::Transform {
        target: Proto::OpenAIChat,
    },
    DispatchRule::Transform {
        target: Proto::OpenAIChat,
    },
    // OpenAI basic ops
    DispatchRule::Native,
    DispatchRule::Native,
    DispatchRule::Native,
    // OAuth / usage (not implemented)
    DispatchRule::Unsupported,
    DispatchRule::Unsupported,
    DispatchRule::Unsupported,
]);

#[derive(Debug, Default)]
pub struct NvidiaProvider;

impl NvidiaProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for NvidiaProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn dispatch_table(&self, _config: &ProviderConfig) -> DispatchTable {
        DISPATCH_TABLE
    }

    async fn build_openai_chat(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::create_chat_completions::request::CreateChatCompletionRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = nvidia_base_url(config)?;
        let api_key = nvidia_api_key(credential)?;
        let url = build_url(Some(base_url), DEFAULT_BASE_URL, "/v1/chat/completions");
        let is_stream = req.body.stream.unwrap_or(false);
        let body =
            serde_json::to_vec(&req.body).map_err(|err| ProviderError::Other(err.to_string()))?;
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, api_key);
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

    async fn build_openai_input_tokens(
        &self,
        ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::count_tokens::request::InputTokenCountRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let _ = nvidia_api_key(credential)?;
        let hf_token = nvidia_hf_token(config);
        let hf_url = nvidia_hf_url(config);
        let data_dir = nvidia_data_dir(config);
        let tokens = tokenizer::count_input_tokens(
            ctx,
            &req.body.model,
            &req.body,
            hf_token,
            hf_url,
            data_dir,
        )
        .map_err(|err| ProviderError::Other(err.to_string()))?;
        let response = gproxy_protocol::openai::count_tokens::response::InputTokenCountResponse {
            object: gproxy_protocol::openai::count_tokens::types::InputTokenObjectType::ResponseInputTokens,
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
        let base_url = nvidia_base_url(config)?;
        let api_key = nvidia_api_key(credential)?;
        let url = build_url(Some(base_url), DEFAULT_BASE_URL, "/v1/models");
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
        let base_url = nvidia_base_url(config)?;
        let api_key = nvidia_api_key(credential)?;
        let url = build_url(
            Some(base_url),
            DEFAULT_BASE_URL,
            &format!("/v1/models/{}", req.path.model),
        );
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
}

fn nvidia_base_url(config: &ProviderConfig) -> ProviderResult<&str> {
    match config {
        ProviderConfig::Nvidia(cfg) => Ok(cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL)),
        _ => Err(ProviderError::InvalidConfig(
            "expected ProviderConfig::Nvidia".to_string(),
        )),
    }
}

fn nvidia_api_key(credential: &Credential) -> ProviderResult<&str> {
    match credential {
        Credential::Nvidia(ApiKeyCredential { api_key }) => Ok(api_key.as_str()),
        _ => Err(ProviderError::InvalidConfig(
            "expected Credential::Nvidia".to_string(),
        )),
    }
}

fn nvidia_hf_token(config: &ProviderConfig) -> Option<&str> {
    match config {
        ProviderConfig::Nvidia(cfg) => cfg.hf_token.as_deref(),
        _ => None,
    }
}

fn nvidia_hf_url(config: &ProviderConfig) -> Option<&str> {
    match config {
        ProviderConfig::Nvidia(cfg) => cfg.hf_url.as_deref(),
        _ => None,
    }
}

fn nvidia_data_dir(config: &ProviderConfig) -> Option<&str> {
    match config {
        ProviderConfig::Nvidia(cfg) => cfg.data_dir.as_deref(),
        _ => None,
    }
}

fn local_json_request(body: Vec<u8>) -> UpstreamHttpRequest {
    let mut headers = Vec::new();
    auth_extractor::set_accept_json(&mut headers);
    auth_extractor::set_content_type_json(&mut headers);
    UpstreamHttpRequest {
        method: HttpMethod::Post,
        url: "local://nvidia".to_string(),
        headers,
        body: Some(Bytes::from(body)),
        is_stream: false,
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
