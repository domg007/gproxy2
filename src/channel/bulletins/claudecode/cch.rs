//! ClaudeCode CCH — the `x-anthropic-billing-header` checksum Claude Code embeds
//! in `system[0]` of every `/v1/messages` body. Reproduced so the impersonated
//! request passes Anthropic's body-integrity check. See `docs/claudecode-cch.md`.
//!
//! Algorithm (confirmed against real `claude-cli` 2.1.162 wire bodies):
//! 1. Inject `metadata.user_id` = the JSON-string `{device_id, account_uuid,
//!    session_id}` the CLI sends.
//! 2. Prepend a `system[0]` text block holding the billing header with a
//!    `cch=00000;` placeholder.
//! 3. `cch = xxh64(final_body_bytes_with_cch_00000, seed=0x4d659218e32a3268)
//!    & 0xfffff`, formatted as 5 lowercase hex, byte-replacing the placeholder.
//!
//! The checksum covers the ENTIRE final body — model/system/messages/tools all
//! affect it — so it is computed over our own serialized bytes (self-consistent;
//! the server re-hashes the received body and matches).

use serde_json::{Value, json};

/// `cc_version` = CLI version + build suffix (`2.1.162` → suffix `553`), mirroring
/// the real client. Keep in lockstep with the channel User-Agent version.
const CC_VERSION: &str = "2.1.162.553";
/// xxh64 seed mined from the 2.1.162 bundle.
const CCH_SEED: u64 = 0x4d65_9218_e32a_3268;
const PLACEHOLDER: &[u8] = b"cch=00000;";

/// Rewrite the outbound `/v1/messages` body to carry the CLI's billing header +
/// `metadata.user_id`, with a valid `cch`. `session_id` is the value also sent as
/// `x-claude-code-session-id`. Non-object bodies are returned unchanged (the
/// checksum only applies to a JSON message body).
pub(super) fn apply(
    body: &[u8],
    device_id: &str,
    account_uuid: &str,
    session_id: &str,
    entrypoint: &str,
) -> Vec<u8> {
    let Ok(mut v) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let Some(obj) = v.as_object_mut() else {
        return body.to_vec();
    };

    // 1. metadata.user_id = JSON string of {device_id, account_uuid, session_id}.
    let user_id = json!({
        "device_id": device_id,
        "account_uuid": account_uuid,
        "session_id": session_id,
    })
    .to_string();
    let metadata = obj
        .entry("metadata")
        .or_insert_with(|| Value::Object(Default::default()));
    if let Some(m) = metadata.as_object_mut() {
        m.insert("user_id".into(), Value::String(user_id));
    }

    // 2. Prepend the billing-header block to `system` (placeholder cch).
    let billing = json!({
        "type": "text",
        "text": format!("x-anthropic-billing-header: cc_version={CC_VERSION}; cc_entrypoint={entrypoint}; cch=00000;"),
    });
    match obj.get_mut("system") {
        Some(Value::Array(arr)) => arr.insert(0, billing),
        Some(s @ Value::String(_)) => {
            let orig = s.take();
            *s = Value::Array(vec![billing, json!({ "type": "text", "text": orig })]);
        }
        _ => {
            obj.insert("system".into(), Value::Array(vec![billing]));
        }
    }

    // 3. Serialize, compute the checksum over the final bytes, rewrite in place.
    let mut bytes = serde_json::to_vec(&v).unwrap_or_else(|_| body.to_vec());
    if let Some(pos) = find(&bytes, PLACEHOLDER) {
        let cch = xxhash_rust::xxh64::xxh64(&bytes, CCH_SEED) & 0xf_ffff;
        let hex = format!("{cch:05x}");
        // placeholder is `cch=00000;` → overwrite the 5 zero digits at +4.
        bytes[pos + 4..pos + 9].copy_from_slice(hex.as_bytes());
    }
    bytes
}

/// First index of `needle` in `haystack` (small, single-use; avoids a dep).
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known-good vector computed from the real algorithm (python `xxhash`):
    /// this exact body → `cch=b3b78`.
    #[test]
    fn cch_matches_known_vector() {
        // No metadata/system mutation noise: feed a body whose serialized form is
        // already the canonical test body, then assert the rewritten cch.
        let body = br#"{"model":"claude-sonnet-4","system":[{"type":"text","text":"x-anthropic-billing-header: cc_version=2.1.162.553; cc_entrypoint=cli; cch=00000;"}],"messages":[]}"#;
        let pos = find(body, PLACEHOLDER).unwrap();
        let cch = xxhash_rust::xxh64::xxh64(body, CCH_SEED) & 0xf_ffff;
        assert_eq!(format!("{cch:05x}"), "b3b78");
        // sanity: placeholder digits are where we think they are.
        assert_eq!(&body[pos..pos + PLACEHOLDER.len()], PLACEHOLDER);
    }

    #[test]
    fn apply_injects_metadata_and_valid_cch() {
        let out = apply(
            br#"{"model":"claude-sonnet-4","messages":[]}"#,
            "devhash",
            "acct-1",
            "sess-uuid",
            "cli",
        );
        let v: Value = serde_json::from_slice(&out).unwrap();
        // metadata.user_id is the JSON-string of the three ids.
        let uid = v["metadata"]["user_id"].as_str().unwrap();
        let ids: Value = serde_json::from_str(uid).unwrap();
        assert_eq!(ids["device_id"], "devhash");
        assert_eq!(ids["account_uuid"], "acct-1");
        assert_eq!(ids["session_id"], "sess-uuid");
        // system[0] carries the billing header with a 5-hex (non-zero) cch.
        let txt = v["system"][0]["text"].as_str().unwrap();
        assert!(txt.contains("cc_version=2.1.162.553"));
        assert!(txt.contains("cc_entrypoint=cli"));
        let cch = txt.split("cch=").nth(1).unwrap().trim_end_matches(';');
        assert_eq!(cch.len(), 5);
        assert_ne!(cch, "00000");
        // Re-hashing the final bytes with cch zeroed reproduces it (server check).
        let pos = find(&out, b"cch=").unwrap();
        let mut zeroed = out.clone();
        zeroed[pos + 4..pos + 9].copy_from_slice(b"00000");
        let expect = xxhash_rust::xxh64::xxh64(&zeroed, CCH_SEED) & 0xf_ffff;
        assert_eq!(format!("{expect:05x}"), cch);
    }
}
