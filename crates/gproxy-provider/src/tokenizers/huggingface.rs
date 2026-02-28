use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokenizers::Tokenizer;
use wreq::Client as WreqClient;

use super::LocalTokenizerError;

const HUGGINGFACE_BASE_URL: &str = "https://huggingface.co";
const HUGGINGFACE_RESOLVE_SUFFIX: &str = "resolve/main/tokenizer.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HuggingFaceTokenizerSource {
    pub cache_key: String,
    pub repos: Vec<String>,
}

impl HuggingFaceTokenizerSource {
    pub fn from_model(model: &str) -> Self {
        Self {
            cache_key: model.to_string(),
            repos: vec![model.to_string()],
        }
    }
}

pub fn load_tokenizer_from_file(
    cache_dir: &Path,
    source: &HuggingFaceTokenizerSource,
) -> Result<Option<Arc<Tokenizer>>, LocalTokenizerError> {
    let path = tokenizer_path(cache_dir, source);
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(LocalTokenizerError::File {
                model: source.cache_key.clone(),
                message: err.to_string(),
            });
        }
    };
    let tokenizer =
        Tokenizer::from_bytes(bytes.as_slice()).map_err(|err| LocalTokenizerError::File {
            model: source.cache_key.clone(),
            message: err.to_string(),
        })?;
    Ok(Some(Arc::new(tokenizer)))
}

pub async fn load_or_download_hf_tokenizer(
    http_client: &WreqClient,
    hf_token: Option<&str>,
    hf_base_url: Option<&str>,
    cache_dir: &Path,
    source: &HuggingFaceTokenizerSource,
) -> Result<Arc<Tokenizer>, LocalTokenizerError> {
    if let Some(tokenizer) = load_tokenizer_from_file(cache_dir, source)? {
        return Ok(tokenizer);
    }

    let hf_base_url = hf_base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(HUGGINGFACE_BASE_URL);

    let mut errors = Vec::new();
    for repo in &source.repos {
        let url = format!(
            "{}/{}/{}",
            hf_base_url.trim_end_matches('/'),
            repo.trim_matches('/'),
            HUGGINGFACE_RESOLVE_SUFFIX
        );
        match download_tokenizer_bytes(http_client, hf_token, &url).await {
            Ok(bytes) => {
                let tokenizer = Tokenizer::from_bytes(bytes.as_slice()).map_err(|err| {
                    LocalTokenizerError::Download {
                        model: source.cache_key.clone(),
                        message: format!("{repo}: {}", err),
                    }
                })?;
                persist_tokenizer_bytes(cache_dir, source, bytes.as_ref())?;
                return Ok(Arc::new(tokenizer));
            }
            Err(err) => errors.push(format!("{repo}: {err}")),
        }
    }

    Err(LocalTokenizerError::Download {
        model: source.cache_key.clone(),
        message: errors.join("; "),
    })
}

fn persist_tokenizer_bytes(
    cache_dir: &Path,
    source: &HuggingFaceTokenizerSource,
    bytes: &[u8],
) -> Result<(), LocalTokenizerError> {
    let path = tokenizer_path(cache_dir, source);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| LocalTokenizerError::File {
            model: source.cache_key.clone(),
            message: err.to_string(),
        })?;
    }
    std::fs::write(path, bytes).map_err(|err| LocalTokenizerError::File {
        model: source.cache_key.clone(),
        message: err.to_string(),
    })
}

fn tokenizer_path(cache_dir: &Path, source: &HuggingFaceTokenizerSource) -> PathBuf {
    cache_dir
        .join(sanitize_key(&source.cache_key))
        .join("tokenizer.json")
}

fn sanitize_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => ch,
            _ => '_',
        })
        .collect()
}

async fn download_tokenizer_bytes(
    http_client: &WreqClient,
    hf_token: Option<&str>,
    url: &str,
) -> Result<Vec<u8>, String> {
    let mut redirects = 0usize;
    let mut current_url = url.to_string();

    loop {
        let mut request = http_client.get(current_url.clone());
        if let Some(token) = hf_token
            && !token.is_empty()
        {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = request.send().await.map_err(|err| err.to_string())?;

        if response.status().is_success() {
            let bytes = response.bytes().await.map_err(|err| err.to_string())?;
            return Ok(bytes.to_vec());
        }

        if response.status().is_redirection() {
            if redirects >= 5 {
                return Err("too many redirects".to_string());
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| "redirect without location".to_string())?;
            current_url = join_redirect_url(&current_url, location);
            redirects += 1;
            continue;
        }

        let status = response.status();
        let body = response.bytes().await.unwrap_or_default();
        if body.is_empty() {
            return Err(format!("http status {}", status));
        }
        return Err(format!(
            "http status {} body={}",
            status,
            String::from_utf8_lossy(&body)
        ));
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
