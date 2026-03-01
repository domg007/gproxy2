# Release Notes

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
