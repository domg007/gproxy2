//! Provider-neutral domain records used by the [`PersistenceBackend`] trait.
//!
//! These are backend-agnostic shapes: the `db` impl maps them to/from SeaORM
//! models, the `file` impl serializes them as JSON. Domain code only ever sees
//! these types — never SeaORM entities.

pub mod identity;
pub mod provider;
pub mod routing;
pub mod rules;
pub mod settings;
pub mod usage;

pub use identity::{
    Org, OrgInput, Quota, QuotaInput, RateLimit, RateLimitInput, RoutePermission,
    RoutePermissionInput, Team, TeamInput, User, UserInput, UserKey, UserKeyInput,
};
pub use provider::{
    Credential, CredentialInput, CredentialStatus, CredentialStatusInput, Provider, ProviderInput,
};
pub use routing::{
    Alias, AliasInput, ProviderModel, ProviderModelInput, Route, RouteInput, RouteMember,
    RouteMemberInput,
};
pub use rules::{
    BetaHeader, BetaHeaderInput, CacheBreakpoint, CacheBreakpointInput, PreludeSystem,
    PreludeSystemInput, RewriteRule, RewriteRuleInput, RoutingRule, RoutingRuleInput, SanitizeRule,
    SanitizeRuleInput,
};
pub use settings::{InstanceSettings, InstanceSettingsInput};
pub use usage::{
    DownstreamRequest, DownstreamRequestInput, UpstreamRequest, UpstreamRequestInput, Usage,
    UsageInput, UsageRollup, UsageRollupInput,
};
