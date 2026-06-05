//! Embedding pairwise transforms.

pub mod gemini_to_openai;
pub mod openai_to_gemini;

pub(in crate::transform::embeddings) const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "unknown";

pub(in crate::transform::embeddings) fn i32_to_u32(
    value: i32,
    field: &'static str,
) -> Result<u32, crate::transform::TransformError> {
    u32::try_from(value).map_err(|_| crate::transform::TransformError::InvalidInput {
        reason: format!("{field} cannot be negative"),
    })
}

pub(in crate::transform::embeddings) fn u32_to_i32(
    value: u32,
    field: &'static str,
) -> Result<i32, crate::transform::TransformError> {
    i32::try_from(value).map_err(|_| crate::transform::TransformError::LossyField {
        field,
        reason: "target integer range is smaller than source value",
    })
}
