//! Claude Code auth (TODO M7): OAuth2 + PKCE against `claude.ai`, token refresh
//! at `{base}/v1/oauth/token` with a session-cookie fallback; injects
//! `anthropic-beta: oauth-2025-04-20` + SDK-mimicking `x-stainless-*` headers
//! under TLS emulation; base `https://api.anthropic.com`.
