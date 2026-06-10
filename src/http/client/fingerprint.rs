//! Map a `tls_fingerprint` JSON value to a wreq [`Emulation`] (§7.4).
//!
//! Fingerprint schema (constrained → wreq): `{"headers"?: {name: value}, "http2"?:
//! {...}, "tls"?: {...}}`. M7a wires the `headers` layer fully; `http2`/`tls` are
//! complex typed wreq builders (`Http2Options`/`TlsOptions` use domain identifiers
//! for cipher/curve/sigalg lists, pseudo-header orderings, TLS versions, …) that do
//! not map trivially from arbitrary JSON — full per-channel TLS presets land in M7b.
//! When present they are logged once and skipped; an unparsable fingerprint yields no
//! emulation (proxy-only client).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use http::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

/// blake3 hex of the canonicalized fingerprint — the pool cache key. Object keys
/// are recursively sorted so two semantically-equal fingerprints that differ only
/// in key insertion order hash identically (and thus share one upstream client).
pub fn fingerprint_hash(fp: &Value) -> String {
    let canonical = canonicalize(fp);
    // Canonical form serializes deterministically (BTreeMap → sorted keys).
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    blake3::hash(&bytes).to_hex().to_string()
}

/// Recursively rebuild `v` with every object's keys sorted, so serialization is
/// order-independent. Arrays keep their order (order is semantic there).
fn canonicalize(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map
                .iter()
                .map(|(k, val)| (k.clone(), canonicalize(val)))
                .collect();
            // serde_json::Map preserves BTreeMap's sorted iteration order on collect.
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

/// Build a wreq [`Emulation`] from the fingerprint, or `None` if nothing applies.
///
/// M7a maps the `headers` object → [`HeaderMap`] → `Emulation::builder().headers(..)`.
/// `http2`/`tls` objects are deferred (warn-once) per the plan.
pub fn to_emulation(fp: &Value) -> Option<wreq::Emulation> {
    let obj = fp.as_object()?;

    if obj.contains_key("http2") || obj.contains_key("tls") {
        warn_deferred_once();
    }

    let headers = obj.get("headers").and_then(headers_from_json);
    let headers = headers?;
    if headers.is_empty() {
        return None;
    }
    // `Group::default()` = no request-partitioning grouping; we only want the
    // header layer applied to the client.
    Some(
        wreq::Emulation::builder()
            .headers(headers)
            .build(wreq::Group::default()),
    )
}

/// Parse a `{name: value}` JSON object into a [`HeaderMap`]. Non-string values and
/// names/values that aren't valid header tokens are skipped (best-effort).
fn headers_from_json(v: &Value) -> Option<HeaderMap> {
    let obj = v.as_object()?;
    let mut hm = HeaderMap::with_capacity(obj.len());
    for (name, val) in obj {
        let Some(val) = val.as_str() else { continue };
        let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(val),
        ) else {
            continue;
        };
        hm.append(hn, hv);
    }
    Some(hm)
}

/// Logs the `http2`/`tls` deferral exactly once per process to avoid log spam on a
/// hot path (one client build per distinct fingerprint).
fn warn_deferred_once() {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::info!("tls/http2 emulation deferred to channel presets (M7b)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fingerprint_hash_canonical() {
        // Same content, different key insertion order (top-level and nested) → equal hash.
        let a = json!({
            "headers": {"user-agent": "x", "accept": "y"},
            "tls": {"min": 1, "max": 2}
        });
        let b = json!({
            "tls": {"max": 2, "min": 1},
            "headers": {"accept": "y", "user-agent": "x"}
        });
        assert_eq!(fingerprint_hash(&a), fingerprint_hash(&b));

        // Different content → different hash.
        let c = json!({"headers": {"user-agent": "z"}});
        assert_ne!(fingerprint_hash(&a), fingerprint_hash(&c));
    }
}
