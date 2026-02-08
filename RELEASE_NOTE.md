# Release Notes

## v0.2.2

### Added
- Custom provider request parameter mask table (`json_param_mask`) for JSON payload rewriting.
- Nested JSON path masking with dot path (`messages[0].content`), wildcard path (`messages[*].content`), and JSON Pointer path (`/messages/0/content`).
- Admin frontend controls for custom provider mask rules (with i18n text updates).
- Claude Code 1M capability controls and status display split by Sonnet and Opus.
- Zeabur deployment template (`zeabur.yaml`).

### Changed
- Default service port updated to `8787`.
- Provider/OAuth controls in admin UI were refactored and expanded for mode-based flows.
- Chinese documentation filenames normalized (`README_zh.md` -> `README.zh.md`, `route_zh.md` -> `route.zh.md`).

### Fixed
- Custom provider routing/integration behavior across admin and proxy paths.
- Credential and provider-side request handling consistency in the latest admin flow.
