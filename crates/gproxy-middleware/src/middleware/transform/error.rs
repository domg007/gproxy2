use std::error::Error;
use std::fmt::{Display, Formatter};

use gproxy_protocol::transform::utils::TransformError;

use super::kinds::{OperationFamily, ProtocolKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MiddlewareTransformError {
    RouteSourceMismatch {
        expected_operation: OperationFamily,
        expected_protocol: ProtocolKind,
        actual_operation: OperationFamily,
        actual_protocol: ProtocolKind,
    },
    Unsupported(&'static str),
    ProtocolTransform(TransformError),
    JsonDecode {
        kind: &'static str,
        operation: OperationFamily,
        protocol: ProtocolKind,
        message: String,
    },
    JsonEncode {
        kind: &'static str,
        operation: OperationFamily,
        protocol: ProtocolKind,
        message: String,
    },
    ProviderPrefix {
        message: String,
    },
}

impl Display for MiddlewareTransformError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RouteSourceMismatch {
                expected_operation,
                expected_protocol,
                actual_operation,
                actual_protocol,
            } => write!(
                f,
                "route source mismatch: expected ({expected_operation:?}, {expected_protocol:?}), got ({actual_operation:?}, {actual_protocol:?})",
            ),
            Self::Unsupported(message) => f.write_str(message),
            Self::ProtocolTransform(err) => Display::fmt(err, f),
            Self::JsonDecode {
                kind,
                operation,
                protocol,
                message,
            } => write!(
                f,
                "failed to decode {kind} json for ({operation:?}, {protocol:?}): {message}",
            ),
            Self::JsonEncode {
                kind,
                operation,
                protocol,
                message,
            } => write!(
                f,
                "failed to encode {kind} json for ({operation:?}, {protocol:?}): {message}",
            ),
            Self::ProviderPrefix { message } => write!(f, "provider prefix error: {message}"),
        }
    }
}

impl Error for MiddlewareTransformError {}

impl From<TransformError> for MiddlewareTransformError {
    fn from(value: TransformError) -> Self {
        Self::ProtocolTransform(value)
    }
}
