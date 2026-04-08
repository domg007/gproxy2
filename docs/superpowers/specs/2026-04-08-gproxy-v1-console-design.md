# Gproxy V1 Console Design

## Summary

Build a new embedded web console for the current v1 `gproxy` backend. The console should preserve most of the visual language, workspace layout, and operator workflow from `samples/gproxy`, but it must be rebuilt against the current v1 API and runtime behavior rather than porting the sample frontend unchanged.

The console is an internal admin and user management surface, not a separate product. It should ship inside the `gproxy` binary, mount under `/console`, authenticate through `POST /login`, and cover the full current admin and user management API surface in stages without forcing operators into JSON-first workflows.

## Goals

- Preserve the overall look, density, and interaction style of `samples/gproxy`.
- Keep the main operator workflows centered on dual-pane workspaces instead of generic dashboard cards.
- Support the current v1 admin and user APIs with typed frontend contracts.
- Default provider and credential editing to channel-aware forms instead of raw JSON.
- Use a single browser session model based on `/login` and `Authorization: Bearer <session_token>`.
- Ship the console as embedded static assets inside `apps/gproxy`.

## Non-Goals

- A playground for provider data-plane inference APIs.
- A brand-new visual design system that diverges from `samples/gproxy`.
- SSR, server components, or Rust-rendered HTML.
- Automatic TS generation directly from Rust types.
- Replacing every advanced JSON shape with a fully structured form on day one. Free-form areas such as dispatch rules may still need an advanced editor path.

## Confirmed Constraints

- The executable entrypoint remains `apps/gproxy`.
- API routes in `crates/gproxy-api` remain authoritative.
- The console must live under `/console`; the root path is not repurposed.
- `provider.channel` is explicit at creation time and immutable afterward.
- `/admin/providers/query` now returns `id`, which the frontend can rely on for provider-backed resources.
- `/admin/credentials/query` now returns raw credentials for admins; this is a management surface and should not mask channel secrets at the API layer.
- `/admin/users/query` does not return passwords.
- Many write endpoints return only `AckResponse`, so the frontend must refresh from source of truth after writes.
- `/admin/config/export-toml` is read-only text export.
- `/admin/update` is a destructive operator action and must use explicit confirmation UX.

## Product Direction

The console should feel like a continuation of `samples/gproxy`, not a new admin SaaS. The first impression should stay operational and tool-like:

- sticky top bar
- left navigation
- large workspace canvas
- list pane plus detail/editor pane
- restrained cards and controls
- bilingual English and Simplified Chinese
- light and dark mode

The UI should reuse the same basic hierarchy decisions as the sample:

- page-level workspaces instead of deeply nested modal flows
- high information density without looking crowded
- monospaced rendering where operators inspect payloads or keys
- explicit refresh buttons for operator confidence
- toast feedback for writes

## Runtime Mounting

The console is a client-rendered SPA embedded into the Rust binary.

- Frontend source lives under `frontend/console`
- Production build output goes to `frontend/console/dist`
- A sync/build step copies built assets into `apps/gproxy/web/console/`
- `apps/gproxy` embeds `apps/gproxy/web/console/`
- Axum serves the console under `/console` and `/console/*`

Route precedence must keep the existing API and proxy surface untouched:

- `/login`, `/admin/*`, `/user/*`, `/v1/*`, `/{provider}/...` keep existing behavior
- `/console` serves the SPA entry
- `/console/assets/*` serves hashed assets
- non-file `/console/*` falls back to `index.html`

## Frontend Stack

- Vite
- React
- TypeScript
- React Router
- Tailwind CSS
- Vitest

The code should stay closer to the sample frontend than to the experimental `embedded-console` SPA layout. Avoid over-abstracting the app into a heavy platform-style `shared/contracts/features` tree.

## Code Organization

Use a sample-aligned structure:

