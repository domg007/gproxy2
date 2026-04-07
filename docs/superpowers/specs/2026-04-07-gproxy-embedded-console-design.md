# Gproxy Embedded Console Design

## Summary

Build a new embedded web console for gproxy that serves both admin and regular-user workflows from a single React application. The console is built with `pnpm`, rendered as a client-side SPA, and embedded into `apps/gproxy` with `rust-embed`.

Phase 1 targets a core usable console rather than full API coverage. It must support:

- Login via `/login`
- Unified console shell with role-based navigation
- Admin dashboard
- Providers
- Credentials
- Credential status
- Provider OAuth assistance
- Provider usage viewer
- Users
- Admin user keys
- My keys
- My quota
- My usages
- Responsive layout
- Light and dark mode
- `en` and `zh-CN` localization

Implementation will be driven primarily by Gemini CLI, with Codex responsible for architectural corrections, Rust embedding/integration, Playwright validation, and final technical review. Claude CLI is used as a secondary review pass if available locally.

## Goals

- Provide a single browser UI for the most common gproxy admin and user workflows.
- Keep deployment simple by shipping the console inside the `gproxy` binary.
- Preserve the current API and proxy route layout without breaking `/admin`, `/user`, `/v1/*`, or `/{provider}/...`.
- Establish a frontend structure that can scale to later admin pages without a rewrite.
- Keep frontend state and API access typed enough to prevent schema drift for the covered management endpoints.

## Non-Goals

- A full provider protocol playground for `Claude`, `OpenAI`, or `Gemini` data-plane APIs.
- Full phase-2 admin coverage such as models, aliases, permissions, file permissions, rate limits, request-log management, update management, or config export UI.
- Automatic TypeScript generation directly from Rust types in phase 1.
- SSR, server components, or a Rust-rendered HTML UI.

## Constraints

- The existing server entrypoint is `apps/gproxy`.
- API routes already exist and must remain authoritative.
- The root path currently belongs to the API/proxy surface, so the console must live under `/console`.
- The console must be embedded with `rust-embed`.
- The repository currently has no Node frontend scaffold.
- The admin/provider surfaces contain JSON-heavy entities such as provider settings, dispatch rules, and credentials.
- Provider OAuth and provider usage endpoints are channel-specific and do not expose a single stable schema beyond the local wrapper fields already documented.

## Recommended Approach

Use a dedicated frontend package under `frontend/console`, but treat it as an internal asset pipeline rather than an independently deployed app:

1. `frontend/console` contains the `pnpm` React SPA.
2. Its production build outputs to `frontend/console/dist`.
3. A sync step copies the built assets into a stable embed directory in the Rust app, for example `apps/gproxy/web/console/`.
4. `apps/gproxy` embeds `apps/gproxy/web/console/` using `rust-embed`.
5. Axum serves the console under `/console` with SPA fallback to `index.html`.

This keeps frontend iteration independent while avoiding direct compile-time dependency on a missing `dist/` folder.

## Architecture

### Runtime Routing

The console is mounted only under `/console` and `/console/*`.

- `/login`, `/admin/*`, `/user/*`, `/v1/*`, `/{provider}/...` remain unchanged
- `/console` serves the SPA entry
- `/console/assets/*` serves hashed static assets
- `/console/*` falls back to SPA `index.html`

The root path `/` is not repurposed for the console in phase 1.

### Rust Integration

`apps/gproxy` gains a console-serving layer on top of the existing API router:

- `gproxy_api::api_router(state)` remains the main API/proxy router
- a new static asset router serves embedded frontend files
- route precedence must prefer API/proxy routes over SPA fallback
- the console-serving router is merged after API routes and only claims `/console*`

Recommended Rust pieces:

- `apps/gproxy/src/web.rs` or similar for embedded asset serving
- `rust-embed` for file inclusion
- content-type detection by extension
- `index.html` fallback for non-file `/console/*` routes
- `Cache-Control: no-cache` for `index.html`
- long-lived immutable caching for hashed assets under `/console/assets/*`

### Frontend Package Layout

Recommended package root:

`frontend/console`

Recommended structure:

- `src/app/`
  - router
  - app providers
  - shell layout
  - theme bootstrap
  - i18n bootstrap
- `src/features/auth/`
- `src/features/dashboard/`
- `src/features/providers/`
- `src/features/credentials/`
- `src/features/provider-admin/`
- `src/features/users/`
- `src/features/account/`
- `src/shared/api/`
- `src/shared/contracts/`
- `src/shared/ui/`
- `src/shared/lib/`
- `src/shared/i18n/`

## Frontend Stack

- Vite
- React
- TypeScript
- React Router
- Tailwind CSS
- TanStack Query
- `react-hook-form`
- `zod`
- `react-i18next`

Why this stack:

