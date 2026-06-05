use std::collections::BTreeMap;

use serde_json::Value;

use crate::transform::TransformError;

pub type JsonFields = BTreeMap<String, Value>;

pub fn move_field(
    source: &mut JsonFields,
    target: &mut JsonFields,
    field: &'static str,
) -> Option<Value> {
    let value = source.remove(field)?;
    target.insert(field.to_owned(), value.clone());
    Some(value)
}

pub fn preserve_remaining_fields(
    source: JsonFields,
    target: &mut JsonFields,
    preserve_unknown_fields: bool,
) -> Result<(), TransformError> {
    if preserve_unknown_fields {
        target.extend(source);
        return Ok(());
    }

    if let Some(field) = source.keys().next() {
        let _ = field;
        return Err(TransformError::LossyField {
            field: "unknown",
            reason: "unknown provider field cannot be preserved",
        });
    }

    Ok(())
}

pub fn require_no_extra_fields(extra: &JsonFields) -> Result<(), TransformError> {
    if let Some(field) = extra.keys().next() {
        let _ = field;
        return Err(TransformError::LossyField {
            field: "extra",
            reason: "target wire shape has no safe extra-field location",
        });
    }

    Ok(())
}
