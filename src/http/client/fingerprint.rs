//! Map a `tls_fingerprint` JSON value to a wreq [`Emulation`] (§7.4).
//!
//! Fingerprint schema (constrained → wreq): `{"headers"?: {name: value}, "http2"?:
//! {...}, "tls"?: {...}}`. The `headers` layer becomes the emulation's default
//! headers; the `tls` object maps to a [`wreq::tls::TlsOptions`] (ALPN, GREASE, TLS
//! version range, cipher/curve/sigalg BoringSSL token lists, extension permutation).
//! An unparsable fingerprint yields no emulation (proxy-only client).
//!
//! Keys per `docs/agent-tls-fingerprints.md` §5. Keys prefixed `_` (`_reference`,
//! `_fidelity`, `_unsupported`) are comments and ignored. The `http2` object is not
//! mapped: wreq's `Http2Options` is a typed `wreq_proto::http2` builder whose knobs
//! (frame settings / priority tree / pseudo-header order) do not map cleanly from the
//! loosely-typed JSON the captures produce, and none of the §5 drafts carry an
//! `http2` block — so it is left documented-as-unmapped rather than forced.
//!
//! BoringSSL fidelity ceiling (per the doc): the achievable target is an exact UA
//! plus ALPN, GREASE-off, and cipher/curve/sigalg approximation — NOT byte-exact
//! JA3/JA4 of OpenSSL/Go/rustls stacks. The structural mapping below is what is
//! verifiable offline; byte-exact JA3/JA4 needs a live capture against the binary.

use std::borrow::Cow;
use std::collections::BTreeMap;

