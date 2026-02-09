use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use bytes::Bytes;
use tokenizers::Tokenizer;

use gproxy_provider_core::ProviderError;

use crate::providers::http_client::{SharedClientKind, client_for_ctx};

pub fn count_input_tokens(
    ctx: &gproxy_provider_core::UpstreamCtx,
    model: &str,
    body: &gproxy_protocol::openai::count_tokens::request::InputTokenCountRequestBody,
    hf_token: Option<&str>,
    hf_url: Option<&str>,
    data_dir: Option<&str>,
) -> Result<i64, ProviderError> {
    let tokenizer = load_tokenizer(ctx, model, hf_token, hf_url, data_dir)?;
    let mut value =
        serde_json::to_value(body).map_err(|err| ProviderError::Other(err.to_string()))?;
    if let Some(map) = value.as_object_mut() {
        map.remove("model");
    }
    let text =
        serde_json::to_string(&value).map_err(|err| ProviderError::Other(err.to_string()))?;
    let encoding = tokenizer
        .encode(text, false)
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    Ok(encoding.get_ids().len() as i64)
}

fn load_tokenizer(
    ctx: &gproxy_provider_core::UpstreamCtx,
    model: &str,
    hf_token: Option<&str>,
    hf_url: Option<&str>,
    data_dir: Option<&str>,
) -> Result<Arc<Tokenizer>, ProviderError> {
    let cache = tokenizer_cache();
    {
        let guard = cache
            .lock()
            .map_err(|_| ProviderError::Other("tokenizer lock failed".to_string()))?;
        if let Some(tokenizer) = guard.get(model) {
            return Ok(tokenizer.clone());
        }
    }

    let path = tokenizer_path(model, data_dir);
    if let Ok(bytes) = std::fs::read(&path)
        && let Ok(tokenizer) = Tokenizer::from_bytes(bytes.as_slice())
    {
        let tokenizer = Arc::new(tokenizer);
        let mut guard = cache
            .lock()
            .map_err(|_| ProviderError::Other("tokenizer lock failed".to_string()))?;
        guard.insert(model.to_string(), tokenizer.clone());
        return Ok(tokenizer);
    }

    let base = hf_url
        .map(|value| value.trim_end_matches('/'))
        .filter(|value| !value.is_empty())
        .unwrap_or("https://huggingface.co");
    let url = format!("{base}/{model}/resolve/main/tokenizer.json");
    let bytes = crate::providers::oauth_common::block_on(fetch_tokenizer_bytes(ctx, url, hf_token))?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, bytes.as_ref());
    let tokenizer = Tokenizer::from_bytes(bytes.as_ref())
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let tokenizer = Arc::new(tokenizer);
    let mut guard = cache
        .lock()
        .map_err(|_| ProviderError::Other("tokenizer lock failed".to_string()))?;
    guard.insert(model.to_string(), tokenizer.clone());
    Ok(tokenizer)
}

fn tokenizer_cache() -> &'static Mutex<HashMap<String, Arc<Tokenizer>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Tokenizer>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn tokenizer_path(model: &str, data_dir: Option<&str>) -> PathBuf {
    let base = data_dir
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "./data".to_string());
    let safe = sanitize_model_name(model);
    Path::new(&base)
        .join("cache")
        .join("tokenizers")
        .join(safe)
        .join("tokenizer.json")
}

fn sanitize_model_name(model: &str) -> String {
    model
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => ch,
            '/' | '\\' => '_',
            _ => '_',
        })
        .collect()
}

async fn fetch_tokenizer_bytes(
    ctx: &gproxy_provider_core::UpstreamCtx,
    url: String,
    hf_token: Option<&str>,
) -> Result<Bytes, ProviderError> {
    let client = client_for_ctx(ctx, SharedClientKind::Global)?;
    let mut redirects = 0usize;
    let mut current_url = url;
    loop {
        let mut req = client.get(current_url.clone());
        if let Some(token) = hf_token
            && !token.is_empty()
        {
            let bearer = format!("Bearer {token}");
            req = req.header("Authorization", bearer);
        }
        let resp = req
            .send()
            .await
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        if resp.status().is_success() {
            return resp
                .bytes()
                .await
                .map_err(|err| ProviderError::Other(err.to_string()));
        }
        if resp.status().is_redirection() {
            if redirects >= 5 {
                return Err(ProviderError::Other(
                    "tokenizer download failed: too many redirects".to_string(),
                ));
            }
            let location = resp
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string());
            let Some(location) = location else {
                return Err(ProviderError::Other(
                    "tokenizer download failed: redirect without location".to_string(),
                ));
            };
            let next_url = join_redirect_url(&current_url, &location);
            redirects += 1;
            current_url = next_url;
            continue;
        }
        let status = resp.status();
        let body = resp.bytes().await.unwrap_or_else(|_| Bytes::new());
        let message = if body.is_empty() {
            format!("tokenizer download failed: {status}")
        } else {
            format!(
                "tokenizer download failed: {status} {}",
                String::from_utf8_lossy(&body)
            )
        };
        return Err(ProviderError::Other(message));
    }
}

fn join_redirect_url(base: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_string();
    }
    if let Some(pos) = base.find("://") {
        let scheme_end = pos + 3;
        if let Some(slash) = base[scheme_end..].find('/') {
            let origin = &base[..scheme_end + slash];
            return format!("{origin}{location}");
        }
        return format!("{base}{location}");
    }
    format!("{}{}", base.trim_end_matches('/'), location)
}
