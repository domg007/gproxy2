//! Framework-independent routing primitives shared by gproxy components.

/// Shared routing error types.
pub mod error;
/// Request classification helpers.
pub mod classify;
/// Model extraction helpers.
pub mod model_extraction;
/// Model alias data types.
pub mod model_alias;
/// Provider-prefix utilities for model identifiers.
pub mod provider_prefix;
/// Model permission helpers.
pub mod permission;
/// Rate-limit rule matching helpers.
pub mod rate_limit;
/// Request sanitization helpers.
pub mod sanitize;

pub use gproxy_protocol::kinds::{OperationFamily, ProtocolKind};
