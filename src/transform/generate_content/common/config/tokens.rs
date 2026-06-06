use crate::transform::TransformError;

pub(in crate::transform::generate_content) fn merge_openai_max_tokens(
    max_completion_tokens: Option<u32>,
    max_tokens: Option<u32>,
) -> Result<Option<u32>, TransformError> {
    match (max_completion_tokens, max_tokens) {
        (Some(current), Some(legacy)) if current != legacy => Err(TransformError::InvalidInput {
            reason: "max_completion_tokens and max_tokens disagree".to_owned(),
        }),
        (Some(value), _) | (_, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    }
}