use http::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use wreq::tls::{AlpnProtocol, ExtensionType, TlsOptions, TlsVersion};

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
/// Maps the `headers` object → [`HeaderMap`] and the `tls` object → [`TlsOptions`],
/// applying only fields that are present. Returns `None` when neither layer yields
/// anything usable.
pub fn to_emulation(fp: &Value) -> Option<wreq::Emulation> {
    let obj = fp.as_object()?;

    let headers = obj
        .get("headers")
        .and_then(headers_from_json)
        .filter(|hm| !hm.is_empty());

    let tls = obj.get("tls").and_then(tls_from_json);

    // Nothing to emulate (an `http2`-only fingerprint, or empty) → no client preset.
    if headers.is_none() && tls.is_none() {
        return None;
    }

    let mut builder = wreq::Emulation::builder();
    if let Some(headers) = headers {
        builder = builder.headers(headers);
    }
    if let Some(tls) = tls {
        builder = builder.tls_options(tls);
    }
    // `Group::default()` = no request-partitioning grouping; we only attach the
    // header + TLS layers to the client.
    Some(builder.build(wreq::Group::default()))
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

/// Parse the `tls` JSON object into [`TlsOptions`], applying only present keys.
///
/// All keys are optional (best-effort): unknown or malformed values are skipped, a
/// partial object still produces options. Returns `None` only if `v` is not an object
/// or carries no recognized TLS field at all.
fn tls_from_json(v: &Value) -> Option<TlsOptions> {
    let obj = v.as_object()?;
    let mut builder = TlsOptions::builder();
    let mut applied = false;

    // ALPN: array of protocol strings ("h2", "http/1.1", "h3"). An explicitly empty
    // array means "send no ALPN" — distinct from the field being absent.
    if let Some(arr) = obj.get("alpn_protocols").and_then(Value::as_array) {
        let alpn: Vec<AlpnProtocol> = arr
            .iter()
            .filter_map(Value::as_str)
            .filter_map(parse_alpn)
            .collect();
        builder = builder.alpn_protocols(alpn);
        applied = true;
    }

    if let Some(b) = obj.get("grease_enabled").and_then(Value::as_bool) {
        builder = builder.grease_enabled(b);
        applied = true;
    }

    if let Some(ver) = obj
        .get("min_tls_version")
        .and_then(Value::as_str)
        .and_then(parse_tls_version)
    {
        builder = builder.min_tls_version(ver);
        applied = true;
    }
    if let Some(ver) = obj
        .get("max_tls_version")
        .and_then(Value::as_str)
        .and_then(parse_tls_version)
    {
        builder = builder.max_tls_version(ver);
        applied = true;
    }

    // BoringSSL token lists carried verbatim (`:`-separated). Owned → 'static Cow.
    if let Some(s) = obj.get("cipher_list").and_then(Value::as_str) {
        builder = builder.cipher_list(s.to_owned());
        applied = true;
    }
    if let Some(s) = obj.get("curves_list").and_then(Value::as_str) {
        builder = builder.curves_list(s.to_owned());
        applied = true;
    }
    if let Some(s) = obj.get("sigalgs_list").and_then(Value::as_str) {
        builder = builder.sigalgs_list(s.to_owned());
        applied = true;
    }

    if let Some(b) = obj
        .get("preserve_tls13_cipher_list")
        .and_then(Value::as_bool)
    {
        builder = builder.preserve_tls13_cipher_list(b);
        applied = true;
    }

    // Extension ordering: array of extension type numbers (u16). Only able to permute
    // extensions BoringSSL actually emits (per the doc's fidelity note).
    if let Some(arr) = obj.get("extension_permutation").and_then(Value::as_array) {
        let exts: Vec<ExtensionType> = arr
            .iter()
            .filter_map(Value::as_u64)
            .filter_map(|n| u16::try_from(n).ok())
            .map(ExtensionType::from)
            .collect();
        if !exts.is_empty() {
            builder = builder.extension_permutation(Cow::Owned(exts));
            applied = true;
        }
    }

    applied.then(|| builder.build())
}

/// Map an ALPN protocol id string to the wreq [`AlpnProtocol`] constant. Unknown
/// protocols are dropped (best-effort).
fn parse_alpn(s: &str) -> Option<AlpnProtocol> {
    match s {
        "h2" => Some(AlpnProtocol::HTTP2),
        "http/1.1" => Some(AlpnProtocol::HTTP1),
        "h3" => Some(AlpnProtocol::HTTP3),
        _ => None,
    }
}

/// Parse a `"tls1.x"` string into a wreq [`TlsVersion`]. Accepts the spec's
/// `tls1.0`..`tls1.3` spelling (case-insensitive).
fn parse_tls_version(s: &str) -> Option<TlsVersion> {
    match s.to_ascii_lowercase().as_str() {
        "tls1.0" | "tls1" => Some(TlsVersion::TLS_1_0),
        "tls1.1" => Some(TlsVersion::TLS_1_1),
        "tls1.2" => Some(TlsVersion::TLS_1_2),
        "tls1.3" => Some(TlsVersion::TLS_1_3),
        _ => None,
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

    // Literal §5 drafts from docs/agent-tls-fingerprints.md, trimmed to the mapped
    // keys (the `_*` comment keys are kept on a couple to prove they are ignored).

    fn claude_fp() -> Value {
        json!({
            "headers": { "user-agent": "claude-code/2.1.162" },
            "tls": {
                "alpn_protocols": ["http/1.1"],
                "grease_enabled": false,
                "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
                "cipher_list": "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256",
                "curves_list": "X25519:P-256:P-384",
                "sigalgs_list": "ecdsa_secp256r1_sha256:rsa_pss_rsae_sha256:rsa_pkcs1_sha256"
            },
            "_reference": { "ja4": "t13d1714h1_5b57614c22b0_43ade6aba3df" },
            "_fidelity": "high",
            "_unsupported": "JA4_c 可能小差。"
        })
    }

    fn agy_fp() -> Value {
        json!({
            "headers": { "user-agent": "codeium-language-server" },
            "tls": {
                "alpn_protocols": ["h2", "http/1.1"],
                "grease_enabled": false,
                "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
                "cipher_list": "ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:TLS_AES_128_GCM_SHA256",
                "curves_list": "X25519MLKEM768:X25519:P-256:P-384:P-521",
                "sigalgs_list": "rsa_pss_rsae_sha256:ecdsa_secp256r1_sha256:ed25519",
                "preserve_tls13_cipher_list": true
            },
            "_fidelity": "medium"
        })
    }

    fn kiro_fp() -> Value {
        json!({
            "headers": { "user-agent": "aws-sdk-rust/1.3.10 os/linux lang/rust/1.92.0" },
            "tls": {
                "alpn_protocols": [],
                "grease_enabled": false,
                "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
                "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:ECDHE-ECDSA-AES256-GCM-SHA384",
                "curves_list": "X25519:P-256:P-384",
                "sigalgs_list": "ecdsa_secp384r1_sha384:ecdsa_secp256r1_sha256:ed25519"
            },
            "_fidelity": "medium"
        })
    }

    fn codex_fp() -> Value {
        json!({
            "headers": { "user-agent": "codex_exec/0.137.0 (Debian 13.0.0; x86_64) xterm-256color" },
            "tls": {
                "alpn_protocols": ["h2"],
                "grease_enabled": false,
                "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
                "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:ECDHE-ECDSA-AES256-GCM-SHA384",
                "curves_list": "X25519:P-256:P-384"
            },
            "_fidelity": "medium"
        })
    }

    fn gemini_fp() -> Value {
        json!({
            "headers": { "user-agent": "google-api-nodejs-client/9.15.1" },
            "tls": {
                "alpn_protocols": [],
                "grease_enabled": false,
                "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
                "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:ECDHE-ECDSA-AES128-GCM-SHA256",
                "curves_list": "X25519MLKEM768:X25519:P-256:P-384:P-521"
            },
            "_fidelity": "low"
        })
    }

    #[test]
    fn channel_drafts_map_to_emulation() {
        for fp in [claude_fp(), agy_fp(), kiro_fp(), codex_fp(), gemini_fp()] {
            assert!(
                to_emulation(&fp).is_some(),
                "channel draft should produce an emulation: {fp}"
            );
        }
    }

    #[test]
    fn tls_options_reflect_json_fields() {
        let opts = tls_from_json(claude_fp().get("tls").unwrap()).expect("tls maps");
        assert_eq!(opts.grease_enabled, Some(false));
        assert_eq!(opts.min_tls_version, Some(TlsVersion::TLS_1_2));
        assert_eq!(opts.max_tls_version, Some(TlsVersion::TLS_1_3));
        assert_eq!(opts.curves_list.as_deref(), Some("X25519:P-256:P-384"));
        // Single-protocol ALPN parsed to the HTTP1 constant.
        let alpn = opts.alpn_protocols.expect("alpn present");
        assert_eq!(alpn.as_ref(), &[AlpnProtocol::HTTP1]);

        // agy: preserve_tls13_cipher_list + multi-ALPN.
        let agy = tls_from_json(agy_fp().get("tls").unwrap()).expect("agy maps");
        assert_eq!(agy.preserve_tls13_cipher_list, Some(true));
        assert_eq!(
            agy.alpn_protocols.unwrap().as_ref(),
            &[AlpnProtocol::HTTP2, AlpnProtocol::HTTP1]
        );

        // Empty ALPN array is honored as "no protocols" (Some but empty), not skipped.
        let kiro = tls_from_json(kiro_fp().get("tls").unwrap()).expect("kiro maps");
        assert_eq!(kiro.alpn_protocols.unwrap().len(), 0);
    }

    #[test]
    fn headers_only_fingerprint_still_maps() {
        let fp = json!({ "headers": { "user-agent": "x" } });
        assert!(to_emulation(&fp).is_some());
    }

    #[test]
    fn empty_and_http2_only_yield_none() {
        assert!(to_emulation(&json!({})).is_none());
        // http2 is documented-as-unmapped; an http2-only fingerprint applies nothing.
        assert!(to_emulation(&json!({ "http2": { "anything": 1 } })).is_none());
        // tls object with no recognized keys → no options → None.
        assert!(to_emulation(&json!({ "tls": { "_note": "x" } })).is_none());
    }
}
