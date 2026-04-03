use std::error::Error;
use std::fmt::{Display, Formatter};

use crate::kinds::{OperationFamily, ProtocolKind};

/// Errors produced by the middleware layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MiddlewareError {
    Unsupported(&'static str),
    JsonDecode {
        kind: &'static str,
        operation: OperationFamily,
        protocol: ProtocolKind,
        message: String,
    },
    ProviderPrefix {
        message: String,
    },
}

impl Display for MiddlewareError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(msg) => f.write_str(msg),
            Self::JsonDecode {
                kind,
                operation,
                protocol,
                message,
            } => write!(
                f,
                "failed to decode {kind} json for ({operation:?}, {protocol:?}): {message}",
            ),
            Self::ProviderPrefix { message } => write!(f, "provider prefix error: {message}"),
        }
    }
}

impl Error for MiddlewareError {}
