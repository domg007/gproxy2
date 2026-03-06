use std::env;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};

use crate::AppState;
use crate::app_state::{UPDATE_SOURCE_CLOUDFLARE, UPDATE_SOURCE_GITHUB};
use crate::normalize_update_source;

use super::{HttpError, authorize_admin};

mod channel;
mod cloudflare;
mod github;
mod runtime;
#[cfg(test)]
mod tests;
mod types;
mod verify;

use channel::{is_semver_update_available, normalize_update_channel};
use cloudflare::{fetch_cloudflare_release_asset, fetch_cloudflare_release_tag};
use github::{fetch_github_release_asset, fetch_github_release_tag};
use runtime::{
    build_self_update_client, download_bytes_with_redirects, extract_binary_from_zip,
    schedule_self_restart, target_release_asset_name,
};
use types::{ResolvedReleaseAsset, SelfUpdateResult, UpdateChannelQuery};
use verify::{resolve_release_asset_sha256, verify_downloaded_asset_sha256};

pub(super) async fn system_self_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UpdateChannelQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    authorize_admin(&headers, &state)?;
    let snapshot = state.config.load();
    let proxy = snapshot.global.proxy.clone();
    let update_source = normalize_update_source(Some(snapshot.global.update_source.as_str()));
    drop(snapshot);
    let update_channel = normalize_update_channel(query.update_channel.as_deref());

    let result =
        self_update_to_latest_release(proxy, update_source.as_str(), update_channel.as_str())
            .await
            .map_err(|err| {
                HttpError::new(
                    StatusCode::BAD_GATEWAY,
                    format!("self_update_failed: {err}"),
                )
            })?;

    schedule_self_restart(result.staged_binary_path.clone()).map_err(|err| {
        HttpError::new(
            StatusCode::BAD_GATEWAY,
            format!("self_restart_schedule_failed: {err}"),
        )
    })?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "from_version": env!("CARGO_PKG_VERSION"),
        "update_source": update_source,
        "update_channel": update_channel,
        "release_tag": result.release_tag,
        "asset": result.asset_name,
        "installed_to": result.installed_to,
        "restart_required": false,
        "restart_scheduled": true,
        "note": "Update prepared and process restart scheduled automatically."
    })))
}

pub(super) async fn system_latest_release(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UpdateChannelQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    authorize_admin(&headers, &state)?;
    let snapshot = state.config.load();
    let proxy = snapshot.global.proxy.clone();
    let update_source = normalize_update_source(Some(snapshot.global.update_source.as_str()));
    drop(snapshot);
    let update_channel = normalize_update_channel(query.update_channel.as_deref());
    let current_version = env!("CARGO_PKG_VERSION").to_string();

    let latest_release_tag =
        fetch_latest_release_tag(proxy, update_source.as_str(), update_channel.as_str())
            .await
            .map_err(|err| {
                HttpError::new(
                    StatusCode::BAD_GATEWAY,
                    format!("fetch_latest_release_failed: {err}"),
                )
            })?;

    let has_update = is_semver_update_available(&current_version, &latest_release_tag);

    Ok(Json(serde_json::json!({
        "ok": true,
        "current_version": current_version,
        "latest_release_tag": latest_release_tag,
        "has_update": has_update,
        "update_source": update_source,
        "update_channel": update_channel
    })))
}

async fn self_update_to_latest_release(
    proxy: Option<String>,
    update_source: &str,
    update_channel: &str,
) -> Result<SelfUpdateResult, String> {
    #[cfg(windows)]
    {
        let target_asset = target_release_asset_name()?;
        let client = build_self_update_client(proxy)?;
        let (release_tag, asset) =
            fetch_release_asset(&client, &target_asset, update_source, update_channel).await?;

        let zip_bytes = download_bytes_with_redirects(&client, &asset.download_url, 8).await?;
        let expected_sha256 = resolve_release_asset_sha256(&client, &asset).await?;
        verify_downloaded_asset_sha256(&zip_bytes, &asset.name, expected_sha256.as_str())?;
        let binary_bytes = extract_binary_from_zip(&zip_bytes)?;
        let staged_binary_path = runtime::stage_windows_binary_bytes(binary_bytes)?;

        let installed_to = env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());

        return Ok(SelfUpdateResult {
            release_tag,
            asset_name: asset.name.clone(),
            installed_to,
            staged_binary_path: Some(staged_binary_path),
        });
    }

    #[cfg(not(windows))]
    {
        let target_asset = target_release_asset_name()?;
        let client = build_self_update_client(proxy)?;
        let (release_tag, asset) =
            fetch_release_asset(&client, &target_asset, update_source, update_channel).await?;

        let zip_bytes = download_bytes_with_redirects(&client, &asset.download_url, 8).await?;
        let expected_sha256 = resolve_release_asset_sha256(&client, &asset).await?;
        verify_downloaded_asset_sha256(&zip_bytes, &asset.name, expected_sha256.as_str())?;
        let binary_bytes = extract_binary_from_zip(&zip_bytes)?;
        runtime::install_binary_bytes(binary_bytes)?;

        let installed_to = env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());

        Ok(SelfUpdateResult {
            release_tag,
            asset_name: asset.name.clone(),
            installed_to,
            staged_binary_path: None,
        })
    }
}

async fn fetch_latest_release_tag(
    proxy: Option<String>,
    update_source: &str,
    update_channel: &str,
) -> Result<String, String> {
    let client = build_self_update_client(proxy)?;
    match update_source {
        UPDATE_SOURCE_CLOUDFLARE => fetch_cloudflare_release_tag(&client, update_channel).await,
        _ => fetch_github_release_tag(&client, update_channel).await,
    }
}

async fn fetch_release_asset(
    client: &wreq::Client,
    target_asset: &str,
    update_source: &str,
    update_channel: &str,
) -> Result<(String, ResolvedReleaseAsset), String> {
    match update_source {
        UPDATE_SOURCE_CLOUDFLARE => {
            fetch_cloudflare_release_asset(client, target_asset, update_channel).await
        }
        UPDATE_SOURCE_GITHUB => {
            fetch_github_release_asset(client, target_asset, update_channel).await
        }
        _ => fetch_github_release_asset(client, target_asset, update_channel).await,
    }
}
