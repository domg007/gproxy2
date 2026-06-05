//! Model-list and model-retrieve pairwise transforms.

use serde::Serialize;

use crate::{
    protocol::{claude, openai},
    transform::TransformError,
};

pub mod get;
pub mod list;

mod model;

pub(super) const DEFAULT_CREATED_AT: &str = "1970-01-01T00:00:00Z";
pub(super) const DEFAULT_OPENAI_OWNED_BY: &str = "unknown";

pub(super) fn wire_string<T: Serialize>(
    value: &T,
    field: &'static str,
) -> Result<String, TransformError> {
    let value = serde_json::to_value(value).map_err(|error| TransformError::Serialization {
        reason: error.to_string(),
    })?;
    value
        .as_str()
        .map(str::to_owned)
        .ok_or(TransformError::InvalidInput {
            reason: format!("{field} did not serialize as a string"),
        })
}

pub(super) const fn openai_model_object() -> openai::ModelObjectType {
    openai::ModelObjectType::Model
}

pub(super) const fn claude_model_object() -> claude::ModelObjectType {
    claude::ModelObjectType::Known(claude::ModelObjectTypeKnown::Model)
}

pub(super) fn default_claude_capabilities() -> claude::ModelCapabilities {
    claude::ModelCapabilities {
        batch: default_capability_support(),
        citations: default_capability_support(),
        code_execution: default_capability_support(),
        context_management: claude::ContextManagementCapability {
            supported: false,
            clear_thinking_20251015: None,
            clear_tool_uses_20250919: None,
            compact_20260112: None,
            extra: Default::default(),
        },
        effort: claude::EffortCapability {
            supported: false,
            low: None,
            medium: None,
            high: None,
            xhigh: None,
            max: None,
            extra: Default::default(),
        },
        image_input: default_capability_support(),
        pdf_input: default_capability_support(),
        structured_outputs: default_capability_support(),
        thinking: claude::ThinkingCapability {
            supported: false,
            types: claude::ThinkingTypes {
                adaptive: None,
                enabled: None,
                extra: Default::default(),
            },
            extra: Default::default(),
        },
        extra: Default::default(),
    }
}

fn default_capability_support() -> claude::CapabilitySupport {
    claude::CapabilitySupport {
        supported: false,
        extra: Default::default(),
    }
}

pub(super) fn u64_to_i32_default(value: u64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

pub(super) fn i32_to_u64_default(value: i32) -> u64 {
    u64::try_from(value).unwrap_or_default()
}
