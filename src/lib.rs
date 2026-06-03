//! gproxy v2 library crate. The binary (`main.rs`) is a thin wiring
//! layer over these modules.

pub mod http;
pub mod store;

#[cfg(not(target_arch = "wasm32"))]
pub mod app;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;

#[cfg(target_arch = "wasm32")]
pub mod wasm_selftest;
