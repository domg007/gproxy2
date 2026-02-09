# Release Notes

## v0.2.25

### Added
- admin API: added `GET /admin/logs` to query persisted upstream/downstream request logs with time range filters, kind selection, status range, and pagination.
- frontend: added a new non-realtime log query page with filter form + table view, replacing the old terminal-style WS stream page in sidebar entry.

### Changed
- credentials usage token APIs now support `model_contains` filter across provider/credential scopes.
- usage responses now include `call_count` (same semantics as `matched_rows`) for easier frontend aggregation display.
- log query route now uses a wider page max-width on frontend to improve table readability.

### Fixed
- downstream log query behavior: when `kind=downstream`, upstream-only filters no longer suppress downstream results unexpectedly.
- admin time serialization in log query responses is normalized to RFC3339 output.

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

## v0.2.9

### Changed
- simplify container startup by switching Docker runtime command to direct binary execution (`CMD ["/usr/local/bin/gproxy"]`) and rely on environment-variable configuration.

### Fixed
- sanitize unresolved proxy placeholders (for example `${GPROXY_PROXY}`) in bootstrap config to avoid startup failure (`relative URL without a base`) on PaaS environments.
- sanitize unresolved placeholder env values across bootstrap config (`host`, `port`, `admin key`, `dsn`, `proxy`, `event_redact_sensitive`) so malformed platform-injected values are treated as unset.

## v0.2.10

### Fixed
- align Docker default SQLite DSN path to `sqlite://app/data/gproxy.db?mode=rwc` (remove outdated `/app/data/db/...` default).
- sync Zeabur template/readme DSN defaults with current Docker runtime path (`/app/data/gproxy.db`).

## v0.2.11

### Fixed
- fix the path problem
## v0.2.12

### Changed
- codex provider now fetches model metadata online from upstream `/models` for both `ModelList` and `ModelGet`.
- `ModelGet` no longer calls `/models/{id}`; it reuses `/models` response and resolves the target model by `id`.
- added `client_version=0.99.0` query parameter when calling upstream codex `/models` to satisfy server-side validation.

### Fixed
- normalized codex upstream model payloads to OpenAI-compatible shapes for both list (`object=list,data=[...]`) and get (`object=model`).
- extended provider non-stream normalization hook to include original request context, enabling provider-side model selection for `ModelGet`.

## v0.2.13

### Added
- admin API: added `POST /admin/system/self_update` to download and install the latest GitHub Release binary in place.
- self update now schedules an automatic process restart after successful binary replacement.
- admin About page now provides one-click binary self-update action.

### Changed
- claudecode provider config now supports `prelude_text` enum selection (`claude_code_system` / `claude_agent_sdk`) instead of raw sentence input.
- kept backward compatibility for legacy `prelude_txt` and full-sentence values by loose parsing/mapping.
- provider config UI now renders claudecode prelude as a select field.

### Docs
- updated route documentation for `/admin/system/self_update` and admin query-key auth note.
- added Star History chart section in `README.md` and `README.zh.md`.

## v0.2.14

### Fixed
- self-update download now follows HTTP redirects for release assets (for example GitHub `302`) instead of failing on the first redirect response.
- frontend About build metadata now reads app version from workspace `Cargo.toml` and improves commit hash fallback handling in CI/runtime environments.

### Changed
- claudecode prelude selector options in frontend now display the two full preset sentences directly while still keeping stable stored enum values (`claude_code_system` / `claude_agent_sdk`).

## v0.2.15

### Added
- codex compact proxy route support: `POST /v1/responses/compact` and `POST /{provider}/v1/responses/compact`.

### Fixed
- codex compact calls now target upstream `/responses/compact` and normalize response shape back to compact payload (`{ "output": [...] }`) for downstream compatibility.
- codex compact route is explicitly restricted to `codex` provider and returns `unsupported_operation` for other providers.

### Changed
- removed local codex instruction patching/injection logic; downstream `instructions` are now passed through directly.
- removed obsolete local codex prompt template bundle under `crates/gproxy-provider-impl/src/providers/codex/instructions/`.

## v0.2.16

### Fixed
- codex upstream compatibility: when downstream request omits `instructions`, gproxy now sends an explicit empty string (`"instructions": ""`) to avoid upstream validation error (`Instructions are required`).

## v0.2.17

