//! LLM channel/provider engine with multi-channel support.
//!
//! This crate provides a trait-based architecture for routing LLM API requests
//! to upstream providers (OpenAI, Anthropic, Gemini, etc.) with automatic
//! credential rotation, health tracking, and retry logic.
//!
//! # Adding a new channel
//!
//! 1. Create a struct implementing [`Channel`]
//! 2. Implement [`ChannelSettings`] and [`ChannelCredential`] for your config/auth types
//! 3. Implement [`CredentialHealth`] for your health tracking shape
//! 4. Call `inventory::submit!` to register the channel
//!
//! That's it — no other files need to change.

pub mod channel;
pub mod channels;
pub mod dispatch;
pub mod health;
pub mod provider;
pub mod registry;
pub mod request;
pub mod response;
pub mod retry;

pub use channel::{Channel, ChannelCredential, ChannelSettings};
pub use dispatch::DispatchTable;
pub use health::CredentialHealth;
pub use provider::ProviderDefinition;
pub use registry::{ChannelRegistration, ChannelRegistry};
pub use request::PreparedRequest;
pub use response::{ResponseClassification, UpstreamError, UpstreamResponse};