```text
frontend/console/src/
  app/
    App.tsx
    main.tsx
    modules.tsx
    session.ts
    theme.ts
    i18n/
  components/
    LoginView.tsx
    Nav.tsx
    Toast.tsx
    ui.tsx
  lib/
    api.ts
    auth.ts
    scope.ts
    form.ts
    datetime.ts
    types/
      shared.ts
      admin.ts
      user.ts
  modules/
    admin/
      DashboardModule.tsx
      GlobalSettingsModule.tsx
      ProvidersModule.tsx
      ModelsModule.tsx
      ModelAliasesModule.tsx
      UsersModule.tsx
      PermissionsModule.tsx
      FilePermissionsModule.tsx
      RateLimitsModule.tsx
      RequestsModule.tsx
      UsageModule.tsx
      ConfigExportModule.tsx
      UpdateModule.tsx
      providers/
      users/
    user/
      MyKeysModule.tsx
      MyQuotaModule.tsx
      MyUsageModule.tsx
```

This keeps the module naming and working style familiar while still separating types and API helpers cleanly enough for the larger v1 surface.

## Routing And Navigation

Use URL-driven navigation so pages are shareable and refresh-safe, but keep the rendered modules visually close to the sample app.

Primary routes:

- `/console/login`
- `/console/dashboard`
- `/console/global-settings`
- `/console/providers`
- `/console/models`
- `/console/model-aliases`
- `/console/users`
- `/console/user-keys`
- `/console/user-permissions`
- `/console/user-file-permissions`
- `/console/user-rate-limits`
- `/console/requests`
- `/console/usages`
- `/console/config-export`
- `/console/update`
- `/console/me/keys`
- `/console/me/quota`
- `/console/me/usages`

Provider sub-workflows stay inside the provider workspace rather than exploding into separate heavy pages. The provider module uses a query-driven tab state such as:

- `/console/providers?tab=config`
- `/console/providers?tab=credentials`
- `/console/providers?tab=status`
- `/console/providers?tab=oauth`
- `/console/providers?tab=usage`

Navigation groups:

- Overview
  - Dashboard
  - Global Settings
- Providers
  - Providers
  - Models
  - Model Aliases
- Access
  - Users
  - Admin User Keys
  - User Permissions
  - User File Permissions
  - User Rate Limits
- Operations
  - Requests
  - Admin Usages
  - Config Export
  - Update
- My Account
  - My Keys
  - My Quota
  - My Usages

Regular users only see `My Account`. Admins see the full tree plus `My Account`.

## Auth Model

The console uses session tokens from `POST /login`.

Stored session fields:

- `user_id`
- `session_token`
- `is_admin`
- `expires_in_secs`
- derived `expires_at`

Request behavior:

- send `Authorization: Bearer <session_token>` for all `/admin/*` and `/user/*` requests
- never use user inference API keys as control-plane browser auth
- never ask operators to paste admin API keys into the browser UI for normal console use

Failure handling:

- `401`: clear session and redirect to `/console/login`
- `403` on admin pages: treat as role loss or disablement; clear admin-only navigation state and send the user to `/console/me/quota`

## Shared Data Flow

Keep state management simple and local.

- module-local `useState`
- `useEffect`-driven loading
- lightweight module-specific hooks where a module grows large
- centralized request helpers in `lib/api.ts`

Request helpers:

- `apiJson<T>()`
- `apiText()`
- `apiVoid()`

All write flows should assume the server response is only an acknowledgement. The default pattern is:

1. submit write
2. show success toast
3. re-query source data
4. restore current selection, tab, and filters if possible

Do not depend on server write responses returning entity ids except where the API explicitly does so, such as user key generation.

## Provider Workspace

`ProvidersModule` is the anchor admin workspace.

It owns:

- provider list and filters
- provider editor
- credential list/editor
- credential health view and manual override
- provider OAuth assistant
- provider usage passthrough viewer

### Provider editing

The provider editor is form-first, not JSON-first.

- `channel` is required on create
- `channel` becomes read-only in edit mode
- `settings_json` is produced from channel-aware form state
- `dispatch_json` can begin as an advanced editor path because the shape is inherently looser

JSON remains available as an advanced fallback, but the default operator path should be structured form controls.

