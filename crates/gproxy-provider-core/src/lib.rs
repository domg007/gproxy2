//! Core provider abstractions for gproxy.
//!
//! This crate intentionally does **not** depend on axum or any concrete HTTP client.
//! Provider implementations should construct `UpstreamHttpRequest` (and optional
//! internal requests like `upstream_usage`), while a higher layer performs IO.

pub mod config;
pub mod credential;
pub mod errors;
pub mod events;
pub mod headers;
pub mod provider;
pub mod registry;

pub use config::{
    ClaudeCodePreludeText, CountTokensMode, DispatchRule, DispatchTable, ModelTable, OperationKind,
    ProviderConfig,
};
pub use credential::{
    AcquireError, Credential, CredentialId, CredentialPool, CredentialState, UnavailableReason,
};
pub use errors::{ProviderError, ProviderResult};
pub use events::{
    DownstreamEvent, Event, EventHub, EventSink, ModelUnavailableEndEvent,
    ModelUnavailableStartEvent, OperationalEvent, TerminalEventSink, UnavailableEndEvent,
    UnavailableStartEvent, UpstreamEvent,
};
pub use headers::{Headers, header_get, header_remove, header_set};
pub use provider::{
    AuthRetryAction, HttpMethod, OAuthCallbackRequest, OAuthCallbackResult, OAuthCredential,
    OAuthStartRequest, UpstreamBody, UpstreamCtx, UpstreamHttpRequest, UpstreamHttpResponse,
    UpstreamProvider,
};
pub use registry::ProviderRegistry;

// Re-export the protocol/transform typed enums from gproxy-transform.
pub use gproxy_transform::middleware::{
    CountTokensRequest, CountTokensResponse, GenerateContentRequest, GenerateContentResponse,
    MemoryTraceSummarizeRequest, MemoryTraceSummarizeResponse, ModelGetRequest, ModelGetResponse,
    ModelListRequest, ModelListResponse, Op, Proto, Request, Response, ResponseCancelRequest,
    ResponseCancelResponse, ResponseCompactRequest, ResponseCompactResponse, ResponseDeleteRequest,
    ResponseDeleteResponse, ResponseGetRequest, ResponseGetResponse, ResponseListInputItemsRequest,
    ResponseListInputItemsResponse, StreamEvent, StreamFormat, TransformContext, TransformError,
    stream_format,
};

// Re-export usage helpers used by the middleware/engine layer.
pub use gproxy_transform::middleware::{
    CountTokensFn, OutputAccumulator, UsageAccumulator, UsageError, UsageSummary,
    fallback_usage_with_count_tokens, output_for_counting, usage_from_response,
};
