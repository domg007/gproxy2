pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_CLAUDE_AI_BASE_URL: &str = "https://claude.ai";
pub const DEFAULT_PLATFORM_BASE_URL: &str = "https://platform.claude.com";

pub const DEFAULT_REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const OAUTH_SCOPE: &str = "user:profile user:inference user:sessions:claude_code";
pub const OAUTH_BETA: &str = "oauth-2025-04-20";
pub const CLAUDECODE_DEFAULT_BETAS: &[&str] = &[
    "claude-code-20250219",
    "adaptive-thinking-2026-01-28",
    "context-management-2025-06-27",
    "prompt-caching-scope-2026-01-05",
    "advanced-tool-use-2025-11-20",
    "effort-2025-11-24",
];
pub const CLAUDE_API_VERSION: &str = "2023-06-01";

pub const TOKEN_UA: &str = "claude-cli/2.1.62 (external, cli)";
pub const CLAUDE_CODE_UA: &str = "claude-code/2.1.62";

pub const OAUTH_STATE_TTL_MS: u64 = 600_000;
pub const TOKEN_REFRESH_SKEW_MS: u64 = 60_000;
