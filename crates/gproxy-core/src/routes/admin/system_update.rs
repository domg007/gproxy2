use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, io::Read};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

use crate::AppState;

use super::{HttpError, authorize_admin};

const GPROXY_REPO_API_LATEST: &str = "https://api.github.com/repos/LeenHawk/gproxy/releases/latest";
const GPROXY_REPO_API_STAGING: &str =
    "https://api.github.com/repos/LeenHawk/gproxy/releases/tags/staging";

#[derive(Debug, Deserialize, Clone)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseInfo {
    tag_name: String,
    assets: Vec<GithubReleaseAsset>,
}

struct SelfUpdateResult {
    release_tag: String,
    asset_name: String,
    installed_to: String,
    staged_binary_path: Option<PathBuf>,
}

pub(super) async fn system_self_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, HttpError> {
    authorize_admin(&headers, &state)?;
    let proxy = state.config.load().global.proxy.clone();
    let update_channel = build_update_channel();

    let result = self_update_to_latest_release(proxy, update_channel.as_str())
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
        "update_channel": update_channel,
        "release_tag": result.release_tag,
        "asset": result.asset_name,
        "installed_to": result.installed_to,
        "restart_required": false,
        "restart_scheduled": true,
        "note": "Update prepared and process restart scheduled automatically."
    })))
}

async fn self_update_to_latest_release(
    proxy: Option<String>,
    update_channel: &str,
) -> Result<SelfUpdateResult, String> {
    #[cfg(windows)]
    {
        let target_asset = target_release_asset_name()?;
        let client = build_self_update_client(proxy)?;
        let (release_tag, asset) =
            fetch_latest_release_asset(&client, &target_asset, update_channel).await?;

        let zip_bytes =
            download_bytes_with_redirects(&client, &asset.browser_download_url, 8).await?;
        let binary_bytes = extract_binary_from_zip(&zip_bytes)?;
        let staged_binary_path = stage_windows_binary_bytes(binary_bytes)?;

        let installed_to = env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());

        return Ok(SelfUpdateResult {
            release_tag,
            asset_name: asset.name,
            installed_to,
            staged_binary_path: Some(staged_binary_path),
        });
    }

    #[cfg(not(windows))]
    {
        let target_asset = target_release_asset_name()?;
        let client = build_self_update_client(proxy)?;
        let (release_tag, asset) =
            fetch_latest_release_asset(&client, &target_asset, update_channel).await?;

        let zip_bytes =
            download_bytes_with_redirects(&client, &asset.browser_download_url, 8).await?;
        let binary_bytes = extract_binary_from_zip(&zip_bytes)?;
        install_binary_bytes(binary_bytes)?;

        let installed_to = env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());

        Ok(SelfUpdateResult {
            release_tag,
            asset_name: asset.name,
            installed_to,
            staged_binary_path: None,
        })
    }
}

async fn fetch_latest_release_asset(
    client: &wreq::Client,
    target_asset: &str,
    update_channel: &str,
) -> Result<(String, GithubReleaseAsset), String> {
    let release_url = release_api_url_for_channel(update_channel);
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

    Ok((release.tag_name, asset))
}

fn build_update_channel() -> String {
    option_env!("GPROXY_UPDATE_CHANNEL")
        .unwrap_or("stable")
        .trim()
        .to_ascii_lowercase()
}

fn release_api_url_for_channel(update_channel: &str) -> &'static str {
    match update_channel {
        "staging" => GPROXY_REPO_API_STAGING,
        _ => GPROXY_REPO_API_LATEST,
    }
}

fn build_self_update_client(proxy: Option<String>) -> Result<wreq::Client, String> {
    let proxy = proxy
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut builder = wreq::Client::builder();
    if let Some(proxy) = proxy.as_deref() {
        let parsed = wreq::Proxy::all(proxy).map_err(|err| format!("invalid_proxy:{err}"))?;
        builder = builder.proxy(parsed);
    } else {
        builder = builder.no_proxy();
    }
    builder
        .build()
        .map_err(|err| format!("build_http_client: {err}"))
}

