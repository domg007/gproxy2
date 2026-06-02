//! gproxy v2 library crate. The binary (`main.rs`) is a thin wiring
//! layer over these modules.

pub mod channel;

#[cfg(not(target_arch = "wasm32"))]
pub mod app;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod http;
#[cfg(not(target_arch = "wasm32"))]
pub mod store;

#[cfg(target_arch = "wasm32")]
pub mod edge;
