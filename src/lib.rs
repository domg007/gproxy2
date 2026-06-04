//! gproxy v2 library crate. The binary (`main.rs`) is a thin wiring
//! layer over these modules.

pub mod app;
pub mod config;
pub mod http;
pub mod protocol;
pub mod store;

#[cfg(target_arch = "wasm32")]
pub mod wasm_selftest;