async fn download_bytes_with_redirects(
    client: &wreq::Client,
    url: &str,
    max_redirects: usize,
) -> Result<Vec<u8>, String> {
    let mut current = url.to_string();

    for _ in 0..=max_redirects {
        let response = client
            .get(&current)
            .header("accept", "application/octet-stream")
            .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")))
            .send()
            .await
            .map_err(|err| format!("download_asset:{current}:{err}"))?;
        let status = response.status();

        if status.is_success() {
            return response
                .bytes()
                .await
                .map(|body| body.to_vec())
                .map_err(|err| format!("read_asset_body:{current}:{err}"));
        }

        if status.is_redirection() {
            let Some(location) = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                return Err(format!("redirect_without_location:{status}:{current}"));
            };

            if !location.starts_with("http://") && !location.starts_with("https://") {
                return Err(format!(
                    "relative_redirect_unsupported:{status}:{current}:{location}"
                ));
            }

            current = location.to_string();
            continue;
        }

        let body = response
            .bytes()
            .await
            .map(|body| String::from_utf8_lossy(&body).to_string())
            .unwrap_or_else(|_| String::new());
        return Err(format!("download_asset_status_{status}:{current}: {body}"));
    }

    Err(format!(
        "download_asset_too_many_redirects:start_url={url}:max={max_redirects}"
    ))
}

fn target_release_asset_name() -> Result<String, String> {
    let arch = match env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => return Err(format!("unsupported_arch:{other}")),
    };

    let os = env::consts::OS;
    let name = match os {
        "linux" => {
            #[cfg(target_env = "musl")]
            let libc_suffix = "-musl";
            #[cfg(not(target_env = "musl"))]
            let libc_suffix = "";
            format!("gproxy-linux-{arch}{libc_suffix}.zip")
        }
        "macos" => format!("gproxy-macos-{arch}.zip"),
        "windows" => format!("gproxy-windows-{arch}.zip"),
        other => return Err(format!("unsupported_os:{other}")),
    };
    Ok(name)
}

fn extract_binary_from_zip(zip_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let cursor = std::io::Cursor::new(zip_bytes.to_vec());
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|err| format!("open_zip_archive:{err}"))?;

    let exe_name = if cfg!(windows) {
        "gproxy.exe"
    } else {
        "gproxy"
    };
    let mut file = archive
        .by_name(exe_name)
        .map_err(|err| format!("zip_entry_not_found:{exe_name}:{err}"))?;

    let mut out = Vec::new();
    file.read_to_end(&mut out)
        .map_err(|err| format!("read_zip_entry:{err}"))?;
    if out.is_empty() {
        return Err("zip_entry_empty".to_string());
    }
    Ok(out)
}

#[cfg(not(windows))]
fn install_binary_bytes(binary: Vec<u8>) -> Result<(), String> {
    let current = env::current_exe().map_err(|err| format!("current_exe:{err}"))?;
    let parent = current
        .parent()
        .ok_or_else(|| "current_exe_parent_missing".to_string())?;
    let temp = temp_update_path(parent);

    fs::write(&temp, &binary)
        .map_err(|err| format!("write_temp_binary:{}:{err}", temp.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&current)
            .map(|meta| meta.permissions().mode())
            .unwrap_or(0o755);
        fs::set_permissions(&temp, fs::Permissions::from_mode(mode))
            .map_err(|err| format!("set_temp_permissions:{}:{err}", temp.display()))?;
    }

    fs::rename(&temp, &current).map_err(|err| {
        let _ = fs::remove_file(&temp);
        format!(
            "replace_binary_failed:{}->{}:{err}",
            temp.display(),
            current.display()
        )
    })?;

    Ok(())
}

#[cfg(windows)]
fn stage_windows_binary_bytes(binary: Vec<u8>) -> Result<PathBuf, String> {
    let current = env::current_exe().map_err(|err| format!("current_exe:{err}"))?;
    let parent = current
        .parent()
        .ok_or_else(|| "current_exe_parent_missing".to_string())?;
    let staged = temp_update_path(parent);
    fs::write(&staged, &binary)
        .map_err(|err| format!("write_staged_binary:{}:{err}", staged.display()))?;
    Ok(staged)
}

