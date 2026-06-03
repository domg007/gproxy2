//! HTTP surface, split by direction:
//! - [`client`] — outbound transport to upstreams (shared; wreq native / fetch edge)
//! - [`server`] — inbound axum router + handlers (shared; native serve + wasm edge)
//! - [`edge`] — inbound WinterCG `fetch` entry (wasm)

pub mod client;
pub mod server;

#[cfg(target_arch = "wasm32")]
pub mod edge;
