use bytes::Bytes;

use gproxy_provider_core::{
    Credential, DispatchRule, DispatchTable, HttpMethod, Proto, ProviderConfig, ProviderError,
    ProviderResult, UpstreamCtx, UpstreamHttpRequest, UpstreamProvider,
    credential::ApiKeyCredential,
};

use crate::auth_extractor;

const PROVIDER_NAME: &str = "openai";
const DEFAULT_BASE_URL: &str = "https://api.openai.com";

const DISPATCH_TABLE: DispatchTable = DispatchTable::new([
    // Claude
    DispatchRule::Transform {
        target: Proto::OpenAIResponse,
    },
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
    DispatchRule::Transform {
        target: Proto::OpenAIResponse,
    },
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
    DispatchRule::Native,
    DispatchRule::Native,
    // OpenAI Responses
    DispatchRule::Native,
    DispatchRule::Native,
    // OpenAI basic ops
    DispatchRule::Native,
    DispatchRule::Native,
    DispatchRule::Native,
    // OAuth / usage (not implemented for this provider)
    DispatchRule::Unsupported,
    DispatchRule::Unsupported,
    DispatchRule::Unsupported,
]);

#[derive(Debug, Default)]
pub struct OpenAIProvider;

impl OpenAIProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl UpstreamProvider for OpenAIProvider {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    fn dispatch_table(&self, _config: &ProviderConfig) -> DispatchTable {
        DISPATCH_TABLE
    }

    async fn build_openai_models_list(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        _req: &gproxy_protocol::openai::list_models::request::ListModelsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');

        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };

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
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');

        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };

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

    async fn build_openai_input_tokens(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::count_tokens::request::InputTokenCountRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');

        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };

        let url = build_url(
            Some(base_url),
            DEFAULT_BASE_URL,
            "/v1/responses/input_tokens",
        );
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
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');

        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };

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

    async fn build_openai_responses(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::create_response::request::CreateResponseRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');

        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };

        let url = build_url(Some(base_url), DEFAULT_BASE_URL, "/v1/responses");
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

    async fn build_openai_response_get(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::get_response::request::GetResponseRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');
        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };
        let mut path = format!("/v1/responses/{}", req.path.response_id);
        let query = serde_urlencoded::to_string(&req.query)
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query);
        }
        let url = build_url(Some(base_url), DEFAULT_BASE_URL, &path);
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

    async fn build_openai_response_delete(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::delete_response::request::DeleteResponseRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');
        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };
        let url = build_url(
            Some(base_url),
            DEFAULT_BASE_URL,
            &format!("/v1/responses/{}", req.path.response_id),
        );
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, api_key);
        auth_extractor::set_accept_json(&mut headers);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Delete,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    async fn build_openai_response_cancel(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::cancel_response::request::CancelResponseRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');
        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };
        let url = build_url(
            Some(base_url),
            DEFAULT_BASE_URL,
            &format!("/v1/responses/{}/cancel", req.path.response_id),
        );
        let mut headers = Vec::new();
        auth_extractor::set_bearer(&mut headers, api_key);
        auth_extractor::set_accept_json(&mut headers);
        Ok(UpstreamHttpRequest {
            method: HttpMethod::Post,
            url,
            headers,
            body: None,
            is_stream: false,
        })
    }

    async fn build_openai_response_list_input_items(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::list_input_items::request::ListInputItemsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');
        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };
        let mut path = format!("/v1/responses/{}/input_items", req.path.response_id);
        let query = serde_urlencoded::to_string(&req.query)
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query);
        }
        let url = build_url(Some(base_url), DEFAULT_BASE_URL, &path);
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

    async fn build_openai_response_compact(
        &self,
        _ctx: &UpstreamCtx,
        config: &ProviderConfig,
        credential: &Credential,
        req: &gproxy_protocol::openai::compact_response::request::CompactResponseRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        let base_url = match config {
            ProviderConfig::OpenAI(cfg) => cfg.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::OpenAI".to_string(),
                ));
            }
        };
        let base_url = base_url.trim_end_matches('/');
        let api_key = match credential {
            Credential::OpenAI(ApiKeyCredential { api_key }) => api_key.as_str(),
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected Credential::OpenAI".to_string(),
                ));
            }
        };
        let url = build_url(Some(base_url), DEFAULT_BASE_URL, "/v1/responses/compact");
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
            is_stream: false,
        })
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