- Vite keeps the initial scaffold and local iteration fast.
- React Router fits a `/console/*` SPA with nested layouts.
- TanStack Query gives predictable caching and refetch behavior for admin lists.
- `zod` provides request/response validation without requiring a Rust-to-TS generator in phase 1.
- `react-hook-form` helps with JSON-heavy forms and conditional validation.

## Information Architecture

A single console shell is used for both admins and regular users.

### Auth Model

- Login posts to `/login`
- The response stores:
  - `session_token`
  - `user_id`
  - `is_admin`
  - `expires_in_secs`
- The frontend uses the session token for all admin and user routes
- The console sends the token as `Authorization: Bearer <token>`

### Role-Based Navigation

Regular users see:

- My Keys
- My Quota
- My Usages

Regular users default to `/console/me/quota` in phase 1. They do not get a separate non-admin dashboard page.

Admins see both admin and user areas:

- Dashboard
- Providers
- Credentials
- Credential Status
- Provider OAuth
- Provider Usage
- Users
- Admin User Keys
- My Keys
- My Quota
- My Usages

Navigation is generated from a local feature manifest instead of being hardcoded directly in layout components. Each manifest item includes:

- route path
- i18n label key
- required role
- icon id
- feature module id

This gives a clean path for phase-2 expansion.

Recommended phase-1 route map:

- `/console/login`
- `/console/dashboard`
- `/console/providers`
- `/console/credentials`
- `/console/credential-status`
- `/console/provider-oauth`
- `/console/provider-usage`
- `/console/users`
- `/console/user-keys`
- `/console/me/keys`
- `/console/me/quota`
- `/console/me/usages`

## Covered Pages

### Login

- Username and password form
- Session persistence
- Loading and invalid-credential states
- Redirect to the appropriate post-login route

### Dashboard

Phase-1 dashboard is intentionally small and operational:

- `/admin/health` summary card
- provider count
- user count
- timestamp
- quick links into providers, credentials, users, and account pages

### Providers

- Query/list providers
- Filter by name and channel
- Create/edit provider
- Delete provider
- JSON editing for `settings_json`
- JSON editing for `dispatch_json`

Provider editing is JSON-first in phase 1. Use validated textareas with:

- parse/format button
- inline JSON error display
- copy/paste-safe monospaced editing

Do not introduce Monaco in phase 1 unless the simpler editor proves unusable.

### Credentials

- List credentials by provider
- Provider filter
- Add credential from JSON
- Delete credential
- Render masked credential payload returned by the server

### Credential Status

- Query health snapshots
- Filter by provider
- Show provider, index, status, availability
- Manual status override with `healthy` / `dead`

### Provider OAuth

The OAuth UI is an operator workflow, not a fully channel-specific abstraction.

It supports:

- selecting a provider
- entering optional query parameters as key-value pairs
- calling `GET /{provider}/v1/oauth`
- showing:
  - authorize URL
  - state
  - redirect URI
  - verification URI
  - user code
  - mode
  - scope
  - instructions

Completion workflow in phase 1:

- open the returned authorization URL in a popup or new tab
- allow the operator to paste the callback URL or callback query string into the console
- the console calls `GET /{provider}/v1/oauth/callback` with those parameters
- the returned credential JSON is displayed for inspection
- the operator can save that credential through the existing admin credential upsert flow

This avoids overcommitting to provider-specific redirect semantics while still making OAuth operational.

### Provider Usage

- Select provider
- Call `GET /{provider}/v1/usage`
- Render formatted JSON response
- Show fetch errors clearly
- Support refresh and copy-json actions

No attempt is made to normalize provider usage payloads in phase 1.

### Users

- List/query users
- Create/edit users
- Delete users
- Show enabled/admin flags

Password handling:

- the UI treats password input as write-only
- phase 1 requires explicit password input on create
- phase 1 also requires explicit password input on edit because the current contract does not support partial password preservation safely from the browser

Because the current server contract expects `password`, phase 1 should avoid fake masked password behavior in the UI.

### Admin User Keys

- Query user keys by user
- Generate a key for a selected user
- Batch-generate keys for a selected user
- Delete keys

### My Keys

- Query current user keys
- Generate a new key

### My Quota

- Show quota
- used cost
- remaining balance

### My Usages

- Query current user usage rows
- Count current user usages
- filters for model/channel/time window where supported by the API

## Typed Client and Contracts

Phase 1 uses a local typed contract layer rather than full codegen.

### Contract Strategy

Create `src/shared/contracts/` with:

- shared response shapes
- per-feature request/response types
- `zod` validators for request forms and critical response parsing

The contract layer covers only gproxy-owned admin and user routes used in phase 1.

It does not attempt to strongly normalize:

- provider usage passthrough JSON
- provider OAuth callback `details`
- arbitrary provider credential JSON shapes

Those remain `unknown` or `Record<string, unknown>` with lightweight structural checks.

### API Client Strategy

Create a small centralized HTTP client in `src/shared/api/client.ts`:

