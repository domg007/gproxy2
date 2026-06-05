//! Protocol wire types and shared protocol taxonomy.

pub mod claude;
pub mod gemini;
pub mod openai;
pub mod operation;

pub use operation::{
    ContentGenerationKind, Endpoint, HttpMethod, Operation, OperationGroup, OperationKey,
    OperationKind, Provider,
};
