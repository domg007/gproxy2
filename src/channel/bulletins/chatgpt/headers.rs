//! Common request headers (non-sentinel) for every chatgpt.com backend-api call.
//!
//! Ported from v1 `channels/chatgpt/session.rs:144-210` (the Edge-147
//! client-hints set + `authorization: Bearer`). Header values are byte-exact;
//! they must stay in sync with the channel's TLS `default_emulation`.

const CHATGPT_ORIGIN: &str = "https://chatgpt.com";

/// Build the recurring "chatgpt web" request header set for `access_token`.
pub fn standard_headers(access_token: &str) -> http::HeaderMap {
    let mut map = http::HeaderMap::new();
    let add = |map: &mut http::HeaderMap, name: &'static str, value: String| {
        let n = http::HeaderName::from_static(name);
        if let Ok(v) = http::HeaderValue::from_str(&value) {
            map.insert(n, v);
        }
    };
    add(&mut map, "accept", "*/*".into());
    add(
        &mut map,
        "accept-language",
        "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7".into(),
    );
    add(&mut map, "content-type", "application/json".into());
    add(&mut map, "origin", CHATGPT_ORIGIN.into());
    add(&mut map, "referer", format!("{CHATGPT_ORIGIN}/"));
    add(&mut map, "authorization", format!("Bearer {access_token}"));
    add(
        &mut map,
        "oai-client-version",
        super::config::DEFAULT_BUILD_ID.into(),
    );
    add(&mut map, "oai-language", "en-US".into());
    add(
        &mut map,
        "sec-ch-ua",
        r#""Microsoft Edge";v="147", "Chromium";v="147", "Not_A Brand";v="24""#.into(),
    );
    add(&mut map, "sec-ch-ua-arch", r#""x86""#.into());
    add(&mut map, "sec-ch-ua-bitness", r#""64""#.into());
    add(
        &mut map,
        "sec-ch-ua-full-version",
        r#""147.0.3912.72""#.into(),
    );
    add(
        &mut map,
        "sec-ch-ua-full-version-list",
        r#""Microsoft Edge";v="147.0.3912.72", "Chromium";v="147.0.7727.102""#.into(),
    );
    add(&mut map, "sec-ch-ua-mobile", "?0".into());
    add(&mut map, "sec-ch-ua-model", r#""""#.into());
    add(&mut map, "sec-ch-ua-platform", r#""Windows""#.into());
    add(&mut map, "sec-ch-ua-platform-version", r#""19.0.0""#.into());
    add(&mut map, "sec-fetch-dest", "empty".into());
    add(&mut map, "sec-fetch-mode", "cors".into());
    add(&mut map, "sec-fetch-site", "same-origin".into());
    map
}
