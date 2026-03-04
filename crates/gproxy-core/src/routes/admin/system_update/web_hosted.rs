use std::env;

use super::types::{
    GPROXY_WEB_HOSTED_DOWNLOADS_BASE_DEFAULT, ResolvedReleaseAsset, WebHostedReleaseManifest,
};
use super::verify::{normalize_sha256_hex, normalized_update_signing_key_id};

pub(super) async fn fetch_web_hosted_release_tag(
    client: &wreq::Client,
    update_channel: &str,
) -> Result<String, String> {
    let manifest_url = web_hosted_manifest_url_for_channel(update_channel);
    let response = client
        .get(&manifest_url)
        .header("accept", "application/json")
        .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|err| format!("fetch_web_hosted_manifest:{manifest_url}:{err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map(|body| String::from_utf8_lossy(&body).to_string())
            .unwrap_or_else(|_| String::new());
        return Err(format!("fetch_web_hosted_manifest_status_{status}: {body}"));
    }

    let body = response
        .bytes()
        .await
        .map_err(|err| format!("read_web_hosted_manifest_body: {err}"))?;
    let manifest: WebHostedReleaseManifest = serde_json::from_slice(&body)
        .map_err(|err| format!("parse_web_hosted_manifest_json: {err}"))?;
    Ok(manifest.tag)
}

pub(super) async fn fetch_web_hosted_release_asset(
    client: &wreq::Client,
    target_asset: &str,
    update_channel: &str,
) -> Result<(String, ResolvedReleaseAsset), String> {
    let manifest_url = web_hosted_manifest_url_for_channel(update_channel);
    let response = client
        .get(&manifest_url)
        .header("accept", "application/json")
        .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|err| format!("fetch_web_hosted_manifest:{manifest_url}:{err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map(|body| String::from_utf8_lossy(&body).to_string())
            .unwrap_or_else(|_| String::new());
        return Err(format!("fetch_web_hosted_manifest_status_{status}: {body}"));
    }

    let body = response
        .bytes()
        .await
        .map_err(|err| format!("read_web_hosted_manifest_body: {err}"))?;
    let manifest: WebHostedReleaseManifest = serde_json::from_slice(&body)
        .map_err(|err| format!("parse_web_hosted_manifest_json: {err}"))?;
    if let Some(channel) = manifest.channel.as_deref() {
        let normalized = channel.trim().to_ascii_lowercase();
        if normalized != update_channel {
            return Err(format!(
                "web_hosted_manifest_channel_mismatch: expected={update_channel} actual={normalized}"
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
            format!("web_hosted_asset_not_found_for_target:{target_asset}; available=[{names}]")
        })?;

    Ok((manifest_tag, {
        let download_url = resolve_web_hosted_asset_url(&manifest_url, asset.url.as_str());
        let expected_sha256 = if let Some(raw) = asset.sha256.as_deref() {
            Some(normalize_sha256_hex(raw).ok_or_else(|| {
                format!("web_hosted_manifest_invalid_sha256:asset={target_asset}:value={raw}")
            })?)
        } else {
            None
        };
        let sha256_url = asset
            .sha256_url
            .as_deref()
            .map(|value| resolve_web_hosted_asset_url(&manifest_url, value))
            .unwrap_or_else(|| format!("{download_url}.sha256"));
        let sha256_signature_url = asset
            .sha256_sig_url
            .as_deref()
            .map(|value| resolve_web_hosted_asset_url(&manifest_url, value))
            .unwrap_or_else(|| format!("{sha256_url}.sig"));
        let signature_key_id = asset
            .key_id
            .clone()
            .or_else(|| manifest_key_id.clone())
            .or_else(|| Some(normalized_update_signing_key_id()));
        ResolvedReleaseAsset {
            name: asset.name,
            download_url,
            expected_sha256,
            sha256_url: Some(sha256_url),
            sha256_signature_url: Some(sha256_signature_url),
            signature_key_id,
        }
    }))
}

fn web_hosted_downloads_base() -> String {
    env::var("GPROXY_WEB_HOSTED_DOWNLOADS_BASE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| GPROXY_WEB_HOSTED_DOWNLOADS_BASE_DEFAULT.to_string())
}

fn web_hosted_manifest_url_for_channel(update_channel: &str) -> String {
    format!(
        "{}/{update_channel}/manifest.json",
        web_hosted_downloads_base().trim_end_matches('/')
    )
}

fn resolve_web_hosted_asset_url(manifest_url: &str, raw_url: &str) -> String {
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
