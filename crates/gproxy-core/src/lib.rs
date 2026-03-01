//! Core domain and orchestration primitives for gproxy.

pub mod app_state;
pub mod http_clients;
pub mod routes;
pub mod upstream_http;

pub use app_state::{AppState, AppStateInit, GlobalSettings, RuntimeConfigSnapshot};
pub use http_clients::UpstreamHttpClients;
pub use routes::management_router;
pub use upstream_http::{
    DEFAULT_SPOOF_EMULATION, SpoofEmulation, UpstreamHttpClientBuildError,
    build_claudecode_spoof_client, build_http_client, normalize_spoof_emulation,
};
