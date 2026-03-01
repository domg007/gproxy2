# Release Notes

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