use gproxy_middleware::{OperationFamily, ProtocolKind, TransformResponse};
use serde_json::{Value, json};
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    CLAUDE_CODE_UA, DEFAULT_CLAUDE_AI_BASE_URL, DEFAULT_PLATFORM_BASE_URL, OAUTH_BETA,
};
use super::oauth::{
    ClaudeCodeRefreshedToken, claudecode_access_token_from_credential,
    resolve_claudecode_access_token,
};
use crate::channels::cache_control::{
    CacheBreakpointRule, apply_magic_string_cache_control_triggers, ensure_cache_breakpoint_rules,
};
use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode, retry_with_eligible_credentials,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamRequestMeta, UpstreamResponse,
    add_or_replace_header, extra_headers_from_payload_value, extra_headers_from_transform_request,
    merge_extra_headers, payload_header_string, payload_header_string_array,
};
use crate::channels::utils::{
    anthropic_header_pairs, append_query_param_if_missing, claude_model_list_query_string,
    claude_model_to_string, is_auth_failure, is_transient_server_failure, join_base_url_and_path,
    retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, RetryWithPayloadRequest};

const BETA_QUERY_KEY: &str = "beta";
const BETA_QUERY_VALUE: &str = "true";
const CLAUDECODE_THINKING_MODEL_SUFFIX: &str = "-thinking";
const CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX: &str = "-adaptive-thinking";
const CLAUDECODE_THINKING_BUDGET_TOKENS: u64 = 4_096;

struct ClaudeCodeRequestParams<'a> {
    method: WreqMethod,
    url: &'a str,
    access_token: &'a str,
    user_agent: &'a str,
    extra_headers: &'a [(String, String)],
    request_headers: &'a [(String, String)],
    body: Option<&'a [u8]>,
}

mod entry;
pub use entry::{execute_claudecode_payload_with_retry, execute_claudecode_with_retry};
mod helpers;
use helpers::*;
mod prepared;
use prepared::*;
mod request;
use request::*;
mod transport;
use transport::*;
#[cfg(test)]
mod tests;
mod usage;
pub use usage::execute_claudecode_upstream_usage_with_retry;
