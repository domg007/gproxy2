use std::path::PathBuf;

use serde::Deserialize;

pub(super) const GPROXY_REPO_API_LATEST: &str =
    "https://api.github.com/repos/LeenHawk/gproxy/releases/latest";
pub(super) const GPROXY_REPO_API_STAGING: &str =
    "https://api.github.com/repos/LeenHawk/gproxy/releases/tags/staging";
pub(super) const GPROXY_CNB_DOWNLOADS_BASE_DEFAULT: &str =
    "https://cnb.cool/ai-rp/gproxy";
pub(super) const UPDATE_CHANNEL_RELEASES: &str = "releases";
pub(super) const UPDATE_CHANNEL_STAGING: &str = "staging";
pub(super) const UPDATE_SIGNING_KEY_ID_DEFAULT: &str = "gproxy-release-v1";
pub(super) const UPDATE_SIGNING_KEY_ID: &str = match option_env!("GPROXY_UPDATE_SIGN_KEY_ID") {
    Some(value) => value,
    None => UPDATE_SIGNING_KEY_ID_DEFAULT,
};
pub(super) const UPDATE_SIGNING_PUBLIC_KEY_B64: &str =
    match option_env!("GPROXY_UPDATE_SIGN_PUBLIC_KEY_B64") {
        Some(value) => value,
        None => "",
    };

#[derive(Debug, Deserialize, Default)]
pub(in crate::routes::admin) struct UpdateChannelQuery {
    #[serde(default)]
    pub(super) update_channel: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(super) struct GithubReleaseAsset {
    pub(super) name: String,
    pub(super) browser_download_url: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct GithubReleaseInfo {
    pub(super) tag_name: String,
    pub(super) assets: Vec<GithubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CnbReleaseManifest {
    pub(super) tag: String,
    #[serde(default)]
    pub(super) channel: Option<String>,
    #[serde(default)]
    pub(super) key_id: Option<String>,
    pub(super) assets: Vec<CnbReleaseAsset>,
}

#[derive(Debug, Deserialize, Clone)]
pub(super) struct CnbReleaseAsset {
    pub(super) name: String,
    pub(super) url: String,
    #[serde(default)]
    pub(super) sha256: Option<String>,
    #[serde(default)]
    pub(super) sha256_url: Option<String>,
    #[serde(default)]
    pub(super) sha256_sig_url: Option<String>,
    #[serde(default)]
    pub(super) key_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedReleaseAsset {
    pub(super) name: String,
    pub(super) download_url: String,
    pub(super) expected_sha256: Option<String>,
    pub(super) sha256_url: Option<String>,
    pub(super) sha256_signature_url: Option<String>,
    pub(super) signature_key_id: Option<String>,
}

pub(super) struct SelfUpdateResult {
    pub(super) release_tag: String,
    pub(super) asset_name: String,
    pub(super) installed_to: String,
    pub(super) staged_binary_path: Option<PathBuf>,
}
