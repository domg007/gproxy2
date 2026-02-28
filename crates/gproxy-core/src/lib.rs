//! Core domain and orchestration primitives for gproxy.

pub mod app_state;
pub mod http_clients;
pub mod routes;

pub use app_state::{AppState, AppStateInit, GlobalSettings, RuntimeConfigSnapshot};
pub use http_clients::UpstreamHttpClients;
pub use routes::management_router;
