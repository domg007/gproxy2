use std::env;

use serde::Deserialize;
use serde_json::Value;

use super::types::{
    GPROXY_CNB_DOWNLOADS_BASE_DEFAULT, ResolvedReleaseAsset, UPDATE_CHANNEL_RELEASES,
    UPDATE_CHANNEL_STAGING,
};
use super::verify::{normalize_sha256_hex, normalized_update_signing_key_id};

#[derive(Debug, Deserialize, Clone)]
struct CnbApiRelease {
    #[serde(default)]
    tag_name: Option<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default, alias = "tagRef")]
    tag_ref: Option<String>,
    #[serde(default)]
    assets: Vec<CnbApiReleaseAsset>,
}

#[derive(Debug, Deserialize, Clone)]
struct CnbApiReleaseAsset {
    name: String,
    #[serde(default)]
    browser_download_url: Option<String>,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default, alias = "hashAlgo")]
    hash_algo: Option<String>,
    #[serde(default, alias = "hashValue")]
    hash_value: Option<String>,
}

pub(super) async fn fetch_cnb_release_tag(
    client: &wreq::Client,
    update_channel: &str,
) -> Result<String, String> {
    let release = fetch_cnb_release(client, update_channel).await?;
    cnb_release_tag(&release)
        .ok_or_else(|| format!("cnb_release_missing_tag: channel={update_channel}"))
}

pub(super) async fn fetch_cnb_release_asset(
    client: &wreq::Client,
    target_asset: &str,
    update_channel: &str,
) -> Result<(String, ResolvedReleaseAsset), String> {
    let release = fetch_cnb_release(client, update_channel).await?;
    let release_tag = cnb_release_tag(&release)
        .ok_or_else(|| format!("cnb_release_missing_tag: channel={update_channel}"))?;
    let asset = release
        .assets
        .iter()
        .find(|item| item.name == target_asset)
        .cloned()
        .ok_or_else(|| {
            let names = release
                .assets
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!("cnb_asset_not_found_for_target:{target_asset}; available=[{names}]")
        })?;

    let download_url = resolve_cnb_asset_download_url(&release_tag, &asset);
    let expected_sha256 = resolve_cnb_asset_expected_sha256(&asset, target_asset)?;
    let sha256_url = format!("{download_url}.sha256");
    let sha256_signature_url = format!("{download_url}.sha256.sig");

    Ok((
        release_tag,
        ResolvedReleaseAsset {
            name: asset.name,
            download_url,
            expected_sha256,
            sha256_url: Some(sha256_url),
            sha256_signature_url: Some(sha256_signature_url),
            signature_key_id: Some(normalized_update_signing_key_id()),
        },
    ))
}

async fn fetch_cnb_release(
    client: &wreq::Client,
    update_channel: &str,
) -> Result<CnbApiRelease, String> {
    match update_channel {
        UPDATE_CHANNEL_STAGING => fetch_cnb_release_by_tag(client, UPDATE_CHANNEL_STAGING).await,
        UPDATE_CHANNEL_RELEASES => fetch_latest_cnb_release(client).await,
        _ => fetch_cnb_release_by_tag(client, update_channel).await,
    }
}

async fn fetch_cnb_release_by_tag(
    client: &wreq::Client,
    tag: &str,
) -> Result<CnbApiRelease, String> {
    let api_base = cnb_release_api_base()?;
    let url = format!("{api_base}/tags/{}", encode_path_segment(tag));
    let release_value = fetch_cnb_api_json(client, &url).await?;
    parse_cnb_release_from_value(&release_value)
        .map_err(|err| format!("parse_cnb_release_by_tag:{tag}:{err}"))
}

async fn fetch_latest_cnb_release(client: &wreq::Client) -> Result<CnbApiRelease, String> {
    let api_base = cnb_release_api_base()?;
    let latest_url = format!("{api_base}/latest");
    match fetch_cnb_api_json(client, &latest_url).await {
        Ok(release_value) => parse_cnb_release_from_value(&release_value)
            .map_err(|err| format!("parse_cnb_release_latest: {err}")),
        Err(primary_err) => {
            let list_url = format!("{api_base}?page=1&per_page=1");
            let release_value = fetch_cnb_api_json(client, &list_url)
                .await
                .map_err(|fallback_err| {
                    format!(
                        "fetch_cnb_release_latest_failed: primary={primary_err}; fallback={fallback_err}"
                    )
                })?;
            parse_cnb_release_from_value(&release_value)
                .map_err(|err| format!("parse_cnb_release_latest_fallback: {err}"))
        }
    }
}

