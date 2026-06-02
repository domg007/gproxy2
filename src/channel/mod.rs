//! Upstream transport: a client-agnostic UpstreamClient trait with a
//! native (wreq) and an edge (fetch) implementation, selected by build target.

mod client;
pub use client::{ChannelError, UpstreamClient};

#[cfg(not(target_arch = "wasm32"))]
mod wreq_client;
#[cfg(not(target_arch = "wasm32"))]
pub use wreq_client::WreqClient;

#[cfg(target_arch = "wasm32")]
mod fetch_client;
#[cfg(target_arch = "wasm32")]
pub use fetch_client::FetchClient;
