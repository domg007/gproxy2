# Release Notes

## v0.3.10

### Changed

- Added top-level cache-control mode enum for Claude/ClaudeCode provider settings and admin UI:
  - `off` / `auto` / `5m` / `1h`.
- Updated top-level cache-control injection behavior:
  - `auto` injects `{"type":"ephemeral"}` without TTL
  - `5m` / `1h` inject `{"type":"ephemeral","ttl":"5m|1h"}`.
- Enhanced provider usage metrics and frontend display:
  - added cache read input tokens
  - added cache creation input tokens
  - added cache creation split by TTL (`5m`, `1h`).
- Updated EN/ZH i18n entries for cache-control modes and usage token labels.

### Fixed

- Fixed user-key copy/visibility button clickability in admin/user key lists.
- Added copy success/failure feedback for admin-side user-key copy action.
- Fixed Claude cache-affinity fallback for top-level cache-control without TTL:
  - default fallback is now `5m` (was `1h`).

### Compatibility

- `enable_top_level_cache_control` no longer accepts legacy boolean values (`true` / `false`).
- Use string mode values only: `off`, `auto`, `5m`, `1h`.

## v0.3.9

### Changed

- Add unquote and normalizeClaudeCodeCookie functions for credential handling

## v0.3.8

### Changed

- Updated workspace/package release version to `0.3.8`.
- Updated CI release workflow:
  - `push` on `main/master` now publishes a `staging` prerelease (not marked as latest)
  - `push` on `main/master` now also publishes Docker staging tags (`staging`, `staging-<sha>`, and `-musl` variants)
  - stable `release` publish behavior remains unchanged (`latest` + tag-based images).
- Refactored cache-affinity key derivation for content-generation routes:
  - switched from whole-body/single-key affinity to protocol-aware prefix-block affinity with multi-candidate matching
  - kept in-memory affinity pool storage/key scoping in v1 format (`{channel}::{affinity_key}`)
  - aligned key derivation across OpenAI Chat/Responses, Claude Messages, and Gemini GenerateContent paths.
- Unified affinity hint calculation callsites across upstream channels:
  - affinity hint now derives from `protocol + prepared model + prepared body`
  - avoids protocol ambiguity and keeps hint calculation consistent with final outbound request payload.
- Updated Claude/ClaudeCode top-level cache-control injection behavior:
  - when enabled and absent in request, injects top-level `{"type":"ephemeral"}` only
  - no longer injects client-side TTL; effective TTL is left to upstream server semantics.
- Add daily update schedule for npm packages in /docs directory

### Fixed

- Fixed self-update channel routing:
  - staging builds now track the `staging` release stream
  - stable builds continue to track the latest stable release stream.
- Fixed Claude auto cache-affinity TTL classification:
  - top-level `cache_control: {"type":"ephemeral"}` now maps to `1h` affinity TTL to match upstream automatic caching behavior
  - explicit/other Claude cache-control TTL handling remains unchanged (`1h` when declared, otherwise `5m`).
- Fixed retry-affinity lifecycle behavior:
  - on retry after affinity-hit failure, only the matched key mapping is cleared
  - on success, bind key is always written and matched key TTL is refreshed.
- Update ANTIGRAVITY_USER_AGENT version and modify request handling for Gemini model

### Docs

- Updated cache-affinity design docs (EN/ZH):
  - clarified credential pick modes and internal affinity pool behavior
  - documented v1 key derivation/TTL rules for OpenAI, Claude, and Gemini
  - documented multi-candidate matching, bind-key behavior, and retry cleanup semantics.

## v0.3.7

### Changed

- Updated workspace/package release version to `0.3.7`.
- Reverted the temporary OAuth-specific provider fallback patch; provider availability now follows unified bootstrap seeding.

### Fixed

- Fixed startup provider bootstrap when `config.toml` is absent:
  - builtin providers are now loaded into in-memory registry during bootstrap seeding
  - builtin providers are persisted to storage with stable IDs at the same time
  - provider-dependent endpoints (including OAuth and usage routes) no longer fail due to missing in-memory providers.

## v0.3.6

### Added

- Added a User-Agent template selector in admin provider config:
  - channel presets for `gproxy`, `codex`, `claudecode`, `geminicli`, `antigravity`
  - classic IDE presets (`VS Code`, `IntelliJ IDEA`, `PyCharm`)
  - classic bot presets (`Googlebot`, `Bingbot`).

### Fixed

- Fixed frontend default gproxy User-Agent draft from placeholder `os,arch` to build-time resolved values.

## v0.3.5

### Changed

- Updated workspace/package release version to `0.3.5`.
- Added stable default IDs for builtin channels during bootstrap seeding.
- Updated login UX defaults: username prefilled as `admin`, password hinted from startup logs.

### Fixed

- Fixed OAuth start/callback provider resolution when provider config exists in storage but is not yet loaded in memory:
  - fallback lookup now loads enabled provider config from storage
  - resolved provider config is cached back into in-memory state for subsequent OAuth requests.

