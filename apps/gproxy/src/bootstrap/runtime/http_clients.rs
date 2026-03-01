use std::time::Duration;

use anyhow::Result;
use wreq::Client as WreqClient;
use wreq::Proxy;
use wreq::header::{ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, HeaderMap, HeaderValue};
use wreq_util::Emulation;

const CLIENT_CONNECT_TIMEOUT_SECS: u64 = 600;
const CLIENT_STREAM_IDLE_TIMEOUT_SECS: u64 = 3600;

fn normalize_proxy(proxy: Option<&str>) -> Option<&str> {
    proxy.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then_some(value)
    })
}

pub(super) fn build_http_client(proxy: Option<&str>) -> Result<WreqClient> {
    let mut builder = WreqClient::builder()
        .connect_timeout(Duration::from_secs(CLIENT_CONNECT_TIMEOUT_SECS))
        .read_timeout(Duration::from_secs(CLIENT_STREAM_IDLE_TIMEOUT_SECS));

    if let Some(proxy_url) = normalize_proxy(proxy) {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }

    Ok(builder.build()?)
}

pub(super) fn build_claudecode_spoof_client(proxy: Option<&str>) -> Result<WreqClient> {
    let mut default_headers = HeaderMap::new();
    default_headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    default_headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    default_headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));

    let mut builder = WreqClient::builder()
        .connect_timeout(Duration::from_secs(CLIENT_CONNECT_TIMEOUT_SECS))
        .read_timeout(Duration::from_secs(CLIENT_STREAM_IDLE_TIMEOUT_SECS))
        .cookie_store(true)
        .emulation(Emulation::Chrome136)
        .default_headers(default_headers);

    if let Some(proxy_url) = normalize_proxy(proxy) {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }

    Ok(builder.build()?)
}