async fn fetch_cnb_api_json(client: &wreq::Client, url: &str) -> Result<Value, String> {
    let mut request = client
        .get(url)
        .header("accept", "application/vnd.cnb.api+json")
        .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")));
    if let Some(token) = cnb_api_token() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = request
        .send()
        .await
        .map_err(|err| format!("fetch_cnb_release_api:{url}:{err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map(|body| String::from_utf8_lossy(&body).to_string())
            .unwrap_or_else(|_| String::new());
        let auth_hint = if status.as_u16() == 401 || status.as_u16() == 403 {
            " (set GPROXY_CNB_API_TOKEN)"
        } else {
            ""
        };
        return Err(format!(
            "fetch_cnb_release_api_status_{status}:{url}: {body}{auth_hint}"
        ));
    }

    let body = response
        .bytes()
        .await
        .map_err(|err| format!("read_cnb_release_api_body: {url}: {err}"))?;
    serde_json::from_slice::<Value>(&body)
        .map_err(|err| format!("parse_cnb_release_api_json:{url}:{err}"))
}

fn parse_cnb_release_from_value(value: &Value) -> Result<CnbApiRelease, String> {
    let release_value = extract_cnb_release_value(value)
        .ok_or_else(|| "cnb_release_not_found_in_response".to_string())?;
    let release: CnbApiRelease = serde_json::from_value(release_value)
        .map_err(|err| format!("decode_cnb_release_json: {err}"))?;
    if cnb_release_tag(&release).is_none() {
        return Err("cnb_release_missing_tag".to_string());
    }
    Ok(release)
}

fn extract_cnb_release_value(value: &Value) -> Option<Value> {
    if let Some(release) = parse_release_candidate(value)
        && cnb_release_tag(&release).is_some()
    {
        return Some(value.clone());
    }

    if let Some(array) = value.as_array() {
        for item in array {
            if let Some(found) = extract_cnb_release_value(item) {
                return Some(found);
            }
        }
        return None;
    }

    if let Some(object) = value.as_object() {
        for key in ["release", "data", "releases", "items", "result", "list"] {
            if let Some(item) = object.get(key)
                && let Some(found) = extract_cnb_release_value(item)
            {
                return Some(found);
            }
        }
        for item in object.values() {
            if let Some(found) = extract_cnb_release_value(item) {
                return Some(found);
            }
        }
    }

    None
}

fn parse_release_candidate(value: &Value) -> Option<CnbApiRelease> {
    serde_json::from_value::<CnbApiRelease>(value.clone()).ok()
}

fn cnb_release_tag(release: &CnbApiRelease) -> Option<String> {
    if let Some(tag_name) = release
        .tag_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(tag_name.to_string());
    }
    if let Some(tag) = release
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(tag.to_string());
    }
    release
        .tag_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .strip_prefix("refs/tags/")
                .unwrap_or(value)
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

fn resolve_cnb_asset_expected_sha256(
    asset: &CnbApiReleaseAsset,
    target_asset: &str,
) -> Result<Option<String>, String> {
    let hash_algo = asset
        .hash_algo
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    if let Some(raw) = asset
        .hash_value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if hash_algo
            .as_deref()
            .map(|value| value == "sha256")
            .unwrap_or(true)
        {
            return Ok(Some(normalize_sha256_hex(raw).ok_or_else(|| {
                format!("cnb_release_invalid_sha256:asset={target_asset}:value={raw}")
            })?));
        }
        return Ok(None);
    }
    if let Some(raw) = asset
        .sha256
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(normalize_sha256_hex(raw).ok_or_else(|| {
            format!("cnb_release_invalid_sha256:asset={target_asset}:value={raw}")
        })?));
    }
    Ok(None)
}

fn resolve_cnb_asset_download_url(release_tag: &str, asset: &CnbApiReleaseAsset) -> String {
    for candidate in [
        asset.browser_download_url.as_deref(),
        asset.download_url.as_deref(),
        asset.url.as_deref(),
    ] {
        if let Some(url) = candidate.map(str::trim).filter(|value| !value.is_empty()) {
            return url.to_string();
        }
    }
    if let Some(path) = asset
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return resolve_cnb_asset_path(path);
    }
    format!(
        "{}/-/releases/download/{}/{}",
        cnb_downloads_base().trim_end_matches('/'),
        encode_path_segment(release_tag),
        encode_path_segment(asset.name.as_str())
    )
}

fn cnb_downloads_base() -> String {
    env::var("GPROXY_CNB_DOWNLOADS_BASE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| GPROXY_CNB_DOWNLOADS_BASE_DEFAULT.to_string())
}

fn cnb_api_token() -> Option<String> {
    env::var("GPROXY_CNB_API_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn cnb_release_api_base() -> Result<String, String> {
    if let Some(base) = env::var("GPROXY_CNB_RELEASES_API_BASE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(base.trim_end_matches('/').to_string());
    }
    let slug = cnb_repo_slug()?;
    Ok(format!("https://api.cnb.cool/{slug}/-/releases"))
}

fn cnb_repo_slug() -> Result<String, String> {
    if let Some(slug) = env::var("GPROXY_CNB_REPO_SLUG")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(slug);
    }
    let base = cnb_downloads_base();
    let without_scheme = base
        .strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))
        .unwrap_or(base.as_str());
    let path = without_scheme
        .split_once('/')
        .map(|(_, path)| path)
        .unwrap_or_default();
    let mut parts = path.split('/').filter(|value| !value.is_empty());
    let owner = parts
        .next()
        .ok_or_else(|| format!("cnb_repo_slug_missing_owner_from_base:{base}"))?;
    let repo = parts
        .next()
        .ok_or_else(|| format!("cnb_repo_slug_missing_repo_from_base:{base}"))?;
    Ok(format!("{owner}/{repo}"))
}

fn cnb_site_origin() -> String {
    let base = cnb_downloads_base();
    let trimmed = base.trim().trim_end_matches('/');
    if let Some(rest) = trimmed.strip_prefix("https://") {
        let host = rest.split('/').next().unwrap_or(rest);
        return format!("https://{host}");
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        let host = rest.split('/').next().unwrap_or(rest);
        return format!("http://{host}");
    }
    trimmed.to_string()
}

fn resolve_cnb_asset_path(raw_path: &str) -> String {
    let trimmed = raw_path.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if trimmed.starts_with('/') {
        return format!("{}{}", cnb_site_origin().trim_end_matches('/'), trimmed);
    }
    format!(
        "{}/{}",
        cnb_downloads_base().trim_end_matches('/'),
        trimmed.trim_start_matches('/')
    )
}

fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
    encoded
}
