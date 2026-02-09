# GProxy Routes (Current)

## Proxy

### Auth (downstream)
Accepted user key sources (first match wins):
- `Authorization: Bearer <key>`
- `x-api-key: <key>`
- `x-goog-api-key: <key>`
- Query `?key=<key>`

Note: downstream auth headers/query are **stripped** before forwarding upstream.
Note: `trace_id` is server-generated as UUIDv7 per downstream request and reused for all corresponding upstream events; request headers `x-trace-id` / `x-request-id` are ignored.

### Aggregate routes (`/...`, no `{provider}`)

#### Claude
- `POST /v1/messages`
- `POST /v1/messages/count_tokens`

#### OpenAI
- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/responses/compact`
- `POST /v1/responses/input_tokens`

#### Shared models
- `GET /v1/models`
- `GET /v1/models/{model}`

Disambiguation: `GET /v1/models` + `GET /v1/models/{model}`:
- Claude when header `anthropic-version` is present.
- Gemini v1 when downstream key style is Gemini (`x-goog-api-key` or `?key=`).
- Otherwise OpenAI.

#### Gemini
- `POST /v1/models/{model}:generateContent`
- `POST /v1/models/{model}:streamGenerateContent`
- `POST /v1/models/{model}:countTokens`
- `POST /v1beta/models/{model}:generateContent`
- `POST /v1beta/models/{model}:streamGenerateContent`
- `POST /v1beta/models/{model}:countTokens`
- `GET /v1beta/models`
- `GET /v1beta/models/{name}`

#### Model prefix rules (`provider/model`)
- Aggregate request model identifiers must be `provider/model`.
- Split rule uses the first `/` only, so model names may still include `/`.
- Missing or invalid prefix returns `400` with `error=missing_provider_prefix`.

#### Aggregate list response extensions
For `GET /v1/models` and `GET /v1beta/models`, response includes:
- `partial: boolean`

Error handling policy:
- `no_active_credentials` is silently skipped.
- `unsupported_operation` / `provider_disabled` are silently skipped.
- Other provider failures only affect `partial=true`; detailed errors are not returned to downstream clients.
- HTTP status is always `200`.

#### Response model normalization
For aggregate routes only, response model identifiers are normalized to include provider prefix:
- OpenAI/Claude model fields: `provider/model`
- Gemini model resource name: `models/provider/model`

Existing provider-prefixed routes (`/{provider}/...`) remain unchanged.

### Provider routes (`/{provider}/...`)

### Claude
- `POST /{provider}/v1/messages`
- `POST /{provider}/v1/messages/count_tokens`
- `GET /{provider}/v1/models`
- `GET /{provider}/v1/models/{model}`

Disambiguation: `GET /v1/models` + `GET /v1/models/{model}` are treated as **Claude** when header `anthropic-version` is present.

### OpenAI
- `POST /{provider}/v1/chat/completions`
- `POST /{provider}/v1/responses`
- `POST /{provider}/v1/responses/compact`
- `POST /{provider}/v1/responses/input_tokens`
- `GET /{provider}/v1/models`
- `GET /{provider}/v1/models/{model}`

Disambiguation: `GET /v1/models` + `GET /v1/models/{model}` default to **OpenAI** when not Claude/Gemini.

### Gemini
#### Generate / Stream / Count (v1 and v1beta)
- `POST /{provider}/v1/models/{model}:generateContent`
- `POST /{provider}/v1/models/{model}:streamGenerateContent`
- `POST /{provider}/v1/models/{model}:countTokens`

- `POST /{provider}/v1beta/models/{model}:generateContent`
- `POST /{provider}/v1beta/models/{model}:streamGenerateContent`
- `POST /{provider}/v1beta/models/{model}:countTokens`

#### Models
- `GET /{provider}/v1beta/models`
- `GET /{provider}/v1beta/models/{name}`

Disambiguation on `GET /v1/models` + `GET /v1/models/{model}`:
- When downstream key is **Gemini style** (`x-goog-api-key` or `?key=`), treat as **Gemini v1**.

### Provider internal downstream abilities
- `GET /{provider}/oauth`
- `GET /{provider}/oauth/callback`
- `GET /{provider}/usage`
  - Required query: `credential_id=<id>`.
  - Usage is fetched against that specific credential under the provider.

#### OAuth behavior notes

##### codex (`device` flow)
- `GET /codex/oauth`
  - Starts Device Authorization flow against OpenAI.
  - Returns: `mode=device`, `auth_url`, `verification_uri`, `user_code`, `interval`, `state`.
- `GET /codex/oauth/callback`
  - Uses `state` to continue flow (no `code` required).
  - Polls device authorization status and exchanges token with `redirect_uri=https://auth.openai.com/deviceauth/callback`.
  - If authorization is not completed yet, returns `409` with `error=authorization_pending: retry after <n>s`.

