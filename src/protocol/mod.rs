//! Protocol wire types and shared protocol taxonomy.

pub mod claude;
pub mod gemini;
pub mod openai;
pub mod operation;

pub use operation::{Endpoint, HttpMethod, Operation, OperationGroup, Provider};
