use std::time::{SystemTime, UNIX_EPOCH};

use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformResponse};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use wreq::{Client as WreqClient, Method as WreqMethod, Response as WreqResponse};

use super::constants::ANTIGRAVITY_USER_AGENT;
use super::oauth::{
    AntigravityRefreshedToken, antigravity_auth_material_from_credential,
    resolve_antigravity_access_token,
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
    gemini_model_list_query_string, is_transient_server_failure, join_base_url_and_path,
    resolve_user_agent_or_default, retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

type ParsedGeminiPayload = (Option<String>, Option<Value>, Option<String>);

fn is_antigravity_auth_failure(status_code: u16) -> bool {
    status_code == 401
}

#[derive(Debug, Clone)]
enum AntigravityRequestKind {
    ModelList {
        page_size: Option<u32>,
        page_token: Option<String>,
    },
    ModelGet {
        target: String,
    },
    Forward {
        requires_project: bool,
        request_type: Option<&'static str>,
    },
}

#[derive(Debug, Clone)]
struct AntigravityPreparedRequest {
    method: WreqMethod,
    path: String,
    query: Option<String>,
    body: Option<Value>,
    model: Option<String>,
    kind: AntigravityRequestKind,
    extra_headers: Vec<(String, String)>,
}

mod entry;
pub use entry::{execute_antigravity_payload_with_retry, execute_antigravity_with_retry};
mod helpers;
use helpers::*;
pub use helpers::{
    normalize_antigravity_upstream_response_body,
    normalize_antigravity_upstream_stream_ndjson_chunk,
};
mod prepared;
mod request;
use request::*;
mod transport;
use transport::*;
#[cfg(test)]
mod tests;
mod usage;
pub use usage::execute_antigravity_upstream_usage_with_retry;