fn temp_update_path(parent: &std::path::Path) -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    if cfg!(windows) {
        parent.join(format!("gproxy-update-{pid}-{nanos}.exe.new"))
    } else {
        parent.join(format!(".gproxy-update-{pid}-{nanos}.new"))
    }
}

fn schedule_self_restart(staged_binary_path: Option<PathBuf>) -> Result<(), String> {
    let exe = env::current_exe().map_err(|err| format!("current_exe_for_restart:{err}"))?;
    let args: Vec<std::ffi::OsString> = env::args_os().skip(1).collect();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        #[cfg(windows)]
        if let Some(staged_binary_path) = staged_binary_path.as_ref() {
            match spawn_windows_update_worker(staged_binary_path.as_path(), exe.as_path(), &args) {
                Ok(()) => std::process::exit(0),
                Err(err) => {
                    eprintln!(
                        "self_update windows updater spawn failed for {} using staged {}: {err}",
                        exe.display(),
                        staged_binary_path.display()
                    );
                }
            }
        }

        #[cfg(not(windows))]
        let _ = &staged_binary_path;

        restart_current_process(exe, args);
    });

    Ok(())
}

#[cfg(windows)]
fn spawn_windows_update_worker(
    staged_binary: &std::path::Path,
    target_binary: &std::path::Path,
    args: &[std::ffi::OsString],
) -> Result<(), String> {
    let parent = target_binary
        .parent()
        .ok_or_else(|| "windows_target_binary_parent_missing".to_string())?;
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let script_path = parent.join(format!("gproxy-update-worker-{pid}-{nanos}.cmd"));

    let src = escape_cmd_set_value(staged_binary.to_string_lossy().as_ref());
    let dst = escape_cmd_set_value(target_binary.to_string_lossy().as_ref());
    let args_line = if args.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            args.iter()
                .map(|arg| quote_cmd_arg(arg.to_string_lossy().as_ref()))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };

    let script = format!(
        "@echo off\r\n\
setlocal enableextensions\r\n\
set \"SRC={src}\"\r\n\
set \"DST={dst}\"\r\n\
:retry\r\n\
move /Y \"%SRC%\" \"%DST%\" >nul 2>&1\r\n\
if errorlevel 1 (\r\n\
  timeout /t 1 /nobreak >nul\r\n\
  goto retry\r\n\
)\r\n\
start \"\" \"%DST%\"{args_line}\r\n\
del \"%~f0\" >nul 2>&1\r\n"
    );

    fs::write(&script_path, script).map_err(|err| {
        format!(
            "write_windows_update_script:{}:{err}",
            script_path.display()
        )
    })?;

    std::process::Command::new("cmd")
        .arg("/C")
        .arg(script_path.as_os_str())
        .spawn()
        .map_err(|err| {
            format!(
                "spawn_windows_update_script:{}:{err}",
                script_path.display()
            )
        })?;

    Ok(())
}

#[cfg(windows)]
fn escape_cmd_set_value(value: &str) -> String {
    value.replace('%', "%%")
}

#[cfg(windows)]
fn quote_cmd_arg(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }

    let mut quoted = String::from("\"");
    let mut backslashes = 0usize;
    for ch in value.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(unix)]
fn restart_current_process(exe: PathBuf, args: Vec<std::ffi::OsString>) {
    use std::os::unix::process::CommandExt;

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(&args);
    let err = cmd.exec();
    eprintln!("self_update exec failed for {}: {err}", exe.display());
    std::process::exit(1);
}

#[cfg(not(unix))]
fn restart_current_process(exe: PathBuf, args: Vec<std::ffi::OsString>) {
    match std::process::Command::new(&exe).args(&args).spawn() {
        Ok(_) => std::process::exit(0),
        Err(err) => {
            eprintln!(
                "self_update spawn failed for {} with args {:?}: {err}",
                exe.display(),
                args
            );
            std::process::exit(1);
        }
    }
}