## v0.3.4

### Fixed

- Fixed default SQLite DSN path for container deployments by switching from `sqlite://app/data/gproxy.db?mode=rwc` to `sqlite:///app/data/gproxy.db?mode=rwc`.
- Fixed Zeabur startup failure (`code: 14 unable to open database file`) caused by the incorrect default DSN path.
- Updated deployment docs and examples (`README`, `README.zh`, docs deployment guides, `zeabur.yaml`) to use the corrected DSN format.
- Update Cargo.toml to use workspace settings for version and edition

## v0.3.3

### Added

- Added per-channel `user_agent` settings across builtin/custom providers in admin config and backend settings schemas.
- Added global `spoof_emulation` setting with admin UI, persistence, and runtime HTTP client wiring.
- Enhanced credential management UX:
  - added OAuth as a dedicated credentials sub-tab
  - added quick single-credential add (raw key or JSON payload)
  - added clipboard copy actions for user keys and credential cards.
- Added automatic provider credential refresh after successful single add, OAuth completion, and batch import.

### Changed

- Refactored provider settings parsing architecture:
  - removed legacy monolithic parser approach
  - moved JSON patch parsing into each channel `settings.rs` via `from_provider_settings_value`
  - kept top-level parser focused on channel `match` dispatch.
- Improved upstream `user_agent` resolution and normalization flow, while preserving channel-specific default UA behaviors.
- ClaudeCode OAuth refresh flow now backfills missing account metadata (for example subscription/rate-limit profile fields) from profile endpoints.

### Fixed

- Improved self-update reliability for GitHub release fetch/download:
  - added proxy/direct client fallback flow for update requests
  - direct self-update client now explicitly disables inherited system proxy (`no_proxy`) to reduce `ProxyConnect` failures.
- Fixed credential copy payload behavior:
  - key-based channels now return/copy plain key value
  - JSON-based channels now return/copy normalized `secret_json`.
- Fixed explicit empty `user_agent` handling so empty UA can be intentionally configured and forwarded.
- Reduced default log noise by suppressing SQL statement-level logs (`sqlx` / `sea_orm` default to `warn` unless `RUST_LOG` overrides).

## v0.3.2

### Changed

- Improved model route protocol selection (`/v1/models`, `/{provider}/v1/models`, and model get routes):
  - prioritize Gemini model flow when `x-goog-api-key` or query `?key=` is provided
  - treat `anthropic-version` as Claude preference only when `Authorization: Bearer ...` exists
  - keep OpenAI as default fallback when no explicit Gemini/Claude preference is detected
- Updated request log UI to expose richer request context with payload drill-down (headers/body preview) for faster debugging.
- Added additional request normalization for Codex channel:
  - normalize model ids by stripping `codex/` prefix when needed
  - convert `system`/`developer` input messages into `instructions` before forwarding to upstream `/responses`

### Fixed

- Fixed local transformed responses for model endpoints not being unwrapped correctly:
  - removed enum wrapper shells like `ModelListOpenAi`
  - removed HTTP wrapper envelope (`stats_code/headers/body`) and returned normalized body payload directly
- Fixed Codex chat completion replay failures caused by upstream rejecting `system` messages (`System messages are not allowed`).
- Fixed GeminiCli long/stream chat completion replay compatibility by stripping unsupported `generationConfig.logprobs` and `generationConfig.responseLogprobs`, avoiding upstream `400` empty-body responses in this scenario.

## v0.3.1

### Changed

- Updated the login view to accept username and password instead of API key.
- Modified the API request to handle username and password for user authentication.
- Added password field to user-related data structures and storage.
- Implemented user key generation upon successful login if no existing key is found.
- Updated the Chinese localization files to reflect changes in the login process.
- Refactored user management to accommodate password handling in user creation and updates.


## v0.3.0

### Added
- Built-in `groq` channel support, including admin-side channel schema and provider execution wiring.
- Admin credential status management APIs (`query/upsert/delete`) and corresponding runtime status handling.
- Zeabur deployment template (`zeabur.yaml`) for one-click cloud deployment.
- Full docs site (Starlight) with bilingual content (`en` default + `zh` locale).

### Changed
- Refactored provider/channel registry flow to consolidate builtin/custom channel metadata and credential creation logic.
- Improved request routing and recording with stronger provider-model prefix parsing for unscoped routes.
- Refactored admin frontend provider/credential modules for clearer channel-specific settings/dispatch handling.
- Updated release/build pipeline and Docker build flow for more stable multi-architecture outputs.

### Improved
- Admin and user usage/request filter experience in frontend, including better searchable filter options.
- i18n message organization in frontend by splitting language messages into dedicated modules.
- Docs structure now includes dedicated deployment guide sections:
  - local deployment: binary + Docker
  - cloud deployment: Zeabur

### Fixed
- Favicon/static asset serving behavior in frontend/docs entry pages.
- Multiple provider/frontend consistency issues in channel settings, dispatch mapping, and filter option loading.
