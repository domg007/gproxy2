# Release Notes

## v0.2.4

### Changed
- Updated Render blueprint (`render.yaml`) to official Blueprint style (`runtime: docker`, preview generation enabled, `autoDeployTrigger`), and removed default managed PostgreSQL creation.
- Render deployment now keeps `GPROXY_DSN` optional by default for external DB wiring.
- Updated README/README.zh deployment docs to reflect current Render behavior and ephemeral default data directory usage.
- Minor code cleanup and refactor simplification in provider implementation internals.

### Fixed
- Fixed Zeabur template defaults to avoid passing literal placeholder strings like `${GPROXY_PORT}` into runtime envs.
- Hardened Docker startup command with defensive env normalization and safe fallbacks (`host`, `port`, `data_dir`, `admin_key`) to prevent crash loops from malformed platform-injected values.

## v0.2.3

### Added
- Frontend About section now shows app version.
- Frontend About section now shows git short commit hash.
- Added Render deployment blueprint (`render.yaml`) and one-click deployment entries in README.

### Changed
- Unified workspace package metadata for all crates with central `version` in `[workspace.package]`.
- Added central `rust-version` in `[workspace.package]` and switched crate manifests to `*.workspace = true` package fields.
- Introduced `[workspace.dependencies]` and migrated common dependencies (`anyhow`, `async-trait`, `bytes`, `serde`, `serde_json`, `time`, `tokio`) to workspace-managed versions.
- README/README.zh language links and route doc references were cleaned up.

### CI/CD
- Refactored Docker publish workflow into architecture matrix builds (`amd64` + `arm64`) with final multi-arch manifest creation.

### Fixed
- Release script now uses clean tag/release title formatting for GitHub Release updates.

## v0.2.5

### Added
 - feat: update axum dependency to include http2 support

## v0.2.6

### Fixed
- update Docker workflow to enable latest tag for releases and modify .gitignore for gproxy.db

## v0.2.7

### Fixed
- update CMD in Dockerfile to correctly handle GPROXY_DSN variable

## v0.2.8

### Fixed
- update Docker workflow to correctly enable latest tag for releases