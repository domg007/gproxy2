//! HTTP surface, split by direction:
//! - [`client`] — outbound transport to upstreams (shared; wreq native / fetch edge)
//! - [`server`] — inbound axum router + handlers (shared; native serve + wasm edge)
//! - [`edge`] — inbound WinterCG `fetch` entry (wasm)

pub mod client;
pub mod server;

// The edge entry wires all edge backends together (runtime-selected), so it
// requires the full edge feature bundle; build with `--features edge`.
#[cfg(all(
    target_arch = "wasm32",
    feature = "persist-libsql",
    feature = "cache-libsql",
    feature = "cache-upstash",
    feature = "upstream-fetch"
))]
pub mod edge;
