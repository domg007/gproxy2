# Unified Session Auth Design

## Goal

Unify browser-facing authentication into a single `/login` flow with a single in-memory session model, and make authorization depend on the user's current state instead of stale session-time role snapshots.

## Scope

This design changes only the single-instance in-memory session system used by `/login`, `/user/*`, and session-backed access to `/admin/*`.

It does not introduce:

- database-backed sessions
- Redis/shared session state
- cross-instance revocation guarantees
- changes to provider data-plane API-key authentication

## Current Problems

The current implementation stores `is_admin` inside `SessionEntry` and then trusts that snapshot during later requests. That creates stale authorization semantics:

- a user disabled after login can keep using the existing session until expiry
- an admin downgraded after login can keep using the existing admin session until expiry
- session meaning depends on which login route issued it rather than only on current user state

The current split between `/login` and `/admin/login` also adds complexity without providing a cleaner authorization model.

## Approved Design

### 1. Single Login Route

Use a single `POST /login` endpoint for all users.

- any enabled user can log in with username and password
- the response returns one session token format for both admin and non-admin users
- `/admin/login` is removed

`LoginResponse` should include:

- `user_id`
- `session_token`
- `expires_in_secs`
- `is_admin`

`is_admin` is returned as a convenience to callers. It is not a trusted authorization source after login.

### 2. Unified Session Model

`SessionEntry` becomes a minimal identity/expiry record:

- `user_id`
- `expires_at_unix_ms`

It must not cache:

- `is_admin`
- `enabled`
- password hash
- permission snapshots

The session proves only:

- which user logged in
- whether the token is still unexpired

### 3. Authorization Uses Current User State

Every session-backed request must:

1. extract the bearer token
2. validate the session exists and is not expired
3. load the current user by `user_id`
4. reject missing or disabled users
5. authorize based on the current user record

Route semantics:

- `/user/*`: any logged-in enabled user may access, including admins
- `/admin/*`: any logged-in enabled user with current `is_admin = true` may access

Admin API-key auth remains supported for `/admin/*` and provider-admin routes. That path already re-checks the current user state and does not need conceptual redesign beyond staying aligned with the new session principal shape.

### 4. Immediate Session Revocation on Sensitive User Changes

Single-instance behavior is explicit: when a user changes in a way that affects identity or authorization, all in-memory sessions for that user are revoked immediately on that instance.

Revocation triggers:

- user deleted
- `enabled` changed
- `is_admin` changed
- password changed

Non-security profile edits do not revoke sessions:

- name-only changes

Revocation is implemented by `user_id`, not by token type.

## Component Design

### `crates/gproxy-server/src/app_state.rs`

Responsibilities:

- store sessions
- create sessions
- validate sessions
- purge expired sessions
- revoke all sessions for a user

Required changes:

- remove `is_admin` from `SessionEntry`
- change `create_session(user_id, ttl_secs)`
- keep `validate_session(token)` expiry-only
- add `revoke_sessions_for_user(user_id)`

### `crates/gproxy-api/src/login.rs`

Responsibilities:

- authenticate username/password
- reject disabled users
- issue unified sessions

Required changes:

- keep one shared password-auth helper
- remove `admin_login`
- remove the current rule that blocks admins from `/login`
- return `is_admin` in `LoginResponse`

### `crates/gproxy-api/src/auth.rs`

Responsibilities:

- bearer extraction
- API-key auth for data-plane/admin key-backed routes
- session-backed auth for browser-facing routes

Required changes:

- replace role-snapshot-based session checks with current-user checks
- introduce a session principal carrying current user state
- let `/user/*` accept admin users too
- require current `is_admin = true` for session-backed `/admin/*`

### `crates/gproxy-api/src/admin/users.rs`

Responsibilities:

- mutate users
- keep in-memory identity state in sync
- trigger session revocation when sensitive user fields change

Required changes:

- compare pre-update and post-update user state
- call `revoke_sessions_for_user(user_id)` when revocation triggers match
- revoke on delete
- apply the same logic in batch upsert/delete paths

### `crates/gproxy-api/src/router.rs`

Responsibilities:

- expose one `/login`
- remove `/admin/login`
- keep `/user/*` and `/admin/*` middleware layering intact

## Data Flow

### Login

1. client calls `POST /login`
2. password is verified against the current user record
3. disabled users are rejected
4. session is created with `user_id` and expiry only
5. response returns token plus `is_admin`

### Session-backed `/user/*`

1. token extracted
2. session validated for expiry
3. current user loaded
4. missing/disabled user rejected
5. request proceeds with current user principal

### Session-backed `/admin/*`

1. token extracted
2. if session token:
   - validate session
   - load current user
   - reject missing/disabled user
   - require current `is_admin = true`
3. if API key:
   - existing admin API-key path remains

### User Mutation and Revocation

1. admin upserts or deletes user
2. storage write succeeds
3. in-memory user cache is updated/removed
4. if sensitive fields changed, revoke all sessions for that `user_id`

## Error Handling

Session-backed requests should use these semantics:

- missing or expired session: `401 Unauthorized`
- session points to missing user: `401 Unauthorized`
- session points to disabled user: `403 Forbidden`
- session valid but current user lacks admin privilege for `/admin/*`: `403 Forbidden`

This keeps authentication failure separate from authorization failure while still making disabled users visibly blocked.

## Testing Strategy

### Login Tests

- normal user can log in through `/login`
- admin user can also log in through `/login`
- disabled user is rejected
- response includes `is_admin`

### Middleware Tests

- admin session can access `/admin/*`
- normal user session cannot access `/admin/*`
- normal user session can access `/user/*`
- admin session can also access `/user/*`

### Revocation Tests

- disabling a user revokes existing sessions
- deleting a user revokes existing sessions
- changing password revokes existing sessions
- changing `is_admin` revokes existing sessions
- changing only the username does not revoke existing sessions

### Regression Tests

- old session does not preserve admin privilege after admin downgrade
- old session does not remain valid after password rotation
- old session does not remain valid after disable/delete

## Non-Goals

- persistent sessions across restart
- session refresh tokens
- cross-instance session invalidation
- replacing admin API-key auth for automation

## Migration Impact

This is a breaking behavioral cleanup:

- `/admin/login` is removed
- admin users now log in via `/login`
- admin sessions can access `/user/*`
- session authorization is derived from current user state, not login-time role snapshots

This is acceptable because the target release is a new major version and backward compatibility is not required.
