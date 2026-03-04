use super::types::{
    GPROXY_REPO_API_LATEST, GPROXY_REPO_API_STAGING, GithubReleaseInfo, ResolvedReleaseAsset,
};
use super::verify::normalized_update_signing_key_id;

pub(super) async fn fetch_github_release_tag(
    client: &wreq::Client,
    update_channel: &str,
) -> Result<String, String> {
    let release_url = github_release_api_url_for_channel(update_channel);
    let release_resp = client
        .get(release_url)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|err| format!("fetch_release_metadata:{release_url}:{err}"))?;

    if !release_resp.status().is_success() {
        let status = release_resp.status();
        let body = release_resp
            .bytes()
            .await
            .map(|body| String::from_utf8_lossy(&body).to_string())
            .unwrap_or_else(|_| String::new());
        return Err(format!("fetch_latest_release_status_{status}: {body}"));
    }

    let release_body = release_resp
        .bytes()
        .await
        .map_err(|err| format!("read_latest_release_body: {err}"))?;
    let release: GithubReleaseInfo = serde_json::from_slice(&release_body)
        .map_err(|err| format!("parse_latest_release_json: {err}"))?;
    Ok(release.tag_name)
}

pub(super) async fn fetch_github_release_asset(
    client: &wreq::Client,
    target_asset: &str,
    update_channel: &str,
) -> Result<(String, ResolvedReleaseAsset), String> {
    let release_url = github_release_api_url_for_channel(update_channel);
    let release_resp = client
        .get(release_url)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|err| format!("fetch_release_metadata:{release_url}:{err}"))?;

    if !release_resp.status().is_success() {
        let status = release_resp.status();
        let body = release_resp
            .bytes()
            .await
            .map(|body| String::from_utf8_lossy(&body).to_string())
            .unwrap_or_else(|_| String::new());
        return Err(format!("fetch_latest_release_status_{status}: {body}"));
    }

    let release_body = release_resp
        .bytes()
        .await
        .map_err(|err| format!("read_latest_release_body: {err}"))?;
    let release: GithubReleaseInfo = serde_json::from_slice(&release_body)
        .map_err(|err| format!("parse_latest_release_json: {err}"))?;
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
            format!("asset_not_found_for_target:{target_asset}; available=[{names}]")
        })?;
    let checksum_asset_name = format!("{target_asset}.sha256");
    let checksum_asset = release
        .assets
        .iter()
        .find(|item| item.name == checksum_asset_name)
        .cloned()
        .ok_or_else(|| {
            let names = release
                .assets
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "checksum_asset_not_found_for_target:{checksum_asset_name}; available=[{names}]"
            )
        })?;
    let checksum_signature_asset_name = format!("{target_asset}.sha256.sig");
    let checksum_signature_asset = release
        .assets
        .iter()
        .find(|item| item.name == checksum_signature_asset_name)
        .cloned()
        .ok_or_else(|| {
            let names = release
                .assets
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "checksum_signature_asset_not_found_for_target:{checksum_signature_asset_name}; available=[{names}]"
            )
        })?;

    Ok((
        release.tag_name,
        ResolvedReleaseAsset {
            name: asset.name,
            download_url: asset.browser_download_url,
            expected_sha256: None,
            sha256_url: Some(checksum_asset.browser_download_url),
            sha256_signature_url: Some(checksum_signature_asset.browser_download_url),
            signature_key_id: Some(normalized_update_signing_key_id()),
        },
    ))
}

fn github_release_api_url_for_channel(update_channel: &str) -> &'static str {
    match update_channel {
        "staging" => GPROXY_REPO_API_STAGING,
        _ => GPROXY_REPO_API_LATEST,
    }
}
