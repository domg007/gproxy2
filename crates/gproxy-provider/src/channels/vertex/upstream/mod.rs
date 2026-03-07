use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformResponse};
use serde_json::{Map, Value, json};
use wreq::{Client as WreqClient, Method as WreqMethod, Response as WreqResponse};

use super::constants::DEFAULT_LOCATION;
use super::oauth::{resolve_vertex_access_token, vertex_auth_material_from_credential};
use crate::channels::retry::{
    CacheAffinityProtocol, CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    cache_affinity_protocol_from_transform_request, configured_pick_mode_uses_cache,
    credential_pick_mode, retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamRequestMeta, UpstreamResponse,
    add_or_replace_header, extra_headers_from_payload_value, extra_headers_from_transform_request,
    merge_extra_headers, payload_body_value,
};
use crate::channels::utils::{
    default_gproxy_user_agent, gemini_model_list_query_string, is_auth_failure,
    is_transient_server_failure, join_base_url_and_path, resolve_user_agent_or_else,
    retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

mod entry;
pub use entry::{execute_vertex_payload_with_retry, execute_vertex_with_retry};
mod helpers;
pub use helpers::normalize_vertex_upstream_response_body;
use helpers::*;
mod prepared;
mod request;
use request::*;
