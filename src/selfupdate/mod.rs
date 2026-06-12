//! Self-update mechanism (§19) — NATIVE only.
//!
//! The `gproxy` binary embeds the Console (rust-embed) and carries no business
//! data (config/credentials live in the persistence layer), so self-update only
//! swaps the executable. Edge (wasm) builds deploy through platform pipelines
//! and do NOT self-update: every item here is `#[cfg(not(target_arch =
//! "wasm32"))]`.
//!
//! Two orthogonal release channels (§19.3):
//! - `releases`: each version is a `vX.X.X` tag/Release; update decided by
//!   **semver** (manifest `version` vs `CARGO_PKG_VERSION`).
//! - `staging`: one fixed `staging` tag, CI re-uploads in place; `version` is
//!   meaningless, so update is decided by comparing the manifest artifact
//!   **sha256** to the running binary's sha256.
//!
//! Trust anchor (§19.2): the manifest is ed25519-signed; the public key is
//! compiled in. No valid signature → the binary is never replaced. The risky
//! I/O (download, integrity/signature check, atomic swap, restart) lives behind
//! the [`download`], [`verify`], and [`swap`] seams; [`version`] and
//! [`manifest`] are pure and unit-tested.

#[cfg(not(target_arch = "wasm32"))]
mod download;
#[cfg(not(target_arch = "wasm32"))]
mod manifest;
#[cfg(not(target_arch = "wasm32"))]
mod swap;
#[cfg(not(target_arch = "wasm32"))]
mod verify;
#[cfg(not(target_arch = "wasm32"))]
mod version;

#[cfg(not(target_arch = "wasm32"))]
pub use manifest::{Artifact, Manifest};
#[cfg(not(target_arch = "wasm32"))]
pub use version::{UpdateDecision, current_target_triple};

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use crate::http::client::UpstreamClient;

/// Release channel (§19.3). One of the two `update_channel` values.
#[cfg_attr(not(target_arch = "wasm32"), derive(clap::ValueEnum))]
#[cfg_attr(not(target_arch = "wasm32"), value(rename_all = "lowercase"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channel {
    /// Versioned `vX.X.X` releases; semver comparison. Production default.
    #[default]
    Releases,
    /// Fixed `staging` tag, rolling re-upload; sha256 comparison.
    Staging,
}

impl Channel {
    fn as_str(self) -> &'static str {
        match self {
            Channel::Releases => "releases",
            Channel::Staging => "staging",
        }
    }
}

/// Update policy (§19.4). Governs whether a detected update is applied.
#[cfg_attr(not(target_arch = "wasm32"), derive(clap::ValueEnum))]
#[cfg_attr(not(target_arch = "wasm32"), value(rename_all = "lowercase"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// Never check.
    Off,
    /// Report availability only.
    Notify,
    /// Check + report; admin approval applies. Default.
    #[default]
    Manual,
    /// Check + apply + restart (opt-in, risky).
    Auto,
}

/// Restart model after a successful swap (§19.6).
#[cfg_attr(not(target_arch = "wasm32"), derive(clap::ValueEnum))]
#[cfg_attr(not(target_arch = "wasm32"), value(rename_all = "kebab-case"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Restart {
    /// Exit with a sentinel code; the supervisor (systemd/docker/k8s) restarts
    /// the new binary. Default for container deploys.
    #[default]
    Supervisor,
    /// `execv` the new binary in place (bare deploy, no supervisor).
    ReExec,
    /// Stage only; do not restart (the caller decides).
    None,
}

/// Errors surfaced by the self-update flow.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("manifest fetch/parse failed: {0}")]
    Manifest(String),
    #[error("no artifact in manifest for target `{0}`")]
    NoArtifact(String),
    #[error("download failed: {0}")]
    Download(String),
    #[error("integrity check failed: {0}")]
    Integrity(String),
    #[error("signature verification failed: {0}")]
    Signature(String),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("binary swap failed: {0}")]
    Swap(String),
    #[error("current version is not valid semver: {0}")]
    Version(String),
}

