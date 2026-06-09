//! Part 1 of inbound header/query handling: the GLOBAL blacklist.
//!
//! Applied ONCE in the pipeline — after auth, before channel selection — so no
//! channel can ever forward the caller's credentials/cookies upstream. The
//! per-channel allow-list (Part 2) runs later, inside `Channel::prepare`
//! (`channel::http_util::allow_headers` / `allow_query`). The two layers take
//! effect at deliberately different pipeline positions.

use http::HeaderMap;

use crate::channel::http_util::HOP_BY_HOP;
use crate::pipeline::context::RequestCtx;

/// Inbound headers globally denied upstream regardless of channel: hop-by-hop,
/// the caller's own credentials, cookies, and `Host` (the channel sets a fresh
/// one from the upstream URI).
fn is_denied_header(name: &str) -> bool {
    HOP_BY_HOP.contains(&name)
        || matches!(
            name,
            "authorization" | "x-api-key" | "x-goog-api-key" | "api-key" | "cookie" | "host"
        )
}

/// Query parameters globally denied upstream — the inbound `?key=` used solely
/// for downstream (client → proxy) authentication.
const DENIED_QUERY: &[&str] = &["key"];

/// Apply the global blacklist to the request in place (Part 1). MUST run after
/// authentication (which reads the credential headers/params) and before the
/// channel's `prepare`.
pub fn apply_global_blacklist(ctx: &mut RequestCtx) {
    let mut headers = HeaderMap::with_capacity(ctx.headers.len());
    for (name, value) in ctx.headers.iter() {
        if !is_denied_header(name.as_str()) {
            headers.append(name.clone(), value.clone());
        }
    }
    ctx.headers = headers;
    ctx.query = ctx.query.as_deref().and_then(strip_denied_query);
}

fn strip_denied_query(query: &str) -> Option<String> {
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| !DENIED_QUERY.contains(&pair.split('=').next().unwrap_or("")))
        .collect();
    (!kept.is_empty()).then(|| kept.join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::context::RoutingMode;
    use bytes::Bytes;
    use http::Method;

    fn ctx(headers: HeaderMap, query: Option<&str>) -> RequestCtx {
        RequestCtx {
            request_id: "t".into(),
            method: Method::POST,
            path: "/v1/chat/completions".into(),
            query: query.map(str::to_string),
            headers,
            body: Bytes::new(),
            mode: RoutingMode::Aggregated,
            identity: None,
            op: None,
            stream: false,
            route_name: None,
        }
    }

    #[test]
    fn strips_creds_cookies_hop_by_hop_keeps_rest() {
        let mut h = HeaderMap::new();
        h.insert(http::header::AUTHORIZATION, "Bearer c".parse().unwrap());
        h.insert("x-goog-api-key", "g".parse().unwrap());
        h.insert("cookie", "s=1".parse().unwrap());
        h.insert(http::header::CONNECTION, "keep-alive".parse().unwrap());
        h.insert(http::header::HOST, "client".parse().unwrap());
        h.insert(
            http::header::CONTENT_TYPE,
            "application/json".parse().unwrap(),
        );
        let mut c = ctx(h, Some("key=secret&alt=sse"));
        apply_global_blacklist(&mut c);

        assert!(c.headers.get(http::header::AUTHORIZATION).is_none());
        assert!(c.headers.get("x-goog-api-key").is_none());
        assert!(c.headers.get("cookie").is_none());
        assert!(c.headers.get(http::header::CONNECTION).is_none());
        assert!(c.headers.get(http::header::HOST).is_none());
        // non-denied header survives (the channel allow-list decides it later)
        assert_eq!(
            c.headers.get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        // ?key= dropped, other params survive for the channel allow-list
        assert_eq!(c.query.as_deref(), Some("alt=sse"));
    }
}
