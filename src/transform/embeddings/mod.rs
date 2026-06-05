//! Embedding pairwise transforms.

pub mod gemini_to_openai;
pub mod openai_to_gemini;

pub(in crate::transform::embeddings) const DEFAULT_OPENAI_EMBEDDING_MODEL: &str = "unknown";

pub(in crate::transform::embeddings) fn i32_to_u32(value: i32) -> u32 {
    u32::try_from(value).unwrap_or_default()
}

pub(in crate::transform::embeddings) fn u32_to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}