- base path aware of `/`
- auth header injection
- JSON request helper
- JSON response parsing
- error parsing from `{ error: string }`
- typed wrappers per feature

Page components do not call `fetch` directly.

## UX, Responsive Layout, and Visual Direction

### Shell

- Desktop: left sidebar + top bar + content pane
- Mobile: top bar + drawer navigation
- Sticky top bar for session controls, language switch, and theme toggle

### Responsive Rules

- Table-heavy pages switch to stacked card rows on narrow screens
- JSON editors collapse into single-column form layouts
- Modals become full-screen sheets on mobile
- Filters wrap into vertically stacked controls below tablet width
- The app must remain operable at 360px width

### Visual Direction

- Avoid generic template-admin feel
- Use a warm-neutral light theme and graphite dark theme
- Define CSS variables for surfaces, borders, muted text, accents, and status colors
- Use stronger spacing and typographic hierarchy on dashboard and empty states
- No purple-default theme

### Theme

- Support `light`, `dark`, and `system`
- Persist selection in `localStorage`
- Apply theme class before React hydration to avoid flash

### Internationalization

- Support `en` and `zh-CN`
- All UI copy is keyed
- Feature manifests use translation keys, not raw strings
- Dates and numbers use locale-aware formatting helpers

## Error Handling and Session Behavior

### Session Handling

- On startup, restore stored session if present
- On `401`, clear session and redirect to login
- On `403`, keep session but show forbidden state
- Provide explicit sign-out action

### Request Errors

- Normalize server `{ error: string }` responses into a shared error shape
- Show errors near forms and in global toasts where appropriate
- Keep destructive actions confirmation-gated

### JSON Editing Errors

- Validate JSON before submit
- Show parse errors inline with line/column if practical
- Disable submit while JSON is invalid

## Build and Embed Pipeline

Recommended file layout additions:

- `pnpm-workspace.yaml`
- `frontend/console/package.json`
- `apps/gproxy/web/console/.gitkeep`

Recommended scripts:

- frontend build: produces `frontend/console/dist`
- embed sync: copies `dist/*` into `apps/gproxy/web/console/`
- Rust build embeds from `apps/gproxy/web/console/`

Recommended behavior:

- `pnpm build` builds frontend
- `pnpm build:embed` builds frontend and syncs assets to the embed directory
- Rust release builds assume the embed directory is already populated

This keeps Cargo builds deterministic and avoids requiring Node to run inside every Rust compile step.

## Verification and Review Strategy

### Local Verification

At minimum, phase-1 completion requires:

- frontend typecheck
- frontend production build
- asset sync into embed directory
- Rust build for `apps/gproxy`
- manual smoke test of embedded console
- Playwright desktop viewport check
- Playwright mobile viewport check

### Playwright Coverage

Use Playwright to verify:

- login page layout
- dashboard layout
- providers page
- credentials page
- users page
- my account pages
- theme switching
- mobile navigation drawer
- card/list fallback on narrow widths

### Review Loop

The intended implementation workflow is:

1. Gemini CLI produces a focused implementation increment.
2. Codex reviews the patch for architecture, Rust integration, and API correctness.
3. Codex runs verification and Playwright layout checks.
4. Claude CLI performs a secondary review pass if available locally.
5. Codex applies corrections and moves to the next increment.

This is expected to repeat over several iterations rather than a single one-shot generation.

If Claude CLI is unavailable during implementation, replace that pass with a second Codex review or a delegated secondary model review.

## Risks

### JSON-Heavy Admin UX

Provider and credential management are configuration-heavy and can become awkward quickly. The phase-1 design deliberately favors reliable JSON editing over premature schema-specific forms.

### OAuth Genericity

Provider OAuth behavior is not fully normalized. The design therefore supports an operator-assisted flow rather than assuming a universal popup callback contract.

### Type Drift

Because phase 1 does not generate TS types directly from Rust, contract maintenance discipline matters. The local contract layer must stay scoped and explicit.

### Embedded Asset Freshness

If the frontend build and embed sync are not run before Rust packaging, stale assets can be shipped. The implementation should make this failure mode obvious in docs and scripts.

## Implementation Staging

Phase 1 should be executed in this order:

1. Scaffold frontend package, workspace, Tailwind, routing, theme, and i18n
2. Add shared API client, contract layer, and auth/session store
3. Implement console shell and guarded routes
4. Implement user pages
5. Implement providers, credentials, and users pages
6. Implement provider OAuth and provider usage pages
7. Add Rust asset serving and `rust-embed` integration
8. Run Playwright responsive verification
9. Run Gemini/Codex/Claude review loop until stable

## Open Follow-Up Items For Phase 2

- Models and aliases
- Permissions and file permissions
- Rate limits
- Admin usage management
- Request log browsing
- Update management
- Config export/import UX
- Stronger codegen or schema-sharing between Rust and TS
