use crate::protocol::OperationKey;

/// Per-call transform settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransformContext {
    pub source: OperationKey,
    pub target: OperationKey,
    pub preserve_unknown_fields: bool,
}

impl TransformContext {
    pub const fn new(source: OperationKey, target: OperationKey) -> Self {
        Self {
            source,
            target,
            preserve_unknown_fields: true,
        }
    }

    pub const fn with_preserve_unknown_fields(mut self, preserve_unknown_fields: bool) -> Self {
        self.preserve_unknown_fields = preserve_unknown_fields;
        self
    }
}