##### claudecode (`manual` flow)
- `GET /claudecode/oauth`
  - Returns manual auth info: `mode=manual`, `auth_url`, `state`, `redirect_uri`.
  - Default redirect: `https://platform.claude.com/oauth/code/callback`.
- `GET /claudecode/oauth/callback`
  - Accepts `?code=...` or `?callback_url=...` (manual `code` takes precedence).

##### geminicli (`manual` flow)
- `GET /geminicli/oauth`
  - Returns manual auth info: `mode=manual`, `auth_url`, `state`, `redirect_uri`.
  - Default redirect: `https://codeassist.google.com/authcode`.
- `GET /geminicli/oauth/callback`
  - Accepts `?code=...` or `?callback_url=...` (manual `code` takes precedence).

##### antigravity (`manual` flow)
- `GET /antigravity/oauth`
  - Returns manual auth info: `mode=manual`, `auth_url`, `state`, `redirect_uri`.
  - Default redirect: `http://localhost:51121/oauth-callback`.
- `GET /antigravity/oauth/callback`
  - Accepts `?code=...` or `?callback_url=...` (manual `code` takes precedence).

##### state resolution rules
- Explicit `state` is preferred when provided.
- If `state` is omitted and exactly one pending OAuth state exists, that state is used.
- If `state` is omitted and multiple pending states exist, callback returns `400` with `error=ambiguous_state`.


## Admin (/admin/...)

### Auth (admin)
Accepted admin key sources (first match wins):
- `x-admin-key: <key>`
- `Authorization: Bearer <key>`
- Query `?admin_key=<key>`

### Routes
- `GET /admin/health`
- `GET /admin/global_config`
- `PUT /admin/global_config`

- `GET /admin/providers`
- `GET /admin/providers/{name}`
- `PUT /admin/providers/{name}`
- `DELETE /admin/providers/{name}` (custom only; builtin must be disabled)

- `GET /admin/providers/{name}/credentials`
- `POST /admin/providers/{name}/credentials`

- `GET /admin/credentials`
- `PUT /admin/credentials/{id}`
- `DELETE /admin/credentials/{id}`
- `PUT /admin/credentials/{id}/enabled`

- `GET /admin/usage/providers/{provider}/tokens?from=<RFC3339>&to=<RFC3339>`
- `GET /admin/usage/providers/{provider}/models/{model}/tokens?from=<RFC3339>&to=<RFC3339>`
- `GET /admin/usage/credentials/{credential_id}/tokens?from=<RFC3339>&to=<RFC3339>`
- `GET /admin/usage/credentials/{credential_id}/models/{model}/tokens?from=<RFC3339>&to=<RFC3339>`

- `GET /admin/users`
- `PUT /admin/users/{id}`
- `DELETE /admin/users/{id}`
- `PUT /admin/users/{id}/enabled`

- `GET /admin/users/{id}/keys`
- `POST /admin/users/{id}/keys`
- `PUT /admin/user_keys/{id}`
- `DELETE /admin/user_keys/{id}`
- `PUT /admin/user_keys/{id}/enabled`

- `GET /admin/logs`
- `POST /admin/system/self_update`

Note: usage records are persisted in DB table `upstream_usages` (not `upstream_requests.usage_json`).
Note: `upstream_usages` includes a `model` column. Model-scoped usage routes filter by this column.
Note: `model` can be `NULL` for historical rows when request body/path did not contain model info, or when `event_redact_sensitive=true` (request body not persisted, so model cannot be extracted/backfilled).
Note: `GET /admin/logs` uses cursor pagination (`cursor_at` + `cursor_id`). `offset>0` is rejected for performance.
Note: `GET /admin/logs` defaults to `include_body=false`; request/response bodies are omitted unless explicitly enabled.

### Self update (`POST /admin/system/self_update`)
- Downloads the latest GitHub release metadata from `LeenHawk/gproxy`.
- Selects release asset by current runtime target (`os` + `arch`, and `linux-musl` when applicable).
- Replaces current executable in place.
- Schedules automatic process restart (about 500ms) after successful replacement.

Response notes:
- Success response includes `ok=true`, `release_tag`, `asset`, `installed_to`, `restart_scheduled=true`.
- If binary replacement succeeds but restart scheduling fails, returns `error=self_restart_schedule_failed`.
- If update flow fails (release fetch/download/extract/install), returns `error=self_update_failed`.
- Running-binary self update is currently rejected on Windows with `self_update_not_supported_on_windows_running_binary`.
