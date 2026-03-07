use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformResponse};
use serde_json::{Map, Value, json};
use wreq::{Client as WreqClient, Method as WreqMethod, Response as WreqResponse};

use super::constants::geminicli_user_agent;
use super::oauth::{
    GeminiCliRefreshedToken, geminicli_auth_material_from_credential,
    resolve_geminicli_access_token,
};
use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode, retry_with_eligible_credentials,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamRequestMeta, UpstreamResponse,
    add_or_replace_header, extra_headers_from_payload_value, extra_headers_from_transform_request,
    merge_extra_headers,
};
use crate::channels::utils::{
    is_auth_failure, is_transient_server_failure, join_base_url_and_path, retry_after_to_millis,
    to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

type ParsedGeminiPayload = (Option<String>, Option<Value>, Option<String>);

#[derive(Debug, Clone)]
enum GeminiCliRequestKind {
    LocalModelList {
        page_size: Option<u32>,
        page_token: Option<String>,
    },
    LocalModelGet {
        target: String,
    },
    Forward {
        requires_project: bool,
    },
}

#[derive(Debug, Clone)]
struct GeminiCliPreparedRequest {
    method: WreqMethod,
    path: String,
    query: Option<String>,
    body: Option<Value>,
    model: Option<String>,
    kind: GeminiCliRequestKind,
    extra_headers: Vec<(String, String)>,
}

struct GeminiCliRequestParams<'a> {
    method: WreqMethod,
    url: &'a str,
    access_token: &'a str,
    custom_user_agent: Option<&'a str>,
    model_for_ua: Option<&'a str>,
    extra_headers: &'a [(String, String)],
    body: Option<&'a [u8]>,
}

mod entry;
pub use entry::{execute_geminicli_payload_with_retry, execute_geminicli_with_retry};
mod helpers;
use helpers::*;
pub use helpers::{
    normalize_geminicli_upstream_response_body, normalize_geminicli_upstream_stream_ndjson_chunk,
};
mod prepared;
mod request;
use request::*;
mod transport;
use transport::*;
#[cfg(test)]
mod tests;
mod usage;
pub use usage::execute_geminicli_upstream_usage_with_retry;
use usage::*;
