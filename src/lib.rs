//! gproxy v2 library crate. The binary (`main.rs`) is a thin wiring
//! layer over these modules.

pub mod app;
pub mod channel;
pub mod config;
pub mod http;
pub mod pipeline;
pub mod process;
pub mod protocol;
pub mod store;
pub mod transform;

// Edge self-test exercises all edge backends; gate on the full edge bundle.
#[cfg(all(
    target_arch = "wasm32",
    feature = "persist-libsql",
    feature = "cache-libsql",
    feature = "cache-upstash"
))]
pub mod wasm_selftest;
