use std::time::{SystemTime, UNIX_EPOCH};

use gproxy_middleware::{
    OperationFamily, ProtocolKind, TransformRequest, TransformResponse, TransformRoute,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    ACCOUNT_ID_HEADER, CLIENT_VERSION, ORIGINATOR_HEADER, ORIGINATOR_VALUE, USER_AGENT_HEADER,
    USER_AGENT_VALUE,
};
use super::oauth::{
    CodexRefreshedToken, codex_auth_material_from_credential, resolve_codex_access_token,
};
use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_codex_openai_response_body,
    cache_affinity_hint_from_codex_transform_request, configured_pick_mode_uses_cache,
    credential_pick_mode, retry_with_eligible_credentials,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamRequestMeta, UpstreamResponse,
    add_or_replace_header, extra_headers_from_payload_value, extra_headers_from_transform_request,
    merge_extra_headers, payload_body_value,
};
use crate::channels::utils::{
    count_openai_input_tokens_with_resolution, is_auth_failure, is_transient_server_failure,
    join_base_url_and_path, resolve_user_agent_or_default, retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, RetryWithPayloadRequest, TokenizerResolutionContext};

#[derive(Debug, Clone)]
enum CodexRequestKind {
    ModelList,
    ModelGet { target: String },
    Forward,
}

const SESSION_ID_HEADER: &str = "session_id";
const SESSION_ID_ALT_HEADER: &str = "session-id";

#[derive(Debug, Clone)]
struct CodexPreparedRequest {
    method: WreqMethod,
    path: String,
    body: Option<Vec<u8>>,
    model: Option<String>,
    kind: CodexRequestKind,
    extra_headers: Vec<(String, String)>,
}

struct CodexRequestParams<'a> {
    method: WreqMethod,
    url: &'a str,
    access_token: &'a str,
    account_id: &'a str,
    user_agent: &'a str,
    extra_headers: &'a [(String, String)],
    body: Option<&'a [u8]>,
}

mod entry;
pub use entry::{execute_codex_payload_with_retry, execute_codex_with_retry};
mod helpers;
use helpers::*;
mod prepared;
mod request;
use request::*;
mod transport;
use transport::*;
#[cfg(test)]
mod tests;
mod usage;
pub use usage::execute_codex_upstream_usage_with_retry;
