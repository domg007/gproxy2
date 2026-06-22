//! Online model list: reshape chatgpt.com's `/backend-api/models/gpts` picker
//! into the OpenAI `{object:"list", data:[...]}` shape. The live picker is
//! preferred (slugs vary by plan / version / A-B group); the bundled static
//! catalogue ([`models.openai.json`]) is a fallback used only when the live
//! payload is missing or unparseable.

use bytes::Bytes;
use serde_json::{Value, json};

/// Static fallback catalogue, used only when the live picker is unparseable.
const STATIC_LIST: &str = include_str!("models.openai.json");

/// Reshape a `/backend-api/models/gpts` body into the OpenAI model-list shape,
/// falling back to the bundled catalogue on an unparseable / empty payload
/// (e.g. an upstream error body).
pub(super) fn reshape_model_list(body: &Bytes) -> Bytes {
    reshape(body).unwrap_or_else(|| Bytes::from_static(STATIC_LIST.as_bytes()))
}

/// The picker payload is shaped `{editor: {models_list: [...],
/// models_list_with_custom_actions: [...]}}`. Surface every slug from both
/// lists plus the image models (routed to `/f/conversation`, absent from the
/// editor list). `None` if nothing parseable was found.
fn reshape(body: &[u8]) -> Option<Bytes> {
    let raw: Value = serde_json::from_slice(body).ok()?;
    let now = crate::util::time::unix_now();
    let mut ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    let pluck = |arr: Option<&Value>, into: &mut std::collections::BTreeSet<String>| {
        if let Some(arr) = arr.and_then(Value::as_array) {
            for s in arr
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                into.insert(s.to_string());
            }
        }
    };
    let editor = raw.get("editor");
    pluck(editor.and_then(|e| e.get("models_list")), &mut ids);
    pluck(
        editor.and_then(|e| e.get("models_list_with_custom_actions")),
        &mut ids,
    );
    for img in ["gpt-image-1", "gpt-image-1-mini", "gpt-image-1.5"] {
        ids.insert(img.to_string());
    }
    if ids.is_empty() {
        return None;
    }

    let data: Vec<Value> = ids
        .into_iter()
        .map(|id| json!({ "id": id, "object": "model", "created": now, "owned_by": "openai" }))
        .collect();
    serde_json::to_vec(&json!({ "object": "list", "data": data }))
        .ok()
        .map(Bytes::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reshapes_editor_models_list() {
        let body = Bytes::from_static(
            br#"{"editor":{"models_list":["gpt-5-4","gpt-5-4-thinking"],"models_list_with_custom_actions":["o3"]}}"#,
        );
        let out = reshape_model_list(&body);
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["object"], "list");
        let ids: Vec<&str> = v["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"gpt-5-4"));
        assert!(ids.contains(&"gpt-5-4-thinking"));
        assert!(ids.contains(&"o3"));
        assert!(ids.contains(&"gpt-image-1")); // image models always added
    }

    #[test]
    fn falls_back_to_static_on_unparseable() {
        let out = reshape_model_list(&Bytes::from_static(b"not json"));
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["object"], "list");
        // The bundled catalogue carries the default slug.
        let ids: Vec<&str> = v["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"gpt-5-4"));
    }
}
