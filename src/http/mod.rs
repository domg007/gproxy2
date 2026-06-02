//! HTTP surface, split by direction:
//! - [`client`] — outbound transport to upstreams (shared; wreq native / fetch edge)
//! - [`server`] — inbound axum router + handlers (native)
//! - [`edge`] — inbound WinterCG `fetch` entry (wasm)

pub mod client;

#[cfg(not(target_arch = "wasm32"))]
pub mod server;

#[cfg(target_arch = "wasm32")]
pub mod edge;
