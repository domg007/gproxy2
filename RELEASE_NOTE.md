# Release Notes

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
