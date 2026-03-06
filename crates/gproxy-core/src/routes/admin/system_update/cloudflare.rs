use std::env;

use serde::Deserialize;

use super::types::{GPROXY_CLOUDFLARE_DOWNLOADS_BASE_DEFAULT, ResolvedReleaseAsset};
use super::verify::{normalize_sha256_hex, normalized_update_signing_key_id};

#[derive(Debug, Deserialize)]
struct CloudflareReleaseManifest {
    tag: String,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    key_id: Option<String>,
    assets: Vec<CloudflareReleaseAsset>,
}

#[derive(Debug, Deserialize, Clone)]
struct CloudflareReleaseAsset {
    name: String,
    url: String,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    sha256_url: Option<String>,
    #[serde(default)]
    sha256_sig_url: Option<String>,
    #[serde(default)]
    key_id: Option<String>,
}

pub(super) async fn fetch_cloudflare_release_tag(
    client: &wreq::Client,
    update_channel: &str,
) -> Result<String, String> {
    let manifest = fetch_cloudflare_release_manifest(client, update_channel).await?;
    Ok(manifest.tag)
}

pub(super) async fn fetch_cloudflare_release_asset(
    client: &wreq::Client,
    target_asset: &str,
    update_channel: &str,
) -> Result<(String, ResolvedReleaseAsset), String> {
    let manifest = fetch_cloudflare_release_manifest(client, update_channel).await?;
    if let Some(channel) = manifest.channel.as_deref() {
        let normalized = channel.trim().to_ascii_lowercase();
        if normalized != update_channel {
            return Err(format!(
                "cloudflare_manifest_channel_mismatch: expected={update_channel} actual={normalized}"
            ));
        }
    }

    let manifest_tag = manifest.tag.clone();
    let manifest_key_id = manifest.key_id.clone();
    let asset = manifest
        .assets
        .iter()
        .find(|item| item.name == target_asset)
        .cloned()
        .ok_or_else(|| {
            let names = manifest
                .assets
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!("cloudflare_asset_not_found_for_target:{target_asset}; available=[{names}]")
        })?;

    let manifest_url = cloudflare_manifest_url_for_channel(update_channel);
    let download_url = resolve_cloudflare_asset_url(&manifest_url, asset.url.as_str());
    let expected_sha256 = if let Some(raw) = asset.sha256.as_deref() {
        Some(normalize_sha256_hex(raw).ok_or_else(|| {
            format!("cloudflare_manifest_invalid_sha256:asset={target_asset}:value={raw}")
        })?)
    } else {
        None
    };
    let sha256_url = asset
        .sha256_url
        .as_deref()
        .map(|value| resolve_cloudflare_asset_url(&manifest_url, value))
        .unwrap_or_else(|| format!("{download_url}.sha256"));
    let sha256_signature_url = asset
        .sha256_sig_url
        .as_deref()
        .map(|value| resolve_cloudflare_asset_url(&manifest_url, value))
        .unwrap_or_else(|| format!("{sha256_url}.sig"));
    let signature_key_id = asset
        .key_id
        .clone()
        .or(manifest_key_id)
        .or_else(|| Some(normalized_update_signing_key_id()));

    Ok((
        manifest_tag,
        ResolvedReleaseAsset {
            name: asset.name,
            download_url,
            expected_sha256,
            sha256_url: Some(sha256_url),
            sha256_signature_url: Some(sha256_signature_url),
            signature_key_id: Some(
                signature_key_id.unwrap_or_else(normalized_update_signing_key_id),
            ),
        },
    ))
}

async fn fetch_cloudflare_release_manifest(
    client: &wreq::Client,
    update_channel: &str,
) -> Result<CloudflareReleaseManifest, String> {
    let manifest_url = cloudflare_manifest_url_for_channel(update_channel);
    let response = client
        .get(&manifest_url)
        .header("accept", "application/json")
        .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|err| format!("fetch_cloudflare_manifest:{manifest_url}:{err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map(|body| String::from_utf8_lossy(&body).to_string())
            .unwrap_or_else(|_| String::new());
        return Err(format!("fetch_cloudflare_manifest_status_{status}: {body}"));
    }

    let body = response
        .bytes()
        .await
        .map_err(|err| format!("read_cloudflare_manifest_body: {err}"))?;
    serde_json::from_slice(&body).map_err(|err| format!("parse_cloudflare_manifest_json: {err}"))
}

fn cloudflare_downloads_base() -> String {
    for key in [
        "GPROXY_CLOUDFLARE_DOWNLOADS_BASE",
        "GPROXY_WEB_HOSTED_DOWNLOADS_BASE",
        "GPROXY_S3_DOWNLOADS_BASE",
        "GPROXY_CNB_DOWNLOADS_BASE",
    ] {
        if let Some(value) = env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return value;
        }
    }

    GPROXY_CLOUDFLARE_DOWNLOADS_BASE_DEFAULT.to_string()
}

fn cloudflare_manifest_url_for_channel(update_channel: &str) -> String {
    format!(
        "{}/{update_channel}/manifest.json",
        cloudflare_downloads_base().trim_end_matches('/')
    )
}

fn resolve_cloudflare_asset_url(manifest_url: &str, raw_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    let base = manifest_url
        .rsplit_once('/')
        .map(|(prefix, _)| prefix)
        .unwrap_or(manifest_url);
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        trimmed.trim_start_matches('/')
    )
}
