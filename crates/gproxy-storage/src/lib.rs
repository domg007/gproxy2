//! Storage layer for gproxy.
//! This crate defines SeaORM entities and storage-facing types.

pub mod query;
pub mod seaorm;
pub mod write;

pub use query::*;
pub use seaorm::SeaOrmStorage;
pub use seaorm::entities;
pub use seaorm::entities::prelude;
pub use write::*;