### Fixed
- do nothing but commit 0.2.16 change

## v0.2.18

### Changed
- refactor admin key handling to store as plaintext and update related components

## v0.2.19

### Added
- frontend credential import now supports `claudecode` session key line import (`one session key per line`) in both Credentials and Batch sections.

### Changed
- `claudecode` line-based import mapping now writes to `session_key` instead of `api_key`.
- updated i18n copy for session key import mode and placeholders (`zh_cn` / `en`).

### Fixed
- admin event stream (`/admin/events/ws`) now serializes `request_body` / `response_body` as readable strings instead of raw byte arrays.
- terminal event sink logging now uses the same readable string serialization for request/response bodies.

## v0.2.20

### Changed
- `openai/codex` responses API now uses provider passthrough request building for `/v1/responses` routes, reducing schema-coupling with local DTO parsing.
- provider-scoped responses routing now supports passthrough for both `/v1/responses` and nested `/v1/responses/*` paths.

### Fixed
- fixed frequent `422` on codex/openai responses caused by strict local body deserialization before upstream forwarding.
- kept credential injection/auth handling in passthrough flow, and preserved codex upstream request normalization (`instructions` fallback, compact stream handling, input normalization).
- fixed ClaudeCode 1M beta header behavior: when 1M is disabled or entitlement is unavailable, `context-1m-*` beta is now stripped from outgoing headers.
- fixed ClaudeCode 1M gating logic: 1M beta is sent only when both `enable_claude_1m_* == true` and `supports_claude_1m_* == true`.

## v0.2.21

### Fixed
- claudecode beta header normalization now actively removes downstream-provided `context-1m-*` entries when current credential/model is not eligible for 1M context.
- preserved non-1M beta entries while still appending required OAuth beta flag, and added unit tests for disabled/enabled context-1m header behavior.

## v0.2.22

### Changed
- simplified frontend Usage page by removing the live provider usage panel (`/{provider}/usage`) and related local state/actions.

## v0.2.23

### Changed
- Introduced a new module `http_client` to manage shared HTTP client instances.
- Replaced direct client instantiation with `client_for_ctx` function to utilize shared clients based on context.
- Updated various provider implementations (ClaudeCode, Codex, GeminiCli, Nvidia, Vertex) to use the new client management approach.
- Modified OAuth handling to ensure consistent client usage across authentication flows.
- Enhanced self-update functionality to support proxy configuration.

## v0.2.24

### Added
- SSE downstream keepalive heartbeat support (default `15s`) for `text/event-stream` responses to reduce idle disconnects on unstable network / reverse-proxy links.
- SSE response header hints for reverse proxies (`Cache-Control: no-cache`, `X-Accel-Buffering: no`) to reduce buffering-related stream interruption.

### Fixed
- codex responses passthrough compatibility: strip unsupported sampling params (`temperature`, `top_p`) for `/v1/responses` and `/v1/responses/compact` upstream calls.
- upstream non-JSON HTTP error normalization: HTML/error pages (for example Cloudflare 4xx pages) are now converted to stable JSON error payloads for downstream clients, while native JSON upstream errors are preserved as-is.


## v0.2.25

### Added
- admin API: added `GET /admin/logs` to query persisted upstream/downstream request logs with time range filters, kind selection, status range, and pagination.
- frontend: added a new non-realtime log query page with filter form + table view, replacing the old terminal-style WS stream page in sidebar entry.

### Changed
- credentials usage token APIs now support `model_contains` filter across provider/credential scopes.
- usage responses now include `call_count` (same semantics as `matched_rows`) for easier frontend aggregation display.
- log query route now uses a wider page max-width on frontend to improve table readability.

### Fixed
- downstream log query behavior: when `kind=downstream`, upstream-only filters no longer suppress downstream results unexpectedly.
- admin time serialization in log query responses is normalized to RFC3339 output.

## v0.2.26

### Added
- frontend logs page now supports row-level expand/collapse detail view for debugging.
- `/admin/logs` now returns `request_body` and `response_body` for each row.

### Changed
- removed legacy admin WS log stream route (`/admin/events/ws`) and switched to table-based log query flow (`/admin/logs`).
- logs page desktop layout is aligned with other admin pages and keeps horizontal scroll inside the table area.

### Fixed
- logs page no longer stretches overall desktop layout width due to wide table content.