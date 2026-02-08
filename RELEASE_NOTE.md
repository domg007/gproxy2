# Release Notes

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
