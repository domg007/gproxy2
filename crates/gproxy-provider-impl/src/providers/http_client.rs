use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gproxy_provider_core::{ProviderError, ProviderResult, UpstreamCtx};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SharedClientKind {
    Global,
    ClaudeCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ClientKey {
    kind: SharedClientKind,
    proxy: Option<String>,
}

static CLIENT_CACHE: OnceLock<Mutex<HashMap<ClientKey, wreq::Client>>> = OnceLock::new();

pub(crate) fn client_for_ctx(
    ctx: &UpstreamCtx,
    kind: SharedClientKind,
) -> ProviderResult<wreq::Client> {
    let key = ClientKey {
        kind,
        proxy: normalize_proxy(ctx.outbound_proxy.clone()),
    };

    let cache = CLIENT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .map_err(|_| ProviderError::Other("http client cache lock failed".to_string()))?;

    if let Some(client) = guard.get(&key) {
        return Ok(client.clone());
    }

    let client = build_client(key.proxy.as_deref())?;
    guard.insert(key, client.clone());
    Ok(client)
}

fn build_client(proxy: Option<&str>) -> ProviderResult<wreq::Client> {
    let mut builder = wreq::Client::builder();
    if let Some(proxy_url) = proxy {
        builder = builder.proxy(
            wreq::Proxy::all(proxy_url).map_err(|err| ProviderError::Other(err.to_string()))?,
        );
    }
    builder
        .build()
        .map_err(|err| ProviderError::Other(err.to_string()))
}

fn normalize_proxy(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}
