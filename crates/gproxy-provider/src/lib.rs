pub mod channel;
mod channels;
pub mod credential;
pub mod credential_state;
pub mod dispatch;
pub mod facade;
pub mod provider;
mod registry;
pub mod settings;
pub mod storage_codec;
pub mod tokenizers;

pub use channel::{BUILTIN_CHANNELS, BuiltinChannel, ChannelId};
pub use channels::retry::CredentialPickMode;
pub use channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamOAuthCallbackResult, UpstreamOAuthCredential,
    UpstreamOAuthRequest, UpstreamOAuthResponse, UpstreamRequestMeta, UpstreamResponse,
};
pub use channels::utils::parse_query_value;
pub use channels::{
    BuiltinChannelCredential, BuiltinChannelSettings, ChannelCredential, ChannelSettings,
    custom::CustomChannelCredential, custom::CustomChannelSettings,
};
pub use credential::{
    ChannelCredentialState, ChannelCredentialStateMap, ChannelCredentialStateStore,
    CredentialHealth, CredentialHealthKind, CredentialRef, ModelCooldown, ProviderCredentialState,
};
pub use credential_state::{
    CredentialStateManager, DEFAULT_RATE_LIMIT_COOLDOWN_MS, DEFAULT_TRANSIENT_COOLDOWN_MS,
};
pub use dispatch::{DispatchRule, ProviderDispatchTable, RouteImplementation, RouteKey};
pub use facade::{
    DefaultProviderRuntime, ExecuteInput, OAuthInput, ProviderRuntime, UpstreamUsageInput,
    credential_from_secret, ensure_project_id_for_credential,
    normalize_upstream_response_body_for_channel,
    normalize_upstream_stream_ndjson_chunk_for_channel, try_local_response_for_channel,
};
pub use provider::{ProviderDefinition, ProviderRegistry, TokenizerResolutionContext};
pub use settings::{
    CACHE_AFFINITY_ENABLED_KEY, CREDENTIAL_PICK_MODE_KEY,
    parse_credential_pick_mode_from_provider_settings_value,
    parse_provider_settings_json_for_channel, parse_provider_settings_value_for_channel,
    provider_settings_to_json_string, provider_settings_to_json_string_with_cache_affinity,
    provider_settings_to_json_string_with_credential_pick_mode, provider_settings_to_json_value,
    provider_settings_to_json_value_with_cache_affinity,
    provider_settings_to_json_value_with_credential_pick_mode,
};
pub use storage_codec::{
    credential_health_from_storage, credential_health_to_storage, credential_kind_for_storage,
};
pub use tokenizers::{
    LocalTokenCount, LocalTokenizerBackend, LocalTokenizerError, LocalTokenizerStore,
};