### Credential editing

Credential editing is also form-first.

- choose provider
- render channel-specific credential fields
- serialize to `credential` JSON payload

Because the admin API now returns raw credential JSON, existing credentials may be opened for editing. The UI should still visually hide sensitive values by default with explicit reveal controls, but this is a presentation decision in the browser, not API masking.

## Resource Modules

### Models

Models use provider ids directly and therefore depend on provider query rows exposing `id`.

The page should provide:

- provider filter
- enabled filter
- model list
- editor for `model_id`, `display_name`, `enabled`, `price_each_call`
- structured price-tier editor with an advanced JSON fallback for `price_tiers_json`

### Model Aliases

Model aliases should let operators choose a provider and target model without forcing them to reason in ids alone. The UI can store provider id internally while displaying provider names.

### Users

User editing must reflect current backend behavior:

- create: password required
- edit: blank password means unchanged
- do not pretend the current password is known

### Admin User Keys

This module focuses on current admin key management semantics:

- list keys by selected user
- generate one key
- batch-generate keys through `/admin/user-keys/batch-upsert`
- delete keys

### User Permissions, File Permissions, Rate Limits

These pages should favor clarity over automation:

- table of existing rules
- right-side editor for one rule
- straightforward filters
- explicit delete

No policy builder or advanced DSL UI is needed.

## Operations Modules

### Requests

Requests are high-noise pages. Keep them operator-first:

- compact filters
- explicit query action
- clear pagination
- body fields collapsed by default
- delete only by selected trace ids

Do not promise delete-by-filter if the current backend path does not truly support it as a first-class UX.

### Admin Usages

Usage querying mirrors request querying:

- filtering
- tabular results
- pagination
- explicit delete of selected rows

### Config Export

Treat config export as read-only:

- preview TOML
- copy
- download

No inline import or edit workflow belongs in phase 1.

### Update

Treat update as a dangerous operator action:

- show current version
- show latest version when available
- show download URL and update source
- require explicit confirmation before triggering update

## My Account Modules

User-facing account pages remain simple:

- My Keys
- My Quota
- My Usages

They use the same session token as admin pages. Admin users can also access them.

## Backend Compatibility Notes

The design assumes the following backend behaviors, all now satisfied:

- `/admin/providers/query` returns `id`
- `/admin/credentials/query` returns raw credentials for admins

The design also intentionally adapts to existing backend realities:

- `/admin/users/query` does not expose passwords
- many write endpoints return only `AckResponse`
- `/admin/config/export-toml` returns text, not JSON
- `/admin/update` is a real binary-replacement operation

## Delivery Phases

The implementation should still be executed in phases even though the architecture is designed as one whole:

1. Shell, auth, dashboard, global settings, providers workspace, my account pages
2. Models, model aliases, users, admin user keys
3. User permissions, file permissions, rate limits
4. Requests, admin usages, config export, update

This keeps each phase shippable while preserving the final information architecture from day one.

## Verification Strategy

Required verification for implementation work:

- frontend typecheck
- frontend unit tests
- frontend production build
- Rust build for embedded asset serving
- manual browser smoke test of `/console`

Recommended smoke paths:

- login as admin
- query providers
- open and edit a provider
- inspect and edit a credential
- query users
- query quota/usages

## Risks And Mitigations

- Sample-UI fidelity can tempt direct code porting.
  - Mitigation: preserve layout and workflows, not outdated request assumptions.
- Provider and credential forms can drift from channel schemas.
  - Mitigation: keep a channel registry in frontend code with explicit tests.
- Large admin surface can create over-abstracted frontend structure.
  - Mitigation: stay module-first and sample-aligned.
- Operator pages can overpromise destructive bulk actions.
  - Mitigation: design only for behavior the backend actually supports.

## Decision

Proceed with a sample-aligned embedded console:

- preserve `samples/gproxy` visual and interaction patterns
- rebuild request, auth, and resource logic for current v1 APIs
- keep providers and credentials form-first
- cover the full current admin and user management surface in phased implementation
