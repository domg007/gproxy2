//! Provider-neutral domain records used by the [`PersistenceBackend`] trait.
//!
//! These are backend-agnostic shapes: the `db` impl maps them to/from SeaORM
//! models, the `file` impl serializes them as JSON. Domain code only ever sees
//! these types — never SeaORM entities.

pub mod provider;

pub use provider::{Provider, ProviderInput};
