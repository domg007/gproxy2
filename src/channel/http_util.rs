//! Shared HTTP plumbing for channels: absolute-URL join, header sanitize, and
//! upstream request building.

use bytes::Bytes;
use http::header::{HOST, HeaderName};
use http::{HeaderMap, Method, Request, Uri};

use crate::channel::ChannelError;

/// Hop-by-hop headers (RFC 7230 §6.1) — stripped on BOTH ingress and egress.
///
/// TODO: this is a fixed list; it does not yet honor `Connection:`-token
/// semantics (headers named in a `Connection` value should also be dropped).
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "content-length",
];

/// Inbound headers stripped before forwarding upstream: hop-by-hop, plus the
/// client's own auth (the channel injects the credential's auth instead), the
/// `Host` (a fresh one is derived from the absolute URI in [`build_request`]),
/// and browser `cookie`s (no reason to relay them to an LLM upstream).
fn is_stripped(name: &HeaderName) -> bool {
    let n = name.as_str(); // HeaderName is already lowercase
    HOP_BY_HOP.contains(&n)
        || matches!(
            n,
            "host" | "authorization" | "x-api-key" | "x-goog-api-key" | "api-key" | "cookie"
        )
}

/// Copy `src` minus hop-by-hop / Host / inbound-auth headers. Used for ingress
/// (toward upstream); egress reuse drops the same hop-by-hop set.
pub fn sanitize_headers(src: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::with_capacity(src.len());
    for (name, value) in src.iter() {
        if is_stripped(name) {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

/// Drop hop-by-hop headers from an upstream response before relaying to the
/// client (egress). Keeps everything else (content-type, etc.).
pub fn sanitize_response_headers(src: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::with_capacity(src.len());
    for (name, value) in src.iter() {
        if HOP_BY_HOP.contains(&name.as_str()) {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

/// Compose an ABSOLUTE upstream URI from `base_url` + provider-relative `path`
/// (+ optional `query`). Trims one trailing `/` off the base. Errors if the
/// result is not absolute (missing scheme/authority).
pub fn join_url(base_url: &str, path: &str, query: Option<&str>) -> Result<Uri, ChannelError> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(ChannelError::MissingSetting("base_url"));
    }
    let mut url = format!("{base}{path}");
    if let Some(q) = query.filter(|q| !q.is_empty()) {
        url.push('?');
        url.push_str(q);
    }
    let uri: Uri = url
        .parse()
        .map_err(|e| ChannelError::Build(format!("bad upstream url {url:?}: {e}")))?;
    if uri.scheme().is_none() || uri.authority().is_none() {
        return Err(ChannelError::Build(format!(
            "upstream url not absolute: {url:?}"
        )));
    }
    Ok(uri)
}

/// Build the upstream request: method + absolute URI + sanitized headers + a
/// fresh `Host` from the URI authority, with `body` moved in. Channel-specific
/// auth headers are inserted by the caller AFTER this.
pub fn build_request(
    method: Method,
    uri: Uri,
    mut headers: HeaderMap,
    body: Bytes,
) -> Result<Request<Bytes>, ChannelError> {
    // Derive Host from host[:port] only — never the full authority, which in the
    // `http` crate includes any `user:pass@` userinfo present in base_url.
    if let Some(host) = uri.host() {
        let host_val = match uri.port_u16() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
        if let Ok(hv) = host_val.parse() {
            headers.insert(HOST, hv);
        }
    }
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .body(body)
        .map_err(|e| ChannelError::Build(e.to_string()))?;
    *req.headers_mut() = headers;
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_is_absolute_and_trims_slash() {
        let uri = join_url("http://127.0.0.1:9009/", "/v1/chat/completions", None).unwrap();
        assert_eq!(uri.to_string(), "http://127.0.0.1:9009/v1/chat/completions");
        assert!(uri.scheme().is_some() && uri.authority().is_some());
    }

    #[test]
    fn join_appends_query() {
        let uri = join_url("http://h", "/v1/x", Some("a=1&b=2")).unwrap();
        assert_eq!(uri.to_string(), "http://h/v1/x?a=1&b=2");
    }

    #[test]
    fn missing_base_is_err() {
        assert!(matches!(
            join_url("  ", "/v1/x", None),
            Err(ChannelError::MissingSetting("base_url"))
        ));
    }

    #[test]
    fn sanitize_drops_auth_and_hop_by_hop() {
        let mut h = HeaderMap::new();
        h.insert(
            http::header::AUTHORIZATION,
            "Bearer client".parse().unwrap(),
        );
        h.insert(http::header::CONTENT_LENGTH, "10".parse().unwrap());
        h.insert(
            http::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        h.insert("x-api-key", "client".parse().unwrap());
        let out = sanitize_headers(&h);
        assert!(out.get(http::header::AUTHORIZATION).is_none());
        assert!(out.get("x-api-key").is_none());
        assert!(out.get(http::header::CONTENT_LENGTH).is_none());
        assert_eq!(
            out.get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }
}