/// Result of a `check` (§19.10 `GET /admin/update/check` shape).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckReport {
    /// Current identity (semver for `releases`, sha256 prefix for `staging`).
    pub current: String,
    /// Latest identity from the manifest.
    pub latest: String,
    /// Whether an update is available.
    pub available: bool,
    /// Release notes URL, if the manifest carries one.
    pub notes_url: Option<String>,
}

/// Runtime context for a self-update run.
#[cfg(not(target_arch = "wasm32"))]
pub struct UpdateContext {
    /// GitHub `owner/repo` whose Releases host the manifest + artifacts.
    pub repo: String,
    /// Channel to track.
    pub channel: Channel,
    /// Data directory; staging happens under `<data_dir>/.update`.
    pub data_dir: PathBuf,
    /// Proxy-aware HTTP transport (reuses the upstream client).
    pub client: Arc<dyn UpstreamClient>,
}

/// Check the configured channel for an available update (§19.4). Pure decision
/// logic lives in [`version`]; this only does the manifest fetch + dispatch.
#[cfg(not(target_arch = "wasm32"))]
pub async fn check(ctx: &UpdateContext) -> Result<CheckReport, UpdateError> {
    let manifest = download::fetch_manifest(ctx).await?;
    let triple = current_target_triple();
    let artifact = manifest
        .artifact_for(&triple)
        .ok_or_else(|| UpdateError::NoArtifact(triple.clone()))?;

    let decision = match ctx.channel {
        Channel::Releases => version::releases_decision(&manifest.version)?,
        Channel::Staging => {
            let local = swap::current_exe_sha256()?;
            version::staging_decision(&local, &artifact.sha256)
        }
    };

    Ok(CheckReport {
        current: decision.current,
        latest: decision.latest,
        available: decision.available,
        notes_url: manifest.notes_url.clone(),
    })
}

/// Download, verify (sha256 + ed25519 signature), atomically swap, and (per
/// `restart`) hand off to a new binary (§19.5 / §19.6). Returns the new
/// version/identity on success when no restart is requested.
///
/// `ReExec` does not return on success (it replaces the process image);
/// `Supervisor` exits the process with the sentinel code after staging.
#[cfg(not(target_arch = "wasm32"))]
pub async fn apply(ctx: &UpdateContext, restart: Restart) -> Result<String, UpdateError> {
    let manifest = download::fetch_manifest(ctx).await?;
    let triple = current_target_triple();
    let artifact = manifest
        .artifact_for(&triple)
        .ok_or_else(|| UpdateError::NoArtifact(triple.clone()))?
        .clone();

    // Gate: only proceed if there is actually something to install.
    let available = match ctx.channel {
        Channel::Releases => version::releases_decision(&manifest.version)?.available,
        Channel::Staging => {
            let local = swap::current_exe_sha256()?;
            version::staging_decision(&local, &artifact.sha256).available
        }
    };
    if !available {
        tracing::info!(channel = ctx.channel.as_str(), "already up to date");
        return Ok(manifest.version.clone());
    }

    // 1. Download to a temp file on the same filesystem as the binary.
    let staged = download::download_artifact(ctx, &artifact).await?;

    // 2. Integrity: sha256 of the downloaded bytes must equal the manifest's.
    verify::verify_sha256(&staged, &artifact.sha256)?;

    // 3. Signature: the embedded ed25519 public key must verify the manifest
    //    signature (§19.2 — the hard floor; staging is verified too).
    verify::verify_manifest_signature(&manifest)?;

    // 4. Atomic swap, retaining `<exe>.prev` for rollback (§19.5 / §19.8).
    swap::install(&staged)?;
    tracing::info!(
        channel = ctx.channel.as_str(),
        version = %manifest.version,
        "new binary staged and verified"
    );

    // 5. Restart / hand off (§19.6).
    match restart {
        Restart::Supervisor => swap::exit_for_supervisor(),
        Restart::ReExec => swap::reexec(), // diverges on success
        Restart::None => Ok(manifest.version.clone()),
    }
}
