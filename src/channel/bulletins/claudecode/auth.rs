//! Claude Code auth (TODO M7): OAuth2 + PKCE against `claude.ai`, token refresh
//! at `{base}/v1/oauth/token` with a session-cookie fallback; injects
//! `anthropic-beta: oauth-2025-04-20` + SDK-mimicking `x-stainless-*` headers
//! under TLS emulation; base `https://api.anthropic.com`.
//!
//! As an impersonation channel it forwards the claude-cli fingerprint headers
//! (its per-channel allow-list, applied after the global blacklist):
//! `user-agent`, `anthropic-beta`, `anthropic-dangerous-direct-browser-access`,
//! `x-app`, `x-claude-code-session-id`, and the `x-stainless-*` family
//! (arch / lang / os / package-version / retry-count / runtime /
//! runtime-version / timeout). `anthropic-version` is injected, not forwarded.
