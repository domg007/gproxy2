use std::collections::HashMap;

use serde::Serialize;

use gproxy_protocol::kinds::{OperationFamily, ProtocolKind};

/// Maps (operation, protocol) pairs to routing strategies.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DispatchTable {
    routes: HashMap<RouteKey, RouteImplementation>,
}

/// A (operation, protocol) pair identifying a route.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct RouteKey {
    pub operation: OperationFamily,
    pub protocol: ProtocolKind,
}

/// How to handle a particular (operation, protocol) pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RouteImplementation {
    /// Forward request as-is to upstream (same protocol).
    Passthrough,
    /// Transform the request to a different (operation, protocol) before sending.
    TransformTo { destination: RouteKey },
    /// Handle locally without contacting upstream.
    Local,
    /// Not supported — return 501.
    Unsupported,
}

impl DispatchTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a route.
    pub fn set(&mut self, key: RouteKey, implementation: RouteImplementation) {
        self.routes.insert(key, implementation);
    }

    /// Look up how to handle a route.
    pub fn resolve(&self, key: &RouteKey) -> Option<&RouteImplementation> {
        self.routes.get(key)
    }

    /// Resolve a source key to its final (source, destination) pair,
    /// following TransformTo chains.
    pub fn resolve_destination(&self, src: &RouteKey) -> Option<RouteKey> {
        match self.routes.get(src)? {
            RouteImplementation::Passthrough => Some(src.clone()),
            RouteImplementation::TransformTo { destination } => Some(destination.clone()),
            RouteImplementation::Local | RouteImplementation::Unsupported => None,
        }
    }
}

impl RouteKey {
    pub const fn new(operation: OperationFamily, protocol: ProtocolKind) -> Self {
        Self {
            operation,
            protocol,
        }
    }
}
