//! Provider-neutral domain records used by the [`PersistenceBackend`] trait.
//!
//! These are backend-agnostic shapes: the `db` impl maps them to/from SeaORM
//! models, the `file` impl serializes them as JSON. Domain code only ever sees
//! these types — never SeaORM entities.

pub mod provider;
pub mod routing;

pub use provider::{
    Credential, CredentialInput, CredentialStatus, CredentialStatusInput, Provider, ProviderInput,
};
pub use routing::{
    Alias, AliasInput, ProviderModel, ProviderModelInput, Route, RouteInput, RouteMember,
    RouteMemberInput,
};
