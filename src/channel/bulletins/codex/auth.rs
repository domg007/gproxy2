//! Codex auth (TODO M7): OAuth2 authorization-code + PKCE against
//! `auth.openai.com`, token refresh there; base
//! `https://chatgpt.com/backend-api/codex`. Request bodies are normalized to the
//! private Responses API and `/responses/compact` (M2).
//!
//! As an impersonation channel it forwards the codex-cli fingerprint / protocol
//! headers (its per-channel allow-list, applied after the global blacklist):
//! `user-agent`, `originator`, `session-id`, `thread-id`, `x-client-request-id`,
//! `x-codex-beta-features`, `x-codex-turn-metadata`, `x-codex-window-id`.
//! (`accept: text/event-stream` rides the base allow-list.)

